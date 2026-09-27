//! Creatures: spawners that keep them alive, and what they do. The only code
//! that creates, removes and moves creatures.
//!
//! Each spawner from the content keeps one creature alive within its radius.
//! When the creature dies (see [`crate::combat`]), it lies there a moment,
//! then goes, and the spawner brings a new one after its respawn time.
//!
//! A creature wanders near home until something starts a fight: an
//! aggressive one goes for any player who comes near; any creature fights
//! back when attacked. It chases its target and attacks at its attack speed
//! (sending [`Strike`]s, which combat resolves), and gives up (going home and
//! healing) if led too far from home.

use std::ops::Range;

use bevy::prelude::*;
use venture_content::{CreatureDef, SpawnerDef};
use venture_navigation::Point;
use venture_protocol::{EntityKind, ServerMessage};
use venture_simulation::{Follow, FollowOutcome, MELEE_RANGE, Movement, plan_route};

use crate::ServerSystems;
use crate::combat::{Health, KnockedOut, Strike};
use crate::content::LoadedContent;
use crate::entities::{DisplayName, EntityIds, Mover, NetworkId, Player, Position, appeared};
use crate::link::Link;
use crate::npcs::resolve_placement;
use crate::world::ServerWorld;

/// A creature led further than this from home (in metres) gives up the fight.
const LEASH: f32 = 30.0;
/// How long a dead creature lies there before it goes, in seconds.
const CORPSE_SECONDS: f32 = 2.0;
/// How long an idle creature waits before wandering again, in seconds.
const IDLE_SECONDS: Range<f32> = 3.0..9.0;
/// How many random spots to try when looking for somewhere walkable.
const SPOT_ATTEMPTS: usize = 12;

/// Runs spawners and creatures.
pub struct CreaturesPlugin;

impl Plugin for CreaturesPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            FixedUpdate,
            (match_content, spawn_creatures, think, remove_the_dead).chain().in_set(ServerSystems::Creatures),
        );
    }
}

/// A spawner, keeping one creature alive.
#[derive(Component)]
struct Spawner {
    def: SpawnerDef,
    /// Where creatures appear around.
    centre: Point,
    /// Its creature, while it has one.
    creature: Option<Entity>,
    /// Counts down to the next creature, while it has none.
    respawn: Timer,
    /// Whether it's already warned that it has nowhere walkable to put a creature.
    warned: bool,
}

/// A creature.
#[derive(Component)]
pub struct Creature {
    /// What kind of creature it is.
    pub def: CreatureDef,
    /// Its spawner.
    spawner: Entity,
    /// Where it lives.
    home: Point,
    /// What it's fighting, if anything.
    target: Option<Entity>,
    /// Keeps it close to its target.
    chase: Follow,
    /// Seconds until it can attack again.
    cooldown: f32,
    /// Counts down to its next wander.
    idle: Timer,
}

impl Creature {
    /// What kind of entity it is, as clients are told.
    pub fn kind(&self) -> EntityKind {
        EntityKind::Creature { level: self.def.level, look: self.def.look.clone() }
    }
}

/// A creature that has died, lying there until it goes.
#[derive(Component)]
pub struct Dead(Timer);

impl Dead {
    /// Just died.
    pub fn new() -> Self {
        Self(Timer::from_seconds(CORPSE_SECONDS, TimerMode::Once))
    }
}

impl Default for Dead {
    fn default() -> Self {
        Self::new()
    }
}

