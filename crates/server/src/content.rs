//! The content pack on the server: loaded at startup, and reloaded whenever
//! its files change (hot reload). If a reload has problems, they're logged
//! and the old content stays in use.

use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use bevy::prelude::*;
use notify::{RecursiveMode, Watcher};
use venture_content::{ClientContent, ContentPack};
use venture_protocol::ServerMessage;

use crate::entities::Player;
use crate::link::Link;
use crate::scripting::Scripts;

/// How long the files must stay unchanged before reloading, so a burst of
/// saves (or an editor writing several files) causes one reload.
const SETTLE_TIME: Duration = Duration::from_millis(300);

/// The client files, shared with the HTTP server that serves them.
pub type SharedClientContent = Arc<RwLock<ClientContent>>;

/// Everything the server needs to know about its content.
pub struct ContentSetup {
    /// The content folder.
    pub folder: PathBuf,
    /// The content, already loaded.
    pub pack: ContentPack,
    /// Where to publish the client files.
    pub client: SharedClientContent,
    /// Whether to watch the folder and reload on changes.
    pub watch: bool,
}

/// The content in use.
#[derive(Resource)]
pub struct LoadedContent {
    /// The content pack.
    pub pack: ContentPack,
}

/// Watches the content folder and reloads it.
pub struct ContentPlugin;

impl Plugin for ContentPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, reload_when_changed.run_if(resource_exists::<ContentWatch>));
    }
}

/// Installs the content: compiles its scripts and, if asked, starts watching.
pub fn install(app: &mut App, setup: ContentSetup, scripts: &mut Scripts) {
    if let Err(error) = scripts.load(&setup.pack) {
        error!("Couldn't compile the content's scripts: {error}");
    }
    *setup.client.write().expect("the client content lock isn't poisoned") = setup.pack.client.clone();
    app.insert_resource(LoadedContent { pack: setup.pack });
    if setup.watch {
        match ContentWatch::start(setup.folder, setup.client) {
            Ok(watch) => {
                app.insert_resource(watch);
            }
            Err(error) => warn!("Content won't reload automatically: {error}"),
        }
    }
}

/// The file watcher and what it has seen.
#[derive(Resource)]
struct ContentWatch {
    folder: PathBuf,
    client: SharedClientContent,
    changes: async_channel::Receiver<()>,
    /// When the latest change was seen (time since startup), if a reload is due.
    changed_at: Option<Duration>,
    // Kept alive for as long as we want notifications.
    _watcher: notify::RecommendedWatcher,
}

impl ContentWatch {
    fn start(folder: PathBuf, client: SharedClientContent) -> notify::Result<Self> {
        let (report, changes) = async_channel::unbounded();
        let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
            if event.is_ok_and(|event| event.kind.is_create() || event.kind.is_modify() || event.kind.is_remove()) {
                let _ = report.try_send(());
            }
        })?;
        watcher.watch(&folder, RecursiveMode::Recursive)?;
        info!("Watching {} for content changes.", folder.display());
        Ok(Self { folder, client, changes, changed_at: None, _watcher: watcher })
    }
}

/// Reloads the content once its files have settled after a change.
fn reload_when_changed(
    time: Res<Time<Real>>,
    mut watch: ResMut<ContentWatch>,
    mut content: ResMut<LoadedContent>,
    mut scripts: ResMut<Scripts>,
    mut link: ResMut<Link>,
    players: Query<&Player>,
) {
    let now = time.elapsed();
    while watch.changes.try_recv().is_ok() {
        watch.changed_at = Some(now);
    }
    let Some(changed_at) = watch.changed_at else { return };
    if now - changed_at < SETTLE_TIME {
        return;
    }
    watch.changed_at = None;

    let pack = match ContentPack::load(&watch.folder) {
        Ok(pack) => pack,
        Err(errors) => {
            error!("The changed content has {} problem(s); keeping the old content:", errors.len());
            for problem in &errors {
                error!("  {problem}");
            }
            return;
        }
    };
    if let Err(error) = scripts.load(&pack) {
        error!("Couldn't compile the changed scripts; keeping the old content: {error}");
        return;
    }
    let old_version = content.pack.client.manifest.version.clone();
    *watch.client.write().expect("the client content lock isn't poisoned") = pack.client.clone();
    let new_version = pack.client.manifest.version.clone();
    content.pack = pack;
    info!("Content reloaded.");

    if new_version != old_version {
        let everyone: Vec<_> = players.iter().map(|player| player.connection).collect();
        link.broadcast(everyone, &ServerMessage::ContentChanged { version: new_version });
    }
}
