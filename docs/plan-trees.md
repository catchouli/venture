# Trees: the implementation plan

Realistic, procedural British trees for Avalon (see [vision.md](vision.md)
and [world-authoring.md](world-authoring.md), which planned trees as the
first **object generator**). It replaces the placeholder blob trees.

The goal: an author picks a species and tweaks a handful of plain
parameters (age, height, branchiness, lean, fullness, seed), sees the tree
at once, and places it (or a whole wood of them) in the world. The same
generator makes glTF models on its own, from the command line.

## The species

Common British trees first, each **data** (`content/trees/species.ron`), so
new ones (elven silverbark...) need no code:

| Species | Form | Bark | Leaves | Branching |
| --- | --- | --- | --- | --- |
| **Oak** (*Quercus robur*) | 20–30 m, short massive trunk, broad spreading crown, crooked limbs | grey-brown, deep vertical fissures | lobed, in clusters | alternate, wandering |
| **Ash** (*Fraxinus excelsior*) | to 35 m, tall airy domed crown, limbs curving up at the tips | pale grey; smooth young, diamond fissures old | pinnate, 9–13 leaflets | opposite |
| **Beech** (*Fagus sylvatica*) | 25–35 m, dense broad dome | smooth silver-grey | oval, wavy-edged, in flat sprays | alternate, fine |
| **Sycamore** (*Acer pseudoplatanus*) | 25–30 m, broad dense dome | grey, flaking in plates | large, five-lobed | opposite |
| **Silver birch** (*Betula pendula*) | 15–25 m, slender, narrow crown, drooping twigs | white with black diamonds; dark and rough at the base | small, triangular, toothed | alternate, pendulous tips |
| **Willow** (weeping, *Salix × sepulcralis*) | 10–20 m, broad crown, curtains of long hanging shoots to the ground | grey-brown, ridged | long, narrow | alternate; shoots weep |
| **Hawthorn** (*Crataegus monogyna*) | 5–10 m, small, dense, gnarled, often several stems | grey-brown, cracked | small, deeply lobed; May blossom, red haws | alternate, twisted |
| **Hazel** (*Corylus avellana*) | 3–8 m, a shrub of many straight stems (coppice) | smooth, coppery-brown | round, toothed | alternate, upright |
| **Scots pine** (*Pinus sylvestris*) | 25–35 m, straight trunk; conical young, flat-topped old, high crown | grey-brown below, orange and flaking above | paired blue-green needles | whorls |
| **Yew** (*Taxus baccata*) | 10–20 m, broad, dense, dark, often fluted or multi-stemmed | reddish-brown, flaking | flat dark needles in two ranks | alternate, dense |

A second willow (the upright white willow, often pollarded along rivers)
can follow; so can alder, rowan, holly, elm and horse chestnut.

## How a tree is made

In `venture_trees`, a crate of its own that knows nothing of Bevy or the
world (like `venture_world`), deterministic for a species, parameters and
seed. It works in four steps:

1. **Skeleton:** a recursive, parametric branching model after **Weber and
   Penn, "Creation and Rendering of Realistic Trees" (SIGGRAPH 1995)**, the
   model behind Blender's Sapling. It starts from a trunk (or several stems)
   with its own length, taper, flare, lean and wobble. Each branch level
   then has these parameters: how many children per metre, down angle,
   rotation (spiral 137.5°, opposite pairs, or whorls), length relative to
   the parent, curvature, and randomness. Three refinements make it look
   more natural than plain Weber–Penn:
   - **Tropism.** Each segment bends towards up (phototropism) or down
     (gravity), by level. Weeping willow and birch are strong downward
     tropism on the fine shoots, ash upward tips.
   - **Crown envelope pruning.** Branches are shortened to stay inside the
     species' crown shape (dome, cone, column, flat-top), scaled by age.
   - **The pipe model** (Shinozaki et al., 1964; "da Vinci's rule"): a
     parent's radius where it forks is the sum of its children's radii,
     each squared (an exponent of about 2.3). So thickness follows the
     branching rather than a fixed taper.
2. **Bark mesh:** each branch is a generalised cylinder along its curve,
   with **parallel-transport frames** so it doesn't twist. Rings have fewer
   sides further out (12 for the trunk, down to 3). The base flares into
   root buttresses, and the trunk can be fluted (lobes, for yew). Branches
   sink into their parent rather than being welded. That's standard for
   games, and invisible. Bark UVs keep the texture the same size on thick
   and thin branches.
