//! Bakes the world map from a baked world's chunks: tiles at every level of
//! detail, and place names (see `venture_world::MapInfo` for the layout, and
//! [`crate::map_labels`] for the names).
//!
//! The map is drawn from the chunks on disk, not generated again, so it
//! always shows the world as baked. Each metre is one pixel at level 0: the
//! ground's colour, lit from the north-west by its slope, water darker the
//! deeper it is, lava, a dot for each tree, bush and boulder, and a line
//! along each wall and fence (from the manifest). Each level after averages
//! the one before, two pixels by two.

use std::io::Cursor;
use std::path::Path;
use std::sync::atomic::{AtomicI32, Ordering};

use image::codecs::jpeg::JpegEncoder;
use image::{Rgb, RgbImage};
use venture_world::{
    BUSH_SCALE, BakedWall, CHUNK_SIZE, ChunkPos, FluidKind, Ground, MAP_LABELS_FILE, MapInfo, PropKind, WallKind,
    WorldManifest, chunk_file_path, decode_chunk, map_tile_path,
};

use crate::map_labels::{LandFacts, place_names};

/// JPEG quality of the tiles (0 to 100).
const TILE_QUALITY: u8 = 85;

/// Draws the map of the baked world in `folder` and writes its tiles and
/// place names there. Returns its layout, for the manifest.
pub fn bake_map(folder: &Path, manifest: &WorldManifest) -> std::io::Result<MapInfo> {
    let info = MapInfo::for_world(manifest.world_size);
    let (picture, facts) = draw_world(folder, manifest)?;
    write_tiles(folder, &info, picture)?;
    let labels = place_names(manifest, &facts);
    std::fs::write(folder.join(MAP_LABELS_FILE), labels.to_ron())?;
    Ok(info)
}

/// The whole world at a metre a pixel, and what the place names need to
/// know about the land. Chunk rows are drawn on every core at once.
fn draw_world(folder: &Path, manifest: &WorldManifest) -> std::io::Result<(RgbImage, LandFacts)> {
    let size = manifest.world_size as u32;
    let chunks = manifest.chunks_per_side();
    let next_row = AtomicI32::new(0);
    let threads = std::thread::available_parallelism().map_or(4, |count| count.get());
    let bands = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..threads)
            .map(|_| {
                scope.spawn(|| -> std::io::Result<Vec<(i32, Band)>> {
                    let mut done = Vec::new();
                    loop {
                        let row = next_row.fetch_add(1, Ordering::Relaxed);
                        if row >= chunks {
                            return Ok(done);
                        }
                        done.push((row, draw_band(folder, manifest, row)?));
                    }
                })
            })
            .collect();
        workers.into_iter().map(|worker| worker.join().expect("a map worker panicked")).collect::<Result<Vec<_>, _>>()
    })?;

    let mut picture = RgbImage::new(size, size);
    let mut facts = LandFacts::new(manifest.world_size);
    for (row, band) in bands.into_iter().flatten() {
        let top = (row * CHUNK_SIZE) as u32;
        for (y, line) in band.pixels.chunks(size as usize).enumerate() {
            for (x, colour) in line.iter().enumerate() {
                picture.put_pixel(x as u32, top + y as u32, Rgb(*colour));
            }
        }
        facts.merge(&band.facts);
    }
    draw_walls(&mut picture, &manifest.walls);
    Ok((picture, facts))
}

/// Draws walls and fences as lines along their samples (leaving their gaps):
/// dry stone grey and as wide as it is, fences brown and thin.
fn draw_walls(picture: &mut RgbImage, walls: &[BakedWall]) {
    let (width, height) = (picture.width() as i32, picture.height() as i32);
    for wall in walls {
        let (colour, reach) = match wall.kind {
            WallKind::DryStone => (Rgb([84, 80, 74]), 0.7),
            WallKind::Fence => (Rgb([96, 70, 44]), 0.45),
        };
        for pair in wall.samples.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            if a.open || b.open {
                continue;
            }
            // Every pixel whose middle is within reach of the line between the samples.
            let (low_x, high_x) = (a.x.min(b.x) - reach, a.x.max(b.x) + reach);
            let (low_y, high_y) = (a.y.min(b.y) - reach, a.y.max(b.y) + reach);
            for y in (low_y.floor() as i32).max(0)..=(high_y.floor() as i32).min(height - 1) {
                for x in (low_x.floor() as i32).max(0)..=(high_x.floor() as i32).min(width - 1) {
                    let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                    let (run_x, run_y) = (b.x - a.x, b.y - a.y);
                    let t = (((px - a.x) * run_x + (py - a.y) * run_y) / (run_x * run_x + run_y * run_y).max(1e-6))
                        .clamp(0.0, 1.0);
                    let (nx, ny) = (a.x + run_x * t, a.y + run_y * t);
                    if (px - nx).hypot(py - ny) <= reach {
                        picture.put_pixel(x as u32, y as u32, colour);
                    }
                }
            }
        }
    }
}

