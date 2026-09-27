# Licences for what Venture uses

What the game includes that we didn't make, and the licence each is under.
Each licence file sits beside this one. When adding anything from outside
(models, textures, sounds, fonts, rules text), add it here with its licence,
and add a line crediting it to `ATTRIBUTION.md` at the top of the repository
(kept short, for the game's credits).

## Rules

| What | Where it's used | Licence |
| --- | --- | --- |
| System Reference Document 5.1, by Wizards of the Coast LLC | The core RPG rules in `crates/rpg`: stat modifiers, hit dice, proficiency, attack rolls against armour class, damage dice, critical hits | Creative Commons Attribution 4.0 International ([SRD-5.1.md](SRD-5.1.md)) |

## 3D models, textures and animations

All by [Quaternius](https://quaternius.com). Most are CC0 1.0 Universal
(public domain: no attribution needed, but it's good to give it); one is
under Quaternius's own licence (below). They're
copied into `content/characters/` by `tools/import_characters.py`, which
shrinks their textures and trims their animations.

| Pack | What we use | Licence |
| --- | --- | --- |
| Universal Base Characters (Standard) | The masculine and feminine bodies, hairstyles, eyebrows, skin textures | CC0 ([quaternius/universal-base-characters.txt](quaternius/universal-base-characters.txt)) |
| Modular Character Outfits – Fantasy (Standard) | Peasant and ranger outfit parts | CC0 ([quaternius/modular-character-outfits-fantasy.txt](quaternius/modular-character-outfits-fantasy.txt)) |
| Universal Animation Library (Standard) | Character animations (`animations.glb`) | CC0 ([quaternius/universal-animation-library.txt](quaternius/universal-animation-library.txt)) |
| Fantasy Props MegaKit (Standard) | The bronze sword and wooden shield | CC0 ([quaternius/fantasy-props-megakit.txt](quaternius/fantasy-props-megakit.txt)) |


### Under the Quaternius Asset License (QAL) v1.0

| Pack | What we use | Licence |
| --- | --- | --- |
| Bestiary – Dungeon Monsters Kit (Standard) | The Imp and the Puglin (goblins), on the same skeleton as the characters (`content/characters/imp.*`, `puglin.*`, `t_imp_*`, `t_puglin_*`) | QAL v1.0 ([quaternius/bestiary-dungeon-monsters-kit.txt](quaternius/bestiary-dungeon-monsters-kit.txt)) |

The QAL allows using the models in the game, commercially and without
credit, but **not redistributing the models themselves as assets** (on
their own, or as a pack, even modified). Shipping them inside the game is
fine. Publishing the raw files (say, if this repository were ever made
public) could count as redistributing them, so check the licence before
doing that, or keep these files out of anything published.

## Ground and surface textures

From [ambientCG](https://ambientcg.com), all CC0 1.0 Universal (public
domain; attribution optional, but we give it in `ATTRIBUTION.md`). Downloaded
(4K PNG versions) to `~/assets/surfaces/` and imported into
`content/textures/` by `venture_tools`'s `import_texture`, which shrinks
them to 1024 × 1024 and converts them to JPEGs (see `docs/textures.md`).

| Texture | What we use it for | Licence |
| --- | --- | --- |
| Grass005 | `content/textures/grass/`: grass and meadows | CC0 ([ambientcg/license.txt](ambientcg/license.txt)) |
| Ground111 | `content/textures/dirt/`: bare earth and paths | CC0 ([ambientcg/license.txt](ambientcg/license.txt)) |
| Ground110 | `content/textures/rock/`: rock, basalt and steep slopes | CC0 ([ambientcg/license.txt](ambientcg/license.txt)) |

## Everything else

Everything else in the repository (code, content, placeholder icons, sounds
and interface art made by the tools in `tools/`) is our own. The game's
code libraries (Bevy and the other Rust crates) come under their own
licences, listed by Cargo; the text font built into Bevy (Fira Mono) is
under the SIL Open Font License.
