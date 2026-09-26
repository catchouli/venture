//! Fighting: resolving attacks, health, death, experience, respawning and
//! regeneration. The only code that damages anything.
//!
//! Anything that attacks (a player auto-attacking, a creature fighting back)
//! sends a [`Strike`]; this module rolls it with the SRD rules in
//! `venture_rpg` (d20 + attack bonus against armour class, then damage dice),
//! applies the damage, and tells everyone nearby. [`Health`] changes only
//! through its own methods: damage only here; healing here (regeneration,
//! levelling up, respawning) and when a creature gives up a fight and goes
//! home ([`crate::creatures`]).
//!
//! A creature that dies gives its killer experience, and is removed shortly
//! after. A player who dies wakes up at the nearest respawn point (see
//! `respawn_points.ron` in the content), with full health.
//!
//! Players also have [`Mana`], which abilities spend (see [`crate::abilities`]) and
//! which comes back on its own. Resting ([`Resting`]) speeds up both, until
//! the player moves or is hit.

use std::collections::HashMap;

use bevy::prelude::*;
use venture_content::ContentPack;
use venture_navigation::Point;
use venture_protocol::{AttackResult, ServerMessage};
use venture_rpg::abilities::{REST_REGENERATION_SECONDS, mana_regeneration};
use venture_rpg::derived::{OUT_OF_COMBAT_SECONDS, REGENERATION_SECONDS, health_regeneration};
use venture_rpg::{AttackOutcome, Dice, Power, resolve_attack};
use venture_simulation::{MELEE_RANGE, Movement};

use crate::ServerSystems;
use crate::abilities::PendingAbility;
use crate::character::{Character, class_or_default};
use crate::content::LoadedContent;
use crate::creatures::{Creature, Dead};
use crate::entities::{DisplayName, Following, Mover, NetworkId, PendingInteraction, Player, Position};
use crate::equipment::{Equipment, player_look};
use crate::link::Link;
use crate::npcs::resolve_placement;
use crate::world::ServerWorld;

/// Players within this distance (in metres) see a fight: attacks and deaths.
const FIGHT_VIEW_RANGE: f32 = 60.0;

/// Runs fights.
pub struct CombatPlugin;

impl Plugin for CombatPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<Strike>()
            .add_message::<KnockedOut>()
            .add_systems(
                FixedUpdate,
                (fit_vitals_to_character, auto_attack, resolve_strikes, regenerate)
                    .chain()
                    .in_set(ServerSystems::Combat),
            )
            .add_systems(FixedUpdate, send_vitals.in_set(ServerSystems::Updates));
    }
}

/// How healthy something is (players and creatures).
#[derive(Component, Clone, Debug, PartialEq)]
pub struct Health {
    current: u32,
    max: u32,
    /// Seconds since it last attacked or was attacked.
    since_fighting: f32,
    /// Seconds towards its next regeneration.
    regeneration: f32,
    /// Who last attacked it, if anyone (so a creature can fight back).
    attacker: Option<Entity>,
}

impl Health {
    /// Full health.
    pub fn full(max: u32) -> Self {
        Self { current: max, max, since_fighting: OUT_OF_COMBAT_SECONDS, regeneration: 0.0, attacker: None }
    }

    /// Health left.
    pub fn current(&self) -> u32 {
        self.current
    }

    /// Most health.
    pub fn max(&self) -> u32 {
        self.max
    }

    /// How healthy, from 0 to 1.
    pub fn fraction(&self) -> f32 {
        if self.max == 0 { 0.0 } else { self.current as f32 / self.max as f32 }
    }

    /// Who last attacked it, if anyone.
    pub fn attacker(&self) -> Option<Entity> {
        self.attacker
    }

    /// Whether it has fought recently (too recently to rest).
    pub fn in_combat(&self) -> bool {
        self.since_fighting < OUT_OF_COMBAT_SECONDS
    }

    /// Heals some health (never past the most). Returns how much it healed.
    pub fn heal(&mut self, amount: u32) -> u32 {
        let healed = amount.min(self.max - self.current);
        self.current += healed;
        healed
    }

