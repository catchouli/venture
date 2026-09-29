//! The content being edited: every data file of the content pack, loaded
//! into memory as the content crate's own types, and written back as tidy
//! RON, and the world's authored files (walls, in `world_authoring/`),
//! which the bake reads. Changes can be undone and redone, and are checked
//! by the same checks the game runs (on a mirror of the content folder, so
//! nothing is written to the real one until it's saved) and the bake runs.

use std::any::Any;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use bevy::prelude::*;
use bevy::reflect::{GetTypeRegistration, PartialReflect};
use serde::Serialize;
use serde::de::DeserializeOwned;
use venture_content::characters::CharacterModels;
use venture_content::{
    AbilityDef, AppearanceDef, ClassDef, ContentError, ContentPack, ItemDef, MonsterDef, NewCharacterDef, NpcDef,
    QuestDef, RespawnPoint, ShopDef, SkillDef, SpawnerDef,
};
use venture_structures::bridges::BRIDGES_FILE;
use venture_structures::buildings::{BUILDINGS_FILE, BuildingStyle, STYLES_FILE, check_buildings};
use venture_structures::{WALLS_FILE, WallDef, check_walls};
use venture_world::{BridgePlan, BuildingPlan};

/// Changes to the same file this close together (seconds) undo as one (so
/// typing a name is one step, not one per letter).
const MERGE_SECONDS: f64 = 1.0;

/// Something a content file holds a list of.
pub trait Entry: Reflect + Serialize + DeserializeOwned + Clone + GetTypeRegistration + Send + Sync + 'static {
    /// What it's called in the list: its id (or name, for things without one).
    fn key(&self) -> String;
    /// Gives it a new id (for copies).
    fn set_key(&mut self, key: String);
}

macro_rules! entries_with_ids {
    ($($kind:ty),*) => {$(
        impl Entry for $kind {
            fn key(&self) -> String {
                self.id.clone()
            }
            fn set_key(&mut self, key: String) {
                self.id = key;
            }
        }
    )*};
}
entries_with_ids!(
    ClassDef,
    AbilityDef,
    ItemDef,
    SkillDef,
    MonsterDef,
    SpawnerDef,
    NpcDef,
    AppearanceDef,
    ShopDef,
    QuestDef
);

impl Entry for WallDef {
    fn key(&self) -> String {
        self.id.clone()
    }
    fn set_key(&mut self, key: String) {
        self.id = key;
    }
}

impl Entry for BridgePlan {
    fn key(&self) -> String {
        self.id.clone()
    }
    fn set_key(&mut self, key: String) {
        self.id = key;
    }
}

impl Entry for BuildingPlan {
    fn key(&self) -> String {
        self.id.clone()
    }
    fn set_key(&mut self, key: String) {
        self.id = key;
    }
}

impl Entry for BuildingStyle {
    fn key(&self) -> String {
        self.id.clone()
    }
    fn set_key(&mut self, key: String) {
        self.id = key;
    }
}

impl Entry for RespawnPoint {
    fn key(&self) -> String {
        self.name.clone()
    }
    fn set_key(&mut self, key: String) {
        self.name = key;
    }
}

impl Entry for CharacterModels {
    fn key(&self) -> String {
        "Character models".to_string()
    }
    fn set_key(&mut self, _key: String) {}
}

impl Entry for NewCharacterDef {
    fn key(&self) -> String {
        "New characters".to_string()
    }
    fn set_key(&mut self, _key: String) {}
}

