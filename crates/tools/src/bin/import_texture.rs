//! Imports a PBR texture set (such as an ambientCG download, unzipped) into
//! the content pack:
//!
//! ```sh
//! cargo run --release -p venture_tools --bin import_texture -- ~/assets/surfaces/Grass005_4K-PNG grass
//! ```
//!
//! writes `content/textures/grass/{colour,normal,surface}.jpg`. See `docs/textures.md`.

use std::path::PathBuf;

use venture_tools::textures::TextureSet;

/// The largest size pictures are kept at, in pixels across.
const MAX_SIZE: u32 = 1024;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [folder, name] = args.as_slice() else {
        eprintln!("usage: import_texture <texture folder> <name>");
        std::process::exit(2);
    };
    let out = PathBuf::from("content/textures").join(name);
    let result = TextureSet::find(&PathBuf::from(folder)).and_then(|set| set.import(&out, MAX_SIZE));
    match result {
        Ok(written) => {
            for path in written {
                let size = std::fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
                println!("{} ({} KB)", path.display(), size / 1024);
            }
        }
        Err(error) => {
            eprintln!("Couldn't import it: {error}");
            std::process::exit(1);
        }
    }
}
