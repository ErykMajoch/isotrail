struct Uniforms {
    // xy where the frame's top-left pixel lands, z the pixels per frame
    // pixel, w 1 when the target encodes sRGB itself
    placement: vec4<i32>,
}

@group(0) @binding(0) var<uniform> uniforms: Uniforms;
@group(0) @binding(1) var frame: texture_2d<f32>;

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
}

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    return select(pow((c + 0.055) / 1.055, vec3<f32>(2.4)), c / 12.92, c <= vec3<f32>(0.04045));
}

@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let from_origin = vec2<i32>(floor(position.xy)) - uniforms.placement.xy;
    let texel = from_origin / uniforms.placement.z;
    let size = vec2<i32>(textureDimensions(frame));
    if any(from_origin < vec2<i32>(0)) || any(texel >= size) {
        return vec4<f32>(0.0);
    }
    var colour = textureLoad(frame, texel, 0);
    if uniforms.placement.w != 0 {
        colour = vec4<f32>(srgb_to_linear(colour.rgb), colour.a);
    }
    return vec4<f32>(colour.rgb * colour.a, colour.a);
}
