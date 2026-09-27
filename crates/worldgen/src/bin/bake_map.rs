//! Draws the world map (its tiles and place names) for a world that's
//! already baked, without generating the world again, and records it in the
//! manifest. `bake` does this too, for new worlds.
//!
//! ```text
//! cargo run --release -p venture_worldgen --bin bake_map -- [world folder]
//! ```
//!
//! The folder defaults to `world`.

use std::path::PathBuf;
use std::time::Instant;

use venture_world::{MANIFEST_FILE, WorldManifest};
use venture_worldgen::map_render::bake_map;

fn main() -> std::io::Result<()> {
    let folder = PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| "world".to_string()));
    let text = std::fs::read_to_string(folder.join(MANIFEST_FILE))?;
    let mut manifest = WorldManifest::from_ron(&text).map_err(|error| std::io::Error::other(format!("{error:?}")))?;

    let started = Instant::now();
    manifest.map = Some(bake_map(&folder, &manifest)?);
    std::fs::write(folder.join(MANIFEST_FILE), manifest.to_ron())?;
    println!("Map drawn in {:.2?}, into {}.", started.elapsed(), folder.join("map").display());
    Ok(())
}
