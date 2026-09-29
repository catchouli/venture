# Plan: precise collision, falling, and real jumps

Status: proposed (2026-09-29), awaiting review.

## Why

Collision today is a 1 m grid, and characters are points on it:

- `ChunkObstacles` marks a 64 × 64 grid of cells per chunk as blocked. Walls and buildings come from the bake's
  `structure_cells`; trees and boulders block every cell whose centre is within their radius; water blocks when it's too
  deep; slopes block when any cell's corners differ by more than `max_step` (1.2 m).
- `find_path` runs A* over those cells, then string-pulls the path by sampling the line every 0.25 m.
- `step` moves a point. When it's steered, it slides along one axis if the next cell is blocked.
- Heights are never simulated: a character's height is always the ground under it.

The grid is fine for terrain, but walls, fences and buildings are thin and can lie at any angle. A 0.5 m fence blocks a
whole metre of cells, so you stop up to a metre short of it. A slanting wall blocks a staircase of cells. A 1.5 m doorway
is either a whole cell or nothing. Characters have no width, so they clip into the edges of walls.

The slope rule has no direction, so a steep cell blocks both ways and you can't go down a slope that you couldn't climb.

Jumping is only an animation: `Gait::jump` lifts the model 0.45 m, and the server only records `LastJump`.

## The idea

