//! Quests: how far each player has got in each of the content's quests.
//!
//! A [`QuestLog`] only moves forward, through [`QuestLog::advance`]: setting
//! a quest to a stage it has already reached (or passed) does nothing. So a
//! conversation can't hand out a quest's reward twice by going back a stage.
//! Dialogue moves quests on (see [`crate::dialogue`]), and a changed log is
//! sent to its player as their journal automatically.

use std::collections::BTreeMap;

use bevy::prelude::*;
use venture_content::QuestDef;
use venture_protocol::{QuestEntry, ServerMessage};

use crate::ServerSystems;
use crate::content::LoadedContent;
use crate::entities::Player;
use crate::link::Link;

/// Sends players their journals when they change.
pub struct QuestsPlugin;

impl Plugin for QuestsPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<QuestAdvanced>()
            .add_systems(FixedUpdate, send_changed_journals.in_set(ServerSystems::Updates));
    }
}

/// A player's progress in their quests.
#[derive(Component, Clone, Debug, Default, PartialEq)]
pub struct QuestLog {
    /// (quest id, stage id) for each quest started, in the order they were started.
    progress: Vec<(String, String)>,
}

/// How a quest moved on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuestProgress {
    /// It has just started.
    Started,
    /// It has moved on to a later stage.
    Advanced,
    /// It has reached its last stage.
    Completed,
}

/// A player's quest has moved on, in a conversation with an NPC (so the
/// NPC's script can react: see [`crate::npcs`]).
#[derive(Message, Clone, Debug)]
pub struct QuestAdvanced {
    /// The NPC they were talking to.
    pub npc: Entity,
    /// The player.
    pub player: Entity,
    /// The quest's id.
    pub quest: String,
    /// The stage it has reached.
    pub stage: String,
}

impl QuestLog {
    /// A log from a saved profile.
    pub fn from_saved(progress: Vec<(String, String)>) -> Self {
        Self { progress }
    }

    /// What to save: (quest id, stage id) for each quest started.
    pub fn saved(&self) -> Vec<(String, String)> {
        self.progress.clone()
    }

    /// The stage the player has reached in a quest, if they've started it.
    pub fn stage(&self, quest: &str) -> Option<&str> {
        self.progress.iter().find(|(id, _)| id == quest).map(|(_, stage)| stage.as_str())
    }

    /// Moves a quest on to a stage, if that's forward from where the player
    /// is (starting the quest if need be). Returns how it moved, or `None` if
    /// it didn't (or the stage isn't one of the quest's).
    pub fn advance(&mut self, quest: &QuestDef, stage: &str) -> Option<QuestProgress> {
        let new_index = quest.stage_index(stage)?;
        let progress = match self.progress.iter().position(|(id, _)| *id == quest.id) {
            Some(entry) => {
                // A stage the content no longer has counts as not started.
                let old_index = quest.stage_index(&self.progress[entry].1);
                if old_index.is_some_and(|old| old >= new_index) {
                    return None;
                }
                self.progress[entry].1 = stage.to_string();
                QuestProgress::Advanced
            }
            None => {
                self.progress.push((quest.id.clone(), stage.to_string()));
                QuestProgress::Started
            }
        };
        Some(if quest.is_final(stage) { QuestProgress::Completed } else { progress })
    }

    /// The journal, as the player sees it. Quests the content no longer has are left out.
    pub fn journal(&self, quests: &BTreeMap<String, QuestDef>) -> Vec<QuestEntry> {
        self.progress
            .iter()
            .filter_map(|(quest, stage)| {
                let def = quests.get(quest)?;
                let journal = def.stages.get(def.stage_index(stage)?)?.journal.clone();
                Some(QuestEntry { name: def.name.clone(), journal, complete: def.is_final(stage) })
            })
            .collect()
    }
}

/// What to tell a player when a quest moves on.
pub fn progress_notice(progress: QuestProgress, quest: &QuestDef) -> String {
    match progress {
        QuestProgress::Started => format!("New quest: {}.", quest.name),
        QuestProgress::Advanced => format!("Quest updated: {}.", quest.name),
        QuestProgress::Completed => format!("Quest complete: {}!", quest.name),
    }
}

fn send_changed_journals(
    content: Res<LoadedContent>,
    mut link: ResMut<Link>,
    players: Query<(&Player, &QuestLog), Changed<QuestLog>>,
) {
    for (player, log) in &players {
        link.send(player.connection, &ServerMessage::Quests { quests: log.journal(&content.pack.quests) });
    }
}

#[cfg(test)]
mod tests {
    use venture_content::QuestStage;

    use super::*;

    fn pipe_quest() -> QuestDef {
        let stage = |id: &str| QuestStage { id: id.to_string(), journal: format!("Stage {id}.") };
        QuestDef {
            id: "pipe".to_string(),
            name: "The Pipe".to_string(),
            stages: vec![stage("asked"), stage("found"), stage("done")],
        }
    }

    #[test]
    fn quests_only_move_forward() {
        let quest = pipe_quest();
        let mut log = QuestLog::default();
        assert_eq!(log.stage("pipe"), None);
        assert_eq!(log.advance(&quest, "asked"), Some(QuestProgress::Started));
        assert_eq!(log.advance(&quest, "asked"), None, "already there");
        assert_eq!(log.advance(&quest, "found"), Some(QuestProgress::Advanced));
        assert_eq!(log.advance(&quest, "asked"), None, "no going back");
        assert_eq!(log.advance(&quest, "done"), Some(QuestProgress::Completed));
        assert_eq!(log.advance(&quest, "done"), None, "so the reward can't be claimed twice");
        assert_eq!(log.advance(&quest, "missing"), None);
    }

    #[test]
    fn a_quest_can_be_completed_straight_away() {
        let mut log = QuestLog::default();
        assert_eq!(log.advance(&pipe_quest(), "done"), Some(QuestProgress::Completed));
    }

    #[test]
    fn the_journal_shows_each_quests_current_stage() {
        let quest = pipe_quest();
        let quests = BTreeMap::from([("pipe".to_string(), quest.clone())]);
        let mut log = QuestLog::from_saved(vec![("gone".to_string(), "x".to_string())]);
        log.advance(&quest, "found");
        assert_eq!(
            log.journal(&quests),
            [QuestEntry { name: "The Pipe".to_string(), journal: "Stage found.".to_string(), complete: false }]
        );
    }
}
