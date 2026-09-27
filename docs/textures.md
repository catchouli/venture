# Ground textures, sky, light and water

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

### The sparkles on distant ground (fixed)

When the textures first went in, distant ground had bright, glowing specks
that jumped about as the camera moved, each with a black pixel in the middle
and a soft halo. The cause was a chain of four things:

1. **Multisampling shades outside the triangle.** The camera uses 4× MSAA,
   which runs the fragment shader once per pixel, at the pixel's centre. On
   a pixel that a triangle only partly covers, the centre can lie outside
   it, and the interpolated vertex attributes are then *extrapolated*. Far
   away, where terrain triangles are a pixel or two across, most pixels are
   edge pixels, and the extrapolation can be large. (GLSL's and WGSL's
   `centroid` interpolation exists to prevent this, but Bevy's vertex
   outputs don't use it.)
2. **The texture weights were divided by their total.** The shader turns the
   weights in UV_0 and UV_1 into shares that add up to 1 by dividing by
   their sum. Extrapolated weights such as (1.6, −1.5, 0) sum to about 0.1,
   giving shares of +16 and −15: a huge colour, and a roughness that's
   negative or nearly 0.
3. **Mirror-smooth ground at a grazing angle overflows.** With roughness
   clamped to Bevy's minimum (0.089) and N·V nearly 0 (distant ground is
   seen edge-on), GGX's D × V is around 10⁷. Times the sun that's far past
   65 504, the largest half-float: the HDR target (`Rgba16Float`, which
   bloom requires) stores it as +Inf.
4. **Tonemapping and bloom.** The tonemapper turns Inf into NaN, which is
   written as black: the black centre. Bloom's downsampling spreads the Inf
   into its neighbours as a bright blob: the halo. The next frame the
   pixels land differently on the triangles, so the sparkles move.

The fix is to clamp the weights to 0..1 before dividing, which is valid
anywhere a pixel can be shaded. Two more bugs turned up on the way and were
fixed with it: blended texture normals were normalised before mixing with
the procedural normal, so ground no texture covers (sand, snow, the sea
floor) normalised a zero vector into a NaN normal; and the vertex colour
is clamped to be non-negative, since it extrapolates in the same way.

If that effect is ever wanted on purpose (glints on snow, sand or water),
the controlled version is very low roughness plus a high-frequency normal,
with the result clamped below the half-float limit, never an overflow.

**Debugging tip.** To find pixels like these, temporarily colour the
terrain shader's output by what's wrong with it. Test for NaN and Inf by
their bits, `(bitcast<u32>(x) & 0x7f800000u) == 0x7f800000u`, because
`x != x` can be optimised away. Flag values over about 1000 as well, since
the overflow happens *after* the shader, when the colour is stored. Then
take screenshots with `VENTURE_SCREENSHOT` and search them for the marker
colours. That is how this bug was found: with the water hidden, NaN normals
showed on the sea floor, and the flagged values sat on silhouette pixels
with low roughness and grazing N·V.

## The skies

