// The terrain's surface, on top of Bevy's standard PBR shading.
//
// The mesh supplies each vertex's ground colour, and how much of it wears
// each ground texture (grass and dirt in its first UVs, rock in its second).
// Per pixel, this shader:
// - lays grass and dirt textures (colour, normal map, roughness, ambient
//   occlusion) from above, in world space, each sampled twice at different
//   sizes and angles so their repeats don't show,
// - lays rock on rocky ground and steep slopes from all three directions
//   (triplanar), so cliffs don't stretch it,
// - tints grass and dirt by the ground's own colour (so biomes and meadows
//   keep their shades), and rock by its brightness only,
// - gives the rest of the ground (sand, snow, ash, and everything until the
//   textures load) procedural grain, mottling and bumps from value noise.
//
// The textures are arrays, one layer each: 0 grass, 1 dirt, 2 rock. Their
// normal maps are OpenGL style (green up).

#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::alpha_discard,
}

#ifdef PREPASS_PIPELINE
#import bevy_pbr::{
    prepass_io::{VertexOutput, FragmentOutput},
    pbr_deferred_functions::deferred_output,
}
#else
#import bevy_pbr::{
    forward_io::{VertexOutput, FragmentOutput},
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
}
#endif

struct TerrainSettings {
    // x: slope (1 - normal.y) where rock starts to show
    // y: slope where the ground is all rock
    // z: strength of the procedural normal bumps
    // w: 1 once the textures have loaded
    params: vec4<f32>,
    // For each texture: its average colour (linear), and metres it covers.
    textures: array<vec4<f32>, 3>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> terrain: TerrainSettings;
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var colours: texture_2d_array<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(102) var colours_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(103) var normals: texture_2d_array<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(104) var normals_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(105) var surfaces: texture_2d_array<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(106) var surfaces_sampler: sampler;

const GRASS: i32 = 0;
const DIRT: i32 = 1;
const ROCK: i32 = 2;

// The second, larger and turned, sampling that hides a texture's repeats.
const TURN: mat2x2<f32> = mat2x2<f32>(0.8, 0.6, -0.6, 0.8);
const SECOND_SCALE: f32 = 0.37;

// A repeatable pseudo-random number in 0..1 for a lattice point.
fn hash(p: vec3<f32>) -> f32 {
    let q = fract(p * 0.3183099 + vec3<f32>(0.71, 0.113, 0.419)) * 17.0;
    return fract(q.x * q.y * q.z * (q.x + q.y + q.z));
}

// Smooth 3D value noise in 0..1.
fn value_noise(p: vec3<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let x00 = mix(hash(i + vec3<f32>(0.0, 0.0, 0.0)), hash(i + vec3<f32>(1.0, 0.0, 0.0)), u.x);
    let x10 = mix(hash(i + vec3<f32>(0.0, 1.0, 0.0)), hash(i + vec3<f32>(1.0, 1.0, 0.0)), u.x);
    let x01 = mix(hash(i + vec3<f32>(0.0, 0.0, 1.0)), hash(i + vec3<f32>(1.0, 0.0, 1.0)), u.x);
    let x11 = mix(hash(i + vec3<f32>(0.0, 1.0, 1.0)), hash(i + vec3<f32>(1.0, 1.0, 1.0)), u.x);
    return mix(mix(x00, x10, u.y), mix(x01, x11, u.y), u.z);
}

// The small-scale bumpiness used to perturb normals.
fn bumps(p: vec3<f32>) -> f32 {
    return value_noise(p * 1.3) * 0.6 + value_noise(p * 4.1) * 0.4;
}

// One texture's look at a pixel: colour (linear), world normal, and
// (ambient occlusion, roughness).
struct Layer {
    colour: vec3<f32>,
    normal: vec3<f32>,
    surface: vec2<f32>,
}

// A tangent-space normal from a normal map sample.
fn unpack_normal(sample: vec4<f32>) -> vec3<f32> {
    return sample.xyz * 2.0 - 1.0;
}

// A texture laid from above: `uv` runs east and north, so the normal map's
// x is east and its green (y) is north (-z in Bevy's world). Blended onto
// the surface normal with a whiteout blend.
fn from_above(layer: i32, p: vec3<f32>, n: vec3<f32>, second: f32) -> Layer {
    let uv = vec2<f32>(p.x, -p.z) / terrain.textures[layer].w;
    let uv2 = TURN * uv * SECOND_SCALE + vec2<f32>(0.31, 0.57);
    var out: Layer;
    out.colour = mix(
        textureSample(colours, colours_sampler, uv, layer).rgb,
        textureSample(colours, colours_sampler, uv2, layer).rgb,
        second,
    );
    out.surface = mix(
        textureSample(surfaces, surfaces_sampler, uv, layer).rg,
        textureSample(surfaces, surfaces_sampler, uv2, layer).rg,
        second,
    );
    // (The second sampling's normals are turned back to match the first.)
    let n1 = unpack_normal(textureSample(normals, normals_sampler, uv, layer));
    let n2 = unpack_normal(textureSample(normals, normals_sampler, uv2, layer));
    let t = mix(n1, vec3<f32>(transpose(TURN) * n2.xy, n2.z), second);
    out.normal = normalize(vec3<f32>(t.x + n.x, abs(t.z) * n.y, -t.y + n.z));
    return out;
}

// A texture laid from all three directions (triplanar), blended by how much
// the surface faces each, with whiteout-blended normals.
fn triplanar(layer: i32, p: vec3<f32>, n: vec3<f32>) -> Layer {
    let scale = 1.0 / terrain.textures[layer].w;
    var blend = pow(abs(n), vec3<f32>(4.0));
    blend = blend / (blend.x + blend.y + blend.z);
    // Each direction's picture coordinates, mirrored on the far faces so
    // nothing reads backwards.
    let uv_x = vec2<f32>(p.z * sign(n.x), -p.y) * scale;
    let uv_y = vec2<f32>(p.x, -p.z * sign(n.y)) * scale;
    let uv_z = vec2<f32>(-p.x * sign(n.z), -p.y) * scale;
    var out: Layer;
    out.colour = textureSample(colours, colours_sampler, uv_x, layer).rgb * blend.x
        + textureSample(colours, colours_sampler, uv_y, layer).rgb * blend.y
        + textureSample(colours, colours_sampler, uv_z, layer).rgb * blend.z;
    out.surface = textureSample(surfaces, surfaces_sampler, uv_x, layer).rg * blend.x
        + textureSample(surfaces, surfaces_sampler, uv_y, layer).rg * blend.y
        + textureSample(surfaces, surfaces_sampler, uv_z, layer).rg * blend.z;
    var t_x = unpack_normal(textureSample(normals, normals_sampler, uv_x, layer));
    var t_y = unpack_normal(textureSample(normals, normals_sampler, uv_y, layer));
    var t_z = unpack_normal(textureSample(normals, normals_sampler, uv_z, layer));
    // Whiteout blends in each projection's plane, then swizzled back to world axes.
    let s = sign(n);
    let w_x = vec3<f32>(t_x.xy * vec2<f32>(s.x, -1.0) + n.zy, abs(t_x.z) * n.x);
    let w_y = vec3<f32>(t_y.x + n.x, -t_y.y * s.y + n.z, abs(t_y.z) * n.y);
    let w_z = vec3<f32>(t_z.xy * vec2<f32>(-s.z, -1.0) + n.xy, abs(t_z.z) * n.z);
    out.normal = normalize(w_x.zyx * blend.x + w_y.xzy * blend.y + w_z.xyz * blend.z);
    return out;
}

@fragment
fn fragment(
    in: VertexOutput,
    @builtin(front_facing) is_front: bool,
) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    let p = in.world_position.xyz;
    let normal = normalize(in.world_normal);
    let ground = pbr_input.material.base_color.rgb;

