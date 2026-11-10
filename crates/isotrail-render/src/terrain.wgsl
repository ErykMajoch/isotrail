struct Uniforms {
    view_proj: mat4x4<f32>,
    light_view_proj: mat4x4<f32>,
    // xyz towards the sun, w the ambient share
    sun: vec4<f32>,
    // rgb the sun's colour times its intensity, w the band count
    sun_colour: vec4<f32>,
    // rgb tints the ambient light
    ambient_colour: vec4<f32>,
    // xyz towards the camera, w the fill share of the ambient
    fill: vec4<f32>,
    // x how far to push lookups along the normal, in metres
    shadow: vec4<f32>,
    // x how deep a passing cloud's shade is (0 for none), y the noise level
    // clouds start at, z their size and w the cell size they snap to, in metres
    clouds: vec4<f32>,
    // xy where the cells start, zw how far the wind has moved the clouds
    cloud_origin: vec4<f32>,
    // The slab's extent: low x and y, then high x and y
    slab: vec4<f32>,
    // x how much hollows lose of the sky's light, y how far shadow edges
    // soften in shadow texels, z the cell size in metres
    shading: vec4<f32>,
    // x how strongly beams of sunlight light the ground (0 for none), y the
    // width of a beam and its gap and z how far the wind has moved them, in
    // metres
    rays: vec4<f32>,
    // xyz across the sheets of light
    ray_axis: vec4<f32>,
    // x the id written to the object-id target, y the clouds' seed
    object: vec4<u32>,
}

@group(0) @binding(0) var<uniform> uniforms: Uniforms;
@group(0) @binding(1) var shadow_map: texture_depth_2d;
@group(0) @binding(2) var shadow_sampler: sampler_comparison;
// How much of the sky each cell sees
@group(1) @binding(0) var sky_view: texture_2d<f32>;
@group(1) @binding(1) var sky_view_sampler: sampler;

const RAY_GAP_SHADE: f32 = 0.5;
const RAY_BOOST: f32 = 1.0;

struct VertexIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) colour: vec4<f32>,
}

struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) @interpolate(flat, either) normal: vec3<f32>,
    @location(1) @interpolate(flat, either) colour: vec3<f32>,
    @location(2) world: vec3<f32>,
}

@vertex
fn vs_shadow(in: VertexIn) -> @builtin(position) vec4<f32> {
    return uniforms.light_view_proj * vec4<f32>(in.position, 1.0);
}

@vertex
fn vs_main(in: VertexIn) -> VertexOut {
    var out: VertexOut;
    out.position = uniforms.view_proj * vec4<f32>(in.position, 1.0);
    out.normal = in.normal;
    out.colour = in.colour.rgb;
    out.world = in.position;
    return out;
}

const SOFT_TAPS: array<vec2<f32>, 12> = array<vec2<f32>, 12>(
    vec2<f32>(-0.326, -0.406), vec2<f32>(-0.840, -0.074), vec2<f32>(-0.696, 0.457),
    vec2<f32>(-0.203, 0.621), vec2<f32>(0.962, -0.195), vec2<f32>(0.473, -0.480),
    vec2<f32>(0.519, 0.767), vec2<f32>(0.185, -0.893), vec2<f32>(0.507, 0.064),
    vec2<f32>(0.896, 0.412), vec2<f32>(-0.322, -0.933), vec2<f32>(-0.792, -0.598),
);

fn sunlit(world: vec3<f32>, normal: vec3<f32>) -> f32 {
    let radius = uniforms.shading.y;
    // Wider softening reaches further across a slope, so lookups move
    // further off it
    let offset = uniforms.shadow.x * (1.0 + 0.5 * radius);
    let clip = uniforms.light_view_proj * vec4<f32>(world + normal * offset, 1.0);
    let uv = vec2<f32>(clip.x * 0.5 + 0.5, 0.5 - clip.y * 0.5);
    if radius <= 0.0 {
        return textureSampleCompareLevel(shadow_map, shadow_sampler, uv, clip.z);
    }
    let texel = radius / f32(textureDimensions(shadow_map).x);
    var light = 0.0;
    for (var i = 0; i < 12; i++) {
        light += textureSampleCompareLevel(shadow_map, shadow_sampler, uv + SOFT_TAPS[i] * texel, clip.z);
    }
    return light / 12.0;
}

// How much of the sky a face sees, from the cell in front of it
fn sky_seen(world: vec3<f32>, normal: vec3<f32>) -> f32 {
    let slab = uniforms.slab;
    let at = world.xy + normal.xy * 0.5 * uniforms.shading.z;
    let uv = vec2<f32>((at.x - slab.x) / (slab.z - slab.x), (slab.w - at.y) / (slab.w - slab.y));
    let seen = textureSampleLevel(sky_view, sky_view_sampler, clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0)), 0.0).r;
    // The slab's cut faces look out into open air
    let outside = any(uv < vec2<f32>(0.0)) || any(uv > vec2<f32>(1.0));
    return select(seen, 1.0, outside);
}