/// One row of chunks, drawn.
struct Band {
    /// `CHUNK_SIZE` rows of the world's width, top to bottom.
    pixels: Vec<[u8; 3]>,
    /// What it tells the place names.
    facts: LandFacts,
}

fn draw_band(folder: &Path, manifest: &WorldManifest, row: i32) -> std::io::Result<Band> {
    let width = manifest.world_size as usize;
    let mut pixels = vec![[0u8; 3]; width * CHUNK_SIZE as usize];
    let mut facts = LandFacts::new(manifest.world_size);
    let top = row * CHUNK_SIZE;
    for column in 0..manifest.chunks_per_side() {
        let pos = ChunkPos::new(column, row);
        let bytes = std::fs::read(folder.join(chunk_file_path(pos)))?;
        let chunk = decode_chunk(&bytes).map_err(|error| std::io::Error::other(format!("{pos:?}: {error:?}")))?;
        let (ox, oy) = pos.origin_cell();
        for y in 0..CHUNK_SIZE as usize {
            for x in 0..CHUNK_SIZE as usize {
                let cell = chunk.cell(x, y);
                let [nw, ne, sw, se] = chunk.cell_corner_heights(x, y);
                let ground = (nw + ne + sw + se) / 4.0;
                let colour = match cell.fluid {
                    Some(fluid) if fluid.kind == FluidKind::Lava => [235, 95, 30],
                    Some(fluid) => water_colour(fluid.level - ground),
                    None => shaded(ground_colour(cell.ground), hill_shade(nw, ne, sw, se)),
                };
                let (wx, wy) = (ox + x as i32, oy + y as i32);
                pixels[(wy - top) as usize * width + wx as usize] = colour;
                facts.count_cell(wx, wy, cell.biome, cell.fluid.is_some_and(|fluid| fluid.kind == FluidKind::Water));
            }
        }
        for prop in &chunk.props {
            // Trees by the size of their crowns (bushes are small young ones).
            let (colour, radius, tree) = match prop.kind {
                PropKind::Tree { .. } if prop.scale < BUSH_SCALE => ([58, 96, 40], 1.3, false),
                PropKind::Tree { variant, .. } => ([44, 80, 34], [2.2, 3.4, 4.2][usize::from(variant.min(2))], true),
                PropKind::DeadTree => ([74, 60, 46], 1.2, false),
                PropKind::Boulder => ([132, 128, 122], 1.5, false),
            };
            let radius = radius * prop.scale.clamp(0.6, 1.6);
            dot(&mut pixels, width, top, (prop.x, prop.y), radius, colour);
            if tree {
                facts.count_tree(prop.x, prop.y);
            }
        }
    }
    Ok(Band { pixels, facts })
}

/// How lit ground is by a sun in the north-west, from its corner heights:
/// 1 on the flat, more facing the sun, less facing away.
fn hill_shade(nw: f32, ne: f32, sw: f32, se: f32) -> f32 {
    // Rise towards the east and towards the south, in metres per metre.
    let east = ((ne + se) - (nw + sw)) / 2.0;
    let south = ((sw + se) - (nw + ne)) / 2.0;
    (1.0 + 0.45 * (east + south) / std::f32::consts::SQRT_2).clamp(0.55, 1.35)
}

/// Draws a filled disc (a tree seen from above) into a band of pixels.
fn dot(pixels: &mut [[u8; 3]], width: usize, top: i32, (cx, cy): (f32, f32), radius: f32, colour: [u8; 3]) {
    let rows = pixels.len() / width;
    let reach = radius.ceil() as i32;
    for dy in -reach..=reach {
        for dx in -reach..=reach {
            if (dx * dx + dy * dy) as f32 > radius * radius {
                continue;
            }
            let (x, y) = (cx as i32 + dx, cy as i32 + dy - top);
            if x >= 0 && y >= 0 && (x as usize) < width && (y as usize) < rows {
                pixels[y as usize * width + x as usize] = colour;
            }
        }
    }
}

