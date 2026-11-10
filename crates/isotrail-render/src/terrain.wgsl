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
    // x the id written to the object-id target, y the clouds' seed
    object: vec4<u32>,
}

@group(0) @binding(0) var<uniform> uniforms: Uniforms;
@group(0) @binding(1) var shadow_map: texture_depth_2d;
@group(0) @binding(2) var shadow_sampler: sampler_comparison;

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

fn sunlit(world: vec3<f32>, normal: vec3<f32>) -> f32 {
    let clip = uniforms.light_view_proj * vec4<f32>(world + normal * uniforms.shadow.x, 1.0);
    let uv = vec2<f32>(clip.x * 0.5 + 0.5, 0.5 - clip.y * 0.5);
    return textureSampleCompareLevel(shadow_map, shadow_sampler, uv, clip.z);
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
    let sky = ambient * uniforms.ambient_colour.rgb * (1.0 + uniforms.fill.w * facing);

    // Sampled for every face so the lookup stays in uniform control flow
    let shadow = sunlit(in.world, in.normal);
    let diffuse = max(dot(in.normal, uniforms.sun.xyz), 0.0) * shadow * cloud_light(in.world);
    let banded = ceil(diffuse * bands - 0.25) / bands;
    let direct = uniforms.sun_colour.rgb * (1.0 - ambient) * clamp(banded, 0.0, 1.0);
    var out: FragmentOut;
    out.colour = vec4<f32>(min(in.colour * (sky + direct), vec3<f32>(1.0)), 1.0);
    out.normal = vec4<f32>(in.normal * 0.5 + 0.5, 1.0);
    out.id = uniforms.object.x;
    return out;
}