3. **Leaves:** leaves are **cards**, quads carrying a texture of a leafy
   sprig (several leaves and their twig). One card stands for the finest
   twigs, which aren't modelled. Cards cluster at the ends of the last
   branch level. Their **normals point out from the crown** (blended with
   the card's own), which is what makes game foliage read as a soft volume
   instead of a pile of flat quads. Their vertex colour darkens deep inside
   the crown (cheap ambient occlusion). Willow's cards are long hanging
   strips.
4. **Textures:** painted by the generator from the species' data:
   - a leaf-sprig atlas: a few sprig variants drawn from the leaf shape
     (lobed, pinnate, oval, palmate, lanceolate, triangular, needles), size,
     arrangement and colours;
   - a bark texture from layered noise, in the species' style (fissured,
     smooth, papery with lenticels, plated).

   **Photographic textures** can be used instead, per species. The
   reference copies are in `~/assets/trees`: ambientCG's bark materials
   (colour, normal, roughness) and leaf sets (atlases of single leaves with
   opacity), all CC0. A species can name a bark material, and a leaf from a
   leaf set. The generator then builds its sprig atlas from that
   photographed leaf, arranged on twigs in the species' pattern. The ones
   used are imported into `content/trees/` (via LFS), and recorded in
   `licenses/` and ATTRIBUTION.md, like the ground textures.

**Authors tweak:** species, **age** (sapling to ancient: height, girth,
crown width, how many levels of branches, and old-age character like
broken limbs and a flat-topped pine), **height** (within what the species
allows), **branchiness** (children per metre), **fullness** (leaf
density), **lean**, and the **seed**. A species' whole parameter set can be
changed too, in the editor.

## Budget and levels of detail

Forests are big. At today's density (a spot every 4 m), a view could hold
over 10,000 trees. The plan:

- **Fewer, bigger trees.** Real trees spaced by their crowns (a mature oak's
  crown is 15–20 m across), with bushes and saplings in between. Spots are
  thinned by the species' crown radius (Poisson-disc style), which also
  reads much better.
- **Templates, not one mesh per tree.** Each species is made in a few
  **variants** (ages × seeds, about 9 a species). Every placed tree uses one
  of them, turned and scaled. Hand-placed "specimen" trees (the old oak on
  the green) can have their own.
- **Levels of detail**, switched with Bevy's `VisibilityRange` (which
  crossfades with dithering):
  - LOD0, near: the full tree, about 8k triangles for a big oak.
  - LOD1: without the finest branches, with fewer and bigger cards (about 2k).
  - LOD2, far: an **impostor**, a few crossed cards textured with pictures
    of the tree rendered by the generator's own CPU rasteriser (about 10
    triangles).
- **Instancing:** same mesh and material means Bevy batches the draws.
- Materials: bark is opaque. Leaves are alpha-tested, double-sided, and a
  little translucent (`diffuse_transmission`). The game already uses 4×
  MSAA, so alpha-to-coverage can soften the cut-out edges.
- **Wind later:** each vertex carries its branch level and a phase, so a
  vertex shader can sway trunk, branches and leaves on their own. The data
  goes in from the start.

## In the world

- **The world stores trees by parameters, not meshes** (as
  world-authoring.md planned). The manifest lists the world's **tree
  templates** (species, age, seed, overrides). Each tree prop names a
  template, with its position, turn and scale. Clients make the templates
  themselves from the content's species: a tree takes about a millisecond,
  so 100 templates cost about 0.1 s. That keeps the download small. (Baking
  meshes instead would be about 200 KB a template.)
- **What blocks movement is baked:** each tree prop carries its trunk's
  radius, so navigation never needs the generator, and floating-point
  differences between machines can't matter for gameplay.
- **Worldgen scatter** picks species by biome and site:
  - grassland: oak, ash, beech and sycamore, with hawthorn and hazel at wood
    edges;
  - riversides and lakesides: willow;
  - highlands: Scots pine and birch;
  - yew is rare, and by old places.
