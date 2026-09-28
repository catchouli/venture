//! Where the baked world comes from: the server over HTTP, or a folder on
//! disk (desktop `--local` mode). The client never generates terrain itself.
//!
//! When the world is baked again (the server says so, or the editor baked
//! it), [`ReloadWorld`] reads the manifest again; once it's here (so world
//! files are asked for with its new fingerprint), [`WorldReloaded`] tells
//! the streaming to load the changed chunks again and the map to redraw.

use std::future::Future;

use bevy::prelude::*;
use bevy::tasks::{IoTaskPool, Task, futures::check_ready};
use venture_protocol::ServerMessage;
use venture_world::{ChunkPos, MANIFEST_FILE, WorldManifest, chunk_file_path};

use crate::network::FromServer;
use crate::network::http::fetch;
use crate::network::launch::ServerChoice;

/// Loads the world manifest, then makes it available as [`WorldInfo`].
pub struct WorldSourcePlugin;

impl Plugin for WorldSourcePlugin {
    fn build(&self, app: &mut App) {
        let source = match app.world().resource::<ServerChoice>() {
            ServerChoice::Remote { base_url } => {
                WorldSource::Http { base_url: format!("{base_url}/world"), version: None }
            }
            #[cfg(not(target_arch = "wasm32"))]
            ServerChoice::Local { world_folder, .. } => WorldSource::Folder(world_folder.clone()),
        };
        app.insert_resource(source)
            .add_message::<ReloadWorld>()
            .add_message::<WorldReloaded>()
            .add_systems(Startup, start_loading_manifest)
            .add_systems(Update, (follow_world_changes, reload_when_asked, finish_loading_manifest).chain());
    }
}

/// Where world files are read from.
#[derive(Resource, Clone)]
pub enum WorldSource {
    /// A web address ending in the world folder, such as `http://host:8080/world`.
    Http {
        /// The address.
        base_url: String,
        /// The world's fingerprint, once its manifest has loaded: other
        /// files are asked for with it (`?v=`), so browsers can keep them
        /// until the world changes.
        version: Option<String>,
    },
    /// A folder on disk (desktop `--local` mode).
    #[cfg(not(target_arch = "wasm32"))]
    Folder(std::path::PathBuf),
}

/// The loaded world's description. Present once the manifest has loaded.
#[derive(Resource)]
pub struct WorldInfo(pub WorldManifest);

/// Why the world couldn't be loaded, to show the player.
#[derive(Resource)]
pub struct WorldLoadError(pub String);

/// The manifest being read: the task, and if it's a reload, the chunks
/// that changed (`None` for all of them).
#[derive(Resource)]
struct ManifestTask(Task<Result<Vec<u8>, String>>, Option<Option<Vec<ChunkPos>>>);

/// Asks for the world to be read again, after it's been baked again: its
/// manifest, and these chunks (`None` for every chunk).
#[derive(Message, Clone, Debug)]
pub struct ReloadWorld {
    /// The chunks that changed, if known.
    pub chunks: Option<Vec<ChunkPos>>,
}

/// The world's manifest has been read again (after [`ReloadWorld`]): these
/// chunks changed (`None` for every chunk).
#[derive(Message, Clone, Debug)]
pub struct WorldReloaded {
    /// The chunks that changed, if known.
    pub chunks: Option<Vec<ChunkPos>>,
}

impl WorldSource {
    /// Reads a file of the world (a path relative to the world folder).
    pub fn read(&self, path: String) -> impl Future<Output = Result<Vec<u8>, String>> + use<> {
        let source = self.clone();
        async move {
            match source {
                WorldSource::Http { base_url, version: Some(version) } => {
                    fetch(format!("{base_url}/{path}?v={version}")).await
                }
                WorldSource::Http { base_url, version: None } => fetch(format!("{base_url}/{path}")).await,
                #[cfg(not(target_arch = "wasm32"))]
                WorldSource::Folder(folder) => {
                    let file = folder.join(path);
                    std::fs::read(&file).map_err(|error| format!("can't read {}: {error}", file.display()))
                }
            }
        }
    }

    /// Reads a chunk's file.
    pub fn read_chunk(&self, pos: ChunkPos) -> impl Future<Output = Result<Vec<u8>, String>> + use<> {
        self.read(chunk_file_path(pos))
    }
}

fn start_loading_manifest(mut commands: Commands, source: Res<WorldSource>) {
    let task = IoTaskPool::get().spawn(source.read(MANIFEST_FILE.to_string()));
    commands.insert_resource(ManifestTask(task, None));
}

/// The server says the world was baked again. (The editor has no server, so no messages.)
fn follow_world_changes(events: Option<MessageReader<FromServer>>, mut reload: MessageWriter<ReloadWorld>) {
    let Some(mut events) = events else { return };
    for FromServer(message) in events.read() {
        if let ServerMessage::WorldChanged { chunks } = message {
            reload.write(ReloadWorld { chunks: Some(chunks.iter().map(|&(x, y)| ChunkPos::new(x, y)).collect()) });
        }
    }
}

/// Reads the manifest again when asked (after the world's been baked again).
fn reload_when_asked(
    mut commands: Commands,
    mut asked: MessageReader<ReloadWorld>,
    source: Res<WorldSource>,
    loading: Option<Res<ManifestTask>>,
) {
    let mut chunks: Option<Option<Vec<ChunkPos>>> = None;
    for ask in asked.read() {
        chunks = Some(match (chunks, &ask.chunks) {
            (Some(Some(mut known)), Some(more)) => {
                known.extend(more);
                Some(known)
            }
            (None, Some(more)) => Some(more.clone()),
            _ => None,
        });
    }
    let Some(chunks) = chunks else { return };
    if loading.is_some() {
        warn!("The world was baked again while its manifest was being read; reading it again.");
    }
    let task = IoTaskPool::get().spawn(source.read(MANIFEST_FILE.to_string()));
    commands.insert_resource(ManifestTask(task, Some(chunks)));
}

fn finish_loading_manifest(
    mut commands: Commands,
    task: Option<ResMut<ManifestTask>>,
    mut source: ResMut<WorldSource>,
    mut reloaded: MessageWriter<WorldReloaded>,
) {
    let Some(mut task) = task else { return };
    let Some(result) = check_ready(&mut task.0) else { return };
    commands.remove_resource::<ManifestTask>();
    if let (Ok(bytes), WorldSource::Http { version, .. }) = (&result, &mut *source) {
        *version = Some(venture_protocol::fingerprint(bytes));
    }
    let manifest = result.and_then(|bytes| {
        let text = String::from_utf8(bytes).map_err(|_| "the world manifest isn't text".to_string())?;
        WorldManifest::from_ron(&text).map_err(|error| error.to_string())
    });
    match manifest {
        Ok(manifest) => {
            commands.insert_resource(WorldInfo(manifest));
            if let Some(chunks) = task.1.take() {
                reloaded.write(WorldReloaded { chunks });
            }
        }
        // (A reload that fails keeps the world as it was.)
        Err(error) if task.1.is_some() => warn!("Couldn't read the world again: {error}"),
        Err(error) => {
            error!("Couldn't load the world: {error}");
            commands.insert_resource(WorldLoadError(error));
        }
    }
}
