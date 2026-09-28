//! The Venture editor: authoring the world and the game's content visually.
//! See `docs/plan-editor.md` and `docs/editor.md`.
//!
//! ```sh
//! cargo run -p venture_editor                               # content/ and world/
//! cargo run -p venture_editor -- --content content --world world --authoring world_authoring
//! ```
//!
//! It edits the content pack's files through the content crate's own types
//! ([`document`]), shows the baked world with the game's own rendering
//! (from `venture_client`), and is laid out by [`shell`]: tabs across the
//! top, a list on the left, properties on the right, problems at the bottom.

// Bevy systems receive everything they use as parameters, so long parameter
// lists and query types are normal here.
#![allow(clippy::too_many_arguments, clippy::type_complexity)]

mod buildings;
mod camera;
mod document;
mod fields;
mod grips;
mod looks;
mod places;
mod previews;
mod screenshot;
mod shell;
mod walls;

use std::path::PathBuf;

use bevy::prelude::*;
use bevy_egui::EguiPlugin;
use venture_client::network::launch::ServerChoice;

/// Where the editor's files are.
#[derive(Resource, Clone, Debug)]
pub struct Project {
    /// The content folder.
    pub content: PathBuf,
    /// The baked world folder.
    pub world: PathBuf,
    /// The world authoring folder (walls), which the bake reads.
    pub authoring: PathBuf,
    /// A copy of the content folder for checking unsaved changes.
    pub mirror: PathBuf,
}

impl Project {
    /// From the command line: `--content <folder>`, `--world <folder>` and `--authoring <folder>`.
    fn from_arguments() -> Self {
        let args: Vec<String> = std::env::args().collect();
        let value_after = |flag: &str| args.iter().position(|arg| arg == flag).and_then(|i| args.get(i + 1)).cloned();
        let mirror = std::env::temp_dir().join(format!("venture-editor-check-{}", std::process::id()));
        Self {
            content: value_after("--content").unwrap_or_else(|| "content".to_string()).into(),
            world: value_after("--world").unwrap_or_else(|| "world".to_string()).into(),
            authoring: value_after("--authoring").unwrap_or_else(|| "world_authoring".to_string()).into(),
            mirror,
        }
    }
}

fn main() {
    let project = Project::from_arguments();
    let document = match document::Document::load_with_authoring(&project.content, Some(&project.authoring)) {
        Ok(document) => document,
        Err(error) => {
            eprintln!("The editor can't read the content in {}: {error}", project.content.display());
            std::process::exit(1);
        }
    };
    if let Err(error) = document::copy_folder(&project.content, &project.mirror) {
        eprintln!("The editor can't make a copy of the content to check it in: {error}");
        std::process::exit(1);
    }

    let mut app = App::new();
    looks::register_content_source(&mut app, &project.content);
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window { title: "Venture editor".into(), ..default() }),
        ..default()
    }))
    // The game's world rendering reads the world from a folder, as `--local` does.
    .insert_resource(ServerChoice::Local { world_folder: project.world.clone(), new_character: false, admin: false })
    .add_plugins(venture_client::world::WorldPlugin)
    .add_plugins(EguiPlugin::default())
    .insert_resource(project)
    .insert_resource(document)
    .add_plugins((
        camera::EditorCameraPlugin,
        shell::ShellPlugin,
        looks::LooksPlugin,
        fields::FieldsPlugin,
        grips::GripsPlugin,
        places::PlacesPlugin,
        screenshot::ScreenshotPlugin,
        walls::WallToolsPlugin,
        buildings::BuildingToolsPlugin,
    ));
    {
        let registry = app.world().resource::<AppTypeRegistry>().clone();
        let mut registry = registry.write();
        for collection in &app.world().resource::<document::Document>().collections {
            collection.register(&mut registry);
        }
    }
    app.add_systems(Last, remove_mirror_on_exit).run();
}

/// Deletes the checking copy of the content when the editor closes.
fn remove_mirror_on_exit(mut exits: MessageReader<AppExit>, project: Res<Project>) {
    if exits.read().next().is_some() {
        let _ = std::fs::remove_dir_all(&project.mirror);
    }
}
