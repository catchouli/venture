//! Monsters: spawners that keep them alive, and what they do. The only code
//! that creates, removes and moves monsters.
//!
//! Each spawner from the content keeps one monster alive within its radius.
//! When the monster dies (see [`crate::combat`]), it lies there a moment,
//! then goes, and the spawner brings a new one after its respawn time.
//!
//! A monster wanders near home until something starts a fight: an
//! aggressive one goes for any player who comes near; any monster fights
//! back when attacked. It chases its target and attacks at its attack speed
//! (sending [`Strike`]s, which combat resolves), and gives up (going home and
//! healing) if led too far from home.

use std::ops::Range;

use bevy::prelude::*;
use venture_content::{MonsterDef, SpawnerDef};
use venture_navigation::Point;
use venture_protocol::{EntityKind, ServerMessage};
use venture_simulation::{Follow, FollowOutcome, MELEE_RANGE, Movement, plan_route};

use crate::ServerSystems;
use crate::combat::{Health, KnockedOut, Strike};
use crate::content::LoadedContent;
use crate::entities::{DisplayName, EntityIds, Mover, NetworkId, Player, Position, appeared};
use crate::link::Link;
use crate::world::ServerWorld;

/// A monster led further than this from home (in metres) gives up the fight.
const LEASH: f32 = 30.0;
/// How long a dead monster lies there before it goes, in seconds.
const CORPSE_SECONDS: f32 = 2.0;
/// How long an idle monster waits before wandering again, in seconds.
const IDLE_SECONDS: Range<f32> = 3.0..9.0;
/// How many random spots to try when looking for somewhere walkable.
const SPOT_ATTEMPTS: usize = 12;

/// Runs spawners and monsters.
pub struct MonstersPlugin;

impl Plugin for MonstersPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            FixedUpdate,
            (match_content, spawn_monsters, think, remove_the_dead).chain().in_set(ServerSystems::Monsters),
        );
    }
}

/// A spawner, keeping one monster alive.
#[derive(Component)]
struct Spawner {
    def: SpawnerDef,
    /// Where monsters appear around.
    centre: Point,
    /// Its monster, while it has one.
    monster: Option<Entity>,
    /// Counts down to the next monster, while it has none.
    respawn: Timer,
    /// Whether it's already warned that it has nowhere walkable to put a monster.
    warned: bool,
}

/// A monster.
#[derive(Component)]
pub struct Monster {
    /// What kind of monster it is.
    pub def: MonsterDef,
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

impl Monster {
    /// What kind of entity it is, as clients are told.
    pub fn kind(&self) -> EntityKind {
        EntityKind::Monster { level: self.def.level, look: self.def.look.clone() }
    }
}

/// A monster that has died, lying there until it goes.
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
/// spawners start over: their monsters go, and new ones come straight away.)
fn match_content(
    mut commands: Commands,
    content: Res<LoadedContent>,
    mut link: ResMut<Link>,
    spawners: Query<(Entity, &Spawner)>,
    monsters: Query<&NetworkId, With<Monster>>,
    players: Query<&Player>,
) {
    if !content.is_changed() {
        return;
    }
    let everyone: Vec<_> = players.iter().map(|player| player.connection).collect();
    for (entity, spawner) in &spawners {
        if let Some(monster) = spawner.monster {
            if let Ok(&NetworkId(id)) = monsters.get(monster) {
                link.broadcast(everyone.iter().copied(), &ServerMessage::EntityLeft { id });
            }
            commands.entity(monster).despawn();
        }
        commands.entity(entity).despawn();
    }
    for def in &content.pack.spawners {
        let centre = def.at;
        let mut respawn = Timer::from_seconds(def.respawn_seconds, TimerMode::Once);
        respawn.finish();
        commands.spawn(Spawner { def: def.clone(), centre, monster: None, respawn, warned: false });
    }
}

/// Brings a new monster to each spawner that's waited long enough.
fn spawn_monsters(
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
        if spawner.monster.is_some() || !spawner.respawn.tick(time.delta()).is_finished() {
            continue;
        }
        let Some(def) = content.pack.monsters.get(&spawner.def.monster) else { continue };
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
        let monster = Monster {
            def: def.clone(),
            spawner: spawner_entity,
            home,
            target: None,
            chase: Follow::default(),
            cooldown: 0.0,
            idle: idle_timer(),
        };
        let everyone: Vec<_> = players.iter().map(|player| player.connection).collect();
        link.broadcast(everyone, &appeared(id, &name, monster.kind(), Position(home)));
        let health = Health::full(def.health);
        let entity = commands.spawn((monster, NetworkId(id), name, Position(home), Mover::default(), health)).id();
        spawner.monster = Some(entity);
    }
}

