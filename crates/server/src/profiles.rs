//! Player profiles: everything saved about a character between sessions.
//!
//! This module is the only code that reads or writes profiles. Each is a RON
//! file named after a hash of the player's login token, so the files don't
//! reveal tokens (see `docs/client-server.md`).
//!
//! Character names are unique (ignoring case): the store keeps an index of
//! every name in use, built from the profiles when it opens, and a new
//! character must claim its name ([`ProfileStore::claim_name`]).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::RwLock;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use venture_protocol::{BodyType, EquipSlot, HotbarEntry, ItemStack};
use venture_rpg::Abilities;

/// Everything saved about a character. Fields added since the first
/// profiles have defaults, so older profiles still load.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    /// The character's name.
    pub name: String,
    /// Where the character is, in metres.
    pub position: (f32, f32),
    /// Their class (a class id), if they've been given one. Characters made
    /// before classes existed get the default class when they next log in.
    #[serde(default)]
    pub class: Option<String>,
    /// Their build (androgynous for characters made before body types existed).
    #[serde(default)]
    pub body: BodyType,
    /// Their level.
    #[serde(default = "first_level")]
    pub level: u32,
    /// Experience towards the next level.
    #[serde(default)]
    pub experience: u64,
    /// Their ability scores (if missing, their class's starting scores).
    #[serde(default)]
    pub stats: Option<Abilities>,
    /// Stat points to spend.
    #[serde(default)]
    pub stat_points: u32,
    /// Skill points to spend.
    #[serde(default)]
    pub skill_points: u32,
    /// The skills they know (if missing, their class's starting skills).
    #[serde(default)]
    pub skills: Option<Vec<String>>,
    /// What they have equipped, slot by slot (if missing, their class's
    /// starting equipment: characters made before equipment existed get it).
    #[serde(default)]
    pub equipment: Option<Vec<(EquipSlot, String)>>,
    /// Their money.
    #[serde(default)]
    pub miasma: u64,
    /// What they carry, slot by slot.
    #[serde(default)]
    pub inventory: Vec<Option<ItemStack>>,
    /// Their quests: (quest id, stage reached), in the order they started them.
    #[serde(default)]
    pub quests: Vec<(String, String)>,
    /// Their hotbar, from key 1 (if missing, the starting one).
    #[serde(default)]
    pub hotbar: Option<Vec<Option<HotbarEntry>>>,
}

fn first_level() -> u32 {
    1
}

impl Profile {
    /// A brand-new character's profile, before class and belongings are added.
    pub fn new(name: String, position: (f32, f32)) -> Self {
        Self {
            name,
            position,
            class: None,
            body: BodyType::default(),
            level: 1,
            experience: 0,
            stats: None,
            stat_points: 0,
            skill_points: 0,
            skills: None,
            equipment: None,
            miasma: 0,
            inventory: Vec::new(),
            quests: Vec::new(),
            hotbar: None,
        }
    }
}

/// Identifies a profile without revealing the token behind it: the hex
/// SHA-256 of the token.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ProfileKey(String);

impl ProfileKey {
    /// The key for a login token. The token must already be validated.
    pub fn from_token(token: &str) -> Self {
        let digest = Sha256::digest(token.as_bytes());
        Self(digest.iter().map(|byte| format!("{byte:02x}")).collect())
    }
}

/// Where profiles are kept.
#[derive(Resource)]
pub struct ProfileStore {
    folder: PathBuf,
    /// Every name in use (lower case), and whose it is.
    names: RwLock<HashMap<String, ProfileKey>>,
}

impl ProfileStore {
    /// A store keeping profiles in `folder`, which is created if needed.
    pub fn new(folder: impl Into<PathBuf>) -> std::io::Result<Self> {
        let folder = folder.into();
        std::fs::create_dir_all(&folder)?;
        let store = Self { folder, names: RwLock::new(HashMap::new()) };
        store.index_names()?;
        Ok(store)
    }

