//! The content pack's client files (NPC appearances, items, abilities, skills, icons,
//! sounds, the interface's theme, skies and tree species):
//! downloaded from the server, and downloaded again when the server reloads
//! its content.
//!
//! The server publishes a manifest listing each file with its hash; only
//! files whose hash has changed are downloaded again. In `--local` mode the
//! files are read straight from the server running inside the client.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use bevy::asset::RenderAssetUsages;
use bevy::asset::io::AssetSourceBuilder;
use bevy::asset::io::memory::{Dir, MemoryAssetReader};
use bevy::image::{CompressedImageFormats, ImageSampler, ImageType};
use bevy::prelude::*;
use bevy::tasks::{IoTaskPool, Task, futures::check_ready};
use venture_content::characters::{CharacterModels, parse_character_models};
use venture_content::layout::{UiNode, UiStyles, parse_layout, parse_styles};
use venture_content::sky::{SKY_CYCLE_FILE, SkyCycle, SkyDef, SkyLight, parse_sky, parse_sky_cycle};
use venture_content::trees::{
    Species, TREE_DRAWING_FILE, TREE_SPECIES_FILE, TreeDrawing, parse_species, parse_tree_drawing,
};
use venture_content::{
    AbilityDef, AppearanceDef, CLIENT_MANIFEST_FILE, ContentManifest, GameSounds, ItemDef, ManifestFile, SkillDef,
    ThemeDef, parse_abilities, parse_appearances, parse_game_sounds, parse_items, parse_skills, parse_theme,
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
            .init_resource::<ContentProgress>()
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
    /// Abilities, by id.
    pub abilities: HashMap<String, AbilityDef>,
    /// Item icons, by file name within `icons/`.
    pub icons: HashMap<String, Handle<Image>>,
    /// Sounds, by file name within `sounds/`.
    pub sounds: HashMap<String, Handle<AudioSource>>,
    /// Which sounds the game itself plays (hits, misses, levelling up).
    pub game_sounds: GameSounds,
    /// Skills (gathering and making), by id.
    pub skills: HashMap<String, SkillDef>,
    /// The interface's look.
    pub theme: ThemeDef,
    /// The theme's pictures, by file name within `ui/`.
    pub ui_images: HashMap<String, Handle<Image>>,
    /// The theme's fonts, by file name within `ui/`.
    pub fonts: HashMap<String, Font>,
    /// The interface's layouts, by name.
    pub layouts: HashMap<String, UiNode>,
    /// The interface's text styles, colours and bar colours, if the content has them.
    pub ui_styles: Option<UiStyles>,
    /// Character models, if the content has them. (The model files
    /// themselves go to the asset server: see [`ContentAssets`].)
    pub character_models: Option<CharacterModels>,
    /// Skies, by name (their pictures go to the asset server).
    pub skies: HashMap<String, SkyFiles>,
    /// Which sky shows when (`skies/cycle.ron`), if the content says.
    pub sky_cycle: Option<SkyCycle>,
    /// Tree species (`trees/species.ron`), for the tree generator.
    pub tree_species: Vec<Species>,
    /// How trees are drawn (`trees/drawing.ron`).
    pub tree_drawing: TreeDrawing,
    /// Tree photographs (bark and leaf sets), by path in the content.
    pub tree_photos: HashMap<String, Arc<Vec<u8>>>,
    /// Model files that changed in the last download (to reload).
    pub changed_models: Vec<String>,
    /// The files held, by path, with their hashes (to skip unchanged files).
    files: HashMap<String, ContentFile>,
}

impl ContentStore {
    /// An item's icon, if the item and its icon are known.
    pub fn icon_for(&self, item: &str) -> Option<Handle<Image>> {
        self.icons.get(&self.items.get(item)?.icon).cloned()
    }
}

/// Content files the asset server loads (character models and their
/// textures), kept in memory. Loaded as `content://characters/...`.
#[derive(Resource, Clone, Default)]
pub struct ContentAssets(pub Dir);

impl ContentAssets {
    /// Adds the `content://` asset source, reading these files. (Must be
    /// done before the asset plugin is added.)
    pub fn register(&self, app: &mut App) {
        let root = self.0.clone();
        app.register_asset_source(
            CONTENT_SOURCE,
            AssetSourceBuilder::new(move || Box::new(MemoryAssetReader { root: root.clone() })),
        );
        app.insert_resource(self.clone());
    }
}

/// A sky's description and light, as they arrive.
#[derive(Clone, Debug, Default)]
pub struct SkyFiles {
    /// Its sun and horizon.
    pub def: Option<SkyDef>,
    /// Its light.
    pub light: Option<SkyLight>,
}

