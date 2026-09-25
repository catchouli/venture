//! Bakes a world to disk: generates every chunk and writes the world folder
//! the server loads (see `venture_world::WorldManifest`).
//!
//! ```text
//! cargo run --release -p venture_worldgen --bin bake -- [output folder] [seed]
//! ```
//!
//! The output folder defaults to `world`, and the seed to the default config's.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use venture_world::{
    CHUNK_FORMAT_VERSION, CHUNK_SIZE, ChunkPos, MANIFEST_FILE, WorldManifest, chunk_file_path, encode_chunk,
};
use venture_worldgen::{GENERATOR_VERSION, WorldGenConfig, WorldGenerator};

fn main() -> std::io::Result<()> {
    let mut args = std::env::args().skip(1);
    let output = PathBuf::from(args.next().unwrap_or_else(|| "world".to_string()));
    let mut config = WorldGenConfig::default();
    if let Some(seed) = args.next() {
        config.seed = seed.parse().expect("the seed must be a whole number");
    }

    let started = Instant::now();
    let world = WorldGenerator::new(config);
    println!("World map built in {:.2?}.", started.elapsed());

    let manifest = WorldManifest {
        world_size: world.config().world_size,
        chunk_size: CHUNK_SIZE,
        chunk_format: CHUNK_FORMAT_VERSION,
        generator_version: GENERATOR_VERSION,
        seed: world.config().seed,
        spawn_point: world.spawn_point(),
    };
    std::fs::create_dir_all(output.join("chunks"))?;

    let started = Instant::now();
    bake_chunks(&world, &manifest, &output)?;
    println!("{} chunks baked in {:.2?}.", manifest.chunks_per_side().pow(2), started.elapsed());

    // The manifest is written last, so a half-finished bake is never mistaken for a complete one.
    std::fs::write(output.join(MANIFEST_FILE), manifest.to_ron())?;
    println!("World written to {}.", output.display());
    Ok(())
}

/// Generates and writes every chunk, spreading the work over all CPU cores.
fn bake_chunks(world: &WorldGenerator, manifest: &WorldManifest, output: &Path) -> std::io::Result<()> {
    let chunks = manifest.chunks_per_side();
    let total = (chunks * chunks) as usize;
    let next = AtomicUsize::new(0);
    let threads = std::thread::available_parallelism().map_or(4, |count| count.get());

    std::thread::scope(|scope| {
        let workers: Vec<_> = (0..threads)
            .map(|_| {
                scope.spawn(|| -> std::io::Result<()> {
                    loop {
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        if index >= total {
                            return Ok(());
                        }
                        let pos = ChunkPos::new(index as i32 % chunks, index as i32 / chunks);
                        let bytes = encode_chunk(&world.generate_chunk(pos));
                        std::fs::write(output.join(chunk_file_path(pos)), bytes)?;
                    }
                })
            })
            .collect();
        workers.into_iter().try_for_each(|worker| worker.join().expect("a bake worker panicked"))
    })
}