    /// Back to full health, forgetting who it was fighting (a creature going home).
    pub fn restore(&mut self) {
        self.current = self.max;
        self.attacker = None;
    }

    /// Changes the most health (a new level, new gear), keeping what's left
    /// in range.
    fn set_max(&mut self, max: u32) {
        self.max = max.max(1);
        self.current = self.current.min(self.max);
    }

    /// Takes damage from an attacker. Returns whether that was the end of it.
    fn damage(&mut self, amount: u32, attacker: Entity) -> bool {
        self.current = self.current.saturating_sub(amount);
        self.attacker = Some(attacker);
        self.since_fighting = 0.0;
        self.current == 0
    }
}

/// A player's mana, which abilities spend.
#[derive(Component, Clone, Debug, PartialEq)]
pub struct Mana {
    current: u32,
    max: u32,
    /// Seconds towards its next regeneration.
    regeneration: f32,
}

impl Mana {
    /// Full mana.
    pub fn full(max: u32) -> Self {
        Self { current: max, max, regeneration: 0.0 }
    }

    /// Mana left.
    pub fn current(&self) -> u32 {
        self.current
    }

    /// Spends mana, if there's enough. Returns whether there was.
    pub fn spend(&mut self, amount: u32) -> bool {
        let enough = self.current >= amount;
        if enough {
            self.current -= amount;
        }
        enough
    }

    /// Changes the most mana (a new level, new stats), keeping what's left in range.
    fn set_max(&mut self, max: u32) {
        self.max = max;
        self.current = self.current.min(max);
    }
}

/// A player sitting down to rest: health and mana come back faster. Set by
/// [`crate::abilities`] (the Rest ability); ended by moving (see
/// [`crate::gameplay`]) or by fighting.
#[derive(Component)]
pub struct Resting;

/// A player attacking a creature: they run into reach (see
/// [`crate::gameplay`]), then attack at their attack speed until it dies or
/// they do something else.
#[derive(Component)]
pub struct Attacking {
    /// What they're attacking.
    pub target: Entity,
    /// Seconds until they can attack again.
    pub cooldown: f32,
    /// Keeps them close to it.
    pub follow: venture_simulation::Follow,
}

/// A player was knocked out (and has woken at a respawn point), so whatever
/// was fighting them should stop.
#[derive(Message, Clone, Copy, Debug)]
pub struct KnockedOut(pub Entity);

/// One attack, to be rolled and applied.
#[derive(Message, Clone, Debug)]
pub struct Strike {
    /// Who attacks.
    pub attacker: Entity,
    /// What they attack.
    pub target: Entity,
    /// The ability it's done with, if any (otherwise it's an ordinary attack).
    pub ability: Option<AbilityHit>,
}

/// An ability's attack, in a [`Strike`].
#[derive(Clone, Debug)]
pub struct AbilityHit {
    /// Which ability (its id).
    pub ability: String,
    /// Its damage dice, before the stat's bonus.
    pub damage: Dice,
    /// What powers it: the weapon or magic.
    pub power: Power,
}

/// Keeps players' most health and mana in line with their level, stats and gear.
fn fit_vitals_to_character(
    content: Res<LoadedContent>,
    mut players: Query<(&Character, &Equipment, &mut Health, &mut Mana), Or<(Changed<Character>, Changed<Equipment>)>>,
) {
    for (character, equipment, mut health, mut mana) in &mut players {
        let max = max_health(character, equipment, &content.pack);
        if health.max != max {
            health.set_max(max);
        }
        let max = max_mana(character, equipment, &content.pack);
        if mana.max != max {
            mana.set_max(max);
        }
    }
}

/// A player's most mana, from their class, level, stats and gear.
pub fn max_mana(character: &Character, equipment: &Equipment, pack: &ContentPack) -> u32 {
    let class = class_or_default(pack, Some(&character.class));
    character.derived(&class, &equipment.gear(&pack.items)).max_mana
}