/// Makes the spawners match the content whenever it changes. (Changed
/// spawners start over: their creatures go, and new ones come straight away.)
fn match_content(
    mut commands: Commands,
    content: Res<LoadedContent>,
    world: Res<ServerWorld>,
    mut link: ResMut<Link>,
    spawners: Query<(Entity, &Spawner)>,
    creatures: Query<&NetworkId, With<Creature>>,
    players: Query<&Player>,
) {
    if !content.is_changed() {
        return;
    }
    let everyone: Vec<_> = players.iter().map(|player| player.connection).collect();
    for (entity, spawner) in &spawners {
        if let Some(creature) = spawner.creature {
            if let Ok(&NetworkId(id)) = creatures.get(creature) {
                link.broadcast(everyone.iter().copied(), &ServerMessage::EntityLeft { id });
            }
            commands.entity(creature).despawn();
        }
        commands.entity(entity).despawn();
    }
    for def in &content.pack.spawners {
        let Some(centre) = resolve_placement(&def.at, &world.manifest) else {
            warn!("Spawner \"{}\" is placed at a settlement the world doesn't have; skipping it.", def.id);
            continue;
        };
        let mut respawn = Timer::from_seconds(def.respawn_seconds, TimerMode::Once);
        respawn.finish();
        commands.spawn(Spawner { def: def.clone(), centre, creature: None, respawn, warned: false });
    }
}

/// Brings a new creature to each spawner that's waited long enough.
fn spawn_creatures(
    mut commands: Commands,
    time: Res<Time>,
    content: Res<LoadedContent>,
    world: Res<ServerWorld>,
    mut ids: ResMut<EntityIds>,
    mut link: ResMut<Link>,
    mut spawners: Query<(Entity, &mut Spawner)>,
    players: Query<&Player>,
) {
    for (spawner_entity, mut spawner) in &mut spawners {
        if spawner.creature.is_some() || !spawner.respawn.tick(time.delta()).is_finished() {
            continue;
        }
        let Some(def) = content.pack.creatures.get(&spawner.def.creature) else { continue };
        // Somewhere walkable, or try again after another wait (rather than put it in a lake).
        let Some(home) = walkable_spot(spawner.centre, spawner.def.radius, &world) else {
            if !spawner.warned {
                warn!("Spawner \"{}\" found nowhere walkable within its radius; it will keep trying.", spawner.def.id);
                spawner.warned = true;
            }
            spawner.respawn.reset();
            continue;
        };
        let id = ids.allocate();
        let name = DisplayName(def.name.clone());
        let creature = Creature {
            def: def.clone(),
            spawner: spawner_entity,
            home,
            target: None,
            chase: Follow::default(),
            cooldown: 0.0,
            idle: idle_timer(),
        };
        let everyone: Vec<_> = players.iter().map(|player| player.connection).collect();
        link.broadcast(everyone, &appeared(id, &name, creature.kind(), Position(home)));
        let health = Health::full(def.health);
        let entity = commands.spawn((creature, NetworkId(id), name, Position(home), Mover::default(), health)).id();
        spawner.creature = Some(entity);
    }
}

