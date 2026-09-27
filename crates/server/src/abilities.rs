//! Using abilities: the only code that does.
//!
//! A player asks to use an ability (a [`AbilityRequest`] from the gate). It's
//! checked here: do they know it, is it ready (cooldowns), can they pay for
//! it (mana), and is the target something they may attack? Then:
//! - **Strike** (Whack): a weapon blow with the ability's own dice. If the
//!   target is out of reach, the player runs up first ([`PendingAbility`],
//!   moved by [`crate::gameplay`]), then auto-attacks after it.
//! - **Bolt** (Air Blast): a spell that flies to the target at its speed, and
//!   lands (rolled like any attack) when it gets there. Out of range, the
//!   player runs into range first.
//! - **Heal** (Second Wind): heals the player.
//! - **Rest**: sits down (see [`Resting`]); using it again stands up.
//!
//! Hits are resolved by [`crate::combat`], like any attack ([`Strike`]).
//! Everyone nearby is told an ability was used ([`ServerMessage::AbilityUsed`]) so
//! their clients can show it.

use std::collections::HashMap;

use bevy::prelude::*;
use venture_content::{AbilityDef, AbilityEffect};
use venture_navigation::Point;
use venture_protocol::{EntityId, ServerMessage};
use venture_rpg::Power;
use venture_simulation::Follow;

use crate::GameRules;
use crate::ServerSystems;
use crate::character::{Character, class_or_default};
use crate::combat::{AbilityHit, Attacking, Health, Mana, Resting, Strike};
use crate::content::LoadedContent;
use crate::entities::{DisplayName, NetworkId, Player, Position};
use crate::equipment::Equipment;
use crate::link::{ConnectionId, Link};
use crate::monsters::{Dead, Monster};
use crate::requests::AbilityRequest;

/// Players within this distance (in metres) see an ability being used.
const ABILITY_VIEW_RANGE: f32 = 60.0;
/// After a weapon ability starts a fight, how long until the first ordinary
/// swing, in seconds (the ability was the first blow).
const SWING_AFTER_ABILITY: f32 = 0.8;

/// Runs abilities.
pub struct AbilitiesPlugin;

impl Plugin for AbilitiesPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            FixedUpdate,
            (tick_cooldowns, use_abilities, use_pending_abilities, fly_bolts).chain().in_set(ServerSystems::Abilities),
        );
    }
}

/// How long until each of a player's abilities can be used again, in seconds
/// (abilities not listed are ready).
#[derive(Component, Default)]
pub struct Cooldowns(HashMap<String, f32>);

impl Cooldowns {
    /// Seconds until this ability is ready (0 if it is).
    fn remaining(&self, ability: &str) -> f32 {
        self.0.get(ability).copied().unwrap_or(0.0)
    }
}

/// An ability waiting for its user to get close enough to the target (they run
/// up to it: see [`crate::gameplay`]).
#[derive(Component)]
pub struct PendingAbility {
    /// Which ability (its id).
    pub ability: String,
    /// What it's for.
    pub target: Entity,
    /// How close they need to be, in metres.
    pub reach: f32,
    /// Keeps them heading for the target.
    pub follow: Follow,
}

/// A spell flying to its target (not seen by clients as an entity: they draw
/// it from [`ServerMessage::AbilityUsed`]).
#[derive(Component)]
struct BoltInFlight {
    attacker: Entity,
    target: Entity,
    hit: AbilityHit,
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

/// Everything using abilities needs.
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
    monsters: Query<'w, 's, (), (With<Monster>, Without<Dead>)>,
    players: Query<'w, 's, (), With<Player>>,
    viewers: Query<'w, 's, (&'static Player, &'static Position)>,
}

/// Handles each request to use an ability.
fn use_abilities(mut requests: MessageReader<AbilityRequest>, mut casting: Casting) {
    for request in requests.read() {
        casting.request(request.player, &request.ability, request.target);
    }
}

/// Uses abilities whose users have got close enough to their targets.
fn use_pending_abilities(
    mut casting: Casting,
    pending: Query<(Entity, &PendingAbility)>,
    positions: Query<&Position, Without<Dead>>,
) {
    for (player, waiting) in &pending {
        let Ok(&Position(target_at)) = positions.get(waiting.target) else {
            // The target has gone (or died): nothing to do.
            casting.commands.entity(player).remove::<PendingAbility>();
            continue;
        };
        let Some(at) = casting.position_of(player) else { continue };
        if distance(at, target_at) > waiting.reach {
            continue;
        }
        casting.commands.entity(player).remove::<PendingAbility>();
        let Some(def) = casting.content.pack.abilities.get(&waiting.ability).cloned() else { continue };
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
            strikes.write(Strike { attacker: bolt.attacker, target: bolt.target, ability: Some(bolt.hit.clone()) });
            commands.entity(entity).despawn();
        }
    }
}