- **Authored trees** (`world_authoring/trees.ron`, read by the bake) are
  mainly **tree areas**. Placing trees one at a time over a big world
  would be slow and fiddly, and would end in lots of tiny, lonely trees. So
  an author **draws an area** and says what grows there. The bake plants it
  over the terrain by itself: it skips water, roads, steep ground and
  buildings, and sits every tree on the ground. An area has:
  - its outline (a polygon);
  - a **species mix** (oak 60%, ash 25%, hawthorn 15%);
  - **density** (from scattered parkland to close woodland; trees are spaced
    by their crowns, so it never packs them unnaturally);
  - an **age spread** (young plantation, mixed, ancient wood) and a height
    range;
  - an **edge** (how the wood thins out at its border, with scrub such as
    hawthorn and hazel at the margin);
  - its **seed**. Change it and you get a different arrangement of the same
    wood.

  Besides areas, there are **clearings** (areas where nothing grows, cut out
  of woods and of the generator's scatter) and **single trees** for the few
  that matter (the old oak on the green, the churchyard yew), each with
  species, age, height and seed. Authored always wins over generated.
- **Every generated thing has a seed**, shown and editable wherever it's
  authored: a tree, a tree area, a species' template, and later a building
  or a rock. The same seed and parameters always give the same result, and
  a new seed gives a new one on demand ("reroll").
- **The editor:**
  - a **Trees** tab: species and templates, with every parameter and a
    live 3D preview, like appearances' "How it looks";
  - in the World tab, **draw a tree area** (click out its outline on the
    terrain, pick the mix, see it planted at once), draw a clearing, or
    place a single tree. A reroll button gives a new seed.

  The affected chunks are re-made at once, since generation is chunk by
  chunk.

## Woodcutting: trees you can select, fell and cut up

Asked for on 2026-09-28, while the trees were being built. Felling is part
of the **Woodworking** skill (which also covers working wood: skills
gather their own materials and make things from them, so there's no
separate Woodcutting). It works like this:
1. **Select a tree** (the ones near you): the target frame names it ("Yew
   tree") with its Woodworking level.
2. **Fell it:** with an axe in hand, click the selected tree (the cursor
   becomes an axe). You run to it and chop. Each swing may bite, by your
   level against the tree's and your axe's power. After enough bites it
   falls, cut near its foot. There's no physics: it tips over (a short
   animation) and lies where it fell, away from you, leaving a stump.
3. **Cut up the fallen tree:** select it and chop again. Each bite gives
   logs and experience, until its logs run out.
4. **After a while** the fallen tree goes, and the tree grows back.

**Different woods:** each species has a Woodworking level, and its own
logs, with a wood (soft to hard) for making things later. Soft woods come
from smaller, quicker trees and need a lower level:

| Tree | Level | Wood |
| --- | --- | --- |
| Dead tree | 1 | dry deadwood (firewood) |
| Hazel | 1 | soft (coppice rods) |
| Birch | 5 | soft |
| Willow | 10 | soft |
| Scots pine | 15 | soft, resinous |
| Hawthorn | 20 | hard, small |
| Sycamore | 25 | medium |
| Ash | 30 | hard, springy |
| Beech | 35 | hard |
| Oak | 40 | very hard |
| Yew | 45 | very hard, prized for bows |

(Making things from wood comes later, in the same skill. The higher levels
are more about making things than felling harder trees; rarer, more
fantastical trees, for special materials, will come.) Felling a tree gives
experience, and so does each log cut from it.

**Axes:** the first is a **stone axe**: a knapped stone head bound to a
haft, for the lowest levels. Metal axes come later, made by
Blacksmithing.

**How it fits the world, which is baked:**
- **Standing trees stay world props.** They're baked with their species
  and variant (step 4), so every client and the server know every tree
  without sending anything. A tree's id is its chunk and its place in the
  chunk's props, stable for a bake.
- **The server only keeps what changed:** which trees are felled (and
  when they grow back). Clients get the felled list when they join, and
  each change as it happens, and hide those trees behind a stump.
- **A fallen tree is an ordinary server entity** (like a fishing trap):
  its place, its species, which way it fell and how many logs are left.
  So selecting it, the target frame and other players seeing it all work
  as for any entity.
- **The rules** (bite chances, logs, experience) are small tested
  functions in `venture_rpg::woodcutting` (felling, as a part of Woodworking). **The numbers** (swing time,
  how long a fallen tree lasts, regrowth) are in `server.ron`. **Which
  tree is which** (level, logs, experience) is content
  (`trees/woodcutting.ron`), and **axes** are items with a woodcutting
  part (their power and the level to use them).
- **On the server,** a player's woodcutting (felling, cutting, trees
  falling and growing back) only changes in `woodcutting.rs`.

## Steps

1. **The generator crate** (`crates/trees`): species data, skeleton, bark and
   leaf meshes, painted textures, a CPU preview renderer, and glTF (`.glb`)
   export. Its `tree` tool writes one tree, or a gallery of every species
   at three ages. It has tests: determinism, sizes staying in bounds, and
   triangle budgets.
