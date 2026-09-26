//! Using skills: the only code that does.
//!
//! A player asks to use a skill (a [`SkillRequest`] from the gate). It's
//! checked here: do they know it, is it ready (cooldowns), can they pay for
//! it (mana), and is the target something they may attack? Then:
//! - **Strike** (Whack): a weapon blow with the skill's own dice. If the
//!   target is out of reach, the player runs up first ([`PendingSkill`],
//!   moved by [`crate::gameplay`]), then auto-attacks after it.
//! - **Bolt** (Air Blast): a spell that flies to the target at its speed, and
//!   lands (rolled like any attack) when it gets there. Out of range, the
//!   player runs into range first.
//! - **Heal** (Second Wind): heals the player.
//! - **Rest**: sits down (see [`Resting`]); using it again stands up.
//!
//! Hits are resolved by [`crate::combat`], like any attack ([`Strike`]).
//! Everyone nearby is told a skill was used ([`ServerMessage::SkillUsed`]) so
//! their clients can show it.

use std::collections::HashMap;

use bevy::prelude::*;
use venture_content::{SkillDef, SkillEffect};
use venture_navigation::Point;
use venture_protocol::{EntityId, ServerMessage};
use venture_rpg::Power;
use venture_simulation::Follow;

use crate::GameRules;
use crate::ServerSystems;
use crate::character::{Character, class_or_default};
use crate::combat::{Attacking, Health, Mana, Resting, SkillHit, Strike};
use crate::content::LoadedContent;
use crate::creatures::{Creature, Dead};
use crate::entities::{DisplayName, NetworkId, Player, Position};
use crate::equipment::Equipment;
use crate::link::{ConnectionId, Link};
use crate::requests::SkillRequest;

/// Players within this distance (in metres) see a skill being used.
const SKILL_VIEW_RANGE: f32 = 60.0;
/// After a weapon skill starts a fight, how long until the first ordinary
/// swing, in seconds (the skill was the first blow).
const SWING_AFTER_SKILL: f32 = 0.8;

/// Runs skills.
pub struct SkillsPlugin;

impl Plugin for SkillsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            FixedUpdate,
            (tick_cooldowns, use_skills, use_pending_skills, fly_bolts).chain().in_set(ServerSystems::Skills),
        );
    }
}

/// How long until each of a player's skills can be used again, in seconds
/// (skills not listed are ready).
#[derive(Component, Default)]
pub struct Cooldowns(HashMap<String, f32>);

impl Cooldowns {
    /// Seconds until this skill is ready (0 if it is).
    fn remaining(&self, skill: &str) -> f32 {
        self.0.get(skill).copied().unwrap_or(0.0)
    }
}

/// A skill waiting for its user to get close enough to the target (they run
/// up to it: see [`crate::gameplay`]).
#[derive(Component)]
pub struct PendingSkill {
    /// Which skill (its id).
    pub skill: String,
    /// What it's for.
    pub target: Entity,
    /// How close they need to be, in metres.
    pub reach: f32,
    /// Keeps them heading for the target.
    pub follow: Follow,
}

/// A spell flying to its target (not seen by clients as an entity: they draw
/// it from [`ServerMessage::SkillUsed`]).
#[derive(Component)]
struct BoltInFlight {
    attacker: Entity,
    target: Entity,
    hit: SkillHit,
    /// Seconds until it lands.
    remaining: f32,
}

/// Counts cooldowns down.
fn tick_cooldowns(time: Res<Time>, mut players: Query<&mut Cooldowns>) {
    let dt = time.delta_secs();
    for mut cooldowns in &mut players {
        if cooldowns.0.is_empty() {
            continue;
        }
        cooldowns.0.retain(|_, remaining| {
            *remaining -= dt;
            *remaining > 0.0
        });
    }
}

