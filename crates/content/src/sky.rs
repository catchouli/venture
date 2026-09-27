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
//!
//! `skies/cycle.ron` ([`SkyCycle`]) says which sky shows at which time of
//! day, and how bright, for the game to blend between (see
//! `docs/plan-day-night.md`).

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

/// The day's skies (`skies/cycle.ron`): which sky shows at which time of
/// day, and how bright. Between two keyframes the game blends smoothly from
/// one to the next (and from the last round to the first).
///
/// ```ron
/// (keyframes: [
///     (hour: 4.5, sky: "day", brightness: 1.0),
///     (hour: 22.0, sky: "night", brightness: 0.04, turn: Stars),
/// ])
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkyCycle {
    /// The keyframes, in order through the day.
    pub keyframes: Vec<SkyKeyframe>,
}

/// One sky at one time of day.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkyKeyframe {
    /// When it shows fully, in hours (0 to 24).
    pub hour: f32,
    /// Which sky (a folder in `skies/`).
    pub sky: String,
    /// How bright it is, as a share of its brightness scale's usual (1 by
    /// day; much less at night).
    pub brightness: f32,
    /// Which way it's turned.
    #[serde(default)]
    pub turn: SkyTurn,
}

/// Which way a sky is turned.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum SkyTurn {
    /// So the glow where its sun was is where the game's sun is.
    #[default]
    FollowSun,
    /// Slowly round the pole star, a turn a day, as the stars go.
    Stars,
    /// Fixed, turned this many degrees (clockwise, seen from above).
    Fixed(f32),
}

/// The file the day's skies are in.
pub const SKY_CYCLE_FILE: &str = "skies/cycle.ron";

/// Parses a `skies/cycle.ron` (checking its own numbers; whether its skies
/// exist is for the content check).
pub fn parse_sky_cycle(text: &str) -> Result<SkyCycle, String> {
    let cycle: SkyCycle = ron::from_str(text).map_err(|error| error.to_string())?;
    if cycle.keyframes.is_empty() {
        return Err("there must be at least one keyframe".to_string());
    }
    for keyframe in &cycle.keyframes {
        if !(0.0..24.0).contains(&keyframe.hour) {
            return Err(format!("keyframe hours are from 0 to 24, not {}", keyframe.hour));
        }
        if !keyframe.brightness.is_finite() || keyframe.brightness < 0.0 {
            return Err(format!("brightness is 0 or more, not {}", keyframe.brightness));
        }
    }
    if cycle.keyframes.windows(2).any(|pair| pair[1].hour <= pair[0].hour) {
        return Err("keyframes go in order through the day, each at a different hour".to_string());
    }
    Ok(cycle)
}

impl SkyCycle {
    /// The two keyframes either side of a time of day, and how far it is
    /// from the first to the second (0 to 1, eased so it starts and ends
    /// gently).
    pub fn at(&self, hour: f32) -> (&SkyKeyframe, &SkyKeyframe, f32) {
        let keyframes = &self.keyframes;
        let after = keyframes.iter().position(|keyframe| keyframe.hour > hour).unwrap_or(0);
        let before = (after + keyframes.len() - 1) % keyframes.len();
        let (from, to) = (&keyframes[before], &keyframes[after]);
        let span = (to.hour - from.hour).rem_euclid(24.0);
        let along = if span > 0.0 { ((hour - from.hour).rem_euclid(24.0) / span).clamp(0.0, 1.0) } else { 0.0 };
        (from, to, along * along * (3.0 - 2.0 * along))
    }
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
    fn the_cycle_blends_between_its_keyframes_round_the_day() {
        let cycle = parse_sky_cycle(
            r#"(keyframes: [
                (hour: 6.0, sky: "day", brightness: 1.0),
                (hour: 20.0, sky: "evening", brightness: 0.4),
                (hour: 22.0, sky: "night", brightness: 0.04, turn: Stars),
            ])"#,
        )
        .unwrap();
        let (from, to, along) = cycle.at(21.0);
        assert_eq!((from.sky.as_str(), to.sky.as_str(), along), ("evening", "night", 0.5));
        // Past the last keyframe, round to the first.
        let (from, to, along) = cycle.at(2.0);
        assert_eq!((from.sky.as_str(), to.sky.as_str(), along), ("night", "day", 0.5));
        let (from, _, along) = cycle.at(6.0);
        assert_eq!((from.sky.as_str(), along), ("day", 0.0));
        assert_eq!(cycle.keyframes[2].turn, SkyTurn::Stars);
        assert!(parse_sky_cycle("(keyframes: [])").is_err());
        assert!(
            parse_sky_cycle(r#"(keyframes: [(hour: 9, sky: "a", brightness: 1), (hour: 3, sky: "b", brightness: 1)])"#)
                .is_err()
        );
    }

    #[test]
    fn skies_need_a_real_sun_direction() {
        assert!(parse_sky("(sun: (0.0, 1.0, 0.0), horizon: (0.5, 0.6, 0.7))").is_ok());
        assert!(parse_sky("(sun: (0.0, 2.0, 0.0), horizon: (0.5, 0.6, 0.7))").is_err());
        assert!(parse_sky("(sun: (0.0, 1.0, 0.0), horizon: (-1.0, 0.6, 0.7))").is_err());
    }
}
