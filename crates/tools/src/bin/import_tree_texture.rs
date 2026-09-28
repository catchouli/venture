//! Imports a photographed bark or leaf set (an ambientCG download, unzipped)
//! for the tree generator (see `docs/plan-trees.md`):
//!
//! ```sh
//! cargo run --release -p venture_tools --bin import_tree_texture -- bark ~/assets/trees/Bark001_2K-PNG
//! cargo run --release -p venture_tools --bin import_tree_texture -- leaves ~/assets/trees/LeafSet016_2K-PNG
//! ```
//!
//! - Bark: the colour picture, [`BARK_WIDTH`] pixels across (keeping its
//!   shape), as `content/trees/bark/<name>.jpg`.
//! - Leaves: the colour picture with its opacity as the alpha, at most
//!   [`LEAVES_SIZE`] across, as `content/trees/leaves/<name>.png`.
//!
//! `<name>` is the set's name without its size (`Bark001`, `LeafSet016`).

use std::path::{Path, PathBuf};

use image::imageops::FilterType;
use image::{DynamicImage, GenericImageView, Rgba, RgbaImage};

/// How wide bark pictures are kept, in pixels.
const BARK_WIDTH: u32 = 512;
/// The largest leaf pictures are kept, in pixels across.
const LEAVES_SIZE: u32 = 1024;
/// JPEG quality for bark.
const JPEG_QUALITY: u8 = 88;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [kind, folder] = args.as_slice() else {
        eprintln!("usage: import_tree_texture bark|leaves <ambientCG folder>");
        std::process::exit(2);
    };
    let result = match kind.as_str() {
        "bark" => import_bark(Path::new(folder)),
        "leaves" => import_leaves(Path::new(folder)),
        _ => Err(format!("\"{kind}\" should be bark or leaves")),
    };
    match result {
        Ok(path) => {
            let size = std::fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
            println!("{} ({} KB)", path.display(), size / 1024);
        }
        Err(error) => {
            eprintln!("Couldn't import it: {error}");
            std::process::exit(1);
        }
    }
}

/// The set's name (`Bark001` from `Bark001_2K-PNG`).
fn set_name(folder: &Path) -> Result<String, String> {
    let name = folder.file_name().and_then(|name| name.to_str()).ok_or("the folder has no name")?;
    Ok(name.split('_').next().unwrap_or(name).to_string())
}

/// The picture in `folder` whose name ends with `ending` (such as `_Color.png`).
fn find(folder: &Path, ending: &str) -> Result<DynamicImage, String> {
    let entries = std::fs::read_dir(folder).map_err(|error| format!("can't read {}: {error}", folder.display()))?;
    let path = entries
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .find(|path| path.file_name().and_then(|name| name.to_str()).is_some_and(|name| name.ends_with(ending)))
        .ok_or_else(|| format!("there's no *{ending} in {}", folder.display()))?;
    image::open(&path).map_err(|error| format!("can't read {}: {error}", path.display()))
}

fn import_bark(folder: &Path) -> Result<PathBuf, String> {
    let name = set_name(folder)?;
    let colour = find(folder, "_Color.png")?;
    let (width, height) = colour.dimensions();
    let new_height = (BARK_WIDTH as f32 * height as f32 / width as f32).round() as u32;
    let small = colour.resize_exact(BARK_WIDTH, new_height, FilterType::Lanczos3).to_rgb8();
    let out = PathBuf::from("content/trees/bark").join(format!("{name}.jpg"));
    std::fs::create_dir_all(out.parent().expect("has a folder")).map_err(|error| error.to_string())?;
    let file = std::fs::File::create(&out).map_err(|error| format!("can't write {}: {error}", out.display()))?;
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(file, JPEG_QUALITY);
    encoder.encode_image(&small).map_err(|error| error.to_string())?;
    Ok(out)
}

fn import_leaves(folder: &Path) -> Result<PathBuf, String> {
    let name = set_name(folder)?;
    let colour = find(folder, "_Color.png")?.to_rgb8();
    let opacity = find(folder, "_Opacity.png")?.to_luma8();
    if colour.dimensions() != opacity.dimensions() {
        return Err("the colour and opacity pictures are different sizes".to_string());
    }
    let combined = RgbaImage::from_fn(colour.width(), colour.height(), |x, y| {
        let [r, g, b] = colour.get_pixel(x, y).0;
        Rgba([r, g, b, opacity.get_pixel(x, y).0[0]])
    });
    let largest = combined.width().max(combined.height());
    let small = if largest > LEAVES_SIZE {
        let scale = LEAVES_SIZE as f32 / largest as f32;
        let (width, height) = (combined.width() as f32 * scale, combined.height() as f32 * scale);
        image::imageops::resize(&combined, width.round() as u32, height.round() as u32, FilterType::Lanczos3)
    } else {
        combined
    };
    let out = PathBuf::from("content/trees/leaves").join(format!("{name}.png"));
    std::fs::create_dir_all(out.parent().expect("has a folder")).map_err(|error| error.to_string())?;
    small.save(&out).map_err(|error| format!("can't write {}: {error}", out.display()))?;
    Ok(out)
}
