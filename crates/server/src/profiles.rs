//! Player profiles: everything saved about a character between sessions.
//!
//! This module is the only code that reads or writes profiles. Each is a RON
//! file named after a hash of the player's login token, so the files don't
//! reveal tokens (see `docs/client-server.md`).

use std::path::PathBuf;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use venture_protocol::ItemStack;

/// Everything saved about a character.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    /// The character's name.
    pub name: String,
    /// Where the character is, in metres.
    pub position: (f32, f32),
    /// What they carry, slot by slot.
    #[serde(default)]
    pub inventory: Vec<Option<ItemStack>>,
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
}

impl ProfileStore {
    /// A store keeping profiles in `folder`, which is created if needed.
    pub fn new(folder: impl Into<PathBuf>) -> std::io::Result<Self> {
        let folder = folder.into();
        std::fs::create_dir_all(&folder)?;
        Ok(Self { folder })
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

    #[test]
    fn profiles_are_saved_and_loaded() {
        let folder = std::env::temp_dir().join(format!("venture-profiles-test-{}", std::process::id()));
        let store = ProfileStore::new(&folder).unwrap();
        let key = ProfileKey::from_token("some-long-test-token-0123456789abcdef");
        assert_eq!(store.load(&key), None);

        let inventory = vec![None, Some(ItemStack { item: "coins".to_string(), count: 3 })];
        let profile = Profile { name: "Aldric".to_string(), position: (1.0, 2.0), inventory };
        store.save(&key, &profile);
        assert_eq!(store.load(&key), Some(profile));
        std::fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn older_profiles_still_load() {
        let profile: Profile = ron::from_str(r#"(name: "Aldric", position: (1.0, 2.0))"#).unwrap();
        assert!(profile.inventory.is_empty());
    }

    #[test]
    fn keys_hide_the_token() {
        let key = ProfileKey::from_token("secret-token");
        assert!(!key.0.contains("secret"));
        assert_eq!(key.0.len(), 64);
    }
}
