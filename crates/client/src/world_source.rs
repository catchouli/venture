//! Where the baked world comes from: the server over HTTP, or a folder on
//! disk (desktop `--local` mode). The client never generates terrain itself.

use std::future::Future;

use bevy::prelude::*;
use bevy::tasks::{IoTaskPool, Task, futures::check_ready};
use venture_world::{ChunkPos, MANIFEST_FILE, WorldManifest, chunk_file_path};

use crate::launch::ServerChoice;

/// Loads the world manifest, then makes it available as [`WorldInfo`].
pub struct WorldSourcePlugin;

impl Plugin for WorldSourcePlugin {
    fn build(&self, app: &mut App) {
        let source = match app.world().resource::<ServerChoice>() {
            ServerChoice::Remote { base_url } => WorldSource::Http { base_url: format!("{base_url}/world") },
            #[cfg(not(target_arch = "wasm32"))]
            ServerChoice::Local { world_folder } => WorldSource::Folder(world_folder.clone()),
        };
        app.insert_resource(source)
            .add_systems(Startup, start_loading_manifest)
            .add_systems(Update, finish_loading_manifest);
    }
}

/// Where world files are read from.
#[derive(Resource, Clone)]
pub enum WorldSource {
    /// A web address ending in the world folder, such as `http://host:8080/world`.
    Http { base_url: String },
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

#[derive(Resource)]
struct ManifestTask(Task<Result<Vec<u8>, String>>);

impl WorldSource {
    /// Reads a file of the world (a path relative to the world folder).
    pub fn read(&self, path: String) -> impl Future<Output = Result<Vec<u8>, String>> + use<> {
        let source = self.clone();
        async move {
            match source {
                WorldSource::Http { base_url } => fetch(format!("{base_url}/{path}")).await,
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

/// Downloads a file over HTTP.
async fn fetch(url: String) -> Result<Vec<u8>, String> {
    let (done, result) = async_channel::bounded(1);
    ehttp::fetch(ehttp::Request::get(&url), move |response| {
        let _ = done.try_send(response);
    });
    match result.recv().await.map_err(|_| "the download was cancelled".to_string())? {
        Ok(response) if response.ok => Ok(response.bytes),
        Ok(response) => Err(format!("{url}: {} {}", response.status, response.status_text)),
        Err(error) => Err(format!("{url}: {error}")),
    }
}

fn start_loading_manifest(mut commands: Commands, source: Res<WorldSource>) {
    let task = IoTaskPool::get().spawn(source.read(MANIFEST_FILE.to_string()));
    commands.insert_resource(ManifestTask(task));
}

fn finish_loading_manifest(mut commands: Commands, task: Option<ResMut<ManifestTask>>) {
    let Some(mut task) = task else { return };
    let Some(result) = check_ready(&mut task.0) else { return };
    commands.remove_resource::<ManifestTask>();
    let manifest = result.and_then(|bytes| {
        let text = String::from_utf8(bytes).map_err(|_| "the world manifest isn't text".to_string())?;
        WorldManifest::from_ron(&text).map_err(|error| error.to_string())
    });
    match manifest {
        Ok(manifest) => commands.insert_resource(WorldInfo(manifest)),
        Err(error) => {
            error!("Couldn't load the world: {error}");
            commands.insert_resource(WorldLoadError(error));
        }
    }
}