/// The sky a `skies/<name>/...` path belongs to.
fn sky_name(path: &str) -> String {
    path.split('/').nth(1).unwrap_or_default().to_string()
}

/// The asset source name for content files (`content://characters/body_male.gltf`).
pub const CONTENT_SOURCE: &str = "content";

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
    Abilities(Vec<AbilityDef>),
    /// A PNG, turned into an image once the download is finished.
    Icon(Vec<u8>),
    /// A WAV, turned into a sound once the download is finished.
    Sound(Vec<u8>),
    GameSounds(GameSounds),
    Skills(Vec<SkillDef>),
    Theme(ThemeDef),
    Layout(Box<UiNode>),
    Styles(UiStyles),
    /// A model file (glTF, its data, or a texture), handed to the asset server.
    Model(Vec<u8>),
    CharacterModels(Box<CharacterModels>),
    /// A PNG of the theme's, turned into an image once the download is finished.
    UiImage(Vec<u8>),
    /// A font of the theme's.
    Font(Vec<u8>),
    /// A sky's description (`skies/<name>/sky.ron`).
    Sky(SkyDef),
    /// A sky's light (`skies/<name>/light.bin`).
    SkyLight(Box<SkyLight>),
    /// Which sky shows when (`skies/cycle.ron`).
    SkyCycle(SkyCycle),
    /// Tree species (`trees/species.ron`).
    TreeSpecies(Vec<Species>),
    /// How trees are drawn (`trees/drawing.ron`).
    TreeDrawing(TreeDrawing),
    /// A tree photograph (bark, or a leaf set), for the tree generator.
    TreePhoto(Arc<Vec<u8>>),
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

/// How far the content download has got, for the loading screen: bytes
/// arrived and bytes wanted (both zero before it starts). Updated by the
/// download as each file arrives.
#[derive(Resource, Clone, Default)]
pub struct ContentProgress(Arc<ProgressCounts>);

/// The counts behind [`ContentProgress`].
#[derive(Default)]
pub struct ProgressCounts {
    done: AtomicU64,
    total: AtomicU64,
}

impl ContentProgress {
    /// Bytes arrived, and bytes wanted.
    pub fn bytes(&self) -> (u64, u64) {
        (self.0.done.load(Ordering::Relaxed), self.0.total.load(Ordering::Relaxed))
    }
}

/// The result of a download: the manifest, and every file (new or kept).
struct Downloaded {
    version: String,
    files: HashMap<String, ContentFile>,
    /// The files that are new or changed since the last download.
    changed: Vec<String>,
}

