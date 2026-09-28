//! Generated structures: dry stone walls and fences along drawn paths (see
//! `docs/plan-walls.md`). Like the tree generator, it knows nothing of Bevy
//! or the world generator.
//!
//! - An author describes a wall ([`WallDef`], in
//!   `world_authoring/walls.ron`): the points its path passes through, its
//!   kind, height and seed.
//! - The bake turns it into a [`BakedWall`] ([`bake_wall`]): the path
//!   sampled evenly ([`path`]), with the ground's height at each sample and
//!   where it's open, and works out the cells it blocks ([`wall_cells`]).
//! - Clients (and the editor) build its meshes ([`walls::build_wall`]).

pub mod buildings;
pub mod mesh;
pub mod path;
pub(crate) mod random;
pub mod walls;

use glam::Vec2;
use serde::{Deserialize, Serialize};
pub use venture_world::{BakedWall, WALL_SAMPLE_SPACING, WallKind, WallSample};

pub use mesh::MeshPart;
pub use walls::{Stretch, build_wall};

/// The authored walls file, in the world authoring folder.
pub const WALLS_FILE: &str = "walls.ron";
/// How far into a wall's path (either way) a gap reaches past what makes it
/// (such as a road's edge), in metres, so the wall stops short of it.
const GAP_MARGIN: f32 = 1.0;
/// The lowest a wall can be, in metres...
pub const MIN_HEIGHT: f32 = 0.3;
/// ...and the tallest.
pub const MAX_HEIGHT: f32 = 6.0;

/// A wall (or fence) as an author describes it.
///
/// ```ron
/// (id: "oakwengate_wall", kind: DryStone, points: [(2290.0, 2130.0), (2370.0, 2135.0)],
///  closed: false, height: 1.3, seed: 7)
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
pub struct WallDef {
    /// Its id.
    pub id: String,
    /// What kind of wall it is.
    #[serde(default)]
    pub kind: WallKind,
    /// The points its path passes through, in metres (the curve runs smoothly through them).
    pub points: Vec<(f32, f32)>,
    /// Whether it comes back round from its last point to its first.
    #[serde(default)]
    pub closed: bool,
    /// How tall it is, in metres.
    #[serde(default = "default_height")]
    pub height: f32,
    /// Its seed: the same seed always builds the same wall.
    #[serde(default)]
    pub seed: u64,
}

fn default_height() -> f32 {
    1.3
}

/// Reads the authored walls file.
pub fn parse_walls(text: &str) -> Result<Vec<WallDef>, String> {
    ron::from_str(text).map_err(|error| error.to_string())
}

/// Problems with authored walls: ids missing or used twice, too few
/// points, or heights out of range (the empty list if there are none).
pub fn check_walls(defs: &[WallDef]) -> Vec<String> {
    let mut problems = Vec::new();
    for (i, def) in defs.iter().enumerate() {
        let name = if def.id.trim().is_empty() { format!("wall {}", i + 1) } else { format!("wall \"{}\"", def.id) };
        if def.id.trim().is_empty() {
            problems.push(format!("{name} has no id"));
        } else if defs[..i].iter().any(|other| other.id == def.id) {
            problems.push(format!("{name} is there twice"));
        }
        let needed = if def.closed { 3 } else { 2 };
        if def.points.len() < needed {
            problems.push(format!("{name} needs at least {needed} points"));
        }
        if !(MIN_HEIGHT..=MAX_HEIGHT).contains(&def.height) {
            problems.push(format!("{name} must be {MIN_HEIGHT} to {MAX_HEIGHT} m tall"));
        }
    }
    problems
}

/// Bakes a wall: its path sampled every [`WALL_SAMPLE_SPACING`] metres, with
/// the ground's height at each sample (`ground`), and open wherever `opens`
/// says something needs a way through (a road), widened by [`GAP_MARGIN`].
pub fn bake_wall(def: &WallDef, ground: impl Fn(f32, f32) -> f32, opens: impl Fn(f32, f32) -> bool) -> BakedWall {
    let points: Vec<Vec2> = def.points.iter().map(|&(x, y)| Vec2::new(x, y)).collect();
    let path = path::sample_path(&points, def.closed, WALL_SAMPLE_SPACING);
    let open: Vec<bool> = path.iter().map(|sample| opens(sample.at.x, sample.at.y)).collect();
    let reach = (GAP_MARGIN / WALL_SAMPLE_SPACING).round() as usize;
    let samples = path
        .iter()
        .enumerate()
        .map(|(i, sample)| {
            let near_gap = (i.saturating_sub(reach)..=(i + reach).min(open.len() - 1)).any(|j| open[j]);
            WallSample { x: sample.at.x, y: sample.at.y, ground: ground(sample.at.x, sample.at.y), open: near_gap }
        })
        .collect();
    BakedWall {
        id: def.id.clone(),
        kind: def.kind,
        height: def.height.clamp(MIN_HEIGHT, MAX_HEIGHT),
        seed: def.seed,
        samples,
    }
}