/// Everything using skills needs.
#[derive(bevy::ecs::system::SystemParam)]
struct Casting<'w, 's> {
    commands: Commands<'w, 's>,
    content: Res<'w, LoadedContent>,
    rules: Res<'w, GameRules>,
    link: ResMut<'w, Link>,
    strikes: MessageWriter<'w, Strike>,
    users: Query<
        'w,
        's,
        (
            &'static Player,
            &'static NetworkId,
            &'static Position,
            &'static Character,
            &'static Equipment,
            &'static mut Cooldowns,
            &'static mut Mana,
            &'static mut Health,
            Has<Resting>,
            Option<&'static mut Attacking>,
        ),
    >,
    targets: Query<'w, 's, (Entity, &'static NetworkId, &'static Position, &'static DisplayName)>,
    creatures: Query<'w, 's, (), (With<Creature>, Without<Dead>)>,
    players: Query<'w, 's, (), With<Player>>,
    viewers: Query<'w, 's, (&'static Player, &'static Position)>,
}

/// Handles each request to use a skill.
fn use_skills(mut requests: MessageReader<SkillRequest>, mut casting: Casting) {
    for request in requests.read() {
        casting.request(request.player, &request.skill, request.target);
    }
}

/// Uses skills whose users have got close enough to their targets.
fn use_pending_skills(
    mut casting: Casting,
    pending: Query<(Entity, &PendingSkill)>,
    positions: Query<&Position, Without<Dead>>,
) {
    for (player, waiting) in &pending {
        let Ok(&Position(target_at)) = positions.get(waiting.target) else {
            // The target has gone (or died): nothing to do.
            casting.commands.entity(player).remove::<PendingSkill>();
            continue;
        };
        let Some(at) = casting.position_of(player) else { continue };
        if distance(at, target_at) > waiting.reach {
            continue;
        }
        casting.commands.entity(player).remove::<PendingSkill>();
        let Some(def) = casting.content.pack.skills.get(&waiting.skill).cloned() else { continue };
        if casting.can_afford(player, &def) {
            casting.fire(player, &def, Some(waiting.target));
        }
    }
}

/// Moves spells along, and lands those that arrive.
fn fly_bolts(
    mut commands: Commands,
    time: Res<Time>,
    mut strikes: MessageWriter<Strike>,
    mut bolts: Query<(Entity, &mut BoltInFlight)>,
    alive: Query<(), Without<Dead>>,
) {
    let dt = time.delta_secs();
    for (entity, mut bolt) in &mut bolts {
        bolt.remaining -= dt;
        if !alive.contains(bolt.target) {
            commands.entity(entity).despawn();
        } else if bolt.remaining <= 0.0 {
            strikes.write(Strike { attacker: bolt.attacker, target: bolt.target, skill: Some(bolt.hit.clone()) });
            commands.entity(entity).despawn();
        }
    }
}

impl Casting<'_, '_> {
    /// Checks a request to use a skill, and uses it (or sets off towards the target).
    fn request(&mut self, player: Entity, skill: &str, target: Option<EntityId>) {
        let Ok((me, _, &Position(at), character, _, cooldowns, _, health, resting, _)) = self.users.get(player) else {
            return;
        };
        let (connection, in_combat) = (me.connection, health.in_combat());
        let Some(def) = self.content.pack.skills.get(skill).cloned() else { return };
        let cooling_down = cooldowns.remaining(&def.id) > 0.0;
        if !character.skills.contains(&def.id) {
            return self.tell(connection, format!("You don't know {}.", def.name));
        }
        // Rest again to stand up.
        if def.effect == SkillEffect::Rest && resting {
            self.commands.entity(player).remove::<Resting>();
            return;
        }
        if cooling_down {
            return self.tell(connection, format!("{} isn't ready yet.", def.name));
        }
        if !self.can_afford(player, &def) {
            return;
        }
        match def.effect {
            SkillEffect::Rest => {
                if in_combat {
                    return self.tell(connection, "You can't rest in the middle of a fight.".to_string());
                }
                self.fire(player, &def, None);
            }
            SkillEffect::Heal { .. } => self.fire(player, &def, None),
            SkillEffect::Strike { .. } | SkillEffect::Bolt { .. } => {
                let Some(target_id) = target else {
                    return self.tell(connection, format!("Select a creature to use {} on first.", def.name));
                };
                // An unknown id is something that has just gone: nothing to say.
                let Some((target, _, &Position(target_at), name)) =
                    self.targets.iter().find(|(_, id, ..)| id.0 == target_id)
                else {
                    return;
                };
                let name = name.0.clone();
                if self.players.contains(target) {
                    return self.tell(connection, format!("You can't attack {name}. Players can't fight each other."));
                }
                if !self.creatures.contains(target) {
                    return self.tell(connection, format!("You can't attack {name}."));
                }
                let gap = distance(at, target_at);
                if gap > self.rules.0.max_move_distance {
                    return self.tell(connection, format!("The {name} is too far away."));
                }
                if gap <= def.reach() {
                    self.fire(player, &def, Some(target));
                } else {
                    // Run up to it first (see `gameplay::update_casters`).
                    self.commands.entity(player).remove::<Resting>().insert(PendingSkill {
                        skill: def.id.clone(),
                        target,
                        reach: def.reach(),
                        follow: Follow::default(),
                    });
                    self.keep_attacking(player, &def, target);
                }
            }
        }
    }

