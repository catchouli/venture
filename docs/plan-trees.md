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
8. **Seasons and health** (autumn colours, bare winter branches, dead
   trees), blossom on hawthorn, more species.

## Status

- [x] 1. The generator crate (first form; see "Where it stands")
- [x] 2. The ten species (first tuning pass)
- [ ] 3. In the game
- [ ] 4. World data
- [ ] 5. Authored trees
- [ ] 6. The editor
- [ ] 7. Impostors and wind
- [ ] 8. Seasons, health, more species

## Where it stands

Steps 1 and 2 are in, in a first form. Try them:

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