/// What each living creature does this tick: pick a fight, chase, attack,
/// give up, or wander.
fn think(
    time: Res<Time>,
    world: Res<ServerWorld>,
    mut knocked_out: MessageReader<KnockedOut>,
    mut strikes: MessageWriter<Strike>,
    mut creatures: Query<(Entity, &mut Creature, &Position, &mut Mover, &mut Health), Without<Dead>>,
    players: Query<(Entity, &Position), With<Player>>,
) {
    let dt = time.delta_secs();
    let blocked = |x, y| world.is_blocked(x, y);
    let knocked_out: Vec<Entity> = knocked_out.read().map(|&KnockedOut(player)| player).collect();
    for (entity, mut creature, &Position(position), mut mover, mut health) in &mut creatures {
        creature.cooldown = (creature.cooldown - dt).max(0.0);
        // A knocked-out player is no longer a target: go home.
        if creature.target.is_some_and(|target| knocked_out.contains(&target)) {
            creature.target = None;
            health.restore();
            mover.0 = plan_route(position, creature.home, blocked);
        }

        // Pick a fight: whoever attacked it, or (if aggressive) any player near enough.
        if creature.target.is_none() {
            creature.target = health.attacker().filter(|attacker| players.contains(*attacker)).or_else(|| {
                let def = &creature.def;
                players
                    .iter()
                    .filter(|_| def.aggressive)
                    .map(|(player, &Position(at))| (player, distance(at, position)))
                    .filter(|&(_, gap)| gap <= def.aggro_radius)
                    .min_by(|a, b| a.1.total_cmp(&b.1))
                    .map(|(player, _)| player)
            });
            if creature.target.is_some() {
                creature.chase = Follow::default();
            }
        }

        let Some(target) = creature.target else {
            wander(&mut creature, position, &mut mover, dt, &world);
            continue;
        };
        // Give up if the target's gone or it's been led too far from home.
        let target_at = players.get(target).ok().map(|(_, &Position(at))| at);
        let Some(target_at) = target_at.filter(|_| distance(position, creature.home) <= LEASH) else {
            creature.target = None;
            health.restore();
            mover.0 = plan_route(position, creature.home, blocked);
            continue;
        };
        if distance(position, target_at) > MELEE_RANGE {
            let outcome = creature.chase.update(position, target_at, &mut mover.0, LEASH * 2.0, blocked);
            if outcome == FollowOutcome::Stop {
                creature.target = None;
            }
            continue;
        }
        mover.0 = Movement::Idle;
        if creature.cooldown <= 0.0 {
            strikes.write(Strike { attacker: entity, target, ability: None });
            creature.cooldown = 1.0 / creature.def.attack.speed;
        }
    }
}

/// An idle creature now and then walks somewhere near home.
fn wander(creature: &mut Creature, position: Point, mover: &mut Mover, dt: f32, world: &ServerWorld) {
    if mover.0.is_moving() || !creature.idle.tick(std::time::Duration::from_secs_f32(dt)).is_finished() {
        return;
    }
    creature.idle = idle_timer();
    if let Some(spot) = walkable_spot(creature.home, creature.def.wander_radius, world) {
        mover.0 = plan_route(position, spot, |x, y| world.is_blocked(x, y));
    }
}

/// Removes creatures that have lain dead long enough, and starts their
/// spawners' countdowns to the next one.
fn remove_the_dead(
    mut commands: Commands,
    time: Res<Time>,
    mut link: ResMut<Link>,
    mut dead: Query<(Entity, &Creature, &NetworkId, &mut Dead, &mut Mover)>,
    mut spawners: Query<&mut Spawner>,
    players: Query<&Player>,
) {
    for (entity, creature, &NetworkId(id), mut corpse, mut mover) in &mut dead {
        mover.0 = Movement::Idle;
        if !corpse.0.tick(time.delta()).is_finished() {
            continue;
        }
        link.broadcast(players.iter().map(|player| player.connection), &ServerMessage::EntityLeft { id });
        commands.entity(entity).despawn();
        if let Ok(mut spawner) = spawners.get_mut(creature.spawner) {
            spawner.creature = None;
            spawner.respawn.reset();
        }
    }
}

/// A random walkable spot within `radius` of `centre`, if one turns up.
fn walkable_spot(centre: Point, radius: f32, world: &ServerWorld) -> Option<Point> {
    let walkable = |spot: Point| !world.is_blocked(spot.0.floor() as i32, spot.1.floor() as i32);
    if radius <= 0.0 {
        return walkable(centre).then_some(centre);
    }
    (0..SPOT_ATTEMPTS).find_map(|_| {
        let angle = fastrand::f32() * std::f32::consts::TAU;
        let reach = radius * fastrand::f32().sqrt();
        let spot = (centre.0 + angle.cos() * reach, centre.1 + angle.sin() * reach);
        walkable(spot).then_some(spot)
    })
}

fn idle_timer() -> Timer {
    let seconds = IDLE_SECONDS.start + fastrand::f32() * (IDLE_SECONDS.end - IDLE_SECONDS.start);
    Timer::from_seconds(seconds, TimerMode::Once)
}

fn distance(a: Point, b: Point) -> f32 {
    (a.0 - b.0).hypot(a.1 - b.1)
}