fn ground_colour(ground: Ground) -> [u8; 3] {
    match ground {
        Ground::Grass => [104, 142, 64],
        Ground::MeadowGrass => [124, 164, 76],
        Ground::Dirt => [132, 108, 72],
        Ground::Sand => [214, 196, 140],
        Ground::Rock => [128, 122, 114],
        Ground::Snow => [236, 240, 244],
        Ground::Path => [176, 146, 100],
        Ground::Ash => [100, 95, 92],
        Ground::Basalt => [52, 48, 48],
    }
}

/// Water: pale in the shallows, deep blue further out.
fn water_colour(depth: f32) -> [u8; 3] {
    let deep = (depth / 8.0).clamp(0.0, 1.0);
    let mix = |shallow: f32, far: f32| (shallow + (far - shallow) * deep) as u8;
    [mix(96.0, 36.0), mix(160.0, 82.0), mix(196.0, 146.0)]
}

fn shaded(colour: [u8; 3], light: f32) -> [u8; 3] {
    colour.map(|channel| (channel as f32 * light).clamp(0.0, 255.0) as u8)
}

/// Writes every level's tiles: level 0 from the picture, each after it half
/// the size (two by two pixels averaged).
fn write_tiles(folder: &Path, info: &MapInfo, mut picture: RgbImage) -> std::io::Result<()> {
    let tile = info.tile_pixels;
    for level in 0..info.levels {
        std::fs::create_dir_all(folder.join(format!("map/{level}")))?;
        let tiles = picture.width().div_ceil(tile);
        for ty in 0..tiles {
            for tx in 0..tiles {
                let mut piece = RgbImage::new(tile, tile);
                for y in 0..tile {
                    for x in 0..tile {
                        // (Past the world's edge, the edge carries on.)
                        let px = (tx * tile + x).min(picture.width() - 1);
                        let py = (ty * tile + y).min(picture.height() - 1);
                        piece.put_pixel(x, y, *picture.get_pixel(px, py));
                    }
                }
                let mut bytes = Vec::new();
                JpegEncoder::new_with_quality(Cursor::new(&mut bytes), TILE_QUALITY)
                    .encode_image(&piece)
                    .map_err(std::io::Error::other)?;
                std::fs::write(folder.join(map_tile_path(level, tx, ty)), bytes)?;
            }
        }
        picture = half_size(&picture);
    }
    Ok(())
}

/// The picture at half the size, each pixel the average of four.
fn half_size(picture: &RgbImage) -> RgbImage {
    let (width, height) = (picture.width().div_ceil(2).max(1), picture.height().div_ceil(2).max(1));
    RgbImage::from_fn(width, height, |x, y| {
        let mut sum = [0u32; 3];
        for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            let pixel =
                picture.get_pixel((x * 2 + dx).min(picture.width() - 1), (y * 2 + dy).min(picture.height() - 1));
            for channel in 0..3 {
                sum[channel] += pixel[channel] as u32;
            }
        }
        Rgb(sum.map(|total| ((total + 2) / 4) as u8))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walls_are_drawn_along_their_line_but_not_their_gaps() {
        use venture_world::WallSample;
        let samples = (0..=40)
            .map(|i| WallSample { x: 2.0 + i as f32 * 0.5, y: 10.5, ground: 0.0, open: (16..24).contains(&i) })
            .collect();
        let wall = BakedWall { id: "w".into(), kind: WallKind::DryStone, height: 1.3, seed: 0, samples };
        let mut picture = RgbImage::new(32, 32);
        draw_walls(&mut picture, &[wall]);
        let drawn = |x: u32, y: u32| picture.get_pixel(x, y).0 != [0, 0, 0];
        assert!(drawn(4, 10) && drawn(18, 10), "along the line");
        assert!(!drawn(12, 10), "not in the gap");
        assert!(!drawn(4, 12), "nor off the line");
    }

    #[test]
    fn slopes_facing_the_sun_are_lighter() {
        assert_eq!(hill_shade(0.0, 0.0, 0.0, 0.0), 1.0);
        // Rising to the south-east faces the north-west sun.
        assert!(hill_shade(0.0, 1.0, 1.0, 2.0) > 1.0);
        assert!(hill_shade(2.0, 1.0, 1.0, 0.0) < 1.0);
    }

    #[test]
    fn halving_averages_and_keeps_odd_edges() {
        let picture = RgbImage::from_fn(3, 3, |x, _| if x == 0 { Rgb([0, 0, 0]) } else { Rgb([200, 100, 40]) });
        let half = half_size(&picture);
        assert_eq!((half.width(), half.height()), (2, 2));
        assert_eq!(half.get_pixel(0, 0), &Rgb([100, 50, 20]));
        assert_eq!(half.get_pixel(1, 0), &Rgb([200, 100, 40]));
    }
}
