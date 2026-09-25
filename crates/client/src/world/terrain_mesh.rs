//! Turning chunk data into terrain and liquid meshes.
//!
//! The terrain is a smooth-shaded heightfield: one vertex per cell corner,
//! with normals and colours worked out from the surrounding corners and
//! cells. Near a chunk's edge those belong to the neighbouring chunks, so
//! chunks are meshed with their eight neighbours at hand (a
//! [`Neighbourhood`]). That way lighting and colour blend seamlessly across
//! chunk borders.

use std::sync::Arc;

use bevy::prelude::*;
use venture_world::{CHUNK_SIZE, Chunk, ChunkPos, FluidKind, TerrainCell};

use crate::common::coords::to_bevy;
use crate::common::mesh_data::MeshData;
use crate::common::palette;

/// Metres of ground covered by one unit of texture coordinates.
const METRES_PER_UV: f32 = 4.0;

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

            let (world_x, world_y) = ((origin_x + x) as f32, (origin_y + y) as f32);
            let uv = Vec2::new(world_x, world_y) / METRES_PER_UV;
            mesh.vertex(to_bevy(x as f32, y as f32, height), normal, colour.to_array(), uv);
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
    mesh.into_mesh()
}

/// Flat surfaces for `kind` of liquid in the centre chunk, or `None` if there
/// is none. In chunk-local coordinates.
///
/// Each surface also spreads one cell past the covered cells. The terrain
/// hides the part that ends up under ground, so shorelines follow the true
/// line where ground meets the surface, instead of the edges of cells.
pub fn fluid_mesh(area: &Neighbourhood, kind: FluidKind) -> Option<Mesh> {
    let mut mesh = MeshData::default();
    let white = [1.0; 4];
    for y in 0..CHUNK_SIZE {
        for x in 0..CHUNK_SIZE {
            let level = (-1..=1)
                .flat_map(|dy| (-1..=1).map(move |dx| (x + dx, y + dy)))
                .filter_map(|(nx, ny)| area.cell(nx, ny).fluid)
                .filter(|fluid| fluid.kind == kind)
                .map(|fluid| fluid.level)
                .reduce(f32::max);
            let Some(level) = level else { continue };

            let (fx, fy) = (x as f32, y as f32);
            let mut corner = |dx: f32, dy: f32| {
                let position = to_bevy(fx + dx, fy + dy, level);
                mesh.vertex(position, Vec3::Y, white, Vec2::new(fx + dx, fy + dy) / METRES_PER_UV)
            };
            let (i00, i10, i01, i11) = (corner(0.0, 0.0), corner(1.0, 0.0), corner(0.0, 1.0), corner(1.0, 1.0));
            mesh.triangle(i00, i01, i10);
            mesh.triangle(i10, i01, i11);
        }
    }
    (!mesh.is_empty()).then(|| mesh.into_mesh())
}
