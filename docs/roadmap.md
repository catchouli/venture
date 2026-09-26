# Roadmap

The near-term goal is a **vertical slice**: one region that looks and feels
like the finished game, with a character walking around it.

## Done

- Cargo workspace; Bevy client for web (WebGL2) and desktop.
- World generation engine: biomes, continents, mountains, hills, volcanoes,
  rivers, lakes, lava, beaches, settlements, roads, props, overlay stamps.
- Chunk streaming with neighbour-aware meshing; smooth terrain with a
  procedural detail shader; merged prop meshes; water and lava; warm haze.
- Dev tools: world preview images, automatic screenshots, free camera and HUD.
- A 1.8 m placeholder character with a procedural run animation; click-to-move
  with A* pathfinding around water, cliffs, trees and rocks; a glowing move
  marker; follow camera; custom cursors.

## Next: the look

- **Ground texture splatting:** blend real (or procedural) textures per
  material, instead of vertex colour plus detail noise.
- **Grass and flowers:** instanced grass tufts and flower clumps near the
  camera, fading out with distance (a big part of Lineage 2's feel).
- **Better trees and rocks:** more varied, more detailed models; wind sway.
- **Water:** gentle waves, shoreline foam, depth-based colour.
- **Sky:** a gradient sky with clouds and a sun, and time of day.
- **Terrain level of detail**, so the view distance can grow (distant chunks
  meshed at lower resolution).

## Next: the world

- Cave entrances and a first generated cave (see [world-model.md](world-model.md)).
- Overlay content from data files, and a first hand-made town on a settlement site.
- Structures: buildings and bridges placed by overlays.
- Multi-layer navigation grid, once there are structures to walk on and under.

- Playing together (milestone 1 of [plan-client-server.md](plan-client-server.md)):
  a baked world; an authoritative server hosting the game on a LAN; token
  profiles with a name prompt; movement prediction; other players with
  names; chat; a disconnected overlay; a desktop `--local` mode.

## Housekeeping

- Trim Bevy features the client doesn't use (audio, glTF and so on) to shrink
  the web download (about 31 MB, or roughly a third of that compressed).

## Content

Done: the content pack (milestone 2), then dialogue, items, inventories, a
shop and the first quest (milestone 3). Still to come for content: saved Yarn
variables, item use (eating), and more shops and quests as the world grows.

## Next: combat (milestone 4)

Classes, levels, stats, skills, equipment, creatures and fighting: see
[plan-combat.md](plan-combat.md). Steps 1 to 4 are done: the RPG rules,
classes and character creation, levels, stats, Miasma, admins, the tabbed
character window, skills to learn, equipment that shows on the character,
creatures and spawners (rabbits and goblins), and fighting (auto-attack,
damage numbers, experience, health regeneration and respawn points).
Next: using skills, and the hotbar.

## Then: the game

- Proper character models and animation (imported, rigged models to replace the placeholder).
- Skills design (see [vision.md](vision.md)), then the first gathering skill
  (felling trees) and the first crafting skill.
