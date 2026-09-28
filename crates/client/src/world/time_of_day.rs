//! The time of day, as the server says ([`ServerMessage::Time`], when joining
//! and when an admin sets it or its pace), run on by the client at the pace
//! of the day cycle in the rules, times the server's timescale. [`crate::world::lighting`] lights the world by it.

use bevy::prelude::*;
use venture_protocol::ServerMessage;

use crate::characters::player::GameRules;
use crate::network::FromServer;

/// The time before the server has said (mid-afternoon: the loading screen's
/// world and the editor look their usual selves).
pub const DEFAULT_HOUR: f32 = 15.0;

/// Keeps the time of day.
pub struct TimeOfDayPlugin;

impl Plugin for TimeOfDayPlugin {
    fn build(&self, app: &mut App) {
        // (The rules say how fast the day goes: the defaults until the server
        // says, and in the editor, which has no server.)
        app.init_resource::<TimeOfDay>().init_resource::<GameRules>().add_systems(Update, keep_time);
    }
}

/// The time of day, in hours (0 to 24).
#[derive(Resource)]
pub struct TimeOfDay(pub f32);

impl Default for TimeOfDay {
    fn default() -> Self {
        Self(DEFAULT_HOUR)
    }
}

/// How fast the clock runs, once a server has said (it only runs once one
/// has: the editor, with no server, stays at mid-afternoon).
#[derive(Default)]
struct Told(Option<f32>);

fn keep_time(
    time: Res<Time>,
    rules: Res<GameRules>,
    // (None in the editor, which has no connection.)
    events: Option<MessageReader<FromServer>>,
    mut clock: ResMut<TimeOfDay>,
    mut told_before: Local<Told>,
) {
    let Some(mut events) = events else { return };
    let told = events.read().filter_map(|FromServer(message)| match message {
        ServerMessage::Time { hour, scale } => Some((*hour, *scale)),
        _ => None,
    });
    if let Some((hour, scale)) = told.last() {
        clock.0 = hour;
        told_before.0 = Some(scale);
    } else if let Some(scale) = told_before.0 {
        clock.0 = rules.0.day.after(clock.0, time.delta_secs() * scale);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_clock_stands_still_without_a_connection() {
        // As in the editor: no network plugin, so no server messages.
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, TimeOfDayPlugin));
        app.update();
        app.update();
        assert_eq!(app.world().resource::<TimeOfDay>().0, DEFAULT_HOUR);
    }
}