2. **The ten species** in `content/trees/species.ron`, tuned against
   reference photos with the preview renderer. The content check reads them.
3. **In the game:** the client makes templates from the content and draws the
   baked tree props with them (LOD0 and LOD1), replacing the placeholder
   oak, birch and pine. Proper materials.
4. **World data:** the manifest's templates, tree props naming them and
   carrying their trunk radius (a new chunk format), and worldgen
   scattering by species with crown spacing.
5. **Authored trees:** tree areas (the main way), clearings and single
   trees in `world_authoring/`, each with a seed, read by the bake.
6. **The editor:** a Trees tab with a live preview, and the World tab's
   area-drawing and placing tools, with affected chunks re-made at once.
7. **Impostors** for the far LOD, and **wind**.
7½. **Trees fade close to the camera** (asked for on 2026-09-28): the
   parts of a tree near the camera fade out smoothly by their distance from
   it, so the view is never blocked while trees by the player stay whole
   (for woodcutting). The distances are tunable in `trees/drawing.ron`.
8. **Seasons and health** (autumn colours, bare winter branches, dead
   trees), blossom on hawthorn, more species.
9. **Woodcutting** (see above), in parts:
   a. selecting trees (standing and fallen), with the target frame naming them;
   b. the rules, content (levels, logs, axes) and the server's felling and cutting;
   c. the client: the axe cursor, chopping, trees falling, stumps, fallen trees;
   d. fallen trees going and trees growing back; tuning in `server.ron`.

## Status

- [x] 1. The generator crate (first form; see "Where it stands")
- [x] 2. The ten species (first tuning pass)
- [x] 3. In the game (first form)
- [x] 4. World data (species and variant baked; spacing by `tree_share`, not yet by each crown)
- [ ] 5. Authored trees
- [ ] 6. The editor (it shows the new trees; no tree tools yet)
- [ ] 7. Impostors and wind (impostors are in; no wind yet)
- [x] 7½. Trees fade close to the camera
- [ ] 8. Seasons, health, more species
- [x] 9. Woodcutting (first form: all four parts)

## Where it stands

Steps 1 to 3 are in, in a first form, with impostors and the fade. Try the
generator on its own:

```sh
cargo run --release -p venture_trees --bin tree -- gallery        # preview/trees.png
cargo run --release -p venture_trees --bin tree -- oak --age 1.5 --seed 7 --out oak.glb --preview oak.png
```

- Every species grows to its own real height, and stays within its budgets:
  at most 3,000 leaf cards (a tree with more gets fewer, bigger ones) and
  16,000 bark triangles close up (the thinnest branches go first).
  Mature trees come to about 5k–22k triangles.
- "Height" means the whole tree, top branch included. The skeleton is
  scaled to it after growing.
- Leaves are photographs (ambientCG leaf sets), re-coloured so their
  average is the species' colour. The species' colours still steer them,
  so autumn needs only a colour change. Pine needles are painted. Birch
  and beech have painted bark, since no photograph matches them.
- Still to tune:
  - willow (its limbs should arch up into a dome before the strands fall,
    rather than hanging from a flat top);
  - ash (a little upright);
  - hawthorn and yew (they could do with a more gnarled trunk showing).

**In the game** (`crates/client/src/world/trees.rs`):
- The content sends `trees/species.ron` and the photographs to clients (the
  content check reads and checks them). Each species grows in three
  variants (young, mature, old) in the background as the content arrives,
  a task per species.
- Each tree is drawn at three levels of detail, switched by Bevy's
  `VisibilityRange` with a dithered crossfade:
  - the full tree to 45–55 m;
  - the reduced one (a quarter of the bark, half the leaf cards, each
    bigger) to 110–130 m;
  - then an **impostor** to 330–360 m. That's two crossed cards with
    pictures of the tree from the front and side, drawn unlit by the
    generator's own rasteriser and lit by the game with crown normals.
  
  Trees of a template share meshes and materials, so they batch.
