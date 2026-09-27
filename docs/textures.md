# Ground textures

The terrain wears PBR textures (colour, normal map, roughness, ambient
occlusion) from the content pack: `content/textures/<name>/`. Today there are
three: **grass** (grass and meadows), **dirt** (bare earth and paths) and
**rock** (rock, basalt, and any steep slope). Sand, snow and ash still get
the procedural look (value-noise grain and bumps), as does everything until
the textures have loaded.

## Importing a texture

Textures come from [ambientCG](https://ambientcg.com) (CC0). Download the PNG
version (4K is fine; the importer shrinks it), unzip it into
`~/assets/surfaces/`, and run:

```sh
cargo run --release -p venture_tools --bin import_texture -- ~/assets/surfaces/Ground111_4K-PNG dirt
```

That writes `content/textures/dirt/`:

| File | What | Size |
| --- | --- | --- |
| `colour.jpg` | Colour, sRGB | 1024 × 1024, quality 85 |
| `normal.jpg` | Normal map, OpenGL style (green up), from `_NormalGL` | 1024 × 1024, quality 88 |
| `surface.jpg` | Ambient occlusion in red, roughness in green (linear) | 512 × 512, quality 85 |

A set comes to about 1.2 to 1.5 MB, against about 17 MB of source PNGs at
1K (the normal maps are 16-bit). Displacement isn't used. Add the texture to
`licenses/README.md` and a line to `ATTRIBUTION.md`, and to the terrain (below)
if it's a new kind of ground.

The source PNGs stay in `~/assets` untouched, so a texture can be imported
again at another size or quality.

### Why JPEG, and mipmaps at load time

- Photographic textures barely compress losslessly: as JPEGs they are about a
  tenth the size of PNGs, which matters most for the web client.
- JPEG can't hold mipmaps, and Bevy doesn't make them for PNG or JPEG, so the
  client makes them when the pictures load (`common/mipmaps.rs`: a box filter,
  averaging colour in linear light through lookup tables, integers only). It
  takes about 100 ms natively and 300 ms in the browser for all nine
  pictures, once, behind the loading screen.
- The alternatives were worse for now: uncompressed KTX2 with mipmaps is
  4 to 5 MB per map (the data doesn't compress); BC7 works on desktop GPUs
  but not most phones; Basis Universal (which transcodes to anything) is a
  C++ library Bevy can't build for WebAssembly. GPU-compressed textures are
  for the high-fidelity tier (below).

JPEG's chroma subsampling (4:2:0) blurs the normal maps' x and y a little.
It isn't visible on ground at game distances; a better normal format is part
of the fidelity work.

## In the game

`world/terrain_material.rs` loads the pictures, makes their mipmaps, and
packs them into **three array textures** (colours, normal maps, surfaces),
one layer per texture: three bindings however many textures there are,
which keeps the shader well within WebGL2's 16 texture units alongside
Bevy's own (shadows, environment). The samplers repeat, use trilinear
filtering, and 16× anisotropic filtering (ground is seen at low angles).

Each terrain vertex says how much of it is each texture (`terrain_mesh.rs`,
from the four cells around the corner: grass and dirt in its first UVs,
rock in its second). `shaders/terrain.wgsl` then, per pixel:

- **grass and dirt** are laid from above in world space, each sampled twice
  (the second 2.7 times larger and turned 37°, mixed by broad noise) so the
  repeats don't show, with normals whiteout-blended onto the surface;
- **rock** is triplanar (laid from all three directions, blended by the
  normal), so cliffs don't stretch it, and it takes over steep slopes
  whatever the ground;
- **tints**: grass and dirt are multiplied by the ground's own colour over
  the texture's average colour (so biomes and meadows keep their shades);
  rock only matches the ground's brightness (so grassy slopes don't turn
  green, and basalt stays dark);
- roughness and ambient occlusion replace the material's, weighted the
  same way.

Every texture is sampled everywhere (21 samples a pixel): sampling can't
depend on the weights without losing mipmap selection (it needs neighbouring
pixels to take the same path). Metres per repeat are in `GROUND_TEXTURES`
(grass 2.5, dirt 3, rock 4).

## Growing to many ground materials

The arrays scale in bindings (WebGL2 guarantees at least 256 layers), but
one weight per material per vertex doesn't. The plan for many materials:

- **Indexed splatting:** each vertex carries the ids and weights of its top
  two to four materials; the shader samples only those layers, so the cost
  per pixel stays the same however many materials exist.
- **Height blending:** blend by each texture's height (its displacement or
  ambient occlusion) rather than linearly, so stones poke through grass
  instead of fading into it.
- **Memory:** an uncompressed 1K layer with mipmaps is about 5.6 MB per map,
  so sixteen materials would be around 180 MB. That needs GPU compression
  (BC7 on desktop, ASTC or ETC2 on phones) or smaller layers on the web,
  which is the fidelity work below.

## Fidelity tiers (planned, not started)

The web client has to load quickly and run on modest machines; the desktop
client can have everything. The plan:

- **Tiers of assets:** the same textures (and later models) at several
  sizes: a small set (512 or less) that loads first, and full sizes (1K,
  later 2K) with GPU-compressed versions for machines that support them.
- **Start low, stream up:** the web client starts with the small set, so
  players are in the world quickly, then downloads and caches the higher
  sizes in the background and swaps them in as they arrive. Cached sizes
  load straight away next time.
- **Graphics settings:** a setting for the highest tier to load and show, so
  slow connections or machines can stop it streaming higher sizes (and
  lower settings for shadows, draw distance and so on).
- **Desktop:** the same settings, but it ships the highest tier and starts
  with it by default.