Each sky comes from an HDR panorama (an ambientCG HDRI's `*_HDR.exr`),
downloaded to `~/assets/skies/` and baked by:

```sh
cargo run --release -p venture_tools --bin import_sky -- ~/assets/skies/DaySkyHDRI070B_4K day
cargo run --release -p venture_tools --bin import_sky -- ~/assets/skies/EveningSkyHDRI047B_4K evening
cargo run --release -p venture_tools --bin import_sky -- ~/assets/skies/NightSkyHDRI008_4K night
```

into `content/skies/<name>/` (see `venture_content::sky`):

| File | What |
| --- | --- |
| `sky.jpg` | The sky: six 1024 × 1024 cube faces stacked, sRGB (about 0.4 MB; the starry night, 2.9 MB, as stars barely compress) |
| `light.bin` | Image-based lighting: a 32² irradiance cube and a 128² radiance cube with a mipmap per roughness (GGX-prefiltered, filtered importance sampling), in RGB9E5 |
| `sky.ron` | Where its sun was, and its horizon colour |

It's baked ahead because Bevy filters environment maps at run time with
compute shaders, which WebGL2 doesn't have. Everything is on one brightness
scale where the bright sky (the 98th percentile of the panorama) is 1; the
sky clips there, and its light clips at 4.

**What's painted out:**

- **The sun**, when there's really one there (far brighter than the sky: a
  sun behind cloud, or a night sky's brightest star, is left alone). The
  game draws its own sun where the time of day puts it, so the photo's
  would be a second one. The patch reaches as far as the sun brightens the
  sky (14° from its middle, for DaySkyHDRI070B), and is filled smoothly from
  the colours round its edge (from a blurred copy, each weighted by how near
  it is), leaving a soft glow, as behind thin cloud.
- **Anything listed in `tools/skies/<panorama folder>.ron`**: corridors
  (lines through the panorama's pixels, with a width) in which the importer
  finds thin bright trails (pixels well above their neighbourhood, joined
  into runs longer than a star) and fills them like a clone stamp from
  clean sky 40 px away, stars and all; then swaps each corridor's blurry
  part for the surrounding sky's, which takes away a meteor's glow but
  keeps the stars in front of it. NightSkyHDRI008's meteor and four
  satellite trails are listed there.

**The day's skies:** `content/skies/cycle.ron` says which sky shows at
which time of day, how bright, and how it's turned (following the sun, so
its glow is on the sun's bearing; or turning round the pole star, a turn a
day, for the stars). The game blends between the keyframes: see
[plan-day-night.md](plan-day-night.md). `SKY_BRIGHTNESS` (in
`world/lighting.rs`) sets how bright a sky's 1 is at a brightness of 1
(1000 cd/m², so with Bevy's default exposure a bright sky shows at about 1).

## Light and colour

The world camera renders in HDR: `Bloom` requires Bevy's `Hdr`, so the
scene goes to an `Rgba16Float` target, then Bevy's default tonemapper
(TonyMcMapface) and deband dithering, with 4× MSAA. All of that works on
WebGL2. Settings that set the look (`camera.rs`, `world/lighting.rs`):

- **Sun:** 14 000 lux at its highest (`SUN_ILLUMINANCE`), less and redder
  as it sinks (the day and night: see [plan-day-night.md](plan-day-night.md)).
  The figures here are for mid-afternoon. The day sky's light on open
  ground is about 900 lux (its irradiance looking up is about 0.29 on its
  scale, × 1000 cd/m² × π), and the sun at its 31° height gives about
  7 300, so about 8:1: clear shadows, while shade still reads.
- **Bloom:** `Bloom::NATURAL` at 0.06. It has no threshold, so it spreads a
  veil over everything bright (the sun on water); at 0.12 that softened the
  whole picture.
- **Colour grading:** saturation × 1.1 after tonemapping (TonyMcMapface
  desaturates bright colours). AgX and a contrast grade were tried and
  looked worse (washed out, and backlit characters went black).
- **Haze:** linear from 110 m to 290 m, where the loaded chunks end (it
  has to be full there). A longer view needs terrain level of detail.

## Water

Water (`world/water_material.rs`, `shaders/water.wgsl`) is Bevy's PBR
material extended with a shader, drawn with premultiplied alpha:

- **Waves:** six travelling sine waves (lengths 0.5 to 9 m, deep-water
  speeds) give the surface's slope, so it reflects the sky and glitters
  under the sun.
- **No sparkle in the distance:** each wave fades out once a pixel spans
  more than about a third of its cycle (`fwidth` of its phase), and the
  slope variance it had is added to GGX's α², so distant water turns
  glossy rather than aliasing (the idea behind Toksvig's method). N·V is
  kept above 0.05, so specular can't overflow (see the sparkles, above).
- **Reflections at full strength:** the shader lights the surface twice,
  once with a black base colour, to separate the reflection from the
  water's own colour. Only the water's colour is see-through (scaled by
  opacity); the sky and sun reflections are added in full. Opacity rises
  towards grazing angles (Schlick Fresnel, F0 0.02).
- **One sheet:** the water mesh (`fluid_mesh`) puts each corner at the
  average level of the liquid cells around it, so streams slope smoothly
  instead of stepping once per cell (which showed as a grid of seams).

Not yet: shoreline foam and colour by depth, which need the depth behind
the water. Bevy's depth prepass can't be read by a transparent pass on
WebGL2, so they'd need another way (such as depth baked into the mesh from
the heightfield).

## Growing to many ground materials

The arrays scale in bindings (WebGL2 guarantees at least 256 layers), but
one weight per material per vertex doesn't. The plan for many materials:

- **Indexed splatting:** each vertex carries the ids and weights of its top
  two to four materials; the shader samples only those layers, so the cost
  per pixel stays the same however many materials exist.
- **Height blending:** blend by each texture's height (its displacement or
  ambient occlusion) rather than linearly, so stones poke through grass
  instead of fading into it. (A first try, with ambient occlusion as the
  height, made things worse: grass's occlusion is brighter overall, so it
  won patches in dirt, where they took the dirt's tint and looked like
  smudges. It needs heights centred on each texture's average, and each
  texture tinted separately rather than by one blended vertex colour.)
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
