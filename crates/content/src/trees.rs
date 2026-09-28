//! Tree species (`trees/species.ron`, for the tree generator: see
//! `venture_trees` and `docs/plan-trees.md`), and the photographs they
//! name, all sent to clients.

use std::collections::BTreeSet;
use std::path::Path;

use serde::{Deserialize, Serialize};

use venture_trees::photos::Photos;
pub use venture_trees::{Species, parse_species};

/// The tree species file.
pub const TREE_SPECIES_FILE: &str = "trees/species.ron";
/// How the game draws trees.
pub const TREE_DRAWING_FILE: &str = "trees/drawing.ron";

/// How the game draws trees (`trees/drawing.ron`). Distances are in metres
/// from the camera, each a pair: where a change starts and where it's done.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
#[serde(default)]
pub struct TreeDrawing {
    /// Trees fade out close to the camera: gone nearer than the first, whole beyond the second.
    pub camera_fade: (f32, f32),
    /// Where full trees give way to reduced ones.
    pub full_until: (f32, f32),
    /// Where reduced trees give way to impostors.
    pub reduced_until: (f32, f32),
    /// Where impostors fade away.
    pub impostors_until: (f32, f32),
}

impl Default for TreeDrawing {
    fn default() -> Self {
        Self {
            camera_fade: (2.0, 7.0),
            full_until: (45.0, 55.0),
            reduced_until: (110.0, 130.0),
            impostors_until: (330.0, 360.0),
        }
    }
}

/// Reads `trees/drawing.ron`.
pub fn parse_tree_drawing(text: &str) -> Result<TreeDrawing, String> {
    ron::from_str(text).map_err(|error| error.to_string())
}

impl TreeDrawing {
    /// What's wrong with it, if anything (each pair must be in order, and
    /// each level of detail must end where the next begins, or later).
    pub fn problems(&self) -> Vec<String> {
        let pairs = [
            ("camera_fade", self.camera_fade),
            ("full_until", self.full_until),
            ("reduced_until", self.reduced_until),
            ("impostors_until", self.impostors_until),
        ];
        let mut problems: Vec<String> = pairs
            .iter()
            .filter(|(_, (start, end))| !(start.is_finite() && end.is_finite() && 0.0 <= *start && start <= end))
            .map(|(name, _)| format!("{name} must be two distances, the smaller first"))
            .collect();
        if !(self.full_until.1 <= self.reduced_until.0 && self.reduced_until.1 <= self.impostors_until.0) {
            problems.push("the levels of detail must follow one another (full, reduced, impostors)".to_string());
        }
        problems
    }
}
/// The largest a tree photograph may be, in bytes.
const MAX_PHOTO_BYTES: usize = 4 << 20;

/// The tree species read from a content folder.
#[derive(Default)]
pub struct LoadedTrees {
    /// The species.
    pub species: Vec<Species>,
    /// Every problem found, as a file and a message.
    pub problems: Vec<(String, String)>,
    /// The files clients need (the species, how trees are drawn, and each photograph once), by path.
    pub files: Vec<(String, Vec<u8>)>,
    /// How the game draws trees.
    pub drawing: TreeDrawing,
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
    let mut drawing = TreeDrawing::default();
    if let Ok(text) = std::fs::read_to_string(folder.join(TREE_DRAWING_FILE)) {
        match parse_tree_drawing(&text) {
            Ok(read) => {
                problems.extend(read.problems().into_iter().map(|problem| (TREE_DRAWING_FILE.to_string(), problem)));
                drawing = read;
            }
            Err(message) => problems.push((TREE_DRAWING_FILE.to_string(), message)),
        }
        files.push((TREE_DRAWING_FILE.to_string(), text.into_bytes()));
    }
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
    LoadedTrees { species, problems, files, drawing }
}