/// A player's most health, from their class, level, stats and gear.
pub fn max_health(character: &Character, equipment: &Equipment, pack: &ContentPack) -> u32 {
    let class = class_or_default(pack, Some(&character.class));
    character.derived(&class, &equipment.gear(&pack.items)).max_health
}

/// Players attacking something in reach attack it whenever they're ready.
fn auto_attack(
    time: Res<Time>,
    content: Res<LoadedContent>,
    mut strikes: MessageWriter<Strike>,
    mut attackers: Query<(Entity, &mut Attacking, &Position, &Character, &Equipment)>,
    positions: Query<&Position>,
) {
    let dt = time.delta_secs();
    for (player, mut attacking, &Position(position), character, equipment) in &mut attackers {
        attacking.cooldown = (attacking.cooldown - dt).max(0.0);
        let Ok(&Position(target)) = positions.get(attacking.target) else { continue };
        if distance(position, target) > MELEE_RANGE || attacking.cooldown > 0.0 {
            continue;
        }
        strikes.write(Strike { attacker: player, target: attacking.target, ability: None });
        let class = class_or_default(&content.pack, Some(&character.class));
        let speed = character.derived(&class, &equipment.gear(&content.pack.items)).attacks_per_second;
        attacking.cooldown = 1.0 / speed;
    }
}

/// What an attacker hits with: its attack bonus, its damage dice (bonus
/// included). An ability uses its own dice (see `venture_rpg::abilities`).
fn attack_of(
    attacker: Entity,
    ability: Option<&AbilityHit>,
    players: &Query<(&mut Character, &Equipment, &Player)>,
    creatures: &Query<&Creature>,
    pack: &ContentPack,
) -> Option<(i32, Dice)> {
    if let Ok((character, equipment, _)) = players.get(attacker) {
        let class = class_or_default(pack, Some(&character.class));
        let derived = character.derived(&class, &equipment.gear(&pack.items));
        return Some(match ability {
            Some(ability) => derived.ability_attack(ability.damage, ability.power),
            None => (derived.attack_bonus, derived.damage),
        });
    }
    let creature = creatures.get(attacker).ok()?;
    Some((creature.def.attack.bonus, creature.def.attack.damage))
}

/// How hard something is to hit.
fn armour_class_of(
    target: Entity,
    players: &Query<(&mut Character, &Equipment, &Player)>,
    creatures: &Query<&Creature>,
    pack: &ContentPack,
) -> Option<i32> {
    if let Ok((character, equipment, _)) = players.get(target) {
        let class = class_or_default(pack, Some(&character.class));
        return Some(character.derived(&class, &equipment.gear(&pack.items)).armour_class);
    }
    creatures.get(target).ok().map(|creature| creature.def.armour_class)
}

