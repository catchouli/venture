//! Imports a sky from an HDR panorama (such as an ambientCG sky, unzipped)
//! into the content pack:
//!
//! ```sh
//! cargo run --release -p venture_tools --bin import_sky -- ~/assets/skies/DaySkyHDRI070B_4K day
//! ```
//!
//! writes `content/skies/day/{sky.jpg,light.bin,sky.ron}`. See `docs/textures.md`.

use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [source, name] = args.as_slice() else {
        eprintln!("usage: import_sky <panorama or sky folder> <name>");
        std::process::exit(2);
    };
    let out = PathBuf::from("content/skies").join(name);
    match venture_tools::skies::import(&PathBuf::from(source), &out) {
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