    /// Reads every profile's name into the index. Names used twice (from
    /// before names had to be unique) are reported, and stay taken.
    fn index_names(&self) -> std::io::Result<()> {
        let mut names = self.names.write().expect("the name index isn't poisoned");
        for entry in std::fs::read_dir(&self.folder)? {
            let path = entry?.path();
            if path.extension().is_none_or(|extension| extension != "ron") {
                continue;
            }
            let Some(key) = path.file_stem().map(|stem| ProfileKey(stem.to_string_lossy().into_owned())) else {
                continue;
            };
            let Some(profile) = self.load(&key) else { continue };
            if let Some(other) = names.insert(profile.name.to_lowercase(), key.clone()) {
                warn!("Two characters are called \"{}\" (profiles {} and {}).", profile.name, other.0, key.0);
            }
        }
        Ok(())
    }

    /// Takes a name for a new character, if nobody else has it (ignoring case).
    pub fn claim_name(&self, name: &str, key: &ProfileKey) -> bool {
        let mut names = self.names.write().expect("the name index isn't poisoned");
        match names.get(&name.to_lowercase()) {
            Some(owner) => owner == key,
            None => {
                names.insert(name.to_lowercase(), key.clone());
                true
            }
        }
    }

    /// Loads a profile, if there is one for this key.
    pub fn load(&self, key: &ProfileKey) -> Option<Profile> {
        let text = std::fs::read_to_string(self.path(key)).ok()?;
        match ron::from_str(&text) {
            Ok(profile) => Some(profile),
            Err(error) => {
                error!("Profile {} is damaged and can't be loaded: {error}", key.0);
                None
            }
        }
    }

    /// Saves a profile. Writes to a temporary file first and then renames it,
    /// so a crash mid-save never leaves a half-written profile.
    pub fn save(&self, key: &ProfileKey, profile: &Profile) {
        let text =
            ron::ser::to_string_pretty(profile, ron::ser::PrettyConfig::default()).expect("profiles always serialise");
        let path = self.path(key);
        let temporary = path.with_extension("ron.tmp");
        let result = std::fs::write(&temporary, text).and_then(|()| std::fs::rename(&temporary, &path));
        if let Err(error) = result {
            error!("Couldn't save profile {}: {error}", key.0);
        }
    }

    fn path(&self, key: &ProfileKey) -> PathBuf {
        self.folder.join(format!("{}.ron", key.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_folder(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("venture-profiles-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        folder
    }

    #[test]
    fn profiles_are_saved_and_loaded() {
        let folder = test_folder("save");
        let store = ProfileStore::new(&folder).unwrap();
        let key = ProfileKey::from_token("some-long-test-token-0123456789abcdef");
        assert_eq!(store.load(&key), None);

        let mut profile = Profile::new("Aldric".to_string(), (1.0, 2.0));
        profile.class = Some("fighter".to_string());
        profile.level = 3;
        profile.miasma = 12;
        profile.inventory = vec![None, Some(ItemStack { item: "bread".to_string(), count: 1 })];
        profile.quests = vec![("tomas_pipe".to_string(), "found".to_string())];
        store.save(&key, &profile);
        assert_eq!(store.load(&key), Some(profile));
        std::fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn older_profiles_still_load() {
        let profile: Profile = ron::from_str(r#"(name: "Aldric", position: (1.0, 2.0))"#).unwrap();
        assert!(profile.inventory.is_empty());
        assert_eq!((profile.class, profile.level), (None, 1));
    }

    #[test]
    fn names_are_unique_ignoring_case() {
        let folder = test_folder("names");
        let store = ProfileStore::new(&folder).unwrap();
        let (alice, bob) = (ProfileKey::from_token("alice-token"), ProfileKey::from_token("bob-token"));
        store.save(&alice, &Profile::new("Aldric".to_string(), (0.0, 0.0)));

        // A new store finds the name in the saved profiles.
        let store = ProfileStore::new(&folder).unwrap();
        assert!(!store.claim_name("aldric", &bob));
        assert!(store.claim_name("Aldric", &alice), "your own name is still yours");
        assert!(store.claim_name("Brienne", &bob));
        assert!(!store.claim_name("BRIENNE", &alice));
        std::fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn keys_hide_the_token() {
        let key = ProfileKey::from_token("secret-token");
        assert!(!key.0.contains("secret"));
        assert_eq!(key.0.len(), 64);
    }
}