fn hash(x: i32, y: i32, seed: u32) -> f32 {
    var h = bitcast<u32>(x) * 374761393u + bitcast<u32>(y) * 668265263u + seed * 2246822519u;
    h = (h ^ (h >> 13u)) * 1274126177u;
    h = h ^ (h >> 16u);
    return f32(h & 0xffffu) / 65535.0;
}

fn value_noise(p: vec2<f32>, seed: u32) -> f32 {
    let i = vec2<i32>(floor(p));
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash(i.x, i.y, seed);
    let b = hash(i.x + 1, i.y, seed);
    let c = hash(i.x, i.y + 1, seed);
    let d = hash(i.x + 1, i.y + 1, seed);
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

// 1 in the open, less under a passing cloud, the same across each cell
fn cloud_light(world: vec3<f32>) -> f32 {
    let depth = uniforms.clouds.x;
    if depth <= 0.0 {
        return 1.0;
    }
    let cell = uniforms.clouds.w;
    let snapped = (floor((world.xy - uniforms.cloud_origin.xy) / cell) + 0.5) * cell;
    let p = (snapped - uniforms.cloud_origin.zw) / uniforms.clouds.z;
    let seed = uniforms.object.y;
    let n = 0.65 * value_noise(p, seed) + 0.35 * value_noise(p * 2.3 + 17.0, seed + 1u);
    return select(1.0, 1.0 - depth, n > uniforms.clouds.y);
}

fn beam_hash(i: i32, seed: u32) -> f32 {
    var h = bitcast<u32>(i) * 374761393u + seed * 668265263u;
    h = (h ^ (h >> 13u)) * 1274126177u;
    h = h ^ (h >> 16u);
    return f32(h & 0xffffu) / 65535.0;
}

fn beam_noise(x: f32, seed: u32) -> f32 {
    let i = floor(x);
    let f = x - i;
    let u = f * f * (3.0 - 2.0 * f);
    return mix(beam_hash(i32(i), seed), beam_hash(i32(i) + 1, seed), u);
}

// How much light reaches a sheet this many beam widths across, from 0 in a
// gap to 1 in a beam; the same as the sky's
fn beam(across: f32) -> f32 {
    let n = 0.6 * beam_noise(across, 71u) + 0.4 * beam_noise(across * 2.7 + 5.0, 72u);
    return smoothstep(0.55, 0.6, n);
}

// How much of the light a beam brings to this point, snapped to whole
// cells across the ground like the clouds' shade
fn in_beam(world: vec3<f32>) -> f32 {
    let cell = uniforms.shading.z;
    let origin = uniforms.slab.xy;
    let snapped = vec3<f32>((floor((world.xy - origin) / cell) + 0.5) * cell + origin, world.z);
    return beam((dot(snapped, uniforms.ray_axis.xyz) + uniforms.rays.z) / uniforms.rays.y);
}

struct FragmentOut {
    @location(0) colour: vec4<f32>,
    @location(1) normal: vec4<f32>,
    @location(2) id: u32,
}

@fragment
fn fs_main(in: VertexOut) -> FragmentOut {
    let ambient = uniforms.sun.w;
    let bands = uniforms.sun_colour.w;
    let facing = round(max(dot(in.normal, uniforms.fill.xyz), 0.0) * bands) / bands;
    // Hollows lose skylight in bands, like the sun's light
    let seen = ceil(sky_seen(in.world, in.normal) * bands) / bands;
    let open = mix(1.0, seen, uniforms.shading.x);
    let sky = open * ambient * uniforms.ambient_colour.rgb * (1.0 + uniforms.fill.w * facing);

    // Sampled for every face so the lookup stays in uniform control flow
    let shadow = sunlit(in.world, in.normal);
    // Ground in the gaps between beams of sunlight gets less of it, and
    // ground in a beam more
    let rays = uniforms.rays.x;
    let lit = select(0.0, in_beam(in.world), rays > 0.0);
    let gap = 1.0 - RAY_GAP_SHADE * rays * (1.0 - lit);
    let diffuse = max(dot(in.normal, uniforms.sun.xyz), 0.0) * shadow * cloud_light(in.world) * gap;
    let banded = ceil(diffuse * bands - 0.25) / bands;
    let beam_boost = 1.0 + RAY_BOOST * rays * lit;
    let direct = beam_boost * mix(1.0, open, 0.3) * uniforms.sun_colour.rgb * (1.0 - ambient) * clamp(banded, 0.0, 1.0);
    var out: FragmentOut;
    out.colour = vec4<f32>(min(in.colour * (sky + direct), vec3<f32>(1.0)), 1.0);
    out.normal = vec4<f32>(in.normal * 0.5 + 0.5, 1.0);
    out.id = uniforms.object.x;
    return out;
}
