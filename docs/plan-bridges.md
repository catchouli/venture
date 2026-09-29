# Bridges

Status: done (2026-09-29).

Bridges cross rivers (and anything else). Like walls and buildings, they're
**authored data**, baked into the world, and built into meshes by a
generator on each client (and live in the editor).

## What an author draws

A bridge (`venture_world::BridgePlan`, in `world_authoring/bridges.ron`, or
drawn in the editor: see [editor.md](editor.md)) is:
- **a path:** the points its deck's middle passes through, end to end
  (a centripetal Catmull–Rom curve through them, as walls are);
- **a kind:**
  - **Stone:** a dry-stone deck on an arch, with low walls either side;
  - **Rope:** wooden planks on ropes between wooden posts, with rope
    handrails and ropes down to the deck's edges;
- **a width**, in metres (3.5 usually for stone, 1.8 for rope);
- **a curve:** how far its middle rises above the straight line between
  its ends (an arch), or hangs below it (a rope bridge's sag), in metres;
- **a seed**, for its variation (the planks' unevenness).

## Baking

`bake` samples each bridge's path every half metre (`BakedBridge::deck`):
- **the ends:** each end is on the ground, a little above it;
- **between them:** a straight line, plus the curve (a parabola, most in
  the middle);
- **never below the ground under it:** a sagging bridge runs along its
  banks and only hangs over the gap.

The manifest has the baked bridges. Trees and rocks near them are cleared,
and `bake --structures` bakes again the chunks they touch (and their
navigation mesh tiles).

**Bridges over rivers.** The bake also places bridges over rivers
(`venture_worldgen::bridges`), because without them rivers can take a long
way round:
- **Spacing:** about one in every 400 m square a river runs through, at its
  narrowest crossing there.
- **Position:** square across the river, reaching 4 m onto each bank, with
  both ends on dry land.
- **Kind:** stone within 220 m of a settlement, rope further out.
- **Clearance:** each is lifted or sagged to stay clear of the water.
- **Kept clear:** they're left out near buildings, walls and authored
  bridges.

The placement is simple on purpose, because the world's land is likely to be
laid out again.

## Solid, and walked on

A bridge is built from **pieces** (`venture_structures::bridges::bridge_pieces`):
skewed boxes along its deck, one for each stretch of each part. Their corners
are both its meshes' and its colliders'. `venture_collision` makes a convex
hull of each:
- **The deck** is marked as walked on, so the navigation mesh includes it
  and routes go over bridges. It's a second level over the river, which the
  navigation mesh already handles.
- **The rest is in the way:** the stone arch and walls, and the rope
  bridge's sides and posts. A rope bridge's sides are solid netting up to
  its handrail, so no one walks off it (though a jump can clear them).
- **Deep water under a bridge:** deep water and lava are only "no entry"
  near their level, so running across a bridge over a deep river is fine.