    // The procedural surface, for ground without a texture.
    let grain = value_noise(p * 4.0);
    let mottle = value_noise(p * 0.7);
    let broad = value_noise(p * 0.11);
    var procedural = ground * (0.8 + 0.14 * grain + 0.16 * mottle + 0.1 * broad);
    // Nudge the hue a little over broad areas, so big fields aren't one flat tone.
    procedural *= vec3<f32>(1.0 + 0.08 * (broad - 0.5), 1.0, 1.0 - 0.08 * (broad - 0.5));
    let steepness = smoothstep(terrain.params.x, terrain.params.y, 1.0 - normal.y);
    let brightness = dot(ground, vec3<f32>(0.3, 0.59, 0.11));
    let strata = 0.82 + 0.18 * sin(p.y * 2.3 + mottle * 5.0) + 0.1 * (grain - 0.5);
    let plain_rock = vec3<f32>(brightness * 1.03, brightness, brightness * 0.95) * strata;
    procedural = mix(procedural, plain_rock, steepness);
    let e = 0.12;
    let b = bumps(p);
    let gradient = vec3<f32>(
        bumps(p + vec3<f32>(e, 0.0, 0.0)) - b,
        bumps(p + vec3<f32>(0.0, e, 0.0)) - b,
        bumps(p + vec3<f32>(0.0, 0.0, e)) - b,
    ) / e;
    let bumped = normalize(pbr_input.N - (gradient - normal * dot(gradient, normal)) * terrain.params.z);

