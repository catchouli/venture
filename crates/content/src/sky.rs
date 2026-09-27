//! Skies: `skies/<name>/`, made from an HDR panorama by `venture_tools`'s
//! `import_sky` (see `docs/textures.md`). Each has:
//!
//! - `sky.jpg`: the sky as seen, six square cube faces stacked top to bottom
//!   (+X, −X, +Y, −Y, +Z, −Z, in the usual cubemap layout), sRGB,
//! - `light.bin`: the light it casts on things (image-based lighting): its
//!   irradiance, and its radiance blurred for each roughness, as cubemaps
//!   ([`SkyLight`]),
//! - `sky.ron`: where its sun is, and the colour of its horizon ([`SkyDef`]).
//!
//! Everything is on one brightness scale, where 1 is as bright as the sky
//! picture shows (the brightest parts of the sky, not the sun).

use serde::{Deserialize, Serialize};

/// The first bytes of a `light.bin` file.
pub const SKY_LIGHT_MAGIC: [u8; 4] = *b"VSKL";
/// The `light.bin` layout this reads and writes.
const SKY_LIGHT_VERSION: u32 = 1;
/// The pictures and files every sky has.
pub const SKY_FILES: [&str; 3] = ["sky.jpg", "light.bin", "sky.ron"];

/// What a sky says about itself.
///
/// ```ron
/// (sun: (0.42, 0.61, -0.67), horizon: (0.71, 0.78, 0.86))
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkyDef {
    /// The direction towards the sun, in the game's world (x east, y up,
    /// z south), from the brightest point of the panorama.
    pub sun: (f32, f32, f32),
    /// The sky's average colour just above the horizon (linear, on the
    /// sky's brightness scale), for distant haze to fade into.
    pub horizon: (f32, f32, f32),
}

/// Parses a `sky.ron`.
pub fn parse_sky(text: &str) -> Result<SkyDef, String> {
    let sky: SkyDef = ron::from_str(text).map_err(|error| error.to_string())?;
    let (x, y, z) = sky.sun;
    let length = (x * x + y * y + z * z).sqrt();
    if !length.is_finite() || (length - 1.0).abs() > 0.01 {
        return Err("sun must be a direction (a vector of length 1)".to_string());
    }
    let (r, g, b) = sky.horizon;
    if ![r, g, b].iter().all(|channel| channel.is_finite() && *channel >= 0.0) {
        return Err("horizon must be a colour (numbers 0 or more)".to_string());
    }
    Ok(sky)
}

/// A sky's light, as cubemaps of RGB9E5 texels (each a little-endian `u32`,
/// the shared-exponent format GPUs read directly), each face in turn
/// (+X, −X, +Y, −Y, +Z, −Z) with all its mipmaps (Bevy's usual order).
#[derive(Clone, Debug, PartialEq)]
pub struct SkyLight {
    /// Width (and height) of the irradiance cube's faces; it has no mipmaps.
    pub irradiance_size: u32,
    /// The irradiance cube's texels.
    pub irradiance: Vec<u32>,
    /// Width of the radiance cube's largest faces.
    pub radiance_size: u32,
    /// How many mipmaps it has (each for a rougher surface, from mirror-like
    /// to completely rough).
    pub radiance_mips: u32,
    /// The radiance cube's texels.
    pub radiance: Vec<u32>,
}

