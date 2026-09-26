//! Using skills: asking the server ([`ClientMessage::UseSkill`]) for the
//! selected target, predicting the run up to it, and keeping track of
//! cooldowns so the hotbar can show them.
//!
//! Anything that uses a skill (a hotbar key, clicking it in the skills tab)
//! writes a [`UseSkill`] message. The obvious refusals (still cooling down,
//! not enough mana, nothing selected) are explained at once without asking;
//! the server checks everything again and explains the rest.

use std::collections::HashMap;

use bevy::prelude::*;
use venture_content::SkillEffect;
use venture_protocol::ServerMessage;

use crate::characters::player::LocalPlayer;
use crate::content::ContentStore;
use crate::dev::settings::DevSettings;
use crate::interaction::orders::Orders;
use crate::interaction::targeting::{Target, TargetKind, Targetable};
use crate::network::{ClientState, FromServer};
use crate::ui::chat::SystemMessage;
use crate::ui::vitals::Vitals;
use crate::world::streaming::StreamingProgress;

/// Uses skills and tracks their cooldowns.
pub struct SkillUsePlugin;

impl Plugin for SkillUsePlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<UseSkill>()
            .init_resource::<SkillCooldowns>()
            .add_systems(
                Update,
                (track_cooldowns, use_once_if_requested, use_skills).chain().run_if(in_state(ClientState::Playing)),
            )
            .add_systems(OnExit(ClientState::Playing), |mut cooldowns: ResMut<SkillCooldowns>| cooldowns.0.clear());
    }
}

/// Asks to use a skill (its id) on the selected target, if it needs one.
#[derive(Message, Clone, Debug)]
pub struct UseSkill(pub String);

/// When each of the player's skills can be used again: (ready at, cooldown
/// length), in seconds of game time.
#[derive(Resource, Default)]
pub struct SkillCooldowns(HashMap<String, (f32, f32)>);

impl SkillCooldowns {
    /// Seconds until a skill is ready, and how much of its cooldown is left
    /// (from 1 down to 0), if it's cooling down.
    pub fn remaining(&self, skill: &str, now: f32) -> Option<(f32, f32)> {
        let &(ready_at, length) = self.0.get(skill)?;
        let left = ready_at - now;
        (left > 0.0).then(|| (left, (left / length.max(0.001)).min(1.0)))
    }
}

/// Starts a skill's cooldown whenever the server says the player used it.
fn track_cooldowns(
    time: Res<Time>,
    content: Res<ContentStore>,
    mut events: MessageReader<FromServer>,
    player: Option<Single<&LocalPlayer>>,
    mut cooldowns: ResMut<SkillCooldowns>,
) {
    let Some(player) = player else { return };
    for FromServer(message) in events.read() {
        if let ServerMessage::SkillUsed { user, skill, .. } = message
            && *user == player.id
            && let Some(def) = content.skills.get(skill).filter(|def| def.cooldown > 0.0)
        {
            cooldowns.0.insert(skill.clone(), (time.elapsed_secs() + def.cooldown, def.cooldown));
        }
    }
}

/// For development: uses the `VENTURE_USE` skills once the world around the
/// player has loaded (and a target is selected, if one is to be).
fn use_once_if_requested(
    dev: Res<DevSettings>,
    progress: Res<StreamingProgress>,
    target: Res<Target>,
    mut used: Local<bool>,
    mut requests: MessageWriter<UseSkill>,
) {
    let waiting_for_target = dev.select_nearest && target.0.is_none();
    if *used || dev.use_skills.is_empty() || !progress.measured || progress.waiting > 0 || waiting_for_target {
        return;
    }
    for skill in &dev.use_skills {
        requests.write(UseSkill(skill.clone()));
    }
    *used = true;
}

fn use_skills(
    time: Res<Time>,
    mut requests: MessageReader<UseSkill>,
    content: Res<ContentStore>,
    cooldowns: Res<SkillCooldowns>,
    vitals: Res<Vitals>,
    target: Res<Target>,
    targetables: Query<&Targetable>,
    mut orders: Orders,
    mut system: MessageWriter<SystemMessage>,
) {
    let mut say = |text: String| {
        system.write(SystemMessage(text));
    };
    for UseSkill(skill) in requests.read() {
        let Some(def) = content.skills.get(skill) else { continue };
        if cooldowns.remaining(skill, time.elapsed_secs()).is_some() {
            say(format!("{} isn't ready yet.", def.name));
            continue;
        }
        if vitals.0.is_some_and(|vitals| vitals.mana < def.mana) {
            say(format!("You don't have enough mana for {}.", def.name));
            continue;
        }
        if !def.needs_target() {
            orders.use_skill_on_self(skill, def.effect == SkillEffect::Rest);
            continue;
        }
        let Some((entity, targetable)) = target.0.and_then(|entity| Some((entity, *targetables.get(entity).ok()?)))
        else {
            say(format!("Select a creature to use {} on first.", def.name));
            continue;
        };
        if targetable.kind == TargetKind::Enemy {
            let melee = matches!(def.effect, SkillEffect::Strike { .. });
            orders.use_skill_on(entity, targetable.id, skill, def.reach(), melee);
        } else {
            // The server will explain why not.
            orders.ask(venture_protocol::ClientMessage::UseSkill { skill: skill.clone(), target: Some(targetable.id) });
        }
    }
}
