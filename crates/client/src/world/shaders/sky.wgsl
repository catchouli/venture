// The sky: two photographed skies (cubemaps), blended, with the sun and the
// moon drawn where they really are. Drawn on the inside of a big sphere
// round the camera, in place of Bevy's one-cubemap skybox. See
// `world/sky.rs` and `docs/plan-day-night.md`.

#import bevy_pbr::{
    forward_io::VertexOutput,
    mesh_view_bindings::view,
}

struct SkySettings {
    // Turns world directions into each sky's own (the inverse of how it's
    // turned in the world).
    first_turn: mat4x4<f32>,
    second_turn: mat4x4<f32>,
    // x, y: how bright each sky is (cd/m² for its brightness scale's 1,
    // already weighted by the blend).
    strengths: vec4<f32>,
    // xyz: towards the sun; w: the cosine of the disc's radius.
    sun: vec4<f32>,
    // rgb: the disc's radiance (cd/m²); w: how strong its glow is.
    sun_colour: vec4<f32>,
    // xyz: towards the moon; w: the cosine of the disc's radius.
    moon: vec4<f32>,
    // rgb: the disc's radiance (cd/m²).
    moon_colour: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> sky: SkySettings;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var first: texture_cube<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var first_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var second: texture_cube<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var second_sampler: sampler;

// A sky's colour towards a world direction (its cubemap is stored with the
// world's z flipped, as Bevy reads cubemaps).
fn look(local: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(local.x, local.y, -local.z);
}

// A disc `radius_cos` across towards `centre`, softened over a pixel.
fn disc(towards: f32, radius_cos: f32) -> f32 {
    let edge = max(fwidth(towards), 1e-6);
    return smoothstep(radius_cos - edge, radius_cos + edge, towards);
}

// Smooth value noise on the moon's face, for its darker seas.
fn hash(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(127.1, 311.7))) * 43758.5453);
}
fn noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    return mix(mix(hash(i), hash(i + vec2<f32>(1.0, 0.0)), u.x), mix(hash(i + vec2<f32>(0.0, 1.0)), hash(i + vec2<f32>(1.0, 1.0)), u.x), u.y);
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let direction = normalize(in.world_position.xyz - view.world_position);
    let a = (sky.first_turn * vec4<f32>(direction, 0.0)).xyz;
    let b = (sky.second_turn * vec4<f32>(direction, 0.0)).xyz;
    var colour = textureSample(first, first_sampler, look(a)).rgb * sky.strengths.x
        + textureSample(second, second_sampler, look(b)).rgb * sky.strengths.y;

    // Nothing of the sun or moon below the horizon (where the far land
    // stops, the sky shows through).
    let above = smoothstep(-0.01, 0.01, direction.y);

    // The sun: a disc, and a glow round it.
    let to_sun = dot(direction, sky.sun.xyz);
    let glow = pow(max(to_sun, 0.0), 1200.0) * 0.02 + pow(max(to_sun, 0.0), 60.0) * 0.0015;
    colour += sky.sun_colour.rgb * (disc(to_sun, sky.sun.w) + glow * sky.sun_colour.w) * above;

    // The moon: a disc, darker at its edge, with seas.
    let to_moon = dot(direction, sky.moon.xyz);
    let lit = disc(to_moon, sky.moon.w);
    if lit > 0.0 {
        // Where on its face (0 in the middle, 1 at the edge).
        let across = sqrt(max(1.0 - to_moon, 0.0) / max(1.0 - sky.moon.w, 1e-6));
        let side = normalize(cross(sky.moon.xyz, vec3<f32>(0.0, 1.0, 0.0)));
        let up = cross(side, sky.moon.xyz);
        let face = vec2<f32>(dot(direction, side), dot(direction, up)) / sqrt(max(1.0 - sky.moon.w * sky.moon.w, 1e-8));
        let seas = 1.0 - 0.35 * smoothstep(0.45, 0.75, noise(face * 3.0 + 4.0) * 0.7 + noise(face * 7.0) * 0.3);
        let limb = 0.7 + 0.3 * sqrt(max(1.0 - across * across, 0.0));
        colour += sky.moon_colour.rgb * lit * seas * limb * above;
    }
    colour += sky.moon_colour.rgb * pow(max(to_moon, 0.0), 800.0) * 0.004 * above;

    return vec4<f32>(colour * view.exposure, 1.0);
}
