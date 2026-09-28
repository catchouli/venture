// Trees: Bevy's standard PBR shading, fading out close to the camera, so
// they never block the view (see `world/trees.rs`, and `trees/drawing.ron`
// in the content for the distances).
//
// A fragment nearer the camera than the fade's start is gone; beyond its
// end it's all there; in between it fades smoothly. It fades through alpha
// to coverage (with multisampling on, which the game has): the fade sets how
// many of each pixel's samples are covered, which needs no sorting, unlike
// blending. Without multisampling it dithers instead, dropping some pixels
// in interleaved gradient noise (Jimenez, "Next Generation Post Processing
// in Call of Duty: Advanced Warfare", 2014). Shadows are drawn by the
// standard shadow pass, so a faded tree still casts its shadow.

#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::alpha_discard,
    mesh_view_bindings::view,
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

struct TreeFade {
    // x: where the fade starts (gone nearer), y: where it ends (all there
    // further), in metres from the camera; z: 1 if fading is on
    settings: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> fade: TreeFade;

// A number from 0 to 1 per pixel, scattered evenly (interleaved gradient noise).
fn dither(pixel: vec2<f32>) -> f32 {
    return fract(52.9829189 * fract(dot(pixel, vec2<f32>(0.06711056, 0.00583715))));
}

// How much of a tree shows at `p`: none right by the camera, all of it further away.
fn opacity(p: vec3<f32>) -> f32 {
    return smoothstep(fade.settings.x, fade.settings.y, distance(p, view.world_position));
}

@fragment
fn fragment(
    in: VertexOutput,
    @builtin(front_facing) is_front: bool,
) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    pbr_input.material.base_color = alpha_discard(pbr_input.material, pbr_input.material.base_color);

    var shown = 1.0;
    if fade.settings.z > 0.5 {
        shown = opacity(in.world_position.xyz);
    }
#ifdef MAY_DISCARD
    // Without multisampling (alpha to coverage falls back to a mask): dither.
    if shown <= dither(in.position.xy) {
        discard;
    }
#endif

#ifdef PREPASS_PIPELINE
    let out = deferred_output(in, pbr_input);
#else
    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
#ifndef MAY_DISCARD
    // With multisampling, alpha to coverage turns the fade into the share of
    // samples covered (a little noise hides the hardware's fixed patterns).
    if shown < 1.0 {
        out.color.a = saturate(out.color.a * shown + (dither(in.position.xy) - 0.5) * 0.2 * (1.0 - shown));
    }
#endif
#endif
    return out;
}
