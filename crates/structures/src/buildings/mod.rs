//! Buildings (see `docs/plan-buildings.md`): a baked building's plan,
//! built in its style into meshes.
//!
//! The building is worked out in its own frame (see [`plan`]), then built
//! in stages:
//! 1. the walls, storey by storey, with their doors and windows cut out:
//!    stones laid in courses with quoins at the corners, or overlapping
//!    boards with corner posts, on a stone plinth, plastered inside
//!    ([`faces`]);
//! 2. the doors, windows, floors and inner walls ([`details`]);
//! 3. the roof: a gable or hip over each wing, thick thatch with a rolled
//!    ridge, and gable walls under it, and the chimney ([`roof`]).
//!
//! The seed varies what the style allows: the roof's pitch, colours,
//! windows, which way doors open, and where the chimney goes.
//!
//! Meshes come by material ([`Surface`]), by detail (full, or far away:
//! flat walls with dark windows), and by whether they're *upper* (above the
//! ground floor: the roof, upper floors and chimney), which fade away while
//! a player's inside. Positions are relative to [`BuildingMeshes::origin`]
//! (the building's first corner, at its floor), in the world's directions.

pub mod details;
pub mod faces;
pub mod plan;
pub mod roof;
pub mod style;

use std::collections::BTreeMap;

use glam::{Vec2, Vec3};
use venture_world::{BakedBuilding, WallRef};

pub use plan::{Footprint, Opening, OpeningKind, check_plan};
pub use style::{BuildingStyle, WallConstruction, check_styles, parse_styles};

use crate::mesh::{MeshPart, linear};
use crate::random::Random;

/// The authored buildings file, in the world authoring folder.
pub const BUILDINGS_FILE: &str = "buildings.ron";
/// The building styles file, in the content.
pub const STYLES_FILE: &str = "buildings/styles.ron";

/// What a part of a building is made of (each is drawn with its own material).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Surface {
    /// Stone (walls, lintels, chimneys, plinths).
    Stone,
    /// Weatherboarding.
    Boards,
    /// Wood: frames, doors, posts, shutters.
    Wood,
    /// Thatch.
    Thatch,
    /// Limewashed plaster (inside).
    Plaster,
    /// Floorboards.
    Floor,
    /// Dark: glass, and the dark inside of a chimney pot.
    Dark,
}

/// How near a mesh is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Detail {
    /// Close up.
    Full,
    /// Far away.
    Far,
}

/// One mesh of a building.
#[derive(Clone, Debug)]
pub struct BuildingPart {
    /// What it's made of.
    pub surface: Surface,
    /// How near it's drawn.
    pub detail: Detail,
    /// Whether it's above the ground floor (and fades while a player's inside).
    pub upper: bool,
    /// The mesh.
    pub mesh: MeshPart,
}

/// A building, built.
#[derive(Clone, Debug, Default)]
pub struct BuildingMeshes {
    /// Where its meshes are relative to: (x, height, y) in the world.
    pub origin: [f32; 3],
    /// Its meshes.
    pub parts: Vec<BuildingPart>,
}

impl BuildingMeshes {
    /// How many triangles it has close up.
    pub fn triangles(&self, detail: Detail) -> usize {
        self.parts.iter().filter(|part| part.detail == detail).map(|part| part.mesh.triangles()).sum()
    }
}

/// Where a building's pieces go while it's built: meshes by material,
/// detail and whether they're upper, and its frame and colours.
pub struct Builder<'a> {
    /// The building's footprint.
    pub footprint: Footprint,
    /// Its style.
    pub style: &'a BuildingStyle,
    /// The seed's numbers.
    pub random: Random,
    /// How far below its floor its footing goes, in metres.
    pub footing: f32,
    /// How many floors it has.
    pub floors: u32,
    /// Its colours (linear): walls, roof, trim, door, plaster.
    pub colours: Colours,
    meshes: BTreeMap<(Surface, Detail, bool), MeshPart>,
}

/// A building's colours (linear, with full coverage).
#[derive(Clone, Copy, Debug)]
pub struct Colours {
    /// Its walls (stone or boards).
    pub walls: [f32; 4],
    /// Its roof.
    pub roof: [f32; 4],
    /// Its frames and posts.
    pub trim: [f32; 4],
    /// Its doors and shutters.
    pub door: [f32; 4],
    /// Inside.
    pub plaster: [f32; 4],
}