/// The cells (1 m squares, by their corner) a baked wall stands on, so
/// can't be walked through: those within its half-width of its line,
/// except where it's open.
pub fn wall_cells(wall: &BakedWall) -> Vec<(i32, i32)> {
    cells_near(wall, wall.kind.half_width())
}

/// The cells (by their corner) within `reach` metres of a baked wall's line,
/// except where it's open (for clearing trees and rocks from its way).
pub fn cells_near(wall: &BakedWall, reach: f32) -> Vec<(i32, i32)> {
    let mut cells: Vec<(i32, i32)> = Vec::new();
    for pair in wall.samples.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        if a.open || b.open {
            continue;
        }
        // Every cell whose middle is within reach of the line between the samples
        // (and each sample's own cell, so thin fences leave no holes).
        let (from, to) = (Vec2::new(a.x, a.y), Vec2::new(b.x, b.y));
        let low = from.min(to) - Vec2::splat(reach + 1.0);
        let high = from.max(to) + Vec2::splat(reach + 1.0);
        for y in low.y.floor() as i32..=high.y.floor() as i32 {
            for x in low.x.floor() as i32..=high.x.floor() as i32 {
                let middle = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
                let on_line = {
                    let run = to - from;
                    let t = ((middle - from).dot(run) / run.length_squared().max(1e-6)).clamp(0.0, 1.0);
                    middle.distance(from + run * t)
                };
                let holds_sample = [from, to].iter().any(|p| p.x.floor() as i32 == x && p.y.floor() as i32 == y);
                if on_line <= reach || holds_sample {
                    cells.push((x, y));
                }
            }
        }
    }
    cells.sort_unstable();
    cells.dedup();
    cells
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(kind: WallKind) -> WallDef {
        WallDef {
            id: "w".to_string(),
            kind,
            points: vec![(0.0, 0.5), (20.0, 0.5)],
            closed: false,
            height: 1.3,
            seed: 1,
        }
    }

    #[test]
    fn baked_walls_sample_the_ground_and_open_at_roads() {
        let baked = bake_wall(&line(WallKind::DryStone), |x, _| x * 0.1, |x, _| (9.0..11.0).contains(&x));
        assert_eq!(baked.samples.len(), 41);
        assert!((baked.samples[20].ground - 1.0).abs() < 1e-4);
        assert!(baked.samples[20].open, "open where the road is");
        assert!(baked.samples[17].open, "and a little either side");
        assert!(!baked.samples[5].open && !baked.samples[35].open);
    }

    #[test]
    fn walls_block_the_cells_they_stand_on_but_not_their_gaps() {
        let baked = bake_wall(&line(WallKind::Fence), |_, _| 0.0, |x, _| (9.0..11.0).contains(&x));
        let cells = wall_cells(&baked);
        assert!(cells.contains(&(3, 0)) && cells.contains(&(15, 0)));
        assert!(!cells.contains(&(10, 0)), "the gap stays open");
        assert!(!cells.contains(&(3, 2)), "nothing well off the line");
        // Continuous: every metre along (outside the gap) has a blocked cell.
        for x in (0..8).chain(13..20) {
            assert!(cells.iter().any(|&(cx, _)| cx == x), "a hole at {x}");
        }
    }

    #[test]
    fn problems_with_walls_are_found() {
        let mut twice = line(WallKind::Fence);
        twice.closed = true;
        twice.height = 9.0;
        let problems = check_walls(&[line(WallKind::DryStone), twice]);
        assert_eq!(problems.len(), 3, "{problems:?}");
        assert!(check_walls(&[line(WallKind::DryStone)]).is_empty());
    }

    #[test]
    fn the_walls_file_reads() {
        let walls = parse_walls(r#"[(id: "a", kind: Fence, points: [(0.0, 0.0), (5.0, 0.0)], seed: 3)]"#).unwrap();
        assert_eq!((walls[0].kind, walls[0].height, walls[0].closed), (WallKind::Fence, 1.3, false));
    }
}
