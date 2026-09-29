//! Navigation mesh tiles, one per chunk, generated from the world's solid
//! shapes with Recast (by [`rerecast`], a Rust port) at bake time, and
//! stored in the chunk (see `docs/plan-collision.md`).
//!
//! A tile is made from the triangles of every solid thing in the chunk and a
//! border round it ([`Collision::triangles_in`]), so the tiles of
//! neighbouring chunks meet exactly along the chunk's edges, where
//! [`crate::NavWorld`] joins them.

use glam::{UVec3, Vec3A};
use rerecast::{AreaType, Config, DetailNavmesh, HeightfieldBuilder, PolygonNavmesh, TriMesh};
use venture_collision::{CHARACTER_HEIGHT, CHARACTER_RADIUS, CharacterSettings, Collision, Surface};
use venture_world::{CHUNK_SIZE, ChunkPos, NavDetail, NavDrop, NavKind, NavTile};

/// The size of Recast's cells across, in metres: a whole number of them to
/// a chunk (so tiles' edges fall on cell edges), and a power of two (so
/// every corner is exact in `f32` anywhere in the world, and corners in a
/// straight line are exactly in line, as landmass checks).
const CELL_SIZE: f32 = 0.125;
/// The size of Recast's cells upright, in metres.
const CELL_HEIGHT: f32 = 0.1;
/// The steepest walkable slope, in degrees (as characters climb: see `venture_simulation::MovementRules`).
pub const WALKABLE_SLOPE_DEGREES: f32 = 50.0;
/// The highest step walked up, in metres (as characters step up).
pub const WALKABLE_CLIMB: f32 = 0.4;

/// Recast's area for dry ground.
const GROUND: AreaType = AreaType(1);
/// Recast's area for shallow water.
const SHALLOWS: AreaType = AreaType(2);

/// Recast's settings for a chunk's tile.
fn config(pos: ChunkPos) -> Config {
    let walkable_radius = (CHARACTER_RADIUS / CELL_SIZE).ceil() as u16;
    let border_size = walkable_radius + 3;
    let tile_size = (CHUNK_SIZE as f32 / CELL_SIZE).round() as u16;
    let (origin_x, origin_y) = pos.origin_cell();
    let pad = border_size as f32 * CELL_SIZE;
    let aabb = rerecast::Aabb3d {
        min: glam::Vec3::new(origin_x as f32 - pad, 0.0, origin_y as f32 - pad),
        max: glam::Vec3::new(origin_x as f32 + CHUNK_SIZE as f32 + pad, 0.0, origin_y as f32 + CHUNK_SIZE as f32 + pad),
    };
    Config {
        width: tile_size + border_size * 2,
        height: tile_size + border_size * 2,
        tile_size,
        border_size,
        cell_size: CELL_SIZE,
        cell_height: CELL_HEIGHT,
        aabb,
        walkable_slope_angle: WALKABLE_SLOPE_DEGREES.to_radians(),
        walkable_height: (CHARACTER_HEIGHT / CELL_HEIGHT).ceil() as u16,
        walkable_climb: (WALKABLE_CLIMB / CELL_HEIGHT).floor() as u16,
        walkable_radius,
        max_edge_len: walkable_radius * 8,
        max_simplification_error: 1.3,
        min_region_area: 8 * 8,
        merge_region_area: 20 * 20,
        max_vertices_per_polygon: 6,
        detail_sample_dist: CELL_SIZE * 6.0,
        detail_sample_max_error: CELL_HEIGHT,
        contour_flags: Default::default(),
        area_volumes: Vec::new(),
    }
}

/// The navigation mesh tile of the chunk at `pos`, from the solid world
/// (which must have that chunk and those round it loaded, with the walls
/// and buildings). Empty if there's nowhere to stand, or Recast fails.
pub fn build_tile(solid: &Collision, pos: ChunkPos) -> NavTile {
    let mut config = config(pos);
    let (low, high) = (config.aabb.min, config.aabb.max);
    let triangles = solid.triangles_in((low.x, low.z), (high.x, high.z));
    if triangles.triangles.is_empty() {
        return NavTile::default();
    }
    let slope = WALKABLE_SLOPE_DEGREES.to_radians().cos();
    let vertices: Vec<Vec3A> = triangles.vertices.iter().map(|&v| Vec3A::from(v)).collect();
    let areas = triangles
        .triangles
        .iter()
        .zip(&triangles.surfaces)
        .map(|(triangle, surface)| {
            let [a, b, c] = triangle.map(|i| vertices[i as usize]);
            let steep = (b - a).cross(c - a).normalize_or_zero().y <= slope;
            match surface {
                _ if steep => AreaType::NOT_WALKABLE,
                Surface::Ground => GROUND,
                Surface::Shallows => SHALLOWS,
                Surface::Solid => AreaType::NOT_WALKABLE,
            }
        })
        .collect();
    let trimesh =
        TriMesh { vertices, indices: triangles.triangles.iter().map(|&t| UVec3::from(t)).collect(), area_types: areas };
    let Some(bounds) = trimesh.compute_aabb() else { return NavTile::default() };
    config.aabb.min.y = bounds.min.y - 1.0;
    config.aabb.max.y = bounds.max.y + 1.0;
    let mut tile = generate(&trimesh, &config).unwrap_or_default();
    tile.drops = find_drops(&tile, solid, pos);
    tile
}

