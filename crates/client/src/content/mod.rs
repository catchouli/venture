//! The content pack's client files (NPC appearances, items, skills and their icons):
//! downloaded from the server, and downloaded again when the server reloads
//! its content.
//!
//! The server publishes a manifest listing each file with its hash; only
//! files whose hash has changed are downloaded again. In `--local` mode the
//! files are read straight from the server running inside the client.

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::image::{CompressedImageFormats, ImageSampler, ImageType};
use bevy::prelude::*;
use bevy::tasks::{IoTaskPool, Task, futures::check_ready};
use venture_content::{
    AppearanceDef, CLIENT_MANIFEST_FILE, ContentManifest, ItemDef, SkillDef, parse_appearances, parse_items,
    parse_skills,
};
use venture_protocol::ServerMessage;

use crate::network::launch::ServerChoice;
use crate::network::{FromServer, NetworkSystems};

/// Downloads and keeps the content's client files.
pub struct ContentPlugin;

impl Plugin for ContentPlugin {
    fn build(&self, app: &mut App) {
        let source = match app.world().resource::<ServerChoice>() {
            ServerChoice::Remote { base_url } => ContentSource::Http { base_url: format!("{base_url}/content") },
            #[cfg(not(target_arch = "wasm32"))]
            ServerChoice::Local { .. } => ContentSource::Local,
        };
        app.insert_resource(source)
            .init_resource::<ContentStore>()
            .init_resource::<ContentDownload>()
            .add_systems(PreUpdate, start_download_when_needed.after(NetworkSystems))
            .add_systems(Update, finish_download);
    }
}

/// The content the client has.
#[derive(Resource, Default)]
pub struct ContentStore {
    /// The version of the content held, if any has been downloaded.
    pub version: Option<String>,
    /// Appearances, by id.
    pub appearances: HashMap<String, AppearanceDef>,
    /// Items, by id.
    pub items: HashMap<String, ItemDef>,
    /// Skills, by id.
    pub skills: HashMap<String, SkillDef>,
    /// Item icons, by file name within `icons/`.
    pub icons: HashMap<String, Handle<Image>>,
    /// The files held, by path, with their hashes (to skip unchanged files).
    files: HashMap<String, ContentFile>,
}

impl ContentStore {
    /// An item's icon, if the item and its icon are known.
    pub fn icon_for(&self, item: &str) -> Option<Handle<Image>> {
        self.icons.get(&self.items.get(item)?.icon).cloned()
    }
}

/// One downloaded, parsed file.
#[derive(Clone)]
struct ContentFile {
    hash: String,
    data: FileData,
}

/// What a file holds, by the folder it's in.
#[derive(Clone)]
enum FileData {
    Appearances(Vec<AppearanceDef>),
    Items(Vec<ItemDef>),
    Skills(Vec<SkillDef>),
    /// A PNG, turned into an image once the download is finished.
    Icon(Vec<u8>),
}

/// Where content files come from.
#[derive(Resource, Clone)]
enum ContentSource {
    /// The server, at this address (ending in `/content`).
    Http { base_url: String },
    /// The server inside this client (`--local` mode).
    #[cfg(not(target_arch = "wasm32"))]
    Local,
}

/// The files of the server running inside this client (`--local` mode).
#[cfg(not(target_arch = "wasm32"))]
#[derive(Resource)]
pub struct LocalContent(pub venture_server::SharedClientContent);

/// A download in progress, if any.
#[derive(Resource, Default)]
struct ContentDownload(Option<Task<Result<Downloaded, String>>>);

/// The result of a download: the manifest, and every file (new or kept).
struct Downloaded {
    version: String,
    files: HashMap<String, ContentFile>,
}

/// Starts downloading when the player joins, and whenever the server says the
/// content has changed to a version the client doesn't have.
fn start_download_when_needed(
    mut events: MessageReader<FromServer>,
    source: Res<ContentSource>,
    #[cfg(not(target_arch = "wasm32"))] local: Option<Res<LocalContent>>,
    store: Res<ContentStore>,
    mut download: ResMut<ContentDownload>,
) {
    let needed = events.read().any(|FromServer(message)| match message {
        ServerMessage::Welcome { .. } => true,
        ServerMessage::ContentChanged { version } => store.version.as_ref() != Some(version),
        _ => false,
    });
    if !needed {
        return;
    }
    let known = store.files.clone();
    let task = match &*source {
        ContentSource::Http { base_url } => IoTaskPool::get().spawn(download_over_http(base_url.clone(), known)),
        #[cfg(not(target_arch = "wasm32"))]
        ContentSource::Local => {
            let Some(local) = local else { return };
            let content = local.0.read().expect("the client content lock isn't poisoned").clone();
            let new_files = changed_files(&content.manifest, &known)
                .into_iter()
                .filter_map(|path| Some((path.clone(), content.file(&path)?.to_vec())))
                .collect();
            let result = assemble(&content.manifest, known, new_files);
            IoTaskPool::get().spawn(async move { result })
        }
    };
    download.0 = Some(task);
}

