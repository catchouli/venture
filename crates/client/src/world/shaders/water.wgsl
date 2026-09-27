// Water: Bevy's standard PBR shading of a moving, rippled surface.
//
// - The surface's slopes come from a handful of travelling waves (sines, in
//   different directions and lengths), so it catches the sky and the sun.
// - Waves too small to see at a pixel's distance fade out (so distant water
//   doesn't sparkle), and the roughness takes up the slope they had (so it
//   still reflects about as much light, only blurrier).
// - Reflections (the sky and the sun) are added at full strength; only the
//   water's own colour is see-through. Seen at a low angle, water reflects
//   more and shows less of what's under it (Fresnel).
//
// Drawn with premultiplied alpha: colour out is (body × opacity +
// reflection), and what's behind is kept by (1 − opacity).

#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    forward_io::{VertexOutput, FragmentOutput},
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
    mesh_view_bindings::globals,
}

// One wave: direction (x, z), length in metres, height in metres.
struct Wave {
    direction: vec2<f32>,
    length: f32,
    height: f32,
}

const WAVE_COUNT: u32 = 6u;
const WAVES: array<Wave, 6> = array<Wave, 6>(
    Wave(vec2<f32>(0.8, 0.6), 9.0, 0.06),
    Wave(vec2<f32>(-0.5, 0.87), 5.3, 0.035),
    Wave(vec2<f32>(0.97, -0.26), 2.9, 0.018),
    Wave(vec2<f32>(-0.2, -0.98), 1.7, 0.009),
    Wave(vec2<f32>(0.6, -0.8), 0.9, 0.004),
    Wave(vec2<f32>(-0.93, 0.37), 0.53, 0.0022),
);

// The least a shading normal may face the camera (N·V, before normalising):
// specular light divides by N·V, and at nearly zero it overflows (see
// docs/textures.md, "The sparkles on distant ground").
const MIN_FACING: f32 = 0.05;
// Fresnel reflectance of water seen straight on.
const WATER_F0: f32 = 0.02;

@fragment
fn fragment(
    in: VertexOutput,
    @builtin(front_facing) is_front: bool,
) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    let p = in.world_position.xz;
    let t = globals.time;

    // The surface's slope (dh/dx, dh/dz), and the slope variance of waves
    // too small to show.
    var slope = vec2<f32>(0.0);
    var lost = 0.0;
    for (var i = 0u; i < WAVE_COUNT; i++) {
        let wave = WAVES[i];
        let k = 6.2831853 / wave.length;
        // Deep-water waves: speed grows with the square root of length.
        let omega = sqrt(9.81 * k);
        let phase = k * dot(wave.direction, p) - omega * t;
        // How much of the wave a pixel spans (in radians of its phase): fade
        // it out between a third and a whole cycle per pixel.
        let per_pixel = fwidth(phase);
        let shown = 1.0 - smoothstep(2.0, 6.0, per_pixel);
        let amplitude = wave.height * k;
        slope += wave.direction * (amplitude * cos(phase) * shown);
        lost += 0.5 * amplitude * amplitude * (1.0 - shown);
    }
    let n = normalize(vec3<f32>(-slope.x, 1.0, -slope.y));
    let v = pbr_input.V;
    pbr_input.N = normalize(n + v * max(MIN_FACING - dot(n, v), 0.0));
    pbr_input.world_normal = pbr_input.N;

    // Hidden slope becomes roughness (added to GGX's alpha squared).
    let roughness = pbr_input.material.perceptual_roughness;
    let alpha = roughness * roughness;
    pbr_input.material.perceptual_roughness = clamp(sqrt(sqrt(alpha * alpha + lost)), roughness, 0.6);

    // How much the surface reflects at this angle, and so how much of what's
    // under it can't be seen.
    let facing = saturate(dot(pbr_input.N, v));
    let fresnel = WATER_F0 + (1.0 - WATER_F0) * pow(1.0 - facing, 5.0);
    let opacity = mix(pbr_input.material.base_color.a, 1.0, fresnel);

    // The reflection alone (lit with no colour of its own), then the whole.
    let body = pbr_input.material.base_color.rgb;
    pbr_input.material.base_color = vec4<f32>(0.0, 0.0, 0.0, 1.0);
    let reflection = apply_pbr_lighting(pbr_input).rgb;
    pbr_input.material.base_color = vec4<f32>(body, 1.0);
    let whole = apply_pbr_lighting(pbr_input).rgb;
    let colour = max(whole - reflection, vec3<f32>(0.0)) * opacity + reflection;

    // Fog works on the colour as seen (not premultiplied), then premultiply.
    var out: FragmentOutput;
    let seen = main_pass_post_lighting_processing(pbr_input, vec4<f32>(colour / opacity, opacity));
    out.color = vec4<f32>(seen.rgb * opacity, opacity);
    return out;
}
