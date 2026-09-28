//! Renders the whole world to PNG images for quick terrain tuning.
//!
//! ```text
//! cargo run --release -p venture_worldgen --example preview [seed]
//! ```
//!
//! Writes to `preview/`:
//! - `terrain.png`: one pixel per 2×2 metres, coloured by material, with hill
//!   shading, liquids and props. Settlements are marked with red squares.
//! - `biomes.png`: the biome regions (grassland, meadow, highlands, volcanic).
//! - `map.png`: the coarse world map, showing height, sea, lakes and rivers.

use std::time::Instant;

use image::{Rgb, RgbImage};
use venture_world::{Biome, CHUNK_SIZE, ChunkPos, FluidKind, Ground, PropKind};
use venture_worldgen::{WorldGenConfig, WorldGenerator};

/// Metres per pixel in `terrain.png`.
const SCALE: i32 = 2;

fn main() {
    let seed = std::env::args().nth(1).and_then(|arg| arg.parse().ok()).unwrap_or(1);
    let config = WorldGenConfig { seed, ..WorldGenConfig::default() };

    let started = Instant::now();
    let world = WorldGenerator::new(config);
    println!("world map built in {:.2?}", started.elapsed());

    std::fs::create_dir_all("preview").expect("create preview directory");

    let started = Instant::now();
    let mut terrain = render_terrain(&world);
    mark_settlements(&world, &mut terrain);
    terrain.save("preview/terrain.png").expect("save terrain.png");
    println!("terrain.png rendered in {:.2?}", started.elapsed());

    render_biomes(&world).save("preview/biomes.png").expect("save biomes.png");
    render_map(&world).save("preview/map.png").expect("save map.png");
    println!("biomes.png and map.png rendered");
}

fn render_terrain(world: &WorldGenerator) -> RgbImage {
    let size = (world.config().world_size / SCALE) as u32;
    let chunks = world.config().world_size / CHUNK_SIZE;
    let mut image = RgbImage::new(size, size);

    for cy in 0..chunks {
        for cx in 0..chunks {
            let chunk = world.generate_chunk(ChunkPos::new(cx, cy));
            let (ox, oy) = chunk.pos.origin_cell();
            for y in (0..CHUNK_SIZE as usize).step_by(SCALE as usize) {
                for x in (0..CHUNK_SIZE as usize).step_by(SCALE as usize) {
                    let cell = chunk.cell(x, y);
                    let [nw, ne, sw, se] = chunk.cell_corner_heights(x, y);
                    // Light from the north-west.
                    let slope = (nw - se) + (ne - sw) * 0.5;
                    let shade = (1.0 + slope * 0.3).clamp(0.55, 1.4);
                    let colour = match cell.fluid {
                        Some(fluid) if fluid.kind == FluidKind::Lava => Rgb([240, 90, 20]),
                        Some(fluid) => water_colour(fluid.level - (nw + ne + sw + se) / 4.0),
                        None => scale(ground_colour(cell.ground), shade),
                    };
                    put(&mut image, ox + x as i32, oy + y as i32, colour);
                }
            }
            for prop in &chunk.props {
                let colour = match prop.kind {
                    PropKind::Tree { .. } => Rgb([40, 80, 30]),
                    PropKind::DeadTree => Rgb([60, 45, 35]),
                    PropKind::Boulder => Rgb([140, 140, 135]),
                };
                put(&mut image, prop.x as i32, prop.y as i32, colour);
            }
        }
    }
    image
}

/// Sets the pixel covering world position `(x, y)` metres, if it's in the image.
fn put(image: &mut RgbImage, x: i32, y: i32, colour: Rgb<u8>) {
    let (px, py) = (x / SCALE, y / SCALE);
    if px >= 0 && py >= 0 && (px as u32) < image.width() && (py as u32) < image.height() {
        image.put_pixel(px as u32, py as u32, colour);
    }
}

/// Draws a red square on each settlement.
fn mark_settlements(world: &WorldGenerator, image: &mut RgbImage) {
    const HALF_SIZE: i32 = 8;
    for settlement in world.map().settlements() {
        let (x, y) = settlement.position;
        for dy in -HALF_SIZE..=HALF_SIZE {
            for dx in -HALF_SIZE..=HALF_SIZE {
                put(image, x as i32 + dx, y as i32 + dy, Rgb([220, 30, 30]));
            }
        }
    }
}

/// One pixel per 8 metres, coloured by biome.
fn render_biomes(world: &WorldGenerator) -> RgbImage {
    const STEP: i32 = 8;
    let size = (world.config().world_size / STEP) as u32;
    let mut image = RgbImage::new(size, size);
    for py in 0..size {
        for px in 0..size {
            let (x, y) = ((px as i32 * STEP) as f32, (py as i32 * STEP) as f32);
            let colour = match world.biome_weights(x, y).dominant() {
                Biome::Grassland => Rgb([130, 170, 80]),
                Biome::Meadow => Rgb([200, 140, 210]),
                Biome::Highlands => Rgb([140, 130, 120]),
                Biome::Volcanic => Rgb([180, 60, 30]),
            };
            image.put_pixel(px, py, colour);
        }
    }
    image
}

/// One pixel per coarse map point.
fn render_map(world: &WorldGenerator) -> RgbImage {
    let map = world.map();
    let size = map.size() as u32;
    let mut image = RgbImage::new(size, size);
    for j in 0..map.size() {
        for i in 0..map.size() {
            let height = map.point_land_height(i, j);
            let grey = (50.0 + height * 1.5).clamp(0.0, 255.0) as u8;
            let colour = match map.point_still_fluid(i, j) {
                Some(fluid) if fluid.kind == FluidKind::Lava => Rgb([240, 90, 20]),
                Some(_) => Rgb([40, 80, 160]),
                None if map.point_river(i, j).is_some() => Rgb([90, 150, 230]),
                None => Rgb([grey, grey, grey]),
            };
            image.put_pixel(i as u32, j as u32, colour);
        }
    }
    image
}

fn ground_colour(ground: Ground) -> Rgb<u8> {
    match ground {
        Ground::Grass => Rgb([96, 140, 58]),
        Ground::MeadowGrass => Rgb([120, 165, 70]),
        Ground::Dirt => Rgb([122, 98, 62]),
        Ground::Sand => Rgb([204, 186, 128]),
        Ground::Rock => Rgb([118, 112, 104]),
        Ground::Snow => Rgb([236, 240, 244]),
        Ground::Path => Rgb([160, 128, 84]),
        Ground::Ash => Rgb([95, 90, 88]),
        Ground::Basalt => Rgb([45, 42, 42]),
    }
}

fn water_colour(depth: f32) -> Rgb<u8> {
    let deep = (depth / 10.0).clamp(0.0, 1.0);
    Rgb([(70.0 - 40.0 * deep) as u8, (130.0 - 60.0 * deep) as u8, (190.0 - 50.0 * deep) as u8])
}

fn scale(Rgb([r, g, b]): Rgb<u8>, factor: f32) -> Rgb<u8> {
    let channel = |c: u8| (c as f32 * factor).clamp(0.0, 255.0) as u8;
    Rgb([channel(r), channel(g), channel(b)])
}
