//! Finding routes over the loaded chunks' navigation mesh tiles, with
//! [`landmass`]: each tile is an island of an archipelago, and islands are
//! joined where their edges meet (along the chunks' edges). Routes go
//! any-angle, straight across open ground, and round shallow water when
//! there's a dry way that isn't much longer.

use std::collections::HashMap;
use std::sync::Arc;

use landmass::{
    AnimationLink, AnimationLinkId, Archipelago, ArchipelagoOptions, FromAgentRadius, HeightNavigationMesh,
    HeightPolygon, Island, IslandId, NavigationMesh, PathStep, PermittedAnimationLinks, PointSampleDistance3d,
    Transform, Vec3, XYZ,
};
use venture_collision::CHARACTER_RADIUS;
use venture_world::{ChunkPos, NavKind, NavTile};

/// The kind of animation link a drop is.
const DROP: usize = 1;
/// What a drop costs, besides how far it falls (as if walked this far, in metres).
const DROP_COST: f32 = 2.0;
/// How much more walking through shallow water costs than dry ground.
const SHALLOWS_COST: f32 = 3.0;
/// How far from the navigation mesh (across) a route may start, in metres.
const START_ACROSS: f32 = 1.0;
/// How far above or below a route's start the navigation mesh may be, in metres.
const START_UP: f32 = 2.0;
/// How far from the navigation mesh (across) a route may end: it ends at
/// the nearest point that can be stood on, looking this far, then further.
const GOAL_ACROSS: [f32; 3] = [1.0, 8.0, 30.0];
/// How far above or below a route's goal the navigation mesh may be, in
/// metres (goals are given across, at about the height of the start).
const GOAL_UP: f32 = 60.0;
/// A goal that can't be reached is gone as near to as can be: trying points
/// this far apart (in metres) on the way back to the start...
const FALLBACK_STEP: f32 = 2.0;
/// ...but no more than this many.
const MOST_FALLBACKS: usize = 40;

/// A point in the world: `[x, y, height]`, in metres.
pub type Point3 = [f32; 3];

/// The loaded chunks' navigation mesh tiles, joined.
pub struct NavWorld {
    archipelago: Archipelago<XYZ>,
    islands: HashMap<ChunkPos, IslandId>,
    /// Each tile's drops, as landmass's animation links.
    drops: HashMap<ChunkPos, Vec<AnimationLinkId>>,
    /// Whether tiles have come or gone since the islands were last joined.
    changed: bool,
}

impl Default for NavWorld {
    fn default() -> Self {
        Self::new()
    }
}

impl NavWorld {
    /// No tiles at all.
    pub fn new() -> Self {
        let mut options = ArchipelagoOptions::<XYZ>::from_agent_radius(CHARACTER_RADIUS);
        options.point_sample_distance = sampling(START_ACROSS, START_UP);
        let mut archipelago = Archipelago::new(options);
        archipelago
            .set_type_index_cost(kind_index(NavKind::Shallows), SHALLOWS_COST)
            .expect("the shallows' cost is positive");
        Self { archipelago, islands: HashMap::new(), drops: HashMap::new(), changed: false }
    }

    /// Adds (or replaces) a chunk's tile. Returns what's wrong with it, if
    /// landmass won't take it (it's left out).
    pub fn add_tile(&mut self, pos: ChunkPos, tile: &NavTile) -> Result<(), String> {
        self.remove_tile(pos);
        if tile.is_empty() {
            return Ok(());
        }
        let mesh = landmass_mesh(tile).map_err(|error| format!("chunk {pos:?}'s navigation mesh tile: {error}"))?;
        let island = Island::new(Transform { translation: Vec3::ZERO, rotation: 0.0 }, Arc::new(mesh));
        self.islands.insert(pos, self.archipelago.add_island(island));
        let links = tile
            .drops
            .iter()
            .map(|drop| {
                let fall = drop.from[0][2] - drop.to[0][2];
                self.archipelago.add_animation_link(AnimationLink {
                    start_edge: (to_landmass(drop.from[0]), to_landmass(drop.from[1])),
                    end_edge: (to_landmass(drop.to[0]), to_landmass(drop.to[1])),
                    kind: DROP,
                    cost: DROP_COST + fall,
                    bidirectional: false,
                })
            })
            .collect();
        self.drops.insert(pos, links);
        self.changed = true;
        Ok(())
    }

