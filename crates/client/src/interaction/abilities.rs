//! Using abilities: asking the server ([`ClientMessage::UseAbility`]) for the
//! selected target, predicting the run up to it, and keeping track of
//! cooldowns so the hotbar can show them.
//!
//! Anything that uses an ability (a hotbar key, clicking it in the abilities tab)
//! writes a [`UseAbility`] message. The obvious refusals (still cooling down,
//! not enough mana, nothing selected) are explained at once without asking;
//! the server checks everything again and explains the rest.

use std::collections::HashMap;

use bevy::prelude::*;
use venture_content::AbilityEffect;
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

/// Uses abilities and tracks their cooldowns.
pub struct AbilityUsePlugin;

impl Plugin for AbilityUsePlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<UseAbility>()
            .init_resource::<AbilityCooldowns>()
            .add_systems(
                Update,
                (track_cooldowns, use_once_if_requested, use_abilities).chain().run_if(in_state(ClientState::Playing)),
            )
            .add_systems(OnExit(ClientState::Playing), |mut cooldowns: ResMut<AbilityCooldowns>| cooldowns.0.clear());
    }
}

/// Asks to use an ability (its id) on the selected target, if it needs one.
#[derive(Message, Clone, Debug)]
pub struct UseAbility(pub String);

/// When each of the player's abilities can be used again: (ready at, cooldown
/// length), in seconds of game time.
#[derive(Resource, Default)]
pub struct AbilityCooldowns(HashMap<String, (f32, f32)>);

impl AbilityCooldowns {
    /// Seconds until an ability is ready, and how much of its cooldown is left
    /// (from 1 down to 0), if it's cooling down.
    pub fn remaining(&self, ability: &str, now: f32) -> Option<(f32, f32)> {
        let &(ready_at, length) = self.0.get(ability)?;
        let left = ready_at - now;
        (left > 0.0).then(|| (left, (left / length.max(0.001)).min(1.0)))
    }
}

/// Starts an ability's cooldown whenever the server says the player used it.
fn track_cooldowns(
    time: Res<Time>,
    content: Res<ContentStore>,
    mut events: MessageReader<FromServer>,
    player: Option<Single<&LocalPlayer>>,
    mut cooldowns: ResMut<AbilityCooldowns>,
) {
    let Some(player) = player else { return };
    for FromServer(message) in events.read() {
        if let ServerMessage::AbilityUsed { user, ability, .. } = message
            && *user == player.id
            && let Some(def) = content.abilities.get(ability).filter(|def| def.cooldown > 0.0)
        {
            cooldowns.0.insert(ability.clone(), (time.elapsed_secs() + def.cooldown, def.cooldown));
        }
    }
}

/// For development: uses the `VENTURE_USE` abilities once the world around the
/// player has loaded (and a target is selected, if one is to be).
fn use_once_if_requested(
    dev: Res<DevSettings>,
    progress: Res<StreamingProgress>,
    target: Res<Target>,
    mut used: Local<bool>,
    mut requests: MessageWriter<UseAbility>,
) {
    let waiting_for_target = dev.select_nearest && target.0.is_none();
    if *used || dev.use_abilities.is_empty() || !progress.measured || progress.waiting > 0 || waiting_for_target {
        return;
    }
    for ability in &dev.use_abilities {
        requests.write(UseAbility(ability.clone()));
    }
    *used = true;
}

fn use_abilities(
    time: Res<Time>,
    mut requests: MessageReader<UseAbility>,
    content: Res<ContentStore>,
    cooldowns: Res<AbilityCooldowns>,
    vitals: Res<Vitals>,
    target: Res<Target>,
    targetables: Query<&Targetable>,
    mut orders: Orders,
    mut system: MessageWriter<SystemMessage>,
) {
    let mut say = |text: String| {
        system.write(SystemMessage(text));
    };
    for UseAbility(ability) in requests.read() {
        let Some(def) = content.abilities.get(ability) else { continue };
        if cooldowns.remaining(ability, time.elapsed_secs()).is_some() {
            say(format!("{} isn't ready yet.", def.name));
            continue;
        }
        if vitals.0.is_some_and(|vitals| vitals.mana < def.mana) {
            say(format!("You don't have enough mana for {}.", def.name));
            continue;
        }
        if !def.needs_target() {
            orders.use_ability_on_self(ability, def.effect == AbilityEffect::Rest);
            continue;
        }
        let Some((entity, targetable)) = target.0.and_then(|entity| Some((entity, *targetables.get(entity).ok()?)))
        else {
            say(format!("Select a creature to use {} on first.", def.name));
            continue;
        };
        if targetable.kind == TargetKind::Enemy {
            let melee = matches!(def.effect, AbilityEffect::Strike { .. });
            orders.use_ability_on(entity, targetable.id, ability, def.reach(), melee);
        } else {
            // The server will explain why not.
            orders.ask(venture_protocol::ClientMessage::UseAbility {
                ability: ability.clone(),
                target: Some(targetable.id),
            });
        }
    }
}
