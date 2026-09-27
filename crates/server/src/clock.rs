//! The world's clock: the time of day, the same for everyone.
//!
//! The time runs on from the real clock (so it carries on across restarts),
//! at the pace of the day cycle in the rules (`day` in `server.ron`). Admins
//! can set the time (`/time`) and how fast it goes (`/timescale`); from then
//! until the server restarts, it runs on from there. Players are told the
//! time and pace when they join and when either is set; their clients keep
//! it running.

use bevy::prelude::*;
use venture_protocol::ServerMessage;

use crate::entities::Player;
use crate::link::Link;
use crate::{GameRules, ServerSystems};

/// Keeps the clock and tells players the time.
pub struct ClockPlugin;

impl Plugin for ClockPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<WorldClock>().add_systems(FixedUpdate, tell_newcomers.in_set(ServerSystems::Updates));
    }
}

/// The world's clock.
#[derive(Resource)]
pub struct WorldClock {
    /// Once an admin has set the time or pace: when (the real clock's
    /// seconds), and the time of day then; it runs on from there.
    since: Option<(f64, f32)>,
    /// How many times the usual pace it runs (0: stopped).
    scale: f32,
}

impl Default for WorldClock {
    fn default() -> Self {
        Self { since: None, scale: 1.0 }
    }
}

impl WorldClock {
    /// The time of day now, in hours.
    pub fn hour(&self, rules: &GameRules) -> f32 {
        self.hour_at(now(), rules)
    }

    fn hour_at(&self, moment: f64, rules: &GameRules) -> f32 {
        match self.since {
            None => rules.0.day.hour_at(moment, 0.0),
            Some((then, hour)) => rules.0.day.after(hour, (moment - then) as f32 * self.scale),
        }
    }

    /// Sets the time of day to `hour` (from now on, it runs on from there).
    pub fn set(&mut self, hour: f32) {
        self.since = Some((now(), hour));
    }

    /// Sets how fast the clock runs (1 the usual pace, 0 stopped), from now.
    pub fn set_scale(&mut self, scale: f32, rules: &GameRules) {
        let moment = now();
        self.since = Some((moment, self.hour_at(moment, rules)));
        self.scale = scale;
    }

    /// How fast the clock runs.
    pub fn scale(&self) -> f32 {
        self.scale
    }

    /// The message telling a player the time (and pace).
    pub fn message(&self, rules: &GameRules) -> ServerMessage {
        ServerMessage::Time { hour: self.hour(rules), scale: self.scale }
    }
}

/// Seconds since 1970 by the real clock.
fn now() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |since| since.as_secs_f64())
}

fn tell_newcomers(
    clock: Res<WorldClock>,
    rules: Res<GameRules>,
    newcomers: Query<&Player, Added<Player>>,
    mut link: ResMut<Link>,
) {
    for player in &newcomers {
        link.send(player.connection, &clock.message(&rules));
    }
}

#[cfg(test)]
mod tests {
    use venture_simulation::Rules;

    use super::*;

    #[test]
    fn the_clock_runs_at_its_scale_from_when_its_set() {
        let rules = GameRules(Rules::default());
        // A two-hour day: an hour of game time is five real minutes.
        let mut clock = WorldClock { since: Some((1000.0, 6.0)), scale: 1.0 };
        assert!((clock.hour_at(1300.0, &rules) - 7.0).abs() < 1e-4);
        clock.scale = 10.0;
        assert!((clock.hour_at(1030.0, &rules) - 7.0).abs() < 1e-4);
        clock.scale = 0.0;
        assert_eq!(clock.hour_at(99_999.0, &rules), 6.0, "stopped");
    }
}
