//! The world's clock: the time of day, the same for everyone.
//!
//! The time runs on from the real clock (so it carries on across restarts),
//! at the pace of the day cycle in the rules (`day` in `server.ron`), moved
//! on by whatever admins have set it to. Players are told the time when
//! they join and when it's set; their clients keep it running.

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
#[derive(Resource, Default)]
pub struct WorldClock {
    /// Hours added to the time the real clock gives (set by admins).
    offset: f32,
}

impl WorldClock {
    /// The time of day now, in hours.
    pub fn hour(&self, rules: &GameRules) -> f32 {
        rules.0.day.hour_at(now(), self.offset)
    }

    /// Sets the time of day to `hour` (from now on, it runs on from there).
    pub fn set(&mut self, hour: f32, rules: &GameRules) {
        let current = rules.0.day.hour_at(now(), 0.0);
        self.offset = (hour - current).rem_euclid(24.0);
    }

    /// The message telling a player the time.
    pub fn message(&self, rules: &GameRules) -> ServerMessage {
        ServerMessage::Time { hour: self.hour(rules) }
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