    /// Takes away a chunk's tile (if it's there).
    pub fn remove_tile(&mut self, pos: ChunkPos) {
        if let Some(island) = self.islands.remove(&pos) {
            self.archipelago.remove_island(island);
            self.changed = true;
        }
        for link in self.drops.remove(&pos).into_iter().flatten() {
            self.archipelago.remove_animation_link(link);
        }
    }

    /// Whether a chunk's tile is there.
    pub fn has_tile(&self, pos: ChunkPos) -> bool {
        self.islands.contains_key(&pos)
    }

    /// Joins the tiles along their edges, once tiles have come or gone
    /// (routes only cross the edges of tiles joined).
    pub fn join(&mut self) {
        if std::mem::take(&mut self.changed) {
            self.archipelago.update(0.0);
        }
    }

    /// Whether tiles have come or gone since they were last joined.
    pub fn needs_joining(&self) -> bool {
        self.changed
    }

    /// The nearest point to `at` a character can stand on, if there's one
    /// within `across` metres (and a few up or down).
    pub fn nearest_standing(&self, at: Point3, across: f32) -> Option<Point3> {
        let sample = self.archipelago.sample_point(to_landmass(at), &sampling(across, START_UP)).ok()?;
        Some(from_landmass(sample.point()))
    }

    /// The nearest point to `(x, y)` a character can stand on, at any
    /// height (the lowest, where there are several), if there's one within
    /// `across` metres.
    pub fn standing_near(&self, (x, y): (f32, f32), across: f32) -> Option<Point3> {
        let sample = self.archipelago.sample_point(Vec3::new(x, y, -ANY_HEIGHT), &above(across)).ok()?;
        Some(from_landmass(sample.point()))
    }

    /// A route from `start` to `goal`, as the points to run through (not
    /// `start` itself), ending at the nearest point to `goal` that can be
    /// stood on (looking up to 30 m round it). If there's no way there (it's
    /// fenced in, say), it goes as near as it can: to the first point that
    /// can be reached on the way back from the goal to the start. `None` if
    /// `start` isn't somewhere to stand, or nowhere near the goal can be reached.
    pub fn route(&self, start: Point3, goal: Point3) -> Option<Vec<Point3>> {
        let from = self.archipelago.sample_point(to_landmass(start), &sampling(START_ACROSS, START_UP)).ok()?;
        let distance = flat_distance(start, goal);
        let tries = (distance / FALLBACK_STEP).ceil().min(MOST_FALLBACKS as f32) as usize;
        (0..=tries).find_map(|i| {
            // (From the goal, back towards the start.)
            let t = if tries == 0 { 0.0 } else { i as f32 / tries as f32 };
            let toward = [0, 1, 2].map(|k| goal[k] + (start[k] - goal[k]) * t);
            let reaches: &[f32] = if i == 0 { &GOAL_ACROSS } else { &GOAL_ACROSS[..1] };
            let to = reaches.iter().find_map(|&across| {
                self.archipelago.sample_point(to_landmass(toward), &sampling(across, GOAL_UP)).ok()
            })?;
            let steps = self.archipelago.find_path(&from, &to, &HashMap::new(), PermittedAnimationLinks::All).ok()?;
            Some(points_of(steps, start))
        })
    }
}

/// A path's points to run through (without the start).
fn points_of(steps: Vec<PathStep<XYZ>>, start: Point3) -> Vec<Point3> {
    let mut points = Vec::new();
    for step in steps {
        match step {
            PathStep::Waypoint(point) => points.push(from_landmass(point)),
            PathStep::AnimationLink { start_point, end_point, .. } => {
                points.push(from_landmass(start_point));
                points.push(from_landmass(end_point));
            }
        }
    }
    // (The path starts where it was sampled, beside `start`.)
    if points.first().is_some_and(|first| flat_distance(*first, start) < 0.05) {
        points.remove(0);
    }
    points
}

/// Further than anything's high or low, in metres.
const ANY_HEIGHT: f32 = 10_000.0;

/// How far round and above a point to look for somewhere to stand.
fn above(across: f32) -> PointSampleDistance3d {
    PointSampleDistance3d {
        horizontal_distance: across,
        distance_above: 2.0 * ANY_HEIGHT,
        distance_below: 0.0,
        // (Vertical distances are divided by this: so the nearest across wins.)
        vertical_preference_ratio: 1.0e6,
        animation_link_max_vertical_distance: 0.5,
    }
}