    // How much each texture covers here: steep slopes are rock, whatever the ground.
    let textured = terrain.params.w;
#ifdef VERTEX_UVS_A
    let grass_dirt = in.uv * (1.0 - steepness);
#else
    let grass_dirt = vec2<f32>(0.0);
#endif
#ifdef VERTEX_UVS_B
    let rock_ground = in.uv_b.x;
#else
    let rock_ground = 0.0;
#endif
    // (Each vertex's weights add up to at most 1, and still do after this.)
    let weights = vec3<f32>(grass_dirt, rock_ground * (1.0 - steepness) + steepness) * textured;
    let covered = min(weights.x + weights.y + weights.z, 1.0);

    // Every texture is sampled (sampling can't depend on the weights, as
    // mipmaps need neighbouring pixels to sample alike); ones weighted zero
    // don't show.
    let second = smoothstep(0.3, 0.7, value_noise(p * 0.13));
    let grass = from_above(GRASS, p, normal, second);
    let dirt = from_above(DIRT, p, normal, second);
    let rock = triplanar(ROCK, p, normal);

    // Tints: grass and dirt take the ground's colour; rock only its brightness.
    let variation = 0.92 + 0.16 * broad;
    let grass_colour = grass.colour * ground / max(terrain.textures[GRASS].rgb, vec3<f32>(0.01)) * variation;
    let dirt_colour = dirt.colour * ground / max(terrain.textures[DIRT].rgb, vec3<f32>(0.01)) * variation;
    let rock_brightness = dot(terrain.textures[ROCK].rgb, vec3<f32>(0.3, 0.59, 0.11));
    let rock_colour = rock.colour * clamp(brightness / max(rock_brightness, 0.01), 0.5, 1.6);

    let total = max(weights.x + weights.y + weights.z, 0.0001);
    let share = weights / total;
    let colour = grass_colour * share.x + dirt_colour * share.y + rock_colour * share.z;
    let surface = grass.surface * share.x + dirt.surface * share.y + rock.surface * share.z;
    let texture_normal = normalize(grass.normal * share.x + dirt.normal * share.y + rock.normal * share.z);

    pbr_input.material.base_color = vec4<f32>(mix(procedural, colour, covered), pbr_input.material.base_color.a);
    pbr_input.material.perceptual_roughness = mix(pbr_input.material.perceptual_roughness, surface.y, covered);
    pbr_input.diffuse_occlusion *= mix(1.0, surface.x, covered);
    pbr_input.N = normalize(mix(bumped, texture_normal, covered));

    pbr_input.material.base_color = alpha_discard(pbr_input.material, pbr_input.material.base_color);

#ifdef PREPASS_PIPELINE
    let out = deferred_output(in, pbr_input);
#else
    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
#endif
    return out;
}