/// What each living monster does this tick: pick a fight, chase, attack,
/// give up, or wander.
fn think(
    time: Res<Time>,
    world: Res<ServerWorld>,
    mut knocked_out: MessageReader<KnockedOut>,
    mut strikes: MessageWriter<Strike>,
    mut monsters: Query<(Entity, &mut Monster, &Position, &mut Mover, &mut Health), Without<Dead>>,
    players: Query<(Entity, &Position), With<Player>>,
) {
    let dt = time.delta_secs();
    let blocked = |x, y| world.is_blocked(x, y);
    let knocked_out: Vec<Entity> = knocked_out.read().map(|&KnockedOut(player)| player).collect();
    for (entity, mut monster, &Position(position), mut mover, mut health) in &mut monsters {
        monster.cooldown = (monster.cooldown - dt).max(0.0);
        // A knocked-out player is no longer a target: go home.
        if monster.target.is_some_and(|target| knocked_out.contains(&target)) {
            monster.target = None;
            health.restore();
            mover.0 = plan_route(position, monster.home, blocked);
        }

        // Pick a fight: whoever attacked it, or (if aggressive) any player near enough.
        if monster.target.is_none() {
            monster.target = health.attacker().filter(|attacker| players.contains(*attacker)).or_else(|| {
                let def = &monster.def;
                players
                    .iter()
                    .filter(|_| def.aggressive)
                    .map(|(player, &Position(at))| (player, distance(at, position)))
                    .filter(|&(_, gap)| gap <= def.aggro_radius)
                    .min_by(|a, b| a.1.total_cmp(&b.1))
                    .map(|(player, _)| player)
            });
            if monster.target.is_some() {
                monster.chase = Follow::default();
            }
        }

        let Some(target) = monster.target else {
            wander(&mut monster, position, &mut mover, dt, &world);
            continue;
        };
        // Give up if the target's gone or it's been led too far from home.
        let target_at = players.get(target).ok().map(|(_, &Position(at))| at);
        let Some(target_at) = target_at.filter(|_| distance(position, monster.home) <= LEASH) else {
            monster.target = None;
            health.restore();
            mover.0 = plan_route(position, monster.home, blocked);
            continue;
        };
        if distance(position, target_at) > MELEE_RANGE {
            let outcome = monster.chase.update(position, target_at, &mut mover.0, LEASH * 2.0, blocked);
            if outcome == FollowOutcome::Stop {
                monster.target = None;
            }
            continue;
        }
        mover.0 = Movement::Idle;
        if monster.cooldown <= 0.0 {
            strikes.write(Strike { attacker: entity, target, ability: None });
            monster.cooldown = 1.0 / monster.def.attack.speed;
        }
    }
}

/// An idle monster now and then walks somewhere near home.
fn wander(monster: &mut Monster, position: Point, mover: &mut Mover, dt: f32, world: &ServerWorld) {
    if mover.0.is_moving() || !monster.idle.tick(std::time::Duration::from_secs_f32(dt)).is_finished() {
        return;
    }
    monster.idle = idle_timer();
    if let Some(spot) = walkable_spot(monster.home, monster.def.wander_radius, world) {
        mover.0 = plan_route(position, spot, |x, y| world.is_blocked(x, y));
    }
}

/// Removes monsters that have lain dead long enough, and starts their
/// spawners' countdowns to the next one.
fn remove_the_dead(
    mut commands: Commands,
    time: Res<Time>,
    mut link: ResMut<Link>,
    mut dead: Query<(Entity, &Monster, &NetworkId, &mut Dead, &mut Mover)>,
    mut spawners: Query<&mut Spawner>,
    players: Query<&Player>,
) {
    for (entity, monster, &NetworkId(id), mut corpse, mut mover) in &mut dead {
        mover.0 = Movement::Idle;
        if !corpse.0.tick(time.delta()).is_finished() {
            continue;
        }
        link.broadcast(players.iter().map(|player| player.connection), &ServerMessage::EntityLeft { id });
        commands.entity(entity).despawn();
        if let Ok(mut spawner) = spawners.get_mut(monster.spawner) {
            spawner.monster = None;
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