/// How far round a point to look for somewhere to stand.
fn sampling(across: f32, up: f32) -> PointSampleDistance3d {
    PointSampleDistance3d {
        horizontal_distance: across,
        distance_above: up,
        distance_below: up,
        vertical_preference_ratio: 2.0,
        animation_link_max_vertical_distance: 0.5,
    }
}

fn flat_distance(a: Point3, b: Point3) -> f32 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

fn to_landmass([x, y, height]: Point3) -> Vec3 {
    Vec3::new(x, y, height)
}

fn from_landmass(point: Vec3) -> Point3 {
    [point.x, point.y, point.z]
}

/// A kind of ground's type index, in landmass.
fn kind_index(kind: NavKind) -> usize {
    match kind {
        NavKind::Ground => 0,
        NavKind::Shallows => 1,
    }
}

/// A tile as landmass takes it (checked).
fn landmass_mesh(tile: &NavTile) -> Result<landmass::ValidNavigationMesh<XYZ>, landmass::ValidationError> {
    let mut height = HeightNavigationMesh { polygons: Vec::new(), vertices: Vec::new(), triangles: Vec::new() };
    for detail in &tile.detail {
        height.polygons.push(HeightPolygon {
            base_vertex_index: height.vertices.len() as u32,
            vertex_count: detail.vertices.len() as u32,
            base_triangle_index: height.triangles.len() as u32,
            triangle_count: detail.triangles.len() as u32,
        });
        height.vertices.extend(detail.vertices.iter().map(|&v| to_landmass(v)));
        // Anticlockwise from above, as landmass wants.
        height.triangles.extend(detail.triangles.iter().map(|&[a, b, c]| {
            let corner = |i: u8| detail.vertices[i as usize];
            let (p, q, r) = (corner(a), corner(b), corner(c));
            let turn = (q[0] - p[0]) * (r[1] - p[1]) - (q[1] - p[1]) * (r[0] - p[0]);
            if turn < 0.0 { [a, c, b] } else { [a, b, c] }
        }));
    }
    let mesh = NavigationMesh {
        vertices: tile.vertices.iter().map(|&v| to_landmass(v)).collect(),
        polygons: tile.polygons.iter().map(|polygon| polygon.iter().map(|&i| i as usize).collect()).collect(),
        polygon_type_indices: tile.kinds.iter().map(|&kind| kind_index(kind)).collect(),
        height_mesh: (tile.detail.len() == tile.polygons.len()).then_some(height),
    };
    mesh.validate()
}

#[cfg(test)]
mod tests {
    use venture_collision::Collision;
    use venture_world::{BakedWall, CHUNK_SIZE, Chunk, WallKind, WallSample};

    use super::*;
    use crate::build_tile;

    /// Flat ground over 2 × 2 chunks (0 to 128 m each way), with `walls`, and its navigation world.
    fn world(walls: &[BakedWall]) -> (Collision, NavWorld) {
        world_with(walls, &[])
    }

    /// The same, with buildings (in the content's styles) too.
    fn world_with(walls: &[BakedWall], buildings: &[venture_world::BakedBuilding]) -> (Collision, NavWorld) {
        world_of(walls, buildings, |_| {})
    }

    /// The same, with the chunks changed first.
    fn world_of(
        walls: &[BakedWall],
        buildings: &[venture_world::BakedBuilding],
        change: impl Fn(&mut Chunk),
    ) -> (Collision, NavWorld) {
        let styles = venture_structures::buildings::parse_styles(include_str!("../../../content/buildings/styles.ron"))
            .expect("the content's styles parse");
        let mut solid = Collision::new();
        for y in 0..2 {
            for x in 0..2 {
                let mut chunk = Chunk::new(ChunkPos::new(x, y));
                change(&mut chunk);
                solid.add_chunk(&chunk);
            }
        }
        solid.set_structures(walls, buildings, &styles);
        let mut nav = NavWorld::new();
        for y in 0..2 {
            for x in 0..2 {
                let tile = build_tile(&solid, ChunkPos::new(x, y));
                assert!(!tile.is_empty(), "chunk {x}, {y} has somewhere to stand");
                nav.add_tile(ChunkPos::new(x, y), &tile).expect("landmass takes it");
            }
        }
        nav.join();
        (solid, nav)
    }

