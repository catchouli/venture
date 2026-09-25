// The terrain's surface detail, added on top of Bevy's standard PBR shading.
//
// The mesh supplies each vertex's ground colour. This shader adds, per pixel:
// - fine grain, mottling and broad colour variation from 3D value noise
//   sampled at the world position (so there is no texture to tile or stretch),
// - bare, layered rock on steep slopes,
// - small bumps in the surface normal, so light catches the ground's texture.

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
    // z: strength of the normal bumps
    // w: unused
    params: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(100)
var<uniform> terrain: TerrainSettings;

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

@fragment
fn fragment(
    in: VertexOutput,
    @builtin(front_facing) is_front: bool,
) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    let p = in.world_position.xyz;
    let normal = normalize(in.world_normal);

    // Grain, mottling and broad variation, averaging out to about 1.
    let grain = value_noise(p * 4.0);
    let mottle = value_noise(p * 0.7);
    let broad = value_noise(p * 0.11);
    var colour = pbr_input.material.base_color.rgb;
    colour *= 0.8 + 0.14 * grain + 0.16 * mottle + 0.1 * broad;
    // Nudge the hue a little over broad areas, so big fields aren't one flat tone.
    colour *= vec3<f32>(1.0 + 0.08 * (broad - 0.5), 1.0, 1.0 - 0.08 * (broad - 0.5));

    // Steep slopes show bare rock in horizontal layers, keeping the ground's brightness.
    let steepness = smoothstep(terrain.params.x, terrain.params.y, 1.0 - normal.y);
    let brightness = dot(colour, vec3<f32>(0.3, 0.59, 0.11));
    let strata = 0.82 + 0.18 * sin(p.y * 2.3 + mottle * 5.0) + 0.1 * (grain - 0.5);
    let rock = vec3<f32>(brightness * 1.03, brightness, brightness * 0.95) * strata;
    colour = mix(colour, rock, steepness);
    pbr_input.material.base_color = vec4<f32>(colour, pbr_input.material.base_color.a);

    // Tilt the normal along the slope of the bump noise.
    let e = 0.12;
    let b = bumps(p);
    let gradient = vec3<f32>(
        bumps(p + vec3<f32>(e, 0.0, 0.0)) - b,
        bumps(p + vec3<f32>(0.0, e, 0.0)) - b,
        bumps(p + vec3<f32>(0.0, 0.0, e)) - b,
    ) / e;
    let along_surface = gradient - normal * dot(gradient, normal);
    pbr_input.N = normalize(pbr_input.N - along_surface * terrain.params.z);

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