    /// Whether a player has the mana for a skill (telling them if not).
    fn can_afford(&mut self, player: Entity, def: &SkillDef) -> bool {
        let Ok((me, .., mana, _, _, _)) = self.users.get(player) else { return false };
        if mana.current() >= def.mana {
            return true;
        }
        let connection = me.connection;
        self.tell(connection, format!("You don't have enough mana for {}.", def.name));
        false
    }

    /// Uses a skill (already checked): pays for it, starts its cooldown,
    /// tells everyone nearby, and does what it does.
    fn fire(&mut self, player: Entity, def: &SkillDef, target: Option<Entity>) {
        let pack = &self.content.pack;
        let Ok((_, &NetworkId(user_id), &Position(at), character, equipment, mut cooldowns, mut mana, mut health, ..)) =
            self.users.get_mut(player)
        else {
            return;
        };
        if !mana.spend(def.mana) {
            return;
        }
        if def.cooldown > 0.0 {
            cooldowns.0.insert(def.id.clone(), def.cooldown);
        }
        let class = class_or_default(pack, Some(&character.class));
        let derived = character.derived(&class, &equipment.gear(&pack.items));
        let healed = match def.effect {
            SkillEffect::Heal { amount } => {
                Some(health.heal(derived.healing(amount, &mut |sides| fastrand::u32(1..=sides.max(1)))))
            }
            _ => None,
        };

        let target_id = target.and_then(|target| self.targets.get(target).ok()).map(|(_, id, ..)| id.0);
        self.tell_nearby(at, &ServerMessage::SkillUsed { user: user_id, skill: def.id.clone(), target: target_id });
        match def.effect {
            SkillEffect::Rest => {
                // Sitting down stops them (see `gameplay::sit_down`).
                self.commands.entity(player).insert(Resting);
            }
            SkillEffect::Heal { .. } => {
                self.commands.entity(player).remove::<Resting>();
                if let Some(amount) = healed {
                    self.tell_nearby(at, &ServerMessage::Healed { id: user_id, amount });
                }
            }
            SkillEffect::Strike { damage } => {
                let Some(target) = target else { return };
                let hit = SkillHit { skill: def.id.clone(), damage, power: Power::Weapon };
                self.strikes.write(Strike { attacker: player, target, skill: Some(hit) });
                self.keep_attacking(player, def, target);
            }
            SkillEffect::Bolt { damage, speed } => {
                let Some(target) = target else { return };
                let Ok((_, _, &Position(target_at), _)) = self.targets.get(target) else { return };
                let hit = SkillHit { skill: def.id.clone(), damage, power: Power::Spell };
                let remaining = distance(at, target_at) / speed;
                self.commands.spawn(BoltInFlight { attacker: player, target, hit, remaining });
            }
        }
    }

    /// After (or on the way to) a weapon skill, the player goes on
    /// attacking the target. The skill is a swing, so the next ordinary one
    /// waits a moment.
    fn keep_attacking(&mut self, player: Entity, def: &SkillDef, target: Entity) {
        if !matches!(def.effect, SkillEffect::Strike { .. }) {
            return;
        }
        let Ok((.., attacking)) = self.users.get_mut(player) else { return };
        match attacking {
            Some(mut attacking) if attacking.target == target => {
                attacking.cooldown = attacking.cooldown.max(SWING_AFTER_SKILL);
            }
            _ => {
                let attacking = Attacking { target, cooldown: SWING_AFTER_SKILL, follow: Follow::default() };
                self.commands.entity(player).insert(attacking);
            }
        }
    }

    /// Where a player is.
    fn position_of(&self, player: Entity) -> Option<Point> {
        self.users.get(player).ok().map(|(_, _, position, ..)| position.0)
    }

    fn tell(&mut self, connection: ConnectionId, text: String) {
        self.link.send(connection, &ServerMessage::Notice { text });
    }

    /// Tells every player near a spot.
    fn tell_nearby(&mut self, at: Point, message: &ServerMessage) {
        let nearby: Vec<_> = self
            .viewers
            .iter()
            .filter(|(_, viewer)| distance(viewer.0, at) <= SKILL_VIEW_RANGE)
            .map(|(player, _)| player.connection)
            .collect();
        self.link.broadcast(nearby, message);
    }
}

fn distance(a: Point, b: Point) -> f32 {
    (a.0 - b.0).hypot(a.1 - b.1)
}
