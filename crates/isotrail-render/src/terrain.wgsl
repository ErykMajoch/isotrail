struct Uniforms {
    view_proj: mat4x4<f32>,
    // xyz towards the sun, w the ambient share
    sun: vec4<f32>,
}

@group(0) @binding(0) var<uniform> uniforms: Uniforms;

struct VertexIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) colour: vec4<f32>,
}

struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) @interpolate(flat, either) normal: vec3<f32>,
    @location(1) @interpolate(flat, either) colour: vec3<f32>,
}

@vertex
fn vs_main(in: VertexIn) -> VertexOut {
    var out: VertexOut;
    out.position = uniforms.view_proj * vec4<f32>(in.position, 1.0);
    out.normal = in.normal;
    out.colour = in.colour.rgb;
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let ambient = uniforms.sun.w;
    let diffuse = max(dot(in.normal, uniforms.sun.xyz), 0.0);
    return vec4<f32>(in.colour * (ambient + (1.0 - ambient) * diffuse), 1.0);
}