impl Builder<'_> {
    /// The mesh for pieces of `surface`, at `detail`, upper or not.
    pub fn mesh(&mut self, surface: Surface, detail: Detail, upper: bool) -> &mut MeshPart {
        self.meshes.entry((surface, detail, upper)).or_default()
    }

    /// A point of the building's frame (`at`, in metres) `height` above its
    /// floor, relative to its origin, in the world's directions.
    pub fn point(&self, at: Vec2, height: f32) -> Vec3 {
        self.footprint.frame.point(at, height)
    }

    /// A direction in the building's frame, in the world's.
    pub fn direction(&self, way: Vec2) -> Vec3 {
        self.footprint.frame.direction(way)
    }

    /// Where the eaves are: the top of the walls, above the floor.
    pub fn eaves(&self) -> f32 {
        self.floors as f32 * self.style.storey
    }
}

/// Adds a quad wound to face `toward` (whichever way its corners go round).
pub fn quad_facing(mesh: &mut MeshPart, corners: [Vec3; 4], uvs: [[f32; 2]; 4], colour: [f32; 4], toward: Vec3) {
    let normal = (corners[1] - corners[0]).cross(corners[3] - corners[0]);
    if normal.dot(toward) >= 0.0 {
        mesh.quad(corners, uvs, colour);
    } else {
        mesh.quad([corners[0], corners[3], corners[2], corners[1]], [uvs[0], uvs[3], uvs[2], uvs[1]], colour);
    }
}

/// Builds a baked building in its style. `None` if its plan can't be built
/// (see [`check_plan`]).
pub fn build_building(building: &BakedBuilding, style: &BuildingStyle) -> Option<BuildingMeshes> {
    let plan = &building.plan;
    if !check_plan(plan, Some(style)).is_empty() {
        return None;
    }
    let footprint = Footprint::of(plan, style.wall_thickness)?;
    let origin = footprint.frame.origin;
    let mut random = Random::new(plan.seed ^ 0x6275_696c_6469_6e67);
    let pick = |random: &mut Random, (a, b): ((f32, f32, f32), (f32, f32, f32))| {
        let t = random.unit();
        linear([a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t, a.2 + (b.2 - a.2) * t])
    };
    let colours = &style.colours;
    let door = colours.doors.get((random.next_u64() % colours.doors.len().max(1) as u64) as usize).copied();
    let colours = Colours {
        walls: pick(&mut random, colours.walls),
        roof: pick(&mut random, colours.roof),
        trim: pick(&mut random, colours.trim),
        door: linear(<[f32; 3]>::from(door.unwrap_or((0.4, 0.3, 0.22)))),
        plaster: linear(<[f32; 3]>::from(colours.plaster)),
    };
    let mut builder = Builder {
        footprint,
        style,
        random,
        footing: (building.floor - building.lowest).max(0.0) + 0.3,
        floors: plan.floors.max(1),
        colours,
        meshes: BTreeMap::new(),
    };

    // Where every opening goes, wall by wall (outer walls, then inner ones).
    let openings = openings(&mut builder, building);
    faces::outer_walls(&mut builder, &openings.outer);
    details::openings(&mut builder, &openings.outer);
    details::floors(&mut builder);
    details::inner_walls(&mut builder, plan.inner_walls.as_slice(), &openings.inner);
    let chimney = plan.chimney.unwrap_or_else(|| builder.random.unit() < style.chimney.chance);
    roof::roofs(&mut builder, plan.roof, chimney);

    let parts = builder
        .meshes
        .into_iter()
        .filter(|(_, mesh)| !mesh.is_empty())
        .map(|((surface, detail, upper), mesh)| BuildingPart { surface, detail, upper, mesh })
        .collect();
    Some(BuildingMeshes { origin: [origin.x, building.floor, origin.y], parts })
}

/// Every wall's openings.
pub struct Openings {
    /// Each outer wall's (by the outline's edges).
    pub outer: Vec<Vec<Opening>>,
    /// Each inner wall's (doors only).
    pub inner: Vec<Vec<Opening>>,
}

