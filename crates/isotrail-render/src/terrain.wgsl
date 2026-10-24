struct Uniforms {
    view_proj: mat4x4<f32>,
    light_view_proj: mat4x4<f32>,
    // xyz towards the sun, w the ambient share
    sun: vec4<f32>,
    // rgb the sun's colour times its intensity, w the band count
    sun_colour: vec4<f32>,
    // xyz towards the camera, w the fill share of the ambient
    fill: vec4<f32>,
    // x how far to push lookups along the normal, in metres
    shadow: vec4<f32>,
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

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let ambient = uniforms.sun.w;
    let bands = uniforms.sun_colour.w;
    let facing = round(max(dot(in.normal, uniforms.fill.xyz), 0.0) * bands) / bands;
    let sky = ambient * (1.0 + uniforms.fill.w * facing);

    // Sampled for every face so the lookup stays in uniform control flow
    let shadow = sunlit(in.world, in.normal);
    let diffuse = max(dot(in.normal, uniforms.sun.xyz), 0.0) * shadow;
    let banded = ceil(diffuse * bands - 0.25) / bands;
    let direct = uniforms.sun_colour.rgb * (1.0 - ambient) * clamp(banded, 0.0, 1.0);
    return vec4<f32>(min(in.colour * (sky + direct), vec3<f32>(1.0)), 1.0);
}