impl SkyLight {
    /// As the bytes of a `light.bin`.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(24 + (self.irradiance.len() + self.radiance.len()) * 4);
        bytes.extend_from_slice(&SKY_LIGHT_MAGIC);
        for value in [SKY_LIGHT_VERSION, self.irradiance_size, self.radiance_size, self.radiance_mips] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        for texel in self.irradiance.iter().chain(&self.radiance) {
            bytes.extend_from_slice(&texel.to_le_bytes());
        }
        bytes
    }

    /// Reads a `light.bin`.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        let word = |index: usize| -> Option<u32> {
            Some(u32::from_le_bytes(bytes.get(4 + index * 4..8 + index * 4)?.try_into().ok()?))
        };
        if bytes.get(..4) != Some(SKY_LIGHT_MAGIC.as_slice()) {
            return Err("this isn't a sky light file".to_string());
        }
        let (Some(version), Some(irradiance_size), Some(radiance_size), Some(radiance_mips)) =
            (word(0), word(1), word(2), word(3))
        else {
            return Err("the sky light file is cut short".to_string());
        };
        if version != SKY_LIGHT_VERSION {
            return Err(format!("the sky light file is version {version}; import the sky again"));
        }
        let sizes_fit = (1..=256).contains(&irradiance_size)
            && (1..=2048).contains(&radiance_size)
            && radiance_mips >= 1
            && radiance_size >> (radiance_mips - 1) >= 1;
        if !sizes_fit {
            return Err("the sky light file's sizes don't make sense".to_string());
        }
        let irradiance_texels = (6 * irradiance_size * irradiance_size) as usize;
        let radiance_texels = (0..radiance_mips).map(|mip| 6 * (radiance_size >> mip).pow(2)).sum::<u32>() as usize;
        let texels = &bytes[20..];
        if texels.len() != (irradiance_texels + radiance_texels) * 4 {
            return Err("the sky light file is the wrong length".to_string());
        }
        let all: Vec<u32> = texels.as_chunks::<4>().0.iter().map(|texel| u32::from_le_bytes(*texel)).collect();
        Ok(Self {
            irradiance_size,
            irradiance: all[..irradiance_texels].to_vec(),
            radiance_size,
            radiance_mips,
            radiance: all[irradiance_texels..].to_vec(),
        })
    }
}

/// A colour (linear, 0 or more) as an RGB9E5 texel: three 9-bit mantissas
/// sharing one 5-bit exponent.
pub fn rgb9e5(colour: [f32; 3]) -> u32 {
    const MANTISSA_BITS: i32 = 9;
    const BIAS: i32 = 15;
    const MAX: f32 = ((1 << MANTISSA_BITS) - 1) as f32 / (1 << MANTISSA_BITS) as f32 * 65536.0;
    let [r, g, b] = colour.map(|channel| if channel.is_finite() { channel.clamp(0.0, MAX) } else { 0.0 });
    let largest = r.max(g).max(b);
    if largest <= 0.0 {
        return 0;
    }
    let mut exponent = (largest.log2().floor() as i32).max(-BIAS - 1) + 1 + BIAS;
    let mut scale = 2f32.powi(exponent - BIAS - MANTISSA_BITS);
    if (largest / scale).round() as i32 == 1 << MANTISSA_BITS {
        exponent += 1;
        scale *= 2.0;
    }
    let mantissa = |channel: f32| ((channel / scale).round() as u32).min(511);
    mantissa(r) | (mantissa(g) << 9) | (mantissa(b) << 18) | ((exponent.clamp(0, 31) as u32) << 27)
}

/// An RGB9E5 texel back to a colour.
pub fn from_rgb9e5(texel: u32) -> [f32; 3] {
    let scale = 2f32.powi((texel >> 27) as i32 - 15 - 9);
    [texel & 511, (texel >> 9) & 511, (texel >> 18) & 511].map(|mantissa| mantissa as f32 * scale)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgb9e5_keeps_colours_to_about_three_digits() {
        for colour in [[0.5, 0.25, 0.125], [12.0, 3.0, 0.01], [0.001, 0.002, 0.0], [0.0; 3]] {
            let back = from_rgb9e5(rgb9e5(colour));
            for (a, b) in colour.iter().zip(back) {
                let largest = colour.iter().cloned().fold(0.0, f32::max);
                assert!((a - b).abs() <= largest / 256.0 + 1e-6, "{colour:?} came back as {back:?}");
            }
        }
    }

    #[test]
    fn light_files_read_back_what_was_written() {
        let light = SkyLight {
            irradiance_size: 2,
            irradiance: vec![7; 6 * 4],
            radiance_size: 4,
            radiance_mips: 3,
            radiance: vec![9; 6 * (16 + 4 + 1)],
        };
        assert_eq!(SkyLight::from_bytes(&light.to_bytes()), Ok(light.clone()));
        let mut short = light.to_bytes();
        short.pop();
        assert!(SkyLight::from_bytes(&short).is_err());
        assert!(SkyLight::from_bytes(b"nope").is_err());
    }

    #[test]
    fn skies_need_a_real_sun_direction() {
        assert!(parse_sky("(sun: (0.0, 1.0, 0.0), horizon: (0.5, 0.6, 0.7))").is_ok());
        assert!(parse_sky("(sun: (0.0, 2.0, 0.0), horizon: (0.5, 0.6, 0.7))").is_err());
        assert!(parse_sky("(sun: (0.0, 1.0, 0.0), horizon: (-1.0, 0.6, 0.7))").is_err());
    }
}