/// Downloads the manifest, then each file whose hash is new.
async fn download_over_http(base_url: String, known: HashMap<String, ContentFile>) -> Result<Downloaded, String> {
    let fetch = crate::network::http::fetch;
    let manifest_bytes = fetch(format!("{base_url}/{CLIENT_MANIFEST_FILE}")).await?;
    let manifest = ContentManifest::from_ron(&String::from_utf8_lossy(&manifest_bytes))?;
    let mut new_files = HashMap::new();
    for path in changed_files(&manifest, &known) {
        let bytes = fetch(format!("{base_url}/{path}")).await?;
        new_files.insert(path, bytes);
    }
    assemble(&manifest, known, new_files)
}

/// The files in the manifest the client doesn't have (or has an old version of).
fn changed_files(manifest: &ContentManifest, known: &HashMap<String, ContentFile>) -> Vec<String> {
    manifest
        .files
        .iter()
        .filter(|file| known.get(&file.path).is_none_or(|kept| kept.hash != file.hash))
        .map(|file| file.path.clone())
        .collect()
}

/// Puts together the files a manifest lists: unchanged ones from `known`,
/// the rest parsed from `new_files`.
fn assemble(
    manifest: &ContentManifest,
    mut known: HashMap<String, ContentFile>,
    new_files: HashMap<String, Vec<u8>>,
) -> Result<Downloaded, String> {
    let mut files = HashMap::new();
    for file in &manifest.files {
        let parsed = match (known.remove(&file.path).filter(|kept| kept.hash == file.hash), new_files.get(&file.path)) {
            (Some(kept), _) => kept,
            (None, Some(bytes)) => parse_file(&file.path, &file.hash, bytes)?,
            (None, None) => return Err(format!("{} is missing", file.path)),
        };
        files.insert(file.path.clone(), parsed);
    }
    Ok(Downloaded { version: manifest.version.clone(), files })
}

/// Parses a client file, according to the folder it's in.
fn parse_file(path: &str, hash: &str, bytes: &[u8]) -> Result<ContentFile, String> {
    let text = || String::from_utf8_lossy(bytes);
    let data = match path.split('/').next() {
        Some("appearances") => FileData::Appearances(parse_appearances(&text())?),
        Some("items") => FileData::Items(parse_items(&text())?),
        Some("skills") => FileData::Skills(parse_skills(&text())?),
        Some("icons") => FileData::Icon(bytes.to_vec()),
        _ => return Err(format!("{path}: the client doesn't know this kind of file")),
    };
    Ok(ContentFile { hash: hash.to_string(), data })
}

/// Puts a finished download into the store.
fn finish_download(
    mut download: ResMut<ContentDownload>,
    mut store: ResMut<ContentStore>,
    mut images: ResMut<Assets<Image>>,
) {
    let Some(task) = download.0.as_mut() else { return };
    let Some(result) = check_ready(task) else { return };
    download.0 = None;
    match result {
        Ok(downloaded) => {
            store.appearances.clear();
            store.items.clear();
            store.skills.clear();
            store.icons.clear();
            for (path, file) in &downloaded.files {
                match &file.data {
                    FileData::Appearances(appearances) => {
                        store.appearances.extend(appearances.iter().map(|a| (a.id.clone(), a.clone())));
                    }
                    FileData::Items(items) => store.items.extend(items.iter().map(|i| (i.id.clone(), i.clone()))),
                    FileData::Skills(skills) => {
                        store.skills.extend(skills.iter().map(|skill| (skill.id.clone(), skill.clone())));
                    }
                    FileData::Icon(png) => match decode_icon(png) {
                        Ok(image) => {
                            let name = path.rsplit('/').next().unwrap_or(path).to_string();
                            store.icons.insert(name, images.add(image));
                        }
                        Err(error) => warn!("Couldn't read the icon {path}: {error}"),
                    },
                }
            }
            store.files = downloaded.files;
            store.version = Some(downloaded.version);
            info!("Content updated: {} appearances, {} items.", store.appearances.len(), store.items.len());
        }
        Err(error) => error!("Couldn't download the content: {error}"),
    }
}

/// Turns an icon's PNG into an image. Icons are small pixel pictures, so
/// they're sampled without smoothing to stay crisp when drawn larger.
fn decode_icon(png: &[u8]) -> Result<Image, String> {
    Image::from_buffer(
        png,
        ImageType::Extension("png"),
        CompressedImageFormats::NONE,
        true,
        ImageSampler::nearest(),
        RenderAssetUsages::RENDER_WORLD,
    )
    .map_err(|error| error.to_string())
}