/// Everything resolving strikes needs to change.
#[derive(bevy::ecs::system::SystemParam)]
struct Fights<'w, 's> {
    commands: Commands<'w, 's>,
    content: Res<'w, LoadedContent>,
    world: Res<'w, ServerWorld>,
    link: ResMut<'w, Link>,
    healths: Query<'w, 's, &'static mut Health>,
    manas: Query<'w, 's, &'static mut Mana>,
    creatures: Query<'w, 's, &'static Creature>,
    dead: Query<'w, 's, (), With<Dead>>,
    identities: Query<'w, 's, (&'static NetworkId, &'static DisplayName, &'static Position)>,
    viewers: Query<'w, 's, (&'static Player, &'static Position)>,
    characters: Query<'w, 's, (&'static mut Character, &'static Equipment, &'static Player)>,
    knocked_out: MessageWriter<'w, KnockedOut>,
}

/// Rolls each attack, applies it, and deals with deaths.
fn resolve_strikes(mut strikes: MessageReader<Strike>, mut fights: Fights) {
    for Strike { attacker, target, ability } in strikes.read() {
        let (attacker, target) = (*attacker, *target);
        // (Something killed earlier this tick isn't marked dead yet, but has no health.)
        let knocked_down = |entity| fights.healths.get(entity).is_ok_and(|health| health.current == 0);
        if fights.dead.contains(attacker)
            || fights.dead.contains(target)
            || knocked_down(attacker)
            || knocked_down(target)
        {
            continue;
        }
        let pack = &fights.content.pack;
        let (Some((bonus, damage)), Some(armour_class)) = (
            attack_of(attacker, ability.as_ref(), &fights.characters, &fights.creatures, pack),
            armour_class_of(target, &fights.characters, &fights.creatures, pack),
        ) else {
            continue;
        };
        let outcome = resolve_attack(bonus, armour_class, damage, 0, &mut |sides| fastrand::u32(1..=sides.max(1)));
        let (Ok((&NetworkId(attacker_id), attacker_name, _)), Ok((&NetworkId(target_id), _, &Position(at)))) =
            (fights.identities.get(attacker), fights.identities.get(target))
        else {
            continue;
        };
        // "the Goblin", or a player's name as it is.
        let attacker_name = if fights.creatures.contains(attacker) {
            format!("the {}", attacker_name.0)
        } else {
            attacker_name.0.clone()
        };
        let result = match outcome {
            AttackOutcome::Miss => AttackResult::Miss,
            AttackOutcome::Hit(amount) => AttackResult::Hit(amount),
            AttackOutcome::Critical(amount) => AttackResult::Critical(amount),
        };
        let ability = ability.as_ref().map(|ability| ability.ability.clone());
        fights.tell_nearby(at, &ServerMessage::Attacked { attacker: attacker_id, target: target_id, result, ability });
        // Fighting ends a rest, for both sides.
        fights.commands.entity(attacker).remove::<Resting>();
        fights.commands.entity(target).remove::<Resting>();

        if let Ok(mut attacker_health) = fights.healths.get_mut(attacker) {
            attacker_health.since_fighting = 0.0;
        }
        let Ok(mut health) = fights.healths.get_mut(target) else { continue };
        if outcome == AttackOutcome::Miss {
            health.since_fighting = 0.0;
            continue;
        }
        if health.damage(outcome.damage(), attacker) {
            fights.tell_nearby(at, &ServerMessage::Died { id: target_id });
            if fights.creatures.contains(target) {
                fights.creature_died(target, attacker);
            } else {
                fights.player_died(target, &attacker_name);
            }
        }
    }
}

impl Fights<'_, '_> {
    /// Tells every player near a spot.
    fn tell_nearby(&mut self, at: Point, message: &ServerMessage) {
        let nearby: Vec<_> = self
            .viewers
            .iter()
            .filter(|(_, viewer)| distance(viewer.0, at) <= FIGHT_VIEW_RANGE)
            .map(|(player, _)| player.connection)
            .collect();
        self.link.broadcast(nearby, message);
    }

    /// A creature died: it's removed shortly (see [`crate::creatures`]), and
    /// its killer, if a player, gets its experience.
    fn creature_died(&mut self, creature: Entity, killer: Entity) {
        self.commands.entity(creature).insert(Dead::new());
        let Ok(def) = self.creatures.get(creature).map(|creature| creature.def.clone()) else { return };
        let Ok((mut character, equipment, player)) = self.characters.get_mut(killer) else { return };
        let connection = player.connection;
        let gained = character.add_experience(def.experience);
        self.link.send(
            connection,
            &ServerMessage::Notice { text: format!("You defeat the {}. (+{} experience)", def.name, def.experience) },
        );
        if gained.levels == 0 {
            return;
        }
        // A new level: full health and mana, and everyone sees the celebration.
        let level = character.level();
        let pack = &self.content.pack;
        let max = max_health(&character, equipment, pack);
        let most_mana = max_mana(&character, equipment, pack);
        let look = player_look(&character, equipment, &pack.items);
        if let Ok(mut health) = self.healths.get_mut(killer) {
            health.set_max(max);
            health.current = max;
        }
        if let Ok(mut mana) = self.manas.get_mut(killer) {
            mana.set_max(most_mana);
            mana.current = most_mana;
        }
        self.link.send(connection, &ServerMessage::Notice { text: format!("You reached level {level}!") });
        let Ok((&NetworkId(id), ..)) = self.identities.get(killer) else { return };
        let everyone: Vec<_> = self.viewers.iter().map(|(player, _)| player.connection).collect();
        self.link.broadcast(everyone, &ServerMessage::PlayerChanged { id, level, look, celebrate: true });
    }

    /// A player died: they wake up at the nearest respawn point, with full health.
    fn player_died(&mut self, player: Entity, killer_name: &str) {
        let Ok((_, _, &Position(at))) = self.identities.get(player) else { return };
        let (name, respawn) = nearest_respawn_point(&self.content.pack, &self.world, at);
        self.commands.entity(player).insert(Position(respawn)).remove::<(
            Attacking,
            Following,
            PendingInteraction,
            PendingAbility,
            Resting,
        )>();
        self.commands.entity(player).insert(Mover(Movement::Idle));
        self.knocked_out.write(KnockedOut(player));
        if let Ok(mut health) = self.healths.get_mut(player) {
            health.current = health.max;
            health.attacker = None;
        }
        if let Ok(mut mana) = self.manas.get_mut(player) {
            mana.current = mana.max;
        }
        if let Ok((_, _, owner)) = self.characters.get(player) {
            let text = format!("You were knocked out by {killer_name}. You wake up at {name}.");
            self.link.send(owner.connection, &ServerMessage::Notice { text });
        }
    }
}

/// The respawn point nearest a spot: its name and position (the world's spawn
/// point if the content has none).
pub fn nearest_respawn_point(pack: &ContentPack, world: &ServerWorld, at: Point) -> (String, Point) {
    pack.respawn_points
        .iter()
        .filter_map(|point| Some((point.name.clone(), resolve_placement(&point.at, &world.manifest)?)))
        .min_by(|a, b| distance(a.1, at).total_cmp(&distance(b.1, at)))
        .unwrap_or_else(|| ("the town square".to_string(), world.manifest.spawn_point))
}

/// Health comes back slowly to anything that hasn't fought for a while, and
/// mana all the time; both come back faster while resting.
fn regenerate(time: Res<Time>, mut vitals: Query<(&mut Health, Option<&mut Mana>, Has<Resting>), Without<Dead>>) {
    let dt = time.delta_secs();
    for (mut health, mana, resting) in &mut vitals {
        let every = if resting { REST_REGENERATION_SECONDS } else { REGENERATION_SECONDS };
        health.since_fighting += dt;
        if let Some(mut mana) = mana {
            if mana.current >= mana.max {
                mana.regeneration = 0.0;
            } else {
                mana.regeneration += dt;
                if mana.regeneration >= every {
                    mana.regeneration = 0.0;
                    mana.current = (mana.current + mana_regeneration(mana.max)).min(mana.max);
                }
            }
        }
        if health.in_combat() || health.current >= health.max {
            health.regeneration = 0.0;
            continue;
        }
        health.regeneration += dt;
        if health.regeneration >= every {
            health.regeneration = 0.0;
            health.current = (health.current + health_regeneration(health.max)).min(health.max);
        }
    }
}

/// Sends players their health and mana when they change. (Their timers tick
/// every frame, so what was last sent is remembered, rather than relying on
/// change detection.)
fn send_vitals(
    mut link: ResMut<Link>,
    players: Query<(Entity, &Player, &Health, &Mana)>,
    mut sent: Local<HashMap<Entity, [u32; 4]>>,
) {
    sent.retain(|player, _| players.contains(*player));
    for (entity, player, health, mana) in &players {
        let vitals = [health.current, health.max, mana.current, mana.max];
        if sent.get(&entity) != Some(&vitals) {
            sent.insert(entity, vitals);
            let [health, max_health, mana, max_mana] = vitals;
            link.send(player.connection, &ServerMessage::Vitals { health, max_health, mana, max_mana });
        }
    }
}

fn distance(a: Point, b: Point) -> f32 {
    (a.0 - b.0).hypot(a.1 - b.1)
}
