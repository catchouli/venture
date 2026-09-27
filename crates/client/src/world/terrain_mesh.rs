//! Turning chunk data into terrain and liquid meshes.
//!
//! The terrain is a smooth-shaded heightfield: one vertex per cell corner,
//! with normals and colours worked out from the surrounding corners and
//! cells. Each vertex also says how much of it wears each ground texture:
//! grass and dirt in its first UVs, rock in its second (the terrain shader
//! works in world space, so it has no other use for UVs). Near a chunk's edge those belong to the neighbouring chunks, so
//! chunks are meshed with their eight neighbours at hand (a
//! [`Neighbourhood`]). That way lighting and colour blend seamlessly across
//! chunk borders.

use std::sync::Arc;

use bevy::prelude::*;
use venture_world::{CHUNK_SIZE, Chunk, ChunkPos, FluidKind, Ground, TerrainCell};

use crate::common::coords::to_bevy;
use crate::common::mesh_data::MeshData;
use crate::common::palette;

/// Metres of liquid surface covered by one unit of texture coordinates.
const METRES_PER_UV: f32 = 4.0;

/// How far under the ground a liquid's surface sinks past its covered
/// cells (metres), so the ground hides it there.
const SHORE_SINK: f32 = 0.05;

/// A chunk plus its eight neighbours, for looking just past the chunk's edges.
pub struct Neighbourhood {
    /// Indexed `(dy + 1) * 3 + (dx + 1)`; the centre is index 4. Neighbours
    /// outside the world are `None`, and the centre chunk stands in for them.
    chunks: [Option<Arc<Chunk>>; 9],
}

impl Neighbourhood {
    /// Collects the chunk at `centre` and its neighbours using `get`.
    pub fn collect(centre: ChunkPos, get: impl Fn(ChunkPos) -> Option<Arc<Chunk>>) -> Self {
        Self {
            chunks: std::array::from_fn(|i| {
                let (dx, dy) = (i as i32 % 3 - 1, i as i32 / 3 - 1);
                get(ChunkPos::new(centre.x + dx, centre.y + dy))
            }),
        }
    }

    /// The centre chunk.
    pub fn centre(&self) -> &Chunk {
        self.chunks[4].as_deref().expect("the centre chunk is always present")
    }

    /// The chunk holding local position `(x, y)` (which may be just outside
    /// the centre chunk), and the position within it.
    fn locate(&self, x: i32, y: i32) -> (&Chunk, usize, usize) {
        let (dx, dy) = (x.div_euclid(CHUNK_SIZE), y.div_euclid(CHUNK_SIZE));
        let (lx, ly) = (x.rem_euclid(CHUNK_SIZE) as usize, y.rem_euclid(CHUNK_SIZE) as usize);
        let index = ((dy + 1) * 3 + (dx + 1)) as usize;
        match self.chunks.get(index).and_then(Option::as_deref) {
            Some(chunk) => (chunk, lx, ly),
            None => {
                let clamp = |v: i32| v.clamp(0, CHUNK_SIZE - 1) as usize;
                (self.centre(), clamp(x), clamp(y))
            }
        }
    }

    /// Height of corner `(x, y)`, in the centre chunk's local corner coordinates.
    fn corner_height(&self, x: i32, y: i32) -> f32 {
        if (0..=CHUNK_SIZE).contains(&x) && (0..=CHUNK_SIZE).contains(&y) {
            return self.centre().corner_height(x as usize, y as usize);
        }
        let (chunk, lx, ly) = self.locate(x, y);
        chunk.corner_height(lx, ly)
    }

    /// Cell `(x, y)`, in the centre chunk's local cell coordinates.
    fn cell(&self, x: i32, y: i32) -> &TerrainCell {
        let (chunk, lx, ly) = self.locate(x, y);
        chunk.cell(lx, ly)
    }
}