/// Where a kind of content lives.
#[derive(Clone, Copy, Debug)]
pub enum Place {
    /// Every `.ron` file in a folder, each a list.
    Folder(&'static str),
    /// One file holding a list.
    List(&'static str),
    /// One file holding a single thing (not a list).
    Single(&'static str),
}

/// One content file and what's in it.
#[derive(Clone)]
pub struct EntryFile<T> {
    /// The file, relative to the content folder.
    pub path: PathBuf,
    /// Its entries, in order.
    pub entries: Vec<T>,
}

/// Every file of one kind of content.
pub struct Collection<T> {
    title: &'static str,
    place: Place,
    files: Vec<EntryFile<T>>,
    /// Whether it's in the world authoring folder (not the content folder).
    authored: bool,
}

/// A kind of content, whatever its type: what the editor's tabs work with.
pub trait AnyCollection: Send + Sync {
    /// Its name, such as "Classes".
    fn title(&self) -> &'static str;
    /// Whether it holds a single thing rather than lists.
    fn is_single(&self) -> bool;
    /// Whether it's in the world authoring folder (not the content folder).
    fn is_authored(&self) -> bool;
    /// Its files (relative paths).
    fn paths(&self) -> Vec<PathBuf>;
    /// Each file's entries' keys.
    fn keys(&self, file: usize) -> Vec<String>;
    /// An entry, to show and edit through reflection.
    fn entry_mut(&mut self, file: usize, index: usize) -> Option<&mut dyn PartialReflect>;
    /// A file as it would be saved.
    fn text(&self, file: usize) -> String;
    /// Replaces a file's entries with those in `text` (for undo and redo).
    fn set_text(&mut self, file: usize, text: &str) -> Result<(), String>;
    /// Adds a copy of an entry just after it, with a new key. Returns its index.
    fn duplicate(&mut self, file: usize, index: usize) -> Option<usize>;
    /// Removes an entry.
    fn remove(&mut self, file: usize, index: usize);
    /// Gives an entry a new key (its id).
    fn rename(&mut self, file: usize, index: usize, key: String);
    /// Moves an entry up (-1) or down (1) in its file. Returns its new index.
    fn shift(&mut self, file: usize, index: usize, by: isize) -> Option<usize>;
    /// Registers the entry type for reflection.
    fn register(&self, registry: &mut bevy::reflect::TypeRegistry);
    /// As `Any`, to get at the typed collection.
    fn as_any(&self) -> &dyn Any;
    /// As `Any`, mutably.
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

impl<T: Entry> Collection<T> {
    /// Reads every file of this kind from the content folder.
    pub fn load(root: &Path, title: &'static str, place: Place) -> Result<Self, String> {
        let mut paths = Vec::new();
        match place {
            Place::Folder(folder) => {
                let mut found: Vec<PathBuf> = std::fs::read_dir(root.join(folder))
                    .map(|entries| entries.filter_map(|entry| entry.ok().map(|entry| entry.path())).collect())
                    .unwrap_or_default();
                found.retain(|path| path.extension().is_some_and(|extension| extension == "ron"));
                found.sort();
                for path in found {
                    let relative = path.strip_prefix(root).map_err(|error| error.to_string())?;
                    paths.push(relative.to_path_buf());
                }
            }
            Place::List(file) | Place::Single(file) => paths.push(PathBuf::from(file)),
        }
        let mut files = Vec::new();
        for path in paths {
            let full = root.join(&path);
            let text = match std::fs::read_to_string(&full) {
                Ok(text) => text,
                // A single file that isn't there yet starts empty.
                Err(_) if !matches!(place, Place::Folder(_)) => String::new(),
                Err(error) => return Err(format!("{}: {error}", path.display())),
            };
            let entries = if text.trim().is_empty() {
                Vec::new()
            } else {
                parse::<T>(&text, matches!(place, Place::Single(_)))
                    .map_err(|error| format!("{}: {error}", path.display()))?
            };
            files.push(EntryFile { path, entries });
        }
        Ok(Self { title, place, files, authored: false })
    }

    /// Makes it one of the world authoring folder's (loaded from there).
    fn authored(mut self) -> Self {
        self.authored = true;
        self
    }
}

impl<T: Entry> Collection<T> {
    /// Its files.
    pub fn files(&self) -> &[EntryFile<T>] {
        &self.files
    }

    /// Its files, to change (then call [`Document::changed`]).
    pub fn files_mut(&mut self) -> &mut [EntryFile<T>] {
        &mut self.files
    }
}

/// Reads a file: a list, or a single thing.
fn parse<T: Entry>(text: &str, single: bool) -> Result<Vec<T>, String> {
    // (As the game reads them: optional values may leave out `Some`.)
    let options = ron::Options::default().with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME);
    if single {
        options.from_str::<T>(text).map(|entry| vec![entry]).map_err(|error| error.to_string())
    } else {
        options.from_str::<Vec<T>>(text).map_err(|error| error.to_string())
    }
}

/// Writes entries as tidy RON: each entry's fields on their own lines, with
/// what's inside a field (lists, colours, placements) kept on one line, and
/// fields at their defaults left out.
fn write<T: Entry>(entries: &[T], single: bool) -> String {
    let config = ron::ser::PrettyConfig::new().indentor("    ".to_string()).new_line("\n".to_string()).depth_limit(1);
    // (RON escapes apostrophes, which don't need it: "Elsie's", not "Elsie\'s".
    // An escaped apostrophe is always a whole `\'`, so this is safe.)
    let entry_text = |entry: &T| {
        ron::ser::to_string_pretty(entry, config.clone())
            .map(|text| text.replace("\\'", "'"))
            .unwrap_or_else(|error| format!("// can't be written: {error}"))
    };
    let mut text = String::new();
    if single {
        text.push_str(&entries.first().map(entry_text).unwrap_or_default());
    } else {
        text.push_str("[\n");
        for entry in entries {
            for line in entry_text(entry).lines() {
                text.push_str("    ");
                text.push_str(line);
                text.push('\n');
            }
            text.pop();
            text.push_str(",\n");
        }
        text.push(']');
    }
    text.push('\n');
    text
}

impl<T: Entry> AnyCollection for Collection<T> {
    fn title(&self) -> &'static str {
        self.title
    }

    fn is_single(&self) -> bool {
        matches!(self.place, Place::Single(_))
    }

    fn is_authored(&self) -> bool {
        self.authored
    }

    fn paths(&self) -> Vec<PathBuf> {
        self.files.iter().map(|file| file.path.clone()).collect()
    }

    fn keys(&self, file: usize) -> Vec<String> {
        self.files.get(file).map(|file| file.entries.iter().map(Entry::key).collect()).unwrap_or_default()
    }

    fn entry_mut(&mut self, file: usize, index: usize) -> Option<&mut dyn PartialReflect> {
        let entry = self.files.get_mut(file)?.entries.get_mut(index)?;
        Some(entry.as_partial_reflect_mut())
    }

    fn text(&self, file: usize) -> String {
        self.files.get(file).map(|file| write(&file.entries, self.is_single())).unwrap_or_default()
    }

    fn set_text(&mut self, file: usize, text: &str) -> Result<(), String> {
        let single = self.is_single();
        let file = self.files.get_mut(file).ok_or("no such file")?;
        file.entries = if text.trim().is_empty() { Vec::new() } else { parse(text, single)? };
        Ok(())
    }

    fn duplicate(&mut self, file: usize, index: usize) -> Option<usize> {
        if self.is_single() {
            return None;
        }
        let taken: Vec<String> = self.files.iter().flat_map(|file| file.entries.iter().map(Entry::key)).collect();
        let entries = &mut self.files.get_mut(file)?.entries;
        let mut copy = entries.get(index)?.clone();
        let base = copy.key();
        let key = (2..).map(|n| format!("{base}_{n}")).find(|key| !taken.contains(key))?;
        copy.set_key(key);
        entries.insert(index + 1, copy);
        Some(index + 1)
    }

    fn remove(&mut self, file: usize, index: usize) {
        if let Some(file) = self.files.get_mut(file)
            && index < file.entries.len()
            && !matches!(self.place, Place::Single(_))
        {
            file.entries.remove(index);
        }
    }

    fn rename(&mut self, file: usize, index: usize, key: String) {
        if let Some(entry) = self.files.get_mut(file).and_then(|file| file.entries.get_mut(index)) {
            entry.set_key(key);
        }
    }

    fn shift(&mut self, file: usize, index: usize, by: isize) -> Option<usize> {
        let entries = &mut self.files.get_mut(file)?.entries;
        let to = index.checked_add_signed(by).filter(|to| *to < entries.len())?;
        entries.swap(index, to);
        Some(to)
    }

    fn register(&self, registry: &mut bevy::reflect::TypeRegistry) {
        registry.register::<T>();
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// A change that can be undone: a file's text before and after.
struct Step {
    collection: usize,
    file: usize,
    before: String,
    after: String,
    at: f64,
}

/// The content being edited.
#[derive(Resource)]
pub struct Document {
    /// The content folder.
    pub root: PathBuf,
    /// The world authoring folder, if its files are being edited too.
    pub authoring: Option<PathBuf>,
    /// Every kind of content, in the order the tabs show them.
    pub collections: Vec<Box<dyn AnyCollection>>,
    /// Each file's text now (as it would be saved), by (collection, file).
    current: HashMap<(usize, usize), String>,
    /// Each file's text as it is on disk.
    on_disk: HashMap<(usize, usize), String>,
    undo: Vec<Step>,
    redo: Vec<Step>,
    /// Goes up by one with every change (so views know to update).
    pub revision: u64,
}

impl Document {
    /// Loads every kind of content the editor edits (but no authored files).
    #[cfg(test)]
    pub fn load(root: &Path) -> Result<Self, String> {
        Self::load_with_authoring(root, None)
    }

    /// Loads every kind of content, and the world's authored files from
    /// `authoring` (if given).
    pub fn load_with_authoring(root: &Path, authoring: Option<&Path>) -> Result<Self, String> {
        let mut collections: Vec<Box<dyn AnyCollection>> = vec![
            Box::new(Collection::<ClassDef>::load(root, "Classes", Place::Folder("classes"))?),
            Box::new(Collection::<AbilityDef>::load(root, "Abilities", Place::Folder("abilities"))?),
            Box::new(Collection::<ItemDef>::load(root, "Items", Place::Folder("items"))?),
            Box::new(Collection::<SkillDef>::load(root, "Skills", Place::Folder("skills"))?),
            Box::new(Collection::<MonsterDef>::load(root, "Monsters", Place::Folder("monsters"))?),
            Box::new(Collection::<SpawnerDef>::load(root, "Spawners", Place::Folder("spawners"))?),
            Box::new(Collection::<NpcDef>::load(root, "NPCs", Place::Folder("npcs"))?),
            Box::new(Collection::<AppearanceDef>::load(root, "Appearances", Place::Folder("appearances"))?),
            Box::new(Collection::<ShopDef>::load(root, "Shops", Place::Folder("shops"))?),
            Box::new(Collection::<QuestDef>::load(root, "Quests", Place::Folder("quests"))?),
            Box::new(Collection::<RespawnPoint>::load(root, "Respawn points", Place::List("respawn_points.ron"))?),
            Box::new(Collection::<NewCharacterDef>::load(root, "New characters", Place::Single("new_characters.ron"))?),
            Box::new(Collection::<CharacterModels>::load(
                root,
                "Character models",
                Place::Single("characters/characters.ron"),
            )?),
            Box::new(Collection::<BuildingStyle>::load(root, "Building styles", Place::List(STYLES_FILE))?),
        ];
        if let Some(folder) = authoring {
            collections
                .push(Box::new(Collection::<WallDef>::load(folder, "Walls", Place::List(WALLS_FILE))?.authored()));
            collections.push(Box::new(
                Collection::<BuildingPlan>::load(folder, "Buildings", Place::List(BUILDINGS_FILE))?.authored(),
            ));
            collections.push(Box::new(
                Collection::<BridgePlan>::load(folder, "Bridges", Place::List(BRIDGES_FILE))?.authored(),
            ));
        }
        let mut document = Self {
            root: root.to_path_buf(),
            authoring: authoring.map(Path::to_path_buf),
            collections,
            current: HashMap::new(),
            on_disk: HashMap::new(),
            undo: Vec::new(),
            redo: Vec::new(),
            revision: 0,
        };
        document.refresh_texts();
        // (What's on disk counts as what was loaded, written tidily, so files
        // are only rewritten once they're changed.)
        document.on_disk = document.current.clone();
        Ok(document)
    }

    fn refresh_texts(&mut self) {
        self.current.clear();
        for (c, collection) in self.collections.iter().enumerate() {
            for f in 0..collection.paths().len() {
                self.current.insert((c, f), collection.text(f));
            }
        }
    }

    /// Notes that a file was changed (after changing it through
    /// [`AnyCollection`]), so it can be undone. `now` is in seconds.
    pub fn changed(&mut self, collection: usize, file: usize, now: f64) {
        let after = self.collections[collection].text(file);
        let before = self.current.insert((collection, file), after.clone()).unwrap_or_default();
        if before == after {
            return;
        }
        self.revision += 1;
        self.redo.clear();
        if let Some(last) = self.undo.last_mut()
            && (last.collection, last.file) == (collection, file)
            && now - last.at < MERGE_SECONDS
        {
            last.after = after;
            last.at = now;
            return;
        }
        self.undo.push(Step { collection, file, before, after, at: now });
    }

    /// Undoes the last change. Returns which (collection, file) it was in.
    /// (Changes made at the same moment, such as a rename touching several
    /// files, undo and redo together.)
    pub fn undo(&mut self) -> Option<(usize, usize)> {
        let at = self.undo.last()?.at;
        let mut place = None;
        while self.undo.last().is_some_and(|step| step.at == at) {
            let step = self.undo.pop()?;
            self.restore(step.collection, step.file, &step.before);
            place = Some((step.collection, step.file));
            self.redo.push(step);
        }
        place
    }

    /// Redoes the last undone change.
    pub fn redo(&mut self) -> Option<(usize, usize)> {
        let at = self.redo.last()?.at;
        let mut place = None;
        while self.redo.last().is_some_and(|step| step.at == at) {
            let step = self.redo.pop()?;
            self.restore(step.collection, step.file, &step.after);
            place = Some((step.collection, step.file));
            self.undo.push(step);
        }
        place
    }

    /// Whether there's anything to undo or redo.
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    /// Whether there's anything to redo.
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    fn restore(&mut self, collection: usize, file: usize, text: &str) {
        if let Err(error) = self.collections[collection].set_text(file, text) {
            error!("Couldn't restore a change: {error}");
        }
        self.current.insert((collection, file), text.to_string());
        self.revision += 1;
    }

    /// One kind of content, by its type, with its place in [`Document::collections`].
    pub fn find<T: Entry>(&self) -> Option<(usize, &Collection<T>)> {
        self.collections
            .iter()
            .enumerate()
            .find_map(|(index, collection)| Some((index, collection.as_any().downcast_ref::<Collection<T>>()?)))
    }

    /// One kind of content, by its type, to change.
    pub fn find_mut<T: Entry>(&mut self) -> Option<(usize, &mut Collection<T>)> {
        self.collections
            .iter_mut()
            .enumerate()
            .find_map(|(index, collection)| Some((index, collection.as_any_mut().downcast_mut::<Collection<T>>()?)))
    }

    /// Files changed since they were saved (relative paths).
    pub fn unsaved(&self) -> Vec<PathBuf> {
        let mut paths: Vec<PathBuf> = self
            .current
            .iter()
            .filter(|(place, text)| self.on_disk.get(place) != Some(text))
            .map(|(&(c, f), _)| self.collections[c].paths()[f].clone())
            .collect();
        paths.sort();
        paths
    }

    /// Whether a file (by collection and number) has unsaved changes.
    pub fn is_unsaved(&self, collection: usize, file: usize) -> bool {
        self.current.get(&(collection, file)) != self.on_disk.get(&(collection, file))
    }

    /// Writes every changed file to `folder` (the content folder, or a
    /// mirror of it), and the world's authored ones to `authoring` (if given).
    fn write_to(&self, folder: &Path, authoring: Option<&Path>, only_changed: bool) -> Result<usize, String> {
        let mut written = 0;
        for (&(c, f), text) in &self.current {
            if only_changed && self.on_disk.get(&(c, f)) == Some(text) {
                continue;
            }
            let base = match (self.collections[c].is_authored(), authoring) {
                (false, _) => folder,
                (true, Some(authoring)) => authoring,
                (true, None) => continue,
            };
            let path = base.join(&self.collections[c].paths()[f]);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
            }
            std::fs::write(&path, text).map_err(|error| format!("{}: {error}", path.display()))?;
            written += 1;
        }
        Ok(written)
    }

    /// Saves every changed file. Returns how many were written.
    pub fn save(&mut self) -> Result<usize, String> {
        let written = self.write_to(&self.root, self.authoring.as_deref(), true)?;
        self.on_disk = self.current.clone();
        Ok(written)
    }

    /// Checks the content as it is now (saved or not), with the game's own
    /// checks, in `mirror`: a copy of the content folder kept for this.
    pub fn check(&self, mirror: &Path) -> Vec<ContentError> {
        if let Err(error) = self.write_to(mirror, None, false) {
            return vec![ContentError { file: PathBuf::new(), message: format!("can't check: {error}") }];
        }
        let mut problems = match ContentPack::load(mirror) {
            Ok(_) => Vec::new(),
            Err(errors) => errors,
        };
        if let Some((_, walls)) = self.find::<WallDef>() {
            for file in walls.files() {
                let styles: Vec<BuildingStyle> = self
                    .find::<BuildingStyle>()
                    .map(|(_, styles)| styles.files().iter().flat_map(|file| file.entries.clone()).collect())
                    .unwrap_or_default();
                if let Some((_, buildings)) = self.find::<BuildingPlan>() {
                    for file in buildings.files() {
                        problems.extend(
                            check_buildings(&file.entries, &styles)
                                .into_iter()
                                .map(|message| ContentError { file: file.path.clone(), message }),
                        );
                    }
                }
                problems.extend(
                    check_walls(&file.entries)
                        .into_iter()
                        .map(|message| ContentError { file: file.path.clone(), message }),
                );
            }
        }
        problems
    }
}

/// Copies a folder (for the checking mirror).
pub fn copy_folder(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_folder(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn content_folder() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content")
    }

    fn scratch(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("venture-editor-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        copy_folder(&content_folder(), &folder).unwrap();
        folder
    }

    /// Saving everything the editor loaded gives the game the same content.
    #[test]
    fn saving_changes_nothing_the_game_reads() {
        let folder = scratch("round-trip");
        let before = ContentPack::load(&folder).expect("the real content is fine");
        let document = Document::load(&folder).unwrap();
        document.write_to(&folder, None, false).unwrap();
        let after = ContentPack::load(&folder).expect("the saved content is fine");
        assert_eq!(before.classes, after.classes);
        assert_eq!(before.abilities, after.abilities);
        assert_eq!(before.items, after.items);
        assert_eq!(before.skills, after.skills);
        assert_eq!(before.monsters, after.monsters);
        assert_eq!(before.spawners, after.spawners);
        assert_eq!(before.npcs, after.npcs);
        assert_eq!(before.appearances, after.appearances);
        assert_eq!(before.shops, after.shops);
        assert_eq!(before.quests, after.quests);
        assert_eq!(before.respawn_points, after.respawn_points);
        assert_eq!(before.new_characters, after.new_characters);
        assert_eq!(before.character_models, after.character_models);
        // Saved files read back the same.
        let again = Document::load(&folder).unwrap();
        for (c, collection) in again.collections.iter().enumerate() {
            for (f, path) in collection.paths().iter().enumerate() {
                let saved = std::fs::read_to_string(folder.join(path)).unwrap();
                assert_eq!(collection.text(f), saved, "{} ({})", path.display(), document.collections[c].title());
            }
        }
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn awkward_text_survives_saving() {
        let mut monster: MonsterDef = ron::from_str(
            r#"(id: "x", name: "X", level: 1, health: 1, armour_class: 1,
                attack: (damage: "1d2", bonus: 0, speed: 1.0), experience: 1,
                look: (shape: Rabbit, main: (0.5, 0.5, 0.5), trim: (0.5, 0.5, 0.5)))"#,
        )
        .unwrap();
        monster.notes = "Say \"hi\" to C:\\path,\nthen leave. It's fine.".to_string();
        let text = write(std::slice::from_ref(&monster), false);
        let back: Vec<MonsterDef> = parse(&text, false).unwrap();
        assert_eq!(back[0].notes, monster.notes, "{text}");
    }

    #[test]
    fn changes_undo_and_redo() {
        let folder = scratch("undo");
        let mut document = Document::load(&folder).unwrap();
        let classes = 0;
        let original = document.collections[classes].keys(0);
        let copy = document.collections[classes].duplicate(0, 0).unwrap();
        document.changed(classes, 0, 0.0);
        assert_eq!(document.collections[classes].keys(0)[copy], format!("{}_2", original[0]));
        assert_eq!(document.unsaved().len(), 1);
        document.undo();
        assert_eq!(document.collections[classes].keys(0), original);
        assert!(document.unsaved().is_empty());
        document.redo();
        assert_eq!(document.collections[classes].keys(0).len(), original.len() + 1);
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn problems_are_found_before_saving() {
        let folder = scratch("check");
        let mirror = scratch("check-mirror");
        let mut document = Document::load(&folder).unwrap();
        assert!(document.check(&mirror).is_empty());
        let classes = 0;
        document.collections[classes].duplicate(0, 0).unwrap();
        let same_ids = document.collections[classes].text(0).replace("_2", "");
        document.collections[classes].set_text(0, &same_ids).unwrap();
        document.changed(classes, 0, 0.0);
        let problems = document.check(&mirror);
        assert!(!problems.is_empty(), "two classes with one id should be a problem");
        let _ = std::fs::remove_dir_all(&folder);
        let _ = std::fs::remove_dir_all(&mirror);
    }
}
