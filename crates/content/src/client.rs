//! The part of the content pack clients download: presentation data only
//! (appearances, items, abilities and their icons), never NPC definitions, scripts or
//! dialogue, which stay on the server.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::definitions::{AbilityDef, AppearanceDef, GameSounds, ItemDef};

/// Where the manifest is served, relative to the content address.
pub const CLIENT_MANIFEST_FILE: &str = "manifest.ron";

/// The files clients download, and the manifest describing them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ClientContent {
    /// Each file's path (relative to the content folder) and bytes.
    pub files: Vec<(String, Vec<u8>)>,
    /// The manifest describing them.
    pub manifest: ContentManifest,
}

/// Describes the client files, so a client can tell what changed.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ContentManifest {
    /// A hash of all the files together: changes whenever any file does.
    pub version: String,
    /// Each file.
    pub files: Vec<ManifestFile>,
}

/// One file in a [`ContentManifest`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ManifestFile {
    /// Path relative to the content folder, with `/` separators.
    pub path: String,
    /// SHA-256 of the file, in hex.
    pub hash: String,
}

impl ClientContent {
    /// Describes a set of client files.
    pub fn new(mut files: Vec<(String, Vec<u8>)>) -> Self {
        files.sort_by(|a, b| a.0.cmp(&b.0));
        let described: Vec<ManifestFile> = files
            .iter()
            .map(|(path, bytes)| ManifestFile { path: path.clone(), hash: hex(&Sha256::digest(bytes)) })
            .collect();
        let mut all = Sha256::new();
        for file in &described {
            all.update(file.path.as_bytes());
            all.update(file.hash.as_bytes());
        }
        let manifest = ContentManifest { version: hex(&all.finalize()), files: described };
        Self { files, manifest }
    }

    /// The bytes of a file, if it's one of the client files.
    pub fn file(&self, path: &str) -> Option<&[u8]> {
        self.files.iter().find(|(file, _)| file == path).map(|(_, bytes)| bytes.as_slice())
    }
}

impl ContentManifest {
    /// Reads a manifest from RON text.
    pub fn from_ron(text: &str) -> Result<Self, String> {
        ron::from_str(text).map_err(|error| error.to_string())
    }

    /// Writes the manifest as RON text.
    pub fn to_ron(&self) -> String {
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default()).expect("a manifest always serialises")
    }
}

/// Reads an appearances file: a list of [`AppearanceDef`]s.
pub fn parse_appearances(text: &str) -> Result<Vec<AppearanceDef>, String> {
    ron::from_str(text).map_err(|error| error.to_string())
}

/// Reads an items file: a list of [`ItemDef`]s.
pub fn parse_items(text: &str) -> Result<Vec<ItemDef>, String> {
    ron::from_str(text).map_err(|error| error.to_string())
}

/// Reads a abilities file: a list of [`AbilityDef`]s.
pub fn parse_abilities(text: &str) -> Result<Vec<AbilityDef>, String> {
    ron::from_str(text).map_err(|error| error.to_string())
}

/// Reads the game sounds file (`sounds.ron`): a [`GameSounds`].
pub fn parse_game_sounds(text: &str) -> Result<GameSounds, String> {
    ron::from_str(text).map_err(|error| error.to_string())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifests_change_when_files_do() {
        let a = ClientContent::new(vec![("appearances/a.ron".to_string(), b"one".to_vec())]);
        let b = ClientContent::new(vec![("appearances/a.ron".to_string(), b"two".to_vec())]);
        assert_ne!(a.manifest.version, b.manifest.version);
        assert_eq!(ContentManifest::from_ron(&a.manifest.to_ron()).unwrap(), a.manifest);
        assert_eq!(a.file("appearances/a.ron"), Some(&b"one"[..]));
        assert_eq!(a.file("npcs/secret.ron"), None);
    }
}