/// The ground mesh for the centre chunk, in chunk-local coordinates.
pub fn terrain_mesh(area: &Neighbourhood) -> Mesh {
    let chunk = area.centre();
    let (origin_x, origin_y) = chunk.pos.origin_cell();
    let corners = CHUNK_SIZE + 1;
    let mut mesh = MeshData::default();
    // How much of each corner is rock (the other weights are in its UVs).
    let mut rockiness: Vec<[f32; 2]> = Vec::new();

    for y in 0..corners {
        for x in 0..corners {
            let height = area.corner_height(x, y);
            // Slope from the neighbouring corners on each side.
            let slope_x = area.corner_height(x + 1, y) - area.corner_height(x - 1, y);
            let slope_y = area.corner_height(x, y + 1) - area.corner_height(x, y - 1);
            let normal = to_bevy(-slope_x, -slope_y, 2.0).normalize();

            // Blend the four cells that share this corner.
            let cells = [(x - 1, y - 1), (x, y - 1), (x - 1, y), (x, y)];
            let colour = cells
                .iter()
                .map(|&(cx, cy)| palette::ground_colour(area.cell(cx, cy), origin_x + cx, origin_y + cy))
                .fold(Vec4::ZERO, |sum, colour| sum + colour)
                / 4.0;

            // How much of the corner wears each ground texture.
            let share = |texture: GroundTexture| {
                let count = cells.iter().filter(|&&(cx, cy)| ground_texture(area.cell(cx, cy).ground) == Some(texture));
                count.count() as f32 / 4.0
            };
            let weights = Vec2::new(share(GroundTexture::Grass), share(GroundTexture::Dirt));
            rockiness.push([share(GroundTexture::Rock), 0.0]);
            mesh.vertex(to_bevy(x as f32, y as f32, height), normal, colour.to_array(), weights);
        }
    }

    let index = |x: i32, y: i32| (y * corners + x) as u32;
    for y in 0..CHUNK_SIZE {
        for x in 0..CHUNK_SIZE {
            let (i00, i10, i01, i11) = (index(x, y), index(x + 1, y), index(x, y + 1), index(x + 1, y + 1));
            let [h00, h10, h01, h11] = chunk.cell_corner_heights(x as usize, y as usize);
            // Split along the diagonal whose ends are closest in height, so
            // ridges and valleys keep their shape.
            if (h00 - h11).abs() < (h10 - h01).abs() {
                mesh.triangle(i00, i01, i11);
                mesh.triangle(i00, i11, i10);
            } else {
                mesh.triangle(i00, i01, i10);
                mesh.triangle(i10, i01, i11);
            }
        }
    }
    mesh.into_mesh().with_inserted_attribute(Mesh::ATTRIBUTE_UV_1, rockiness)
}

/// The ground textures (see `terrain_material`), in the order the shader has them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroundTexture {
    /// Grass and meadows.
    Grass,
    /// Bare earth and paths.
    Dirt,
    /// Rock and basalt (and, in the shader, any steep slope).
    Rock,
}

/// The texture a kind of ground wears, if any (the rest get procedural detail).
pub fn ground_texture(ground: Ground) -> Option<GroundTexture> {
    match ground {
        Ground::Grass | Ground::MeadowGrass => Some(GroundTexture::Grass),
        Ground::Dirt | Ground::Path => Some(GroundTexture::Dirt),
        Ground::Rock | Ground::Basalt => Some(GroundTexture::Rock),
        Ground::Sand | Ground::Snow | Ground::Ash => None,
    }
}