- Leaves and bark use alpha to coverage (the game has 4× MSAA). Leaves have
  mipmaps that keep their coverage (Castaño's method), so distant crowns
  don't thin away.
- **Trees fade close to the camera** (`shaders/tree.wgsl`):
  - A tree fragment nearer the camera than 2 m is gone, beyond 7 m it's
    whole, and in between it fades smoothly.
  - The fade goes through alpha to coverage, so it's a smooth dissolve,
    with no sorting and no dots.
  - Shadows stay.
  - The distances, and those of the levels of detail, are in the
    content's `trees/drawing.ron`. It's sent to clients, so tuning it needs
    no rebuild.

  (The first version faded trees near the line from the camera to the
  player. The playtest found it cut a circle round the character, when
  trees by the player need to stay visible for woodcutting.)
- Bark was inside out (its triangles wound clockwise seen from outside), so
  the insides of trunks showed. It's fixed, with a test. Leaves and
  impostors are drawn from both sides with their own normals, so they
  weren't affected.
- **The world names each tree's species and variant** (step 4, chunk
  format 3): a tree prop is `Tree { species, variant }`, where `species`
  indexes the manifest's `tree_species` (content ids) and `variant` is
  young, mature or old. Worldgen picks species from each biome's mix in
  its config (`BiomeTerrain::trees`), with willows by water
  (`waterside_tree`), and bushes are young trees of the biome's
  `bush_species`, small (below `BUSH_SCALE`, so they don't block).
- Worldgen plants a tree on only 22% of the spots it used to
  (`tree_share`), so woods have a tree every 8 to 10 m. This is generator
  version 3: rebake the world (`cargo run --release -p venture_worldgen --bin bake`).
- Trees sink into the ground by the slope under them, and stems have extra
  rings through their flare, so trunk bases sit in the ground.

Next:
- step 4 (tree props naming a species and variant, spaced by their crowns);
- then tree areas (step 5) and the editor's tools (step 6).

Things seen in playtest screenshots that want work:
- pine sprays look coarse close up (a photographed pine spray would help);
- willow's shape;
- measuring how long the templates take to grow in the browser.

**Woodcutting** (step 9, first form; see "Woodcutting" above and
`docs/client-server.md`):
- **Selecting:** standing trees near the player (within 40 m) can be
  selected, bushes included (they're small young trees: mostly hazel, level
  1, and hawthorn). The target frame shows the tree's name and Woodworking
  level ("Yew tree, Level 45").
- **Felling:** clicking a selected tree again with an axe in hand (the
  cursor is an axe) runs the player beside it and chops. When it falls, a
  stump is left, and the fallen tree tips over, slowly then faster, landing
  with a small bounce, away from the feller. Fallen trees are selectable
  too.
- **Logs and experience:** cutting up a fallen tree gives logs (2, 4 or 6,
  by the tree's size) and Woodworking experience.
- **Growing back:** after 3 minutes the fallen tree goes, and a minute later
  the tree grows back.
- **Gear:** new characters carry a stone axe. There are eleven kinds of
  wood, from deadwood and hazel rods (level 1) to yew (level 45).
- **Something to start on:** dead trees (a bare, grey species of their own,
  5–10 m tall, level 1) stand among the living in every biome's woods, and
  are volcanic land's only trees. Near the starting town there are about
  ten dead trees and sixteen hazel bushes within 150 m (generator version 4:
  rebake).
- **Trying it without a keyboard:**
  `VENTURE_CHOP=1 ... --local` fells the nearest tree the character's level
  can fell.
- **Still to come:**
  - chopping sounds;
  - a woodsman who sells axes and buys logs;
  - fallen trees and felled lists surviving a server restart;
  - making things from wood.

**The first playtest's round** (2026-09-28):
- **Levels:** the top levels are lower: beech 35, oak 40, yew 45. Higher
  levels are for making things, and rarer, fantastical trees.
- **Small trees:** young trees and bushes are "small" ("Small silver
  birch"). They take 10 levels less to fell (at least 1), and give half the
  experience.
- **Experience:** felling gives experience (`felling_experience` in
  `trees/woodcutting.ron`) as well as each log.
- **Starting to chop:** the first swing comes at once, and the chopper
  turns to face the tree (every swing says where it's aimed). Clicking
  again while chopping carries on, rather than starting over.
- **Selection:** a felled tree stops being selected (and selectable).
- **Fallen trees:** a fallen tree goes once its logs are all cut (and grows
  back as usual). It rests on its foot and the highest ground under it
  (sampled along where it lies, and to each side under the crown) instead
  of lying through a hillside.
- **Glints:** with an axe in hand, trees within 25 m that the player can
  fell now and then show a small, faint golden glint on the trunk (or on a
  bush's body): easy to spot if you're looking, and not otherwise
  (`woodcutting/sparkles.rs`).
- **Picking:** a tree's pick shape reaches no higher than the plant itself,
  so a bush can't be selected by clicking the air above it.
