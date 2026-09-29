# The map

A minimap in the top-right corner and a full map (**M**), drawn from
pictures of the land baked with the world, with place names written over
them.

## What's done (first pass)

1. **Interface rejig.** The character window's tab buttons are a row along
   the bottom edge, left of the hotbar; the windows open just above them,
   beside the hotbar. The top-right corner is the minimap's.
2. **Minimap** (`client/src/map/minimap.rs`): a square frame in the top
   right, following the player (always in the middle). Clicking it runs
   there, just like clicking the ground (at most `max_move_distance` away).
   The mouse wheel zooms it; the Map button in its corner opens the full map.
3. **Map window** (`client/src/map/window.rs`, **M**): a big window in the
   middle of the screen. It opens on the player; the wheel zooms towards the
   mouse, out as far as the whole world, and dragging moves it. The bottom
   right shows the point under the mouse (`x, y` in metres, as `/tp` takes).
4. **Tiles** (baked; `venture_world::MapInfo`, `venture_worldgen::map_render`):
   see below.
5. **Zoom:** the wheel on both, smoothly between tile levels (below).
6. **Markers:** anything with a `MapMarker` shows on both maps: the player's
   own character, as an arrow turned the way they face, and where they're
   running to (the move marker left where they clicked), as a blue dot.

## Tiles

A tile pyramid, like a web map server's, in the baked world folder:

- `map/{level}/{x}_{y}.jpg`: 256 × 256 JPEG tiles. Level 0 is a metre a
  pixel (16 × 16 tiles for the 4 km world); each level after covers twice
  as much per pixel, down to one tile for the whole world (level 4). About
  8 MB in all, 32 KB for the coarsest level; clients only fetch the tiles
  they show, and browsers keep them (the world's fingerprint is in their
  addresses).
- `manifest.ron` says the world has a map (`map: Some((...))`), so worlds
  baked before maps still load (they just have no map).

For now the tiles are **drawn from the baked chunks**: each metre coloured
by its ground, lit from the north-west by its slope, water darker the
deeper it is, lava, and a dot for each tree, bush and boulder; each level
averages the one before. It takes about half a second, and always matches
the world. `bake` draws it after the chunks; to draw it again for a world
already baked (after changing how it's drawn, say):

```sh
cargo run --release -p venture_worldgen --bin bake_map
```

**Later:** a hand-drawn, higher-level map that informs the generator
(regions, roads, towns), which the tiles could be painted from instead, in
a more illustrated style. The format stays the same.

### Smooth zooming

A view draws three layers of tiles: the whole world (coarsest level)
underneath, so nothing is ever blank while tiles load; then the two levels
either side of the zoom. With the zoom as a fractional level `f` (metres a
pixel, as a power of two), level `floor(f) + 1` is drawn fully and level
`floor(f)` over it, faded out as `f` goes up. Zooming out, the finer level
fades into the coarser one as they meet, so there's no jump from one to
the next.

## Place names

`map/labels.ron` lists names (`venture_world::MapLabels`), each with a
place and a kind, and the client styles and shows them by kind
(`client/src/map/view.rs`):

| Kind | What | Written | Shown |
| --- | --- | --- | --- |
| `Region` | a stretch of one kind of land, a sea | large pale gold capitals, slightly see-through | zoomed out (from 2.6 m a pixel) |
| `Area` | a town, a big lake | white, just above its place | up to 5 m a pixel, fading out by 8 |
| `Place` | a village, a pond, a wood (later dungeons) | small, pale green | zoomed in (to 1.4 m a pixel, gone by 2.2) |

Names grow a little as the map zooms in and shrink as it zooms out (within
limits, so they stay readable), fade in and out at the ends of their
ranges, and are written in order, regions first: one that would overlap a
name already written is left out.

For now the names are **made up** from the land and the world's seed
(`venture_worldgen::map_labels`): regions are big stretches of one biome
("The Windswept Downs") and seas; areas are the settlements; places are
ponds and meres (not rivers) and woods and forests. **Later** they come
from the hand-made map (and content, for dungeons and so on), in the same
file format.

## Markers

`MapMarker` is a component for things in the world to show on maps: add it
(with a style) to any entity with a `Transform`, and every map view draws
it. Only the player's own character has one so far (`MapMarker::Player`,
an arrow); other players, NPCs, quest givers or party members can be added
the same way, each with their own look.

## Not yet

- Showing other things than the player (party members would be a good first).
- Clicking the map (to move there, or to mark a spot).
- A hand-authored map and names (above).

(The minimap stays north up, and there's no fog of war: both decided against.)