/// The surfaces of `kind` of liquid in the centre chunk, or `None` if there
/// is none. In chunk-local coordinates.
///
/// Each surface also spreads one cell past the covered cells, and the
/// terrain hides the part that ends up under ground. Each vertex carries how
/// deep the liquid is there (in its second UVs), and the water shader fades
/// out as that nears nothing, so shorelines follow the true line where
/// ground meets the surface, softly, instead of the edges of cells.
///
/// The surface is one continuous sheet: each corner of a covered cell is at
/// the average level of the covered cells around it, so still water is flat
/// and streams slope smoothly downhill (rather than in steps, one per cell,
/// which showed as a grid of seams). The corners past them, which no
/// covered cell touches, sink to the ground (or stay at the liquid's level
/// where the ground rises above it): a cell the world says is dry never
/// shows liquid, even where a bank is lower than the river beside it.
/// Corners on a chunk's edge see the same cells from either side, so
/// neighbouring chunks' surfaces meet.
pub fn fluid_mesh(area: &Neighbourhood, kind: FluidKind) -> Option<Mesh> {
    // Each covered cell's level, for the chunk's cells and a ring of two
    // cells around them.
    let side = (CHUNK_SIZE + 4) as usize;
    let at = |x: i32, y: i32| (y + 2) as usize * side + (x + 2) as usize;
    let mut levels = vec![None; side * side];
    for y in -2..=CHUNK_SIZE + 1 {
        for x in -2..=CHUNK_SIZE + 1 {
            levels[at(x, y)] = area.cell(x, y).fluid.filter(|fluid| fluid.kind == kind).map(|fluid| fluid.level);
        }
    }
    // The highest level in a block of cells.
    let highest = |xs: std::ops::RangeInclusive<i32>, ys: std::ops::RangeInclusive<i32>| {
        ys.flat_map(|y| xs.clone().map(move |x| (x, y))).filter_map(|(x, y)| levels[at(x, y)]).reduce(f32::max)
    };

    let mut mesh = MeshData::default();
    let white = [1.0; 4];
    // How deep the liquid is over the ground at each vertex.
    let mut depths: Vec<[f32; 2]> = Vec::new();
    // Each corner's vertex, made the first time a cell needs it.
    let corners = (CHUNK_SIZE + 1) as usize;
    let mut vertices: Vec<Option<u32>> = vec![None; corners * corners];
    let mut corner = |mesh: &mut MeshData, x: i32, y: i32| {
        *vertices[y as usize * corners + x as usize].get_or_insert_with(|| {
            let ground = area.corner_height(x, y);
            let around = [(x - 1, y - 1), (x, y - 1), (x - 1, y), (x, y)];
            let (sum, count) = around
                .iter()
                .filter_map(|&(cx, cy)| levels[at(cx, cy)])
                .fold((0.0, 0.0), |(sum, count), level| (sum + level, count + 1.0));
            let level = if count > 0.0 {
                sum / count
            } else {
                // Past the edge: just under the ground, but no higher than
                // the liquid it spreads from.
                let near = highest(x - 2..=x + 1, y - 2..=y + 1).unwrap_or(ground);
                (ground - SHORE_SINK).min(near)
            };
            depths.push([level - ground, 0.0]);
            let (fx, fy) = (x as f32, y as f32);
            mesh.vertex(to_bevy(fx, fy, level), Vec3::Y, white, Vec2::new(fx, fy) / METRES_PER_UV)
        })
    };
    for y in 0..CHUNK_SIZE {
        for x in 0..CHUNK_SIZE {
            if highest(x - 1..=x + 1, y - 1..=y + 1).is_none() {
                continue;
            }
            let (i00, i10) = (corner(&mut mesh, x, y), corner(&mut mesh, x + 1, y));
            let (i01, i11) = (corner(&mut mesh, x, y + 1), corner(&mut mesh, x + 1, y + 1));
            // Split the way the ground under it is (see `terrain_mesh`), so
            // the depth is linear on each triangle, and the shader's fade
            // meets the ground exactly where the surface does.
            let [h00, h10, h01, h11] = area.centre().cell_corner_heights(x as usize, y as usize);
            if (h00 - h11).abs() < (h10 - h01).abs() {
                mesh.triangle(i00, i01, i11);
                mesh.triangle(i00, i11, i10);
            } else {
                mesh.triangle(i00, i01, i10);
                mesh.triangle(i10, i01, i11);
            }
        }
    }
    (!mesh.is_empty()).then(|| mesh.into_mesh().with_inserted_attribute(Mesh::ATTRIBUTE_UV_1, depths))
}

#[cfg(test)]
mod tests {
    use bevy::mesh::VertexAttributeValues;
    use venture_world::Fluid;

    use super::*;

    /// A river three cells wide (x 10 to 12) at level 1 in a channel at -1,
    /// through land at 0: lower than the river, but dry.
    fn river() -> Neighbourhood {
        let mut chunk = Chunk::new(ChunkPos::new(0, 0));
        for y in 0..CHUNK_SIZE as usize {
            for x in 10..=12 {
                chunk.cell_mut(x, y).fluid = Some(Fluid { kind: FluidKind::Water, level: 1.0 });
            }
        }
        for y in 0..=CHUNK_SIZE as usize {
            for x in 10..=13 {
                chunk.set_corner_height(x, y, -1.0);
            }
        }
        let chunk = Arc::new(chunk);
        Neighbourhood::collect(ChunkPos::new(0, 0), |pos| (pos == chunk.pos).then(|| chunk.clone()))
    }

    #[test]
    fn water_shows_only_over_covered_cells() {
        let mesh = fluid_mesh(&river(), FluidKind::Water).expect("there is water");
        let Some(VertexAttributeValues::Float32x3(positions)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else {
            panic!("positions")
        };
        let Some(VertexAttributeValues::Float32x2(depths)) = mesh.attribute(Mesh::ATTRIBUTE_UV_1) else {
            panic!("depths")
        };
        for (position, [depth, _]) in positions.iter().zip(depths) {
            let x = position[0];
            // The sheet spreads one cell past the river, each side.
            assert!((9.0..=14.0).contains(&x), "a vertex at x {x}");
            if (10.0..=13.0).contains(&x) {
                // The river's own corners: at its level, two metres deep.
                assert_eq!((position[1], *depth), (1.0, 2.0));
            } else {
                // Past it, under the ground (though the ground is lower
                // than the river), so the dry land shows no water.
                assert!(*depth < 0.0 && position[1] < 0.0, "depth {depth} at x {x}");
            }
        }
    }
}