Each character becomes a capsule that moves through real collision shapes. We use
[Rapier](https://rapier.rs)'s kinematic character controller, which already does the parts that are easy to get
subtly wrong:

- sliding along surfaces;
- a maximum slope it can climb;
- sliding down slopes that are too steep;
- stepping up small ledges;
- snapping to the ground on the way down.

We don't run a physics simulation. There are no rigid bodies to integrate: the controller only asks the query pipeline
what is in the way. This keeps it cheap and predictable. Rapier has no Bevy dependency, builds for wasm, and has an
`enhanced-determinism` feature. It's the same crate that `bevy_rapier` wraps; we use it directly, so the server and the
client share one plain-Rust implementation.

Height becomes simulated state: `(x, y, height)` plus a vertical speed. Gravity pulls characters down whatever they can't
stand on. That gives falling down steep slopes and cliffs for free, and turns jumps into real jumps.

Pathfinding stays on a grid, which we already have and which works. The grid gets finer, its slope rule becomes
one-way, and it is built from the same shapes. Only the final line-of-sight check moves from grid samples to exact
queries. See "Pathfinding" below for why this beats a navmesh here, for now.

## Shapes

A new crate, `venture_collision`, turns world data into Rapier colliders. It has no Bevy dependency. It sits beside
`venture_navigation`, and `venture_simulation` uses it.

| Source | Collider |
|---|---|
| Terrain (per chunk) | `HeightField`, 64 × 64 cells at 1 m (the chunk's corner heights). |
| Deep water and lava | No collider. Movement rules keep characters out, as they do today, via a per-chunk "no entry" cell mask. |
| Dry-stone wall, fence, hedge, curtain | A convex box per segment between baked samples. Its width is `half_width × 2`, its height is the wall's height, and its bottom follows the baked ground heights. The segments overlap at joints, so there are no seams. |
| Building walls | A box per outer-wall face and per inner wall, from the same `Frame`/`Footprint` as the generator (scaled, anticlockwise), split at doorways. Floors are the levelled ground, so they need nothing extra. |
| Trees | A capsule for the trunk. Its radius is today's `blocking_radius × scale`, and its height is the trunk's height. Bushes get none. Felled trees are removed when woodcutting changes. |
| Dead trees, boulders | A capsule or a ball of today's radius. |

The character is a capsule: 0.3 m radius and 1.8 m tall.

Colliders are built per chunk and kept in a map keyed by `ChunkPos`, with one `QueryPipeline` over everything loaded:

- **On the client**, chunks are added and removed as they stream in, next to `ObstacleStore`.
- **On the server**, colliders are built only for chunks within one chunk of a player, NPC or monster that is moving.
  They are built from the chunk files on demand, then dropped after a while unused. A full 4096 m world as heightfields
  would be about 70 MB, so building everything up front isn't worth it.

A world reload (`WorldChanged`) drops the changed chunks' colliders, as streaming already does.

## Movement

`venture_simulation::movement::step` becomes a thin wrapper round `KinematicCharacterController::move_shape`. It runs
on the server, which decides, and on the client, which predicts. The settings go in `server.ron`, so they can be tuned
without code:

| Setting | Proposed value | What it does |
|---|---|---|
| `max_slope_climb_angle` | ≈ 50° | Matches today's 1.2 m per 1 m. You can't walk up anything steeper. |
| `min_slope_slide_angle` | ≈ 50° | Anything steeper slides you down under gravity. |
| `autostep` | 0.4 m high, 0.3 m wide, not on dynamic bodies | Kerbs, plinths and doorsteps. |
| `snap_to_ground` | 0.5 m | Running downhill sticks to the ground instead of hopping. |
| `offset` | 0.02 m | Gap kept from surfaces. |
| gravity | 20 m/s² | Snappier than 9.8, as most games are. |

Each tick, the desired translation is the horizontal run towards the next waypoint (or the steering direction), plus
the vertical speed × dt. The controller returns the movement it actually made and whether the character is grounded.
When grounded, the vertical speed resets.

- **Steep slopes:** you can't climb them, because the climb angle stops you. You can go down them, because gravity
  plus sliding takes you down. Characters don't take fall damage in this plan; that could come later.
- **Water:** stays a rule, not a collider. A step that would end in a "no entry" cell is refused, as today.
- **Routes:** following a route still drops waypoints as they're reached. When the controller is blocked for about a
  second, for example by another character's route or a tree that has just grown back, the route is re-planned.

Characters don't collide with each other, as today.

## Jumping

The jump becomes a vertical speed set by the server when it accepts `ClientMessage::Jump`, and only while grounded. It
is still rate-limited by `LastJump`. `jump_height` goes in `server.ron`, and the initial speed is
`sqrt(2 × g × jump_height)`.

What a jump clears is a design choice (see Questions):

- At **1.4 m**, you clear fences (1.2) and dry-stone walls (1.3), but not hedges (1.7), buildings or curtain walls. A
  jump takes about 0.75 s in the air at g = 20.
- At **0.6 m**, it's close to a real person's jump. You clear nothing but kerbs, bushes and logs, and it looks more
  grounded.

The client predicts the jump at once, as it does now. `Gait` takes its lift from the simulated height instead of its own
parabola, so the animation and the capsule agree. The jump animation plays while the character isn't grounded.
Landing could get a short blend later.

## Protocol

Positions in snapshots gain a height, becoming `(x, y, height)`. Reconciliation compares all three. Remote characters
are interpolated with their height, so others are seen jumping over walls and falling off cliffs. This bumps the
protocol version to 32.

## Pathfinding

Click-to-move, NPCs and monsters keep `find_path` (A* and string-pulling), with three changes.

1. **A 0.5 m walk grid built from the colliders.** A node is open if the character's capsule fits there. This is a
   Rapier `intersection_with_shape` test with a vertical capsule standing on the heightfield. The grid is built per
   chunk (128 × 128) when a chunk's colliders are built, and cached with them. Doorways, fences and slanting walls come
   out right to within 0.5 m. The final path is then exact, because of (3).
2. **One-way slopes.** An edge from a node to a lower neighbour is always allowed. An edge to a higher neighbour is
   allowed only if the rise is within the climb angle. This keeps routes consistent with movement: A* can take you down
   a bank, but never plans you up one.
3. **Exact string-pulling.** `line_is_clear` becomes a capsule shape-cast along the segment, instead of grid samples
   every 0.25 m. Paths hug corners and doorways closely, instead of cell centres.

`MAX_ROUTE_SEARCH_CELLS` scales by 4 to keep the same reach. Profiling decides whether that's fast enough. A
hierarchical fallback (coarse 1 m A*, then 0.5 m A* within a corridor) is ready if it isn't.

Routes never plan a jump; you can jump over things yourself while steering.

### Why not a navmesh (yet)

The standard answer is Recast to build a navmesh and an any-angle search to use it. In Rust today that's
[rerecast](https://github.com/janhohenheim/rerecast) (a Rust port of Recast, 0.4, with a Bevy-free core) and
[polyanya](https://crates.io/crates/polyanya) (0.17, no Bevy dependency, supports layers and one-way layers). They give
optimal any-angle paths with very little search. They'd pay off once there are overlapping walkable levels: bridges,
upper floors with stairs, caves. The grid can't express those.

For now, the costs outweigh that:

- a 4096 m world means tiling and stitching navmeshes;
- felled and regrowing trees mean rebuilding tiles at runtime;
- rerecast is still marked unstable, with 4 breaking releases so far.

The grid already handles dynamic trees by rebuilding one chunk's cache. With the colliders as the single source of
truth, swapping the grid for a navmesh later is contained to `venture_navigation`.

## Steps

Each step is committed and tested on its own.

1. **`venture_collision`: shapes.** Build the colliders from the chunk, walls, buildings and props. Test them: a fence
   blocks within 0.3 m of its face, not 1 m; doorways are open; a tree's trunk blocks at its radius.
2. **Movement with the controller.** Height and vertical speed go in `venture_simulation`. `step` uses
   `move_shape`, with the settings in `server.ron`. Test: sliding along a slanting wall; being stopped at the foot of a
   steep bank but sliding down it from the top; stepping onto a 0.3 m ledge; not tunnelling at high speed.
3. **Server.** Chunk colliders are loaded on demand around characters and reloaded with the world. Players, NPCs and
   monsters move with them. `open_ground_near` and spawn checks use them.
4. **Protocol and client.** Height goes in snapshots, and the protocol version becomes 32. Prediction uses the same
   `step`, and reconciliation and interpolation include height. The client builds colliders as chunks stream in.
5. **Jumping.** The jump speed is set on the server, the client predicts it, and `Gait` follows the simulated height.
6. **Pathfinding.** The 0.5 m walk grid, one-way slopes, and exact string-pulling. Then profile A* on the server with
   many NPCs routing at once.
7. **Clean up.** Remove `structure_cells` from the bake and the chunk format; the chunk format becomes 5, and the world
   is rebaked. Remove the grid slope rule from `ChunkObstacles`, so only "no entry" water and lava remain. Update the docs:
   `world-model.md`, `client-server.md` and `architecture.md`.

Steps 1–5 give the visible wins: close contact with walls, falling, and jumping. Step 6 makes click-to-move as precise.

## Risks

- **Prediction drift.** The client and server run the same code on the same data, but f32 results can differ between
  wasm and native. The `enhanced-determinism` feature addresses this. The existing snap-back threshold covers what's
  left.
- **Server cost.** Each character costs a few shape-casts per tick. At 20 Hz with hundreds of characters, that's
  microseconds each. Idle characters skip the controller entirely.
- **Rapier's API churn.** Rapier has moved from nalgebra to glam and renamed types between versions. The version gets
  pinned, and use of it is kept inside `venture_collision` and `venture_simulation::movement`.

## Questions

1. **Jump height:** 1.4 m (over fences and dry-stone walls, but not hedges) or about 0.6 m (realistic)?
2. **Fall damage:** leave it out for now (as proposed), or add it with step 2? The rule would go in `venture_rpg`.
3. **Order:** is it fine for click-to-move to stay on the 1 m grid until step 6? It would still work, just less
   precisely than steering.