    fn length(from: Point3, route: &[Point3]) -> f32 {
        let mut total = 0.0;
        let mut at = from;
        for &point in route {
            total += flat_distance(at, point);
            at = point;
        }
        total
    }

    #[test]
    fn open_ground_is_crossed_in_a_straight_line_over_chunks_edges() {
        let (_, nav) = world(&[]);
        let (start, goal) = ([10.0, 10.0, 0.0], [110.0, 120.0, 0.0]);
        let route = nav.route(start, goal).expect("a way across all four tiles");
        let end = *route.last().unwrap();
        assert!(flat_distance(end, goal) < 0.2, "{end:?}");
        let straight = flat_distance(start, goal);
        assert!(length(start, &route) < straight + 0.5, "{route:?}");
    }

    #[test]
    fn walls_are_gone_round_close_to_their_ends() {
        // A wall along x = 64 (the chunks' edge) from y = 0 to y = 100.
        let samples = (0..=100).map(|y| WallSample { x: 64.0, y: y as f32, ground: 0.0, open: false }).collect();
        let wall = BakedWall { id: "wall".into(), kind: WallKind::DryStone, height: 1.3, seed: 0, samples };
        let (_, nav) = world(&[wall]);
        let (start, goal) = ([50.0, 50.0, 0.0], [80.0, 50.0, 0.0]);
        let route = nav.route(start, goal).expect("a way round the wall");
        assert!(flat_distance(*route.last().unwrap(), goal) < 0.2);
        // Round its end at y = 100 (characters are 0.3 m round).
        let turn = route.iter().map(|p| p[1]).fold(0.0_f32, f32::max);
        assert!((100.2..101.0).contains(&turn), "hugs the wall's end: {route:?}");
    }

    #[test]
    fn buildings_are_gone_into_through_their_doors() {
        use venture_world::{BuildingPlan, DoorPlan, RoofShape, WallRef};
        // A 10 × 8 m cottage with its corner at (40, 40), and a door in the middle of its south wall.
        let plan = BuildingPlan {
            id: "cottage".into(),
            style: "stone_cottage".into(),
            floors: 1,
            seed: 1,
            roof: RoofShape::Gable,
            chimney: None,
            outline: vec![(40.0, 40.0), (50.0, 40.0), (50.0, 48.0), (40.0, 48.0)],
            inner_walls: Vec::new(),
            doors: vec![DoorPlan { wall: WallRef::Outer(0), along: 0.5 }],
        };
        let building = venture_world::BakedBuilding { plan, floor: 0.0, lowest: 0.0 };
        let (_, nav) = world_with(&[], &[building]);
        // From behind it (north) to inside: round to the door on the south.
        let (start, goal) = ([45.0, 55.0, 0.0], [45.0, 44.0, 0.0]);
        let route = nav.route(start, goal).expect("a way in");
        assert!(flat_distance(*route.last().unwrap(), goal) < 0.2, "{route:?}");
        let lowest = route.iter().map(|p| p[1]).fold(f32::INFINITY, f32::min);
        assert!(lowest < 40.0, "round to the south side, and in through the door: {route:?}");
        assert!(
            route.iter().any(|p| (p[0] - 45.0).abs() < 1.0 && (39.0..41.5).contains(&p[1])),
            "through the door: {route:?}"
        );
    }

    #[test]
    fn cliffs_are_jumped_down_but_not_climbed() {
        // A cliff 3 m high along x = 90 (from 90 to 91 m), across the whole world.
        let (_, nav) = world_of(&[], &[], |chunk| {
            let (origin_x, _) = chunk.pos.origin_cell();
            for y in 0..=CHUNK_SIZE as usize {
                for x in 0..=CHUNK_SIZE as usize {
                    if origin_x + x as i32 >= 91 {
                        chunk.set_corner_height(x, y, 3.0);
                    }
                }
            }
        });
        let (top, bottom) = ([110.0, 60.0, 3.0], [70.0, 60.0, 0.0]);
        let down = nav.route(top, bottom).expect("a way down");
        assert!(flat_distance(*down.last().unwrap(), bottom) < 0.2, "{down:?}");
        assert!(down.len() < 5, "straight over the edge: {down:?}");
        // Up: no way, so as near as it can: the foot of the cliff.
        let up = nav.route(bottom, top).expect("as near as it can");
        let end = *up.last().unwrap();
        assert!(end[0] < 91.0 && end[2] < 0.5, "at the foot: {end:?}");
    }
}
