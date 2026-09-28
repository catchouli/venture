//! The game's character and monster models, in the editor: the same
//! rendering as the game (from `venture_client`), reading the models straight
//! from the content folder rather than downloading them.

use std::collections::HashMap;
use std::path::Path;

use bevy::asset::io::AssetSourceBuilder;
use bevy::prelude::*;
use venture_client::characters::{humans, model, monsters};
use venture_client::content::{CONTENT_SOURCE, ContentStore, SkyFiles};
use venture_content::characters::{CharacterModels, parse_character_models};
use venture_content::sky::{SKY_CYCLE_FILE, SkyLight, parse_sky, parse_sky_cycle};
use venture_content::{AppearanceDef, ItemDef};

use crate::document::Document;

/// Makes `content://` read the content folder. (Must be done before the
/// asset plugin is added.)
pub fn register_content_source(app: &mut App, content: &Path) {
    let folder = content.canonicalize().unwrap_or_else(|_| content.to_path_buf());
    app.register_asset_source(CONTENT_SOURCE, AssetSourceBuilder::platform_default(&folder.to_string_lossy(), None));
}

/// Shows people and monsters with the game's models.
pub struct LooksPlugin;

impl Plugin for LooksPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((model::CharacterPlugin, humans::HumansPlugin, monsters::MonstersPlugin))
            .add_systems(Startup, load_character_models)
            .add_systems(Update, keep_appearances);
    }
}

/// Gives the game's rendering the character models, as the client would
/// have downloaded them.
fn load_character_models(mut commands: Commands, project: Res<crate::Project>) {
    let path = project.content.join("characters/characters.ron");
    let models = std::fs::read_to_string(&path).ok().and_then(|text| match parse_character_models(&text) {
        Ok(models) => Some(models),
        Err(error) => {
            warn!("The editor can't read {}: {error}", path.display());
            None
        }
    });
    let mut store = ContentStore::default();
    // (Fixed: a new version would make the game load every model again.)
    store.version = Some("editor".to_string());
    store.character_models = models;
    store.skies = read_skies(&project.content);
    store.sky_cycle =
        std::fs::read_to_string(project.content.join(SKY_CYCLE_FILE)).ok().and_then(|text| parse_sky_cycle(&text).ok());
    // The tree species and their photographs, for the world's trees.
    let trees = venture_content::trees::load_trees(&project.content);
    for (file, problem) in trees.problems {
        warn!("{file}: {problem}");
    }
    store.tree_species = trees.species;
    store.tree_drawing = trees.drawing;
    store.tree_photos = trees
        .files
        .into_iter()
        .filter(|(path, _)| path.ends_with(".png") || path.ends_with(".jpg"))
        .map(|(path, bytes)| (path, std::sync::Arc::new(bytes)))
        .collect();
    commands.insert_resource(store);
}

/// Every sky's description and light, from the content folder (their
/// pictures are loaded from `content://`, which reads the folder too).
fn read_skies(content: &Path) -> HashMap<String, SkyFiles> {
    let mut skies = HashMap::new();
    let folders = std::fs::read_dir(content.join("skies")).into_iter().flatten().filter_map(|entry| entry.ok());
    for folder in folders.map(|entry| entry.path()).filter(|path| path.is_dir()) {
        let name = folder.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        let def = std::fs::read_to_string(folder.join("sky.ron")).ok().and_then(|text| parse_sky(&text).ok());
        let light = std::fs::read(folder.join("light.bin")).ok().and_then(|bytes| SkyLight::from_bytes(&bytes).ok());
        skies.insert(name, SkyFiles { def, light });
    }
    skies
}

/// Keeps the appearances the game's rendering sees up to date.
fn keep_appearances(document: Res<Document>, store: Option<ResMut<ContentStore>>, mut seen: Local<Option<u64>>) {
    let Some(mut store) = store else { return };
    if *seen == Some(document.revision) {
        return;
    }
    *seen = Some(document.revision);
    // The character models as they are now (grips being tuned, say): the
    // game's rendering dresses everyone again when their version changes.
    let models =
        document.find::<CharacterModels>().and_then(|(_, models)| models.files().first()?.entries.first().cloned());
    if models.is_some() && models != store.character_models {
        store.character_models = models;
        store.version = Some(format!("editor-{}", document.revision));
    }
    if let Some((_, items)) = document.find::<ItemDef>() {
        store.items = items.files().iter().flat_map(|file| &file.entries).map(|i| (i.id.clone(), i.clone())).collect();
    }
    if let Some((_, appearances)) = document.find::<AppearanceDef>() {
        store.appearances =
            appearances.files().iter().flat_map(|file| &file.entries).map(|a| (a.id.clone(), a.clone())).collect();
    }
}