/// Recast, from triangles to the tile.
fn generate(trimesh: &TriMesh, config: &Config) -> Option<NavTile> {
    let mut heightfield =
        HeightfieldBuilder { aabb: config.aabb, cell_size: config.cell_size, cell_height: config.cell_height }
            .build()
            .ok()?;
    heightfield.rasterize_triangles(trimesh, config.walkable_climb).ok()?;
    heightfield.filter_low_hanging_walkable_obstacles(config.walkable_climb);
    heightfield.filter_ledge_spans(config.walkable_height, config.walkable_climb);
    heightfield.filter_walkable_low_height_spans(config.walkable_height);
    let mut compact = heightfield.into_compact(config.walkable_height, config.walkable_climb).ok()?;
    compact.erode_walkable_area(config.walkable_radius);
    compact.build_distance_field();
    compact.build_regions(config.border_size, config.min_region_area, config.merge_region_area).ok()?;
    let contours = compact.build_contours(config.max_simplification_error, config.max_edge_len, config.contour_flags);
    let polygons = contours.into_polygon_mesh(config.max_vertices_per_polygon).ok()?;
    let detail =
        DetailNavmesh::new(&polygons, &compact, config.detail_sample_dist, config.detail_sample_max_error).ok()?;
    let mut tile = tile_of(&polygons, &detail);
    tidy(&mut tile);
    Some(tile)
}

/// How far apart drops are looked for along an edge, in metres.
const DROP_STEP: f32 = 1.0;
/// How far out from an edge a drop lands (the first that fits), in metres.
const DROP_OUT: [f32; 3] = [0.6, 1.0, 1.5];
/// How much further out than where it can first stand a drop lands, in metres.
const LANDING_CLEAR: f32 = 0.6;
/// The least and most a drop falls, in metres (less is climbed; more is too far).
const DROP_HEIGHTS: std::ops::Range<f32> = 0.6..6.0;

/// The ways down from a tile's edges (where the ground falls away too
/// steeply to walk, but is somewhere to stand below), found in the solid
/// world: one-way links, landing where a character can stand. Edges along
/// the chunk's own edges join the next tile instead, so are left alone.
fn find_drops(tile: &NavTile, solid: &Collision, pos: ChunkPos) -> Vec<NavDrop> {
    use std::collections::HashMap;
    let mut uses: HashMap<(u16, u16), usize> = HashMap::new();
    for polygon in &tile.polygons {
        for i in 0..polygon.len() {
            let (a, b) = (polygon[i], polygon[(i + 1) % polygon.len()]);
            *uses.entry((a.min(b), a.max(b))).or_default() += 1;
        }
    }
    let (origin_x, origin_y) = pos.origin_cell();
    let (low, high) =
        ((origin_x as f32, origin_y as f32), ((origin_x + CHUNK_SIZE) as f32, (origin_y + CHUNK_SIZE) as f32));
    let on_chunk_edge = |a: [f32; 3], b: [f32; 3]| {
        [(0, low.0), (0, high.0), (1, low.1), (1, high.1)]
            .iter()
            .any(|&(axis, line)| a[axis] == line && b[axis] == line)
    };
    let mut drops = Vec::new();
    for polygon in &tile.polygons {
        for i in 0..polygon.len() {
            let (ia, ib) = (polygon[i], polygon[(i + 1) % polygon.len()]);
            if uses[&(ia.min(ib), ia.max(ib))] != 1 {
                continue;
            }
            let (a, b) = (tile.vertices[ia as usize], tile.vertices[ib as usize]);
            if on_chunk_edge(a, b) {
                continue;
            }
            drops.extend(drops_along(a, b, solid));
        }
    }
    drops
}

