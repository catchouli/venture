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

## Next: the game

- Proper character models and animation (imported, rigged models to replace the placeholder).
- Targeting: selecting creatures and players, with the sword cursor to attack.
- Skills design (see [vision.md](vision.md)), then the first gathering skill
  (felling trees) and the first crafting skill.
- Server and protocol crates, with the client receiving chunks from the server.
- Content pipeline ([content-authoring.md](content-authoring.md)): a `venture_content`
  crate loading RON data, Yarn dialogue and Rhai scripts with hot reload, then
  a first NPC with a conversation and a shop in the first town.
