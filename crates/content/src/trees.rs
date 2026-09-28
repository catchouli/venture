//! Tree species (`trees/species.ron`, for the tree generator: see
//! `venture_trees` and `docs/plan-trees.md`), and the photographs they
//! name, all sent to clients.

use std::collections::BTreeSet;
use std::path::Path;

use venture_trees::photos::Photos;
pub use venture_trees::{Species, parse_species};

/// The tree species file.
pub const TREE_SPECIES_FILE: &str = "trees/species.ron";
/// The largest a tree photograph may be, in bytes.
const MAX_PHOTO_BYTES: usize = 4 << 20;

/// The tree species read from a content folder.
#[derive(Default)]
pub struct LoadedTrees {
    /// The species.
    pub species: Vec<Species>,
    /// Every problem found, as a file and a message.
    pub problems: Vec<(String, String)>,
    /// The files clients need (the species, and each photograph once), by path.
    pub files: Vec<(String, Vec<u8>)>,
}

/// Reads and checks the tree species, if the pack has any.
pub fn load_trees(folder: &Path) -> LoadedTrees {
    let mut problems = Vec::new();
    let Ok(text) = std::fs::read_to_string(folder.join(TREE_SPECIES_FILE)) else {
        return LoadedTrees::default();
    };
    let species = match parse_species(&text) {
        Ok(species) => species,
        Err(message) => {
            problems.push((TREE_SPECIES_FILE.to_string(), message));
            return LoadedTrees { problems, ..LoadedTrees::default() };
        }
    };
    let mut files = vec![(TREE_SPECIES_FILE.to_string(), text.into_bytes())];
    let mut ids = BTreeSet::new();
    let mut photos_sent = BTreeSet::new();
    for tree in &species {
        let mut problem = |message: String| problems.push((TREE_SPECIES_FILE.to_string(), message));
        if tree.id.is_empty() || !ids.insert(tree.id.clone()) {
            problem(format!("tree species ids must be unique and not empty (\"{}\")", tree.id));
        }
        let (shortest, tallest) = tree.height;
        if !(shortest > 0.0 && shortest <= tallest) {
            problem(format!("{}: heights must be positive, the shortest first", tree.id));
        }
        if tree.branches.len() > 3 {
            problem(format!("{}: trees have at most three levels of branches", tree.id));
        }
        // Its photographs: there, not too big, and readable (with leaves in a leaf set).
        let named = tree.bark.texture.iter().chain(tree.leaves.photo.iter().map(|photo| &photo.file));
        let mut readable = true;
        for file in named {
            match std::fs::read(folder.join(file)) {
                Ok(bytes) if bytes.len() > MAX_PHOTO_BYTES => {
                    problem(format!("{file} must be at most {} MB", MAX_PHOTO_BYTES >> 20));
                    readable = false;
                }
                Ok(bytes) => {
                    if photos_sent.insert(file.clone()) {
                        files.push((file.clone(), bytes));
                    }
                }
                Err(_) => {
                    problem(format!("{}: there's no {file}", tree.id));
                    readable = false;
                }
            }
        }
        if readable {
            match Photos::load(tree, folder) {
                Ok(photos) if photos.leaves.as_ref().is_some_and(|leaves| leaves.leaves.is_empty()) => {
                    problem(format!("{}: no leaves were found in its leaf set", tree.id));
                }
                Ok(_) => {}
                Err(message) => problem(message),
            }
        }
    }
    LoadedTrees { species, problems, files }
}