/// The drops off one outer edge of a polygon (anticlockwise, so outside is on the right).
fn drops_along(a: [f32; 3], b: [f32; 3], solid: &Collision) -> Vec<NavDrop> {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let length = dx.hypot(dy);
    if length < 0.3 {
        return Vec::new();
    }
    let (out_x, out_y) = (dy / length, -dx / length);
    let steps = (length / DROP_STEP).ceil() as usize;
    let settings = CharacterSettings { max_climb_degrees: WALKABLE_SLOPE_DEGREES, ..CharacterSettings::default() };
    // Where each point along the edge lands, if it can.
    let landings: Vec<Option<([f32; 3], [f32; 3])>> = (0..=steps)
        .map(|k| {
            // (Kept a little in from the edge's ends.)
            let t = (0.15 + 0.7 * k as f32 / steps as f32).clamp(0.0, 1.0);
            let from = [a[0] + dx * t, a[1] + dy * t, a[2] + (b[2] - a[2]) * t];
            // Where it can first stand, then a little further out (onto the
            // navigation mesh, which keeps a character's width back from the
            // foot of the drop).
            let landing = |out: f32| {
                let at = (from[0] + out_x * out, from[1] + out_y * out);
                let ground = solid.surface_below(at, from[2] + 0.5, DROP_HEIGHTS.end + 1.0)?;
                let spot = (at.0, at.1, ground);
                let stands = solid.fits(spot) && solid.stands_on_ground(spot, &settings);
                (DROP_HEIGHTS.contains(&(from[2] - ground)) && stands).then_some([at.0, at.1, ground])
            };
            DROP_OUT.iter().find_map(|&out| {
                landing(out)?;
                landing(out + LANDING_CLEAR).map(|to| (from, to))
            })
        })
        .collect();
    // Each run of points in a row that land becomes one drop.
    let mut drops = Vec::new();
    let mut run: Vec<([f32; 3], [f32; 3])> = Vec::new();
    for landing in landings.into_iter().chain([None]) {
        match landing {
            Some(pair) => run.push(pair),
            None if run.len() >= 2 => {
                let (first, last) = (run[0], run[run.len() - 1]);
                drops.push(NavDrop { from: [first.0, last.0], to: [first.1, last.1] });
                run.clear();
            }
            None => run.clear(),
        }
    }
    drops
}

/// Leaves out any polygon sharing an edge that two others already share
/// (which Recast can make where layers of ground overlap, and landmass
/// won't take).
fn tidy(tile: &mut NavTile) {
    use std::collections::HashMap;
    let mut uses: HashMap<(u16, u16), usize> = HashMap::new();
    let keep: Vec<bool> = tile
        .polygons
        .iter()
        .map(|polygon| {
            let edges: Vec<(u16, u16)> = (0..polygon.len())
                .map(|i| {
                    let (a, b) = (polygon[i], polygon[(i + 1) % polygon.len()]);
                    (a.min(b), a.max(b))
                })
                .collect();
            let fits = edges.iter().all(|edge| uses.get(edge).copied().unwrap_or(0) < 2);
            if fits {
                edges.into_iter().for_each(|edge| *uses.entry(edge).or_default() += 1);
            }
            fits
        })
        .collect();
    fn kept<T>(items: Vec<T>, keep: &[bool]) -> Vec<T> {
        items.into_iter().zip(keep).filter(|(_, keep)| **keep).map(|(item, _)| item).collect()
    }
    tile.polygons = kept(std::mem::take(&mut tile.polygons), &keep);
    tile.kinds = kept(std::mem::take(&mut tile.kinds), &keep);
    tile.detail = kept(std::mem::take(&mut tile.detail), &keep);
}

/// Recast's meshes as a tile: world `[x, y, height]`, polygons anticlockwise from above.
fn tile_of(polygons: &PolygonNavmesh, detail: &DetailNavmesh) -> NavTile {
    let origin = polygons.aabb.min;
    let vertices: Vec<[f32; 3]> = polygons
        .vertices
        .iter()
        .map(|v| {
            let x = origin.x + v.x as f32 * polygons.cell_size;
            let height = origin.y + v.y as f32 * polygons.cell_height;
            let y = origin.z + v.z as f32 * polygons.cell_size;
            [x, y, height]
        })
        .collect();
    let stride = polygons.max_vertices_per_polygon as usize;
    let mut tile = NavTile { vertices, ..NavTile::default() };
    for (index, entry) in polygons.polygons.chunks(stride).enumerate() {
        let mut corners: Vec<u16> = entry.iter().copied().take_while(|&i| i != PolygonNavmesh::NO_INDEX).collect();
        if corners.len() < 3 {
            continue;
        }
        // (Recast's are anticlockwise in its own Y-up frame, which is a
        // mirror image of ours: turn them round if they came out clockwise.)
        if signed_area(&tile.vertices, &corners) < 0.0 {
            corners.reverse();
        }
        tile.polygons.push(corners);
        tile.kinds.push(if polygons.areas[index] == SHALLOWS { NavKind::Shallows } else { NavKind::Ground });
        let sub = &detail.meshes[index];
        let vertices = &detail.vertices[sub.base_vertex_index as usize..][..sub.vertex_count as usize];
        let triangles = &detail.triangles[sub.base_triangle_index as usize..][..sub.triangle_count as usize];
        tile.detail.push(NavDetail {
            vertices: vertices.iter().map(|v| [v.x, v.z, v.y]).collect(),
            triangles: triangles.to_vec(),
        });
    }
    tile
}

/// Twice a polygon's area, anticlockwise from above positive. (Measured
/// from its first corner: far from the world's origin, products of whole
/// coordinates lose the small differences that matter.)
fn signed_area(vertices: &[[f32; 3]], corners: &[u16]) -> f32 {
    let origin = vertices[corners[0] as usize];
    let relative = |i: usize| {
        let v = vertices[corners[i % corners.len()] as usize];
        (v[0] - origin[0], v[1] - origin[1])
    };
    (1..corners.len() - 1)
        .map(|i| {
            let (a, b) = (relative(i), relative(i + 1));
            a.0 * b.1 - b.0 * a.1
        })
        .sum()
}