/// Places the doors where the plan puts them, and windows where the style
/// wants them (clear of doors, corners, and inner walls meeting the wall).
fn openings(builder: &mut Builder, building: &BakedBuilding) -> Openings {
    let plan = &building.plan;
    let style = builder.style;
    let count = builder.footprint.outline.len();
    let frame = builder.footprint.frame;
    let doors_on = |wall: WallRef| -> Vec<f32> {
        plan.doors.iter().filter(|door| door.wall == wall).map(|door| door.along).collect()
    };
    let mut outer = Vec::with_capacity(count);
    for i in 0..count {
        let (a, b) = builder.footprint.edge(i);
        let length = a.distance(b);
        let along = (b - a) / length.max(1e-4);
        let mut openings = plan::doors_in(length, &doors_on(WallRef::Outer(i as u32)), style);
        // Where inner walls meet this wall (along it).
        let meets: Vec<(f32, f32)> = plan
            .inner_walls
            .iter()
            .flat_map(|&(from, to)| [from, to])
            .map(|point| frame.local(Vec2::from(point)))
            .filter(|&point| {
                let offset = point - a;
                let across = offset.perp_dot(along).abs();
                across < style.wall_thickness + 0.3 && (-0.3..length + 0.3).contains(&offset.dot(along))
            })
            .map(|point| {
                let s = (point - a).dot(along);
                (s - style.inner_thickness, s + style.inner_thickness)
            })
            .collect();
        let doors: Vec<(f32, f32)> = openings.iter().map(|door| (door.from, door.to)).collect();
        for storey in 0..builder.floors {
            let mut clear = meets.clone();
            if storey == 0 {
                clear.extend(doors.iter().copied());
            }
            openings.extend(plan::windows_in(length, storey, &clear, style, &mut builder.random));
        }
        outer.push(openings);
    }
    let inner = plan
        .inner_walls
        .iter()
        .enumerate()
        .map(|(i, &(from, to))| {
            let length = Vec2::from(from).distance(Vec2::from(to));
            plan::doors_in(length, &doors_on(WallRef::Inner(i as u32)), style)
        })
        .collect();
    Openings { outer, inner }
}

#[cfg(test)]
mod tests {
    use super::*;
    use venture_world::{BuildingPlan, DoorPlan, RoofShape};

    pub fn cottage(style: &str, roof: RoofShape, floors: u32) -> BakedBuilding {
        BakedBuilding {
            plan: BuildingPlan {
                id: "cottage".into(),
                style: style.into(),
                floors,
                seed: 7,
                roof,
                chimney: Some(true),
                outline: vec![(0.0, 0.0), (9.0, 0.0), (9.0, 5.5), (0.0, 5.5)],
                inner_walls: vec![((4.5, 0.0), (4.5, 5.5))],
                doors: vec![
                    DoorPlan { wall: WallRef::Outer(0), along: 0.3 },
                    DoorPlan { wall: WallRef::Inner(0), along: 0.5 },
                ],
            },
            floor: 10.0,
            lowest: 9.6,
        }
    }

    pub fn styles() -> Vec<BuildingStyle> {
        parse_styles(
            r#"[(id: "stone", name: "Stone", walls: Stone),
                (id: "wood", name: "Wood", walls: Timber, wall_thickness: 0.3,
                 window: (shutters: true))]"#,
        )
        .unwrap()
    }

    #[test]
    fn buildings_are_built_in_every_style_and_shape() {
        for style in &styles() {
            for roof in RoofShape::ALL {
                for floors in [1, 2] {
                    let built = build_building(&cottage(&style.id, roof, floors), style).expect("it builds");
                    let full = built.triangles(Detail::Full);
                    let far = built.triangles(Detail::Far);
                    assert!(full > 500 && full < 120_000, "{} {roof:?} {floors}: {full} triangles", style.id);
                    assert!(far > 0 && far * 5 < full, "far away it's lighter ({far} of {full})");
                    assert!(built.parts.iter().any(|part| part.surface == Surface::Thatch && part.upper));
                    assert!(built.parts.iter().any(|part| part.surface == Surface::Floor && !part.upper));
                    for part in &built.parts {
                        let vertices = part.mesh.positions.len();
                        assert!(part.mesh.indices.iter().all(|&i| (i as usize) < vertices));
                        assert_eq!(part.mesh.normals.len(), vertices);
                        assert!(part.mesh.positions.iter().flatten().all(|v| v.is_finite()));
                    }
                }
            }
        }
    }

    #[test]
    fn the_same_seed_builds_the_same_building() {
        let style = &styles()[0];
        let once = build_building(&cottage("stone", RoofShape::Gable, 1), style).unwrap();
        let again = build_building(&cottage("stone", RoofShape::Gable, 1), style).unwrap();
        assert_eq!(once.parts.len(), again.parts.len());
        for (a, b) in once.parts.iter().zip(&again.parts) {
            assert_eq!(a.mesh, b.mesh);
        }
    }

    #[test]
    fn an_l_shaped_building_builds() {
        let style = &styles()[0];
        let mut building = cottage("stone", RoofShape::Gable, 1);
        building.plan.outline = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 5.0), (5.0, 5.0), (5.0, 10.0), (0.0, 10.0)];
        building.plan.inner_walls.clear();
        building.plan.doors.truncate(1);
        let built = build_building(&building, style).unwrap();
        assert!(built.triangles(Detail::Full) > 500);
    }
}
