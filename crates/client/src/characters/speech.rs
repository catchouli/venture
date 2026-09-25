//! What characters have said recently, shown in speech bubbles above their
//! heads (see [`crate::characters::nameplates`]). Each line disappears after
//! a while; the chat box keeps the full history.

use std::collections::VecDeque;

use bevy::prelude::*;
use venture_protocol::ServerMessage;

use crate::characters::player::LocalPlayer;
use crate::interaction::targeting::{TargetKind, Targetable};
use crate::network::FromServer;

/// How long each line stays above a character's head, in seconds.
const SHOW_SECONDS: f32 = 8.0;
/// The most lines shown above one character at once.
const MAX_LINES: usize = 3;
/// Longer lines are cut to this many characters (plus "...").
const MAX_CHARS: usize = 60;

/// Keeps track of what characters have said recently.
pub struct SpeechPlugin;

impl Plugin for SpeechPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (record_speech, forget_old_speech).chain());
    }
}

/// A character's recent lines, oldest first.
#[derive(Component, Default)]
pub struct Speech {
    /// The lines still showing.
    pub lines: VecDeque<SpeechLine>,
}

/// One recent line.
pub struct SpeechLine {
    /// What was said (shortened if long).
    pub text: String,
    /// When it stops showing, in seconds since startup.
    expires_at: f32,
}

/// Adds each chat message to the speaker's recent lines.
fn record_speech(
    time: Res<Time>,
    mut events: MessageReader<FromServer>,
    me: Option<Single<(Entity, &LocalPlayer)>>,
    others: Query<(Entity, &Targetable)>,
    mut speech: Query<&mut Speech>,
) {
    for FromServer(message) in events.read() {
        let ServerMessage::Chat { from, text, .. } = message else { continue };
        let speaker = match &me {
            Some(me) if me.1.id == *from => Some(me.0),
            _ => others.iter().find(|(_, t)| t.0 == TargetKind::Player(*from)).map(|(entity, _)| entity),
        };
        let Some(mut lines) = speaker.and_then(|entity| speech.get_mut(entity).ok()) else { continue };
        lines.lines.push_back(SpeechLine { text: shorten(text), expires_at: time.elapsed_secs() + SHOW_SECONDS });
        if lines.lines.len() > MAX_LINES {
            lines.lines.pop_front();
        }
    }
}

/// Removes lines whose time is up.
fn forget_old_speech(time: Res<Time>, mut speech: Query<&mut Speech>) {
    let now = time.elapsed_secs();
    for mut lines in &mut speech {
        // Only touch (and so mark as changed) characters with something to remove.
        if lines.lines.front().is_some_and(|line| line.expires_at <= now) {
            lines.lines.retain(|line| line.expires_at > now);
        }
    }
}

/// Cuts a line to [`MAX_CHARS`] characters, adding "..." if anything was cut.
/// (Three dots rather than "…", which the default font doesn't have.)
fn shorten(text: &str) -> String {
    if text.chars().count() <= MAX_CHARS {
        return text.to_string();
    }
    let cut: String = text.chars().take(MAX_CHARS).collect();
    format!("{}...", cut.trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_lines_are_shortened() {
        assert_eq!(shorten("hello"), "hello");
        let long = "a".repeat(100);
        let short = shorten(&long);
        assert_eq!(short.chars().count(), MAX_CHARS + 3);
        assert!(short.ends_with("..."));
    }
}