/// Starts downloading when the player joins, and whenever the server says the
/// content has changed to a version the client doesn't have.
fn start_download_when_needed(
    mut events: MessageReader<FromServer>,
    source: Res<ContentSource>,
    #[cfg(not(target_arch = "wasm32"))] local: Option<Res<LocalContent>>,
    store: Res<ContentStore>,
    mut download: ResMut<ContentDownload>,
    progress: Res<ContentProgress>,
) {
    let needed = events.read().any(|FromServer(message)| match message {
        // (Before joining too, so a new player's character creation has the interface's look.)
        ServerMessage::Welcome { .. } | ServerMessage::CharacterNeeded { .. } => true,
        ServerMessage::ContentChanged { version } => store.version.as_ref() != Some(version),
        _ => false,
    });
    if !needed {
        return;
    }
    let known = store.files.clone();
    let task = match &*source {
        ContentSource::Http { base_url } => {
            IoTaskPool::get().spawn(download_over_http(base_url.clone(), known, progress.clone()))
        }
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

/// How many content files download at once. (Browsers queue what's beyond
/// their own limit, and can share a connection over HTTP/2; on desktop,
/// each is its own connection.)
const PARALLEL_DOWNLOADS: usize = if cfg!(target_arch = "wasm32") { 16 } else { 6 };
/// How many times a content file is tried before giving up.
const DOWNLOAD_ATTEMPTS: u32 = 3;

/// Downloads the manifest, then every file whose hash is new, several at a
/// time (so a distant server's round trips overlap). Each is asked for with
/// its hash, so the browser can keep it for next time.
async fn download_over_http(
    base_url: String,
    known: HashMap<String, ContentFile>,
    progress: ContentProgress,
) -> Result<Downloaded, String> {
    use crate::network::http::{fetch_retrying, start_fetch};
    let manifest_bytes = fetch_retrying(format!("{base_url}/{CLIENT_MANIFEST_FILE}"), DOWNLOAD_ATTEMPTS).await?;
    let manifest = ContentManifest::from_ron(&String::from_utf8_lossy(&manifest_bytes))?;
    let wanted: Vec<&ManifestFile> = changed_files(&manifest, &known)
        .iter()
        .filter_map(|path| manifest.files.iter().find(|f| &f.path == path))
        .collect();
    progress.0.done.store(0, Ordering::Relaxed);
    progress.0.total.store(wanted.iter().map(|file| file.size).sum(), Ordering::Relaxed);
    let url = |file: &ManifestFile| format!("{base_url}/{}?h={}", file.path, file.hash);
    // A few at a time, starting the next as each arrives. (All at once
    // overflowed the server's queue of new connections on desktop, which
    // then turned some away.)
    let mut waiting = wanted.into_iter();
    let mut under_way = std::collections::VecDeque::new();
    let mut new_files = HashMap::new();
    loop {
        while under_way.len() < PARALLEL_DOWNLOADS
            && let Some(file) = waiting.next()
        {
            under_way.push_back((file, start_fetch(url(file))));
        }
        let Some((file, download)) = under_way.pop_front() else { break };
        let bytes = match download.await {
            Ok(bytes) => bytes,
            Err(_) => fetch_retrying(url(file), DOWNLOAD_ATTEMPTS - 1).await?,
        };
        progress.0.done.fetch_add(file.size, Ordering::Relaxed);
        new_files.insert(file.path.clone(), bytes);
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
    let changed = new_files.keys().cloned().collect();
    for file in &manifest.files {
        let parsed = match (known.remove(&file.path).filter(|kept| kept.hash == file.hash), new_files.get(&file.path)) {
            (Some(kept), _) => kept,
            (None, Some(bytes)) => parse_file(&file.path, &file.hash, bytes)?,
            (None, None) => return Err(format!("{} is missing", file.path)),
        };
        files.insert(file.path.clone(), parsed);
    }
    Ok(Downloaded { version: manifest.version.clone(), files, changed })
}

/// Parses a client file, according to the folder it's in.
fn parse_file(path: &str, hash: &str, bytes: &[u8]) -> Result<ContentFile, String> {
    let text = || String::from_utf8_lossy(bytes);
    let data = match path.split('/').next() {
        Some("appearances") => FileData::Appearances(parse_appearances(&text())?),
        Some("items") => FileData::Items(parse_items(&text())?),
        Some("abilities") => FileData::Abilities(parse_abilities(&text())?),
        Some("icons") => FileData::Icon(bytes.to_vec()),
        Some("sounds") => FileData::Sound(bytes.to_vec()),
        Some("characters") if path.ends_with(".ron") => {
            FileData::CharacterModels(Box::new(parse_character_models(&text())?))
        }
        Some("characters") => FileData::Model(bytes.to_vec()),
        // (Loaded by the asset server, like models: `content://textures/grass/colour.jpg`.)
        Some("textures") => FileData::Model(bytes.to_vec()),
        // (The picture is loaded by the asset server: `content://skies/day/sky.jpg`.)
        Some("skies") if path == SKY_CYCLE_FILE => FileData::SkyCycle(parse_sky_cycle(&text())?),
        Some("skies") if path.ends_with("sky.ron") => FileData::Sky(parse_sky(&text())?),
        Some("skies") if path.ends_with("light.bin") => FileData::SkyLight(Box::new(SkyLight::from_bytes(bytes)?)),
        Some("skies") => FileData::Model(bytes.to_vec()),
        Some("sounds.ron") => FileData::GameSounds(parse_game_sounds(&text())?),
        Some("skills") => FileData::Skills(parse_skills(&text())?),
        Some("trees") if path == TREE_SPECIES_FILE => FileData::TreeSpecies(parse_species(&text())?),
        Some("trees") if path == TREE_DRAWING_FILE => FileData::TreeDrawing(parse_tree_drawing(&text())?),
        Some("trees") => FileData::TreePhoto(Arc::new(bytes.to_vec())),
        Some("ui") if path.starts_with("ui/layouts/") => FileData::Layout(Box::new(parse_layout(&text())?)),
        Some("ui") if path == "ui/styles.ron" => FileData::Styles(parse_styles(&text())?),
        Some("ui") if path.ends_with(".ron") => FileData::Theme(parse_theme(&text())?),
        Some("ui") if path.ends_with(".png") => FileData::UiImage(bytes.to_vec()),
        Some("ui") => FileData::Font(bytes.to_vec()),
        _ => return Err(format!("{path}: the client doesn't know this kind of file")),
    };
    Ok(ContentFile { hash: hash.to_string(), data })
}

/// Puts a finished download into the store.
fn finish_download(
    mut download: ResMut<ContentDownload>,
    mut store: ResMut<ContentStore>,
    mut images: ResMut<Assets<Image>>,
    mut audio: ResMut<Assets<AudioSource>>,
    assets: Res<ContentAssets>,
) {
    let Some(task) = download.0.as_mut() else { return };
    let Some(result) = check_ready(task) else { return };
    download.0 = None;
    match result {
        Ok(downloaded) => {
            store.appearances.clear();
            store.items.clear();
            store.abilities.clear();
            store.icons.clear();
            store.sounds.clear();
            store.game_sounds = GameSounds::default();
            store.skills.clear();
            store.theme = ThemeDef::default();
            store.ui_images.clear();
            store.fonts.clear();
            store.layouts.clear();
            store.ui_styles = None;
            store.character_models = None;
            store.skies.clear();
            store.sky_cycle = None;
            store.tree_species.clear();
            store.tree_drawing = TreeDrawing::default();
            store.tree_photos.clear();
            store.changed_models =
                downloaded.changed.iter().filter(|path| path.starts_with("characters/")).cloned().collect();
            for (path, file) in &downloaded.files {
                match &file.data {
                    FileData::Appearances(appearances) => {
                        store.appearances.extend(appearances.iter().map(|a| (a.id.clone(), a.clone())));
                    }
                    FileData::Items(items) => store.items.extend(items.iter().map(|i| (i.id.clone(), i.clone()))),
                    FileData::Abilities(abilities) => {
                        store.abilities.extend(abilities.iter().map(|ability| (ability.id.clone(), ability.clone())));
                    }
                    FileData::Icon(png) => match decode_icon(png) {
                        Ok(image) => {
                            let name = path.rsplit('/').next().unwrap_or(path).to_string();
                            store.icons.insert(name, images.add(image));
                        }
                        Err(error) => warn!("Couldn't read the icon {path}: {error}"),
                    },
                    FileData::Sound(wav) => {
                        let name = path.rsplit('/').next().unwrap_or(path).to_string();
                        store.sounds.insert(name, audio.add(AudioSource { bytes: wav.as_slice().into() }));
                    }
                    FileData::GameSounds(sounds) => store.game_sounds = sounds.clone(),
                    FileData::Skills(skills) => {
                        store.skills.extend(skills.iter().map(|skill| (skill.id.clone(), skill.clone())));
                    }
                    FileData::Theme(theme) => store.theme = theme.clone(),
                    FileData::Layout(root) => {
                        let name = path.rsplit('/').next().unwrap_or(path).trim_end_matches(".ron").to_string();
                        store.layouts.insert(name, (**root).clone());
                    }
                    FileData::Styles(styles) => store.ui_styles = Some(styles.clone()),
                    FileData::Model(bytes) => assets.0.insert_asset(Path::new(path), bytes.clone()),
                    FileData::CharacterModels(models) => store.character_models = Some((**models).clone()),
                    FileData::UiImage(png) => match decode_ui_image(png) {
                        Ok(image) => {
                            let name = path.rsplit('/').next().unwrap_or(path).to_string();
                            store.ui_images.insert(name, images.add(image));
                        }
                        Err(error) => warn!("Couldn't read the interface image {path}: {error}"),
                    },
                    FileData::Font(bytes) => {
                        let name = path.rsplit('/').next().unwrap_or(path).to_string();
                        store.fonts.insert(name, Font::from_bytes(bytes.clone()));
                    }
                    FileData::Sky(def) => store.skies.entry(sky_name(path)).or_default().def = Some(def.clone()),
                    FileData::SkyCycle(cycle) => store.sky_cycle = Some(cycle.clone()),
                    FileData::TreeSpecies(species) => store.tree_species = species.clone(),
                    FileData::TreeDrawing(drawing) => store.tree_drawing = drawing.clone(),
                    FileData::TreePhoto(bytes) => {
                        store.tree_photos.insert(path.clone(), bytes.clone());
                    }
                    FileData::SkyLight(light) => {
                        store.skies.entry(sky_name(path)).or_default().light = Some((**light).clone());
                    }
                }
            }
            store.files = downloaded.files;
            store.version = Some(downloaded.version);
            info!("Content updated: {} appearances, {} items.", store.appearances.len(), store.items.len());
        }
        Err(error) => error!("Couldn't download the content: {error}"),
    }
}

/// Turns one of the theme's PNGs into an image (smoothly sampled, since frames
/// are stretched).
fn decode_ui_image(png: &[u8]) -> Result<Image, String> {
    Image::from_buffer(
        png,
        ImageType::Extension("png"),
        CompressedImageFormats::NONE,
        true,
        ImageSampler::linear(),
        RenderAssetUsages::RENDER_WORLD,
    )
    .map_err(|error| error.to_string())
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
