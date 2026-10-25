struct Uniforms {
    view_proj: mat4x4<f32>,
    // xyz added to every point, towards the camera
    lift: vec4<f32>,
    // x the furthest distance drawn, y the hidden line's dash length in pixels
    route: vec4<f32>,
}

@group(0) @binding(0) var<uniform> uniforms: Uniforms;

struct VertexIn {
    @location(0) position: vec3<f32>,
    @location(1) distance: f32,
    @location(2) length: f32,
}

struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) distance: f32,
    @location(1) length: f32,
}

@vertex
fn vs_main(in: VertexIn) -> VertexOut {
    var out: VertexOut;
    out.position = uniforms.view_proj * vec4<f32>(in.position + uniforms.lift.xyz, 1.0);
    out.distance = in.distance;
    out.length = in.length;
    return out;
}

@fragment
fn fs_visible(in: VertexOut) -> @location(0) u32 {
    if in.distance > uniforms.route.x {
        discard;
    }
    return 2u;
}

@fragment
fn fs_hidden(in: VertexOut) -> @location(0) u32 {
    let dash = uniforms.route.y;
    if in.distance > uniforms.route.x || (dash > 0.0 && (u32(in.length / dash) & 1u) == 1u) {
        discard;
    }
    return 1u;
}
