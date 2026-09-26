//! The game's character and creature models, in the editor: the same
//! rendering as the game (from `venture_client`), reading the models straight
//! from the content folder rather than downloading them.

use std::path::Path;

use bevy::asset::io::AssetSourceBuilder;
use bevy::prelude::*;
use venture_client::characters::{creatures, humans, model};
use venture_client::content::{CONTENT_SOURCE, ContentStore};
use venture_content::AppearanceDef;
use venture_content::characters::{CharacterModels, parse_character_models};

use crate::document::Document;

/// Makes `content://` read the content folder. (Must be done before the
/// asset plugin is added.)
pub fn register_content_source(app: &mut App, content: &Path) {
    let folder = content.canonicalize().unwrap_or_else(|_| content.to_path_buf());
    app.register_asset_source(CONTENT_SOURCE, AssetSourceBuilder::platform_default(&folder.to_string_lossy(), None));
}

/// Shows people and creatures with the game's models.
pub struct LooksPlugin;

impl Plugin for LooksPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((model::CharacterPlugin, humans::HumansPlugin, creatures::CreaturesPlugin))
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
    commands.insert_resource(store);
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
    if let Some((_, appearances)) = document.find::<AppearanceDef>() {
        store.appearances =
            appearances.files().iter().flat_map(|file| &file.entries).map(|a| (a.id.clone(), a.clone())).collect();
    }
}
