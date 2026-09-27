//! How the light changes with the time of day: pure functions of the sun's
//! height, used by [`crate::world::lighting`].
//!
//! - **The sun's colour and brightness** come from the air its light passes
//!   through: the lower the sun, the more air (Kasten and Young's air mass),
//!   and blue is scattered away most (a rough Rayleigh-like extinction per
//!   channel), so it's white at midday and orange to red as it sets.
//! - **The sky** dims through dusk to a dim night level.
//! - **Exposure** opens up a few stops at night, as eyes (and cameras) do, so
//!   night looks dark and blue but stays playable.

use bevy::math::Vec3;

/// How strongly the air dims each colour of sunlight (red, green, blue),
/// per air mass.
const EXTINCTION: Vec3 = Vec3::new(0.05, 0.11, 0.25);
/// Exposure (EV100) by day (a little under Bevy's usual 9.7, so bright
/// ground keeps its colour rather than washing out)...
pub const DAY_EV100: f32 = 10.0;
/// ...and at night, about one and a half stops more open (enough to see by,
/// but dark).
pub const NIGHT_EV100: f32 = 8.6;
/// How bright the sky is at night, as a share of by day.
const NIGHT_SKY: f32 = 0.04;

/// How many atmospheres of air the sunlight passes through, for a sun
/// `height` radians above the horizon (1 straight overhead, about 38 at the
/// horizon).
pub fn air_mass(height: f32) -> f32 {
    let degrees = height.to_degrees().max(-0.5);
    1.0 / (degrees.to_radians().sin() + 0.50572 * (degrees + 6.07995).powf(-1.6364))
}

/// How much of each colour of sunlight gets through the air.
fn transmittance(height: f32) -> Vec3 {
    (-EXTINCTION * air_mass(height)).exp()
}

fn luminance(colour: Vec3) -> f32 {
    colour.dot(Vec3::new(0.2126, 0.7152, 0.0722))
}

/// The sun's colour (its brightest channel 1) at a height.
pub fn sun_colour(height: f32) -> Vec3 {
    let light = transmittance(height);
    light / light.max_element()
}

/// How bright the sun is at a height, as a share of straight overhead. It
/// fades out as it sinks below the horizon.
pub fn sun_strength(height: f32) -> f32 {
    let below = smoothstep(-3f32.to_radians(), 1f32.to_radians(), height);
    luminance(transmittance(height)) / luminance(transmittance(std::f32::consts::FRAC_PI_2)) * below
}

/// How much it's night (0 by day, 1 in the dark), by the sun's height:
/// dusk runs from sunset until the sun is 12° down.
pub fn night(sun_height: f32) -> f32 {
    1.0 - smoothstep(-12f32.to_radians(), 2f32.to_radians(), sun_height)
}

/// How bright the sky (and the light it casts) is, as a share of by day.
pub fn sky_level(sun_height: f32) -> f32 {
    NIGHT_SKY + (1.0 - NIGHT_SKY) * (1.0 - night(sun_height))
}

/// The camera's exposure (EV100) at a sun height.
pub fn exposure(sun_height: f32) -> f32 {
    DAY_EV100 + (NIGHT_EV100 - DAY_EV100) * night(sun_height)
}

/// 0 below `edge0`, 1 above `edge1`, and smoothly between.
pub fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sun_is_white_high_and_red_low() {
        let high = sun_colour(60f32.to_radians());
        assert!(high.z > 0.75, "{high}");
        let low = sun_colour(2f32.to_radians());
        assert!(low.x == 1.0 && low.z < 0.1, "{low}");
        assert!(sun_strength(60f32.to_radians()) > 0.85);
        assert!(sun_strength(3f32.to_radians()) < 0.4);
        assert_eq!(sun_strength(-5f32.to_radians()), 0.0);
    }

    #[test]
    fn nights_are_dim_but_open_up_the_exposure() {
        assert_eq!(night(30f32.to_radians()), 0.0);
        assert_eq!(night(-20f32.to_radians()), 1.0);
        assert!((sky_level(-20f32.to_radians()) - NIGHT_SKY).abs() < 1e-6);
        assert_eq!(exposure(40f32.to_radians()), DAY_EV100);
        assert_eq!(exposure(-20f32.to_radians()), NIGHT_EV100);
        assert!(air_mass(std::f32::consts::FRAC_PI_2) < 1.01);
        assert!(air_mass(0.0) > 30.0);
    }
}
