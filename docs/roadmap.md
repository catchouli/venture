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
- A minimap and a full map (M) of baked tiles with place names; the
  window tabs moved to the bottom edge (see [plan-map.md](plan-map.md)).
- Parties: `/party`, invitations that run out, a party tab and frames for
  the other members; `/target` by name, or the nearest monster within 5 m.

## Now: soul (the vertical slice's look and feel)

The prototype plays; this phase is about giving the graphics soul, to nail
the vibe for the vertical slice, before more mechanics and content. In order:

1. **The interface:** layouts in the content pack (`content/ui/layouts/`,
   see [ui-layouts.md](ui-layouts.md)), so designers can move, resize and
   restyle every window, with textures and fonts from the content. Then an
   art pass over the theme.
2. **Characters:** real models to replace the placeholder shapes: a template
   human (for players and NPCs) with ready-made animations (running,
   swimming and so on), then a character creator (sliders for build and
   face, hair styles and colours, outfits) used to make NPCs from the same
   parts, and later offered to players. See
   [plan-characters.md](plan-characters.md).
3. **The environment:** ground, grass, trees, water and sky (the list below).

After this phase: back to mechanics and content (class advancement, skills
that do something, more quests and places).

## Next: the look (environment)

- **Ground texture splatting:** done for grass, dirt and rock (PBR
  textures from ambientCG; see [textures.md](textures.md)). Next: more
  materials (sand, snow), indexed splatting and height blending.
- **Grass and flowers:** instanced grass tufts and flower clumps near the
  camera, fading out with distance (a big part of Lineage 2's feel).
- **Better trees and rocks:** more varied, more detailed models; wind sway.
- **Water:** waves and sky reflections are done; shoreline foam and
  depth-based colour next.
- **Sky:** an HDR sky with image-based lighting is done; time of day next.
- **Terrain level of detail**, so the view distance can grow (distant chunks
  meshed at lower resolution).

## Next: the world

- Cave entrances and a first generated cave (see [world-model.md](world-model.md)).
- World authoring (see [world-authoring.md](world-authoring.md)): authored roads,
  forests and clearings baked into the world; standalone tree and building
  generators; a world edit app for placing content, drawing roads and
  blocking out buildings; a first hand-made town on a settlement site.
- Multi-layer navigation grid, once there are structures to walk on and under.

- Playing together (milestone 1 of [plan-client-server.md](plan-client-server.md)):
  a baked world; an authoritative server hosting the game on a LAN; token
  profiles with a name prompt; movement prediction; other players with
  names; chat; a disconnected overlay; a desktop `--local` mode.

## Housekeeping

- Trim Bevy features the client doesn't use (glTF and so on) to shrink
  the web download (about 31 MB, or roughly a third of that compressed).

## Content

Done: the content pack (milestone 2), then dialogue, items, inventories, a
shop and the first quest (milestone 3). Still to come for content: saved Yarn
variables, item use (eating), and more shops and quests as the world grows.

## Next: combat (milestone 4)

Classes, levels, stats, abilities, equipment, monsters and fighting: see
[plan-combat.md](plan-combat.md). Steps 1 to 4 are done: the RPG rules,
classes and character creation, levels, stats, Miasma, admins, the tabbed
character window, abilities to learn, equipment that shows on the character,
monsters and spawners (rabbits and goblins), fighting (auto-attack,
damage numbers, experience, health regeneration and respawn points), and
abilities in use (Whack, Air Blast, Rest and the rest), with mana, cooldowns,
a hotbar on keys 1 to 0, and placeholder sounds. Class advancement (Fighter
to Warrior or Rogue, Mage to Wizard or Sage) is waiting on design work.

## The interface

A themed interface: every frame, button, slot, tab and bar is drawn from
pictures in the content (`ui/`), with placeholders to paint over (see
"The interface's look" in [content-authoring.md](content-authoring.md)).
The toolbar has an icon for each tab of the character window, and there's a
Skills tab (Fishing, Woodcutting, Mining, Blacksmithing, all at level 1 until
the skills are designed).

## Then: the game

- Proper character models and animation (imported, rigged models to replace the placeholder).
- Skills design (see [vision.md](vision.md)), then the first gathering skill
  (felling trees) and the first crafting skill.