impl Casting<'_, '_> {
    /// Checks a request to use an ability, and uses it (or sets off towards the target).
    fn request(&mut self, player: Entity, ability: &str, target: Option<EntityId>) {
        let Ok((me, _, &Position(at), character, _, cooldowns, _, health, resting, _)) = self.users.get(player) else {
            return;
        };
        let (connection, in_combat) = (me.connection, health.in_combat());
        let Some(def) = self.content.pack.abilities.get(ability).cloned() else { return };
        let cooling_down = cooldowns.remaining(&def.id) > 0.0;
        if !character.abilities.contains(&def.id) {
            return self.tell(connection, format!("You don't know {}.", def.name));
        }
        // Rest again to stand up.
        if def.effect == AbilityEffect::Rest && resting {
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
            AbilityEffect::Rest => {
                if in_combat {
                    return self.tell(connection, "You can't rest in the middle of a fight.".to_string());
                }
                self.fire(player, &def, None);
            }
            AbilityEffect::Heal { .. } => self.fire(player, &def, None),
            AbilityEffect::Strike { .. } | AbilityEffect::Bolt { .. } => {
                let Some(target_id) = target else {
                    return self.tell(connection, format!("Select a monster to use {} on first.", def.name));
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
                if !self.monsters.contains(target) {
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
                    self.commands.entity(player).remove::<Resting>().insert(PendingAbility {
                        ability: def.id.clone(),
                        target,
                        reach: def.reach(),
                        follow: Follow::default(),
                    });
                    self.keep_attacking(player, &def, target);
                }
            }
        }
    }

    /// Whether a player has the mana for an ability (telling them if not).
    fn can_afford(&mut self, player: Entity, def: &AbilityDef) -> bool {
        let Ok((me, .., mana, _, _, _)) = self.users.get(player) else { return false };
        if mana.current() >= def.mana {
            return true;
        }
        let connection = me.connection;
        self.tell(connection, format!("You don't have enough mana for {}.", def.name));
        false
    }

    /// Uses an ability (already checked): pays for it, starts its cooldown,
    /// tells everyone nearby, and does what it does.
    fn fire(&mut self, player: Entity, def: &AbilityDef, target: Option<Entity>) {
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
            AbilityEffect::Heal { amount } => {
                Some(health.heal(derived.healing(amount, &mut |sides| fastrand::u32(1..=sides.max(1)))))
            }
            _ => None,
        };

        let target_id = target.and_then(|target| self.targets.get(target).ok()).map(|(_, id, ..)| id.0);
        self.tell_nearby(at, &ServerMessage::AbilityUsed { user: user_id, ability: def.id.clone(), target: target_id });
        match def.effect {
            AbilityEffect::Rest => {
                // Sitting down stops them (see `gameplay::sit_down`).
                self.commands.entity(player).insert(Resting);
            }
            AbilityEffect::Heal { .. } => {
                self.commands.entity(player).remove::<Resting>();
                if let Some(amount) = healed {
                    self.tell_nearby(at, &ServerMessage::Healed { id: user_id, amount });
                }
            }
            AbilityEffect::Strike { damage } => {
                let Some(target) = target else { return };
                let hit = AbilityHit { ability: def.id.clone(), damage, power: Power::Weapon };
                self.strikes.write(Strike { attacker: player, target, ability: Some(hit) });
                self.keep_attacking(player, def, target);
            }
            AbilityEffect::Bolt { damage, speed } => {
                let Some(target) = target else { return };
                let Ok((_, _, &Position(target_at), _)) = self.targets.get(target) else { return };
                let hit = AbilityHit { ability: def.id.clone(), damage, power: Power::Spell };
                let remaining = distance(at, target_at) / speed;
                self.commands.spawn(BoltInFlight { attacker: player, target, hit, remaining });
            }
        }
    }

    /// After (or on the way to) a weapon ability, the player goes on
    /// attacking the target. The ability is a swing, so the next ordinary one
    /// waits a moment.
    fn keep_attacking(&mut self, player: Entity, def: &AbilityDef, target: Entity) {
        if !matches!(def.effect, AbilityEffect::Strike { .. }) {
            return;
        }
        let Ok((.., attacking)) = self.users.get_mut(player) else { return };
        match attacking {
            Some(mut attacking) if attacking.target == target => {
                attacking.cooldown = attacking.cooldown.max(SWING_AFTER_ABILITY);
            }
            _ => {
                let attacking = Attacking { target, cooldown: SWING_AFTER_ABILITY, follow: Follow::default() };
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
            .filter(|(_, viewer)| distance(viewer.0, at) <= ABILITY_VIEW_RANGE)
            .map(|(player, _)| player.connection)
            .collect();
        self.link.broadcast(nearby, message);
    }
}

fn distance(a: Point, b: Point) -> f32 {
    (a.0 - b.0).hypot(a.1 - b.1)
}
