//! Imports a sky from an HDR panorama (such as an ambientCG sky, unzipped)
//! into the content pack:
//!
//! ```sh
//! cargo run --release -p venture_tools --bin import_sky -- ~/assets/skies/DaySkyHDRI070B_4K day
//! ```
//!
//! writes `content/skies/day/{sky.jpg,light.bin,sky.ron}`. The sun is
//! painted out (the game draws its own), and anything listed in
//! `tools/skies/<panorama folder>.ron` (run from the repository's root).
//! See `docs/textures.md`.

use std::path::{Path, PathBuf};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [source, name] = args.as_slice() else {
        eprintln!("usage: import_sky <panorama or sky folder> <name>");
        std::process::exit(2);
    };
    let out = PathBuf::from("content/skies").join(name);
    let source = PathBuf::from(source);
    let options = match venture_tools::skies::import_options(Path::new("tools/skies"), &source) {
        Ok(options) => options,
        Err(error) => {
            eprintln!("Couldn't read its import options: {error}");
            std::process::exit(1);
        }
    };
    match venture_tools::skies::import(&source, &out, &options) {
        Ok((baked, written)) => {
            match baked.sun_removed {
                Some(radius) => println!("Painted out the sun ({radius:.1} degrees across from its middle)."),
                None => println!("No sun to paint out."),
            }
            if !options.paint_out.is_empty() {
                println!("Painted out {} lines (tools/skies/).", options.paint_out.len());
            }
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
