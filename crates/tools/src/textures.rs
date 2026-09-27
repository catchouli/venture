//! Turning a PBR texture set (as ambientCG publishes them: separate colour,
//! normal, roughness and ambient occlusion pictures, often 16-bit PNGs) into
//! the three small JPEGs the game uses:
//!
//! - `colour.jpg`: the colour (sRGB),
//! - `normal.jpg`: the normal map, OpenGL style (green is up),
//! - `surface.jpg`: ambient occlusion in red, roughness in green (linear), at
//!   half the size (they're soft, and half the size is a quarter the download).
//!
//! The game makes the smaller versions (mipmaps) itself when it loads them.

use std::path::{Path, PathBuf};

use image::codecs::jpeg::JpegEncoder;
use image::{DynamicImage, GrayImage, RgbImage, imageops::FilterType};

/// JPEG quality for the colour and surface pictures.
const QUALITY: u8 = 85;
/// JPEG quality for normal maps (higher: blocky normals show in the lighting).
const NORMAL_QUALITY: u8 = 88;

/// The pictures of one texture set, found in a folder.
#[derive(Debug)]
pub struct TextureSet {
    /// The colour picture.
    pub colour: PathBuf,
    /// The OpenGL-style normal map.
    pub normal: PathBuf,
    /// The roughness picture, if there is one.
    pub roughness: Option<PathBuf>,
    /// The ambient occlusion picture, if there is one.
    pub occlusion: Option<PathBuf>,
}

impl TextureSet {
    /// Finds the pictures in an ambientCG folder by their name endings
    /// (`_Color`, `_NormalGL`, `_Roughness`, `_AmbientOcclusion`).
    pub fn find(folder: &Path) -> Result<Self, String> {
        let files: Vec<PathBuf> = std::fs::read_dir(folder)
            .map_err(|error| format!("can't read {}: {error}", folder.display()))?
            .filter_map(|entry| Some(entry.ok()?.path()))
            .collect();
        let ending = |suffix: &str| {
            files.iter().find(|path| {
                path.file_stem().and_then(|stem| stem.to_str()).is_some_and(|stem| stem.ends_with(suffix))
                    && path.extension().and_then(|e| e.to_str()).is_some_and(|e| matches!(e, "png" | "jpg" | "jpeg"))
            })
        };
        let needed = |suffix: &str| ending(suffix).cloned().ok_or(format!("there's no *{suffix} picture"));
        Ok(Self {
            colour: needed("_Color")?,
            normal: needed("_NormalGL")?,
            roughness: ending("_Roughness").cloned(),
            occlusion: ending("_AmbientOcclusion").cloned(),
        })
    }

    /// Writes the game's three pictures into `out`, at most `size` pixels
    /// across. Returns the files written.
    pub fn import(&self, out: &Path, size: u32) -> Result<Vec<PathBuf>, String> {
        std::fs::create_dir_all(out).map_err(|error| format!("can't make {}: {error}", out.display()))?;
        let colour = open(&self.colour, size)?.to_rgb8();
        let normal = open(&self.normal, size)?.to_rgb8();
        let (width, height) = (colour.width().div_ceil(2), colour.height().div_ceil(2));
        let grey = |path: &Option<PathBuf>, fallback: u8| -> Result<GrayImage, String> {
            match path {
                Some(path) => {
                    Ok(image::imageops::resize(&open(path, size)?.to_luma8(), width, height, FilterType::Triangle))
                }
                None => Ok(GrayImage::from_pixel(width, height, image::Luma([fallback]))),
            }
        };
        let occlusion = grey(&self.occlusion, 255)?;
        let roughness = grey(&self.roughness, 230)?;
        let surface = RgbImage::from_fn(width, height, |x, y| {
            image::Rgb([occlusion.get_pixel(x, y)[0], roughness.get_pixel(x, y)[0], 0])
        });
        let written = [
            (out.join("colour.jpg"), colour, QUALITY),
            (out.join("normal.jpg"), normal, NORMAL_QUALITY),
            (out.join("surface.jpg"), surface, QUALITY),
        ];
        let mut paths = Vec::new();
        for (path, picture, quality) in written {
            save_jpeg(&path, &picture, quality)?;
            paths.push(path);
        }
        Ok(paths)
    }
}

/// Opens a picture (any bit depth), shrunk to at most `size` pixels across.
fn open(path: &Path, size: u32) -> Result<DynamicImage, String> {
    let picture = image::open(path).map_err(|error| format!("can't open {}: {error}", path.display()))?;
    Ok(if picture.width() > size || picture.height() > size {
        picture.resize(size, size, FilterType::Lanczos3)
    } else {
        picture
    })
}

fn save_jpeg(path: &Path, picture: &RgbImage, quality: u8) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|error| format!("can't write {}: {error}", path.display()))?;
    JpegEncoder::new_with_quality(std::io::BufWriter::new(file), quality)
        .encode_image(picture)
        .map_err(|error| format!("can't write {}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_texture_set_becomes_three_jpegs() {
        let folder = std::env::temp_dir().join(format!("venture-texture-import-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        RgbImage::from_pixel(64, 64, image::Rgb([40, 120, 30])).save(folder.join("Test_Color.png")).unwrap();
        RgbImage::from_pixel(64, 64, image::Rgb([128, 128, 255])).save(folder.join("Test_NormalGL.png")).unwrap();
        GrayImage::from_pixel(64, 64, image::Luma([200])).save(folder.join("Test_Roughness.png")).unwrap();

        let set = TextureSet::find(&folder).unwrap();
        assert!(set.occlusion.is_none());
        let out = folder.join("out");
        let written = set.import(&out, 32).unwrap();
        assert_eq!(written.len(), 3);
        let surface = image::open(out.join("surface.jpg")).unwrap().to_rgb8();
        assert_eq!(surface.dimensions(), (16, 16));
        let pixel = surface.get_pixel(8, 8);
        assert!(pixel[0] > 245 && (190..=210).contains(&pixel[1]), "no occlusion, roughness kept: {pixel:?}");
        let _ = std::fs::remove_dir_all(&folder);
    }
}
