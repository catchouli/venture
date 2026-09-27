//! The day: the time of day, and where the sun and moon are in the sky.
//!
//! The server keeps the clock (it's the same for everyone) and says what
//! time it is; the client runs its clock on from there, and lights the
//! world by where the sun and moon are. See `docs/plan-day-night.md`.
//!
//! Times are hours on the game's clock, from 0 (midnight) to 24. The sun
//! rises in the east-north-east at [`DayCycle::sunrise`], is highest
//! (in the south) halfway to [`DayCycle::sunset`], and sets in the
//! west-north-west; the moon is up while it's down.

use serde::{Deserialize, Serialize};

/// How the days go (from `server.ron`).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DayCycle {
    /// How long a whole day (and night) lasts, in real minutes.
    pub day_minutes: f32,
    /// When the sun rises, in hours (0 to 24).
    pub sunrise: f32,
    /// When the sun sets, in hours (after sunrise).
    pub sunset: f32,
    /// How high the sun gets at midday, in degrees above the horizon.
    pub noon_height: f32,
}

impl Default for DayCycle {
    fn default() -> Self {
        Self { day_minutes: 120.0, sunrise: 3.0, sunset: 21.0, noon_height: 60.0 }
    }
}

/// Where something is in the sky: its bearing (radians clockwise from
/// north, so east is a quarter turn) and its height (radians above the
/// horizon; below it, negative).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SkyPosition {
    /// Radians clockwise from north.
    pub bearing: f32,
    /// Radians above the horizon.
    pub height: f32,
}

impl SkyPosition {
    /// The direction towards it, as world `(east, south, up)` (a unit vector).
    pub fn direction(self) -> (f32, f32, f32) {
        let flat = self.height.cos();
        (flat * self.bearing.sin(), -flat * self.bearing.cos(), self.height.sin())
    }
}

/// How far below the horizon the sun goes at midnight, in degrees.
const MIDNIGHT_DEPTH: f32 = 25.0;
/// The bearings the sun rises and sets at, in degrees clockwise from north.
const SUNRISE_BEARING: f32 = 70.0;
const SUNSET_BEARING: f32 = 290.0;
/// How high the moon gets, in degrees.
const MOON_HEIGHT: f32 = 45.0;

impl DayCycle {
    /// Real seconds a game hour lasts.
    pub fn seconds_per_hour(&self) -> f32 {
        self.day_minutes * 60.0 / 24.0
    }

    /// The time of day (hours) `seconds` real seconds after `hour`.
    pub fn after(&self, hour: f32, seconds: f32) -> f32 {
        (hour + seconds / self.seconds_per_hour()).rem_euclid(24.0)
    }

    /// The time of day (hours) at a moment on the real clock (seconds since
    /// 1970), moved on by `offset` hours: days run on from there, so the
    /// time carries on across server restarts.
    pub fn hour_at(&self, unix_seconds: f64, offset: f32) -> f32 {
        let day_seconds = f64::from(self.day_minutes) * 60.0;
        let into_day = unix_seconds.rem_euclid(day_seconds) / day_seconds;
        ((into_day * 24.0) as f32 + offset).rem_euclid(24.0)
    }

    /// How long the sun is up, in hours.
    fn daylight(&self) -> f32 {
        (self.sunset - self.sunrise).clamp(1.0, 23.0)
    }

    /// Where the sun is at a time of day. By day it arcs from its rising
    /// bearing through the south to its setting one; by night it carries on
    /// round under the horizon, lowest at the middle of the night.
    pub fn sun(&self, hour: f32) -> SkyPosition {
        let since_rise = (hour - self.sunrise).rem_euclid(24.0);
        let daylight = self.daylight();
        let (rise, set) = (SUNRISE_BEARING.to_radians(), SUNSET_BEARING.to_radians());
        if since_rise < daylight {
            let along = since_rise / daylight;
            SkyPosition {
                bearing: rise + (set - rise) * along,
                height: self.noon_height.to_radians() * (std::f32::consts::PI * along).sin(),
            }
        } else {
            let along = (since_rise - daylight) / (24.0 - daylight);
            let round = std::f32::consts::TAU - (set - rise);
            SkyPosition {
                bearing: (set + round * along).rem_euclid(std::f32::consts::TAU),
                height: -MIDNIGHT_DEPTH.to_radians() * (std::f32::consts::PI * along).sin(),
            }
        }
    }

    /// Where the moon is: across the sky from the sun, up while it's down
    /// (not as high, and turning the other way round the sky).
    pub fn moon(&self, hour: f32) -> SkyPosition {
        let sun = self.sun(hour);
        let height = if sun.height < 0.0 {
            MOON_HEIGHT.to_radians() * (-sun.height / MIDNIGHT_DEPTH.to_radians())
        } else {
            -sun.height * 0.5
        };
        SkyPosition { bearing: (sun.bearing + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU), height }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn degrees(radians: f32) -> f32 {
        radians.to_degrees().round()
    }

    #[test]
    fn the_sun_rises_peaks_in_the_south_and_sets() {
        let day = DayCycle::default();
        let rise = day.sun(day.sunrise);
        assert_eq!((degrees(rise.bearing), degrees(rise.height)), (70.0, 0.0));
        let noon = day.sun((day.sunrise + day.sunset) / 2.0);
        assert_eq!((degrees(noon.bearing), degrees(noon.height)), (180.0, 60.0));
        let set = day.sun(day.sunset);
        assert_eq!((degrees(set.bearing), degrees(set.height)), (290.0, 0.0));
        let midnight = day.sun((day.sunset + 3.0) % 24.0);
        assert!(midnight.height < 0.0 && degrees(midnight.bearing) == 0.0, "{midnight:?}");
    }

    #[test]
    fn the_moon_is_up_at_night() {
        let day = DayCycle::default();
        let midnight = (day.sunset + 3.0) % 24.0;
        assert_eq!(degrees(day.moon(midnight).height), 45.0);
        assert!(day.moon(12.0).height < 0.0);
    }

    #[test]
    fn the_clock_runs_on() {
        let day = DayCycle::default();
        // A two-hour day: five real minutes is an hour.
        assert_eq!(day.after(23.5, 300.0), 0.5);
        assert_eq!(day.hour_at(0.0, 0.0), 0.0);
        assert!((day.hour_at(3600.0, 0.0) - 12.0).abs() < 1e-4);
        assert!((day.hour_at(3600.0, 13.0) - 1.0).abs() < 1e-4);
    }

    #[test]
    fn directions_point_the_right_way() {
        let east = SkyPosition { bearing: std::f32::consts::FRAC_PI_2, height: 0.0 }.direction();
        assert!((east.0 - 1.0).abs() < 1e-6 && east.1.abs() < 1e-6);
        let south = SkyPosition { bearing: std::f32::consts::PI, height: 0.0 }.direction();
        assert!((south.1 - 1.0).abs() < 1e-6);
        let up = SkyPosition { bearing: 0.0, height: std::f32::consts::FRAC_PI_2 }.direction();
        assert!((up.2 - 1.0).abs() < 1e-6);
    }
}
