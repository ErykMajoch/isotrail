struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) colour: vec3<f32>,
}

const QUAD: vec3<f32> = vec3<f32>(240.0, 200.0, 64.0) / 255.0;

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VertexOut {
    var positions = array<vec2<f32>, 9>(
        vec2<f32>(-0.85, -0.7),
        vec2<f32>(-0.1, -0.7),
        vec2<f32>(-0.475, 0.75),
        vec2<f32>(0.1, -0.5),
        vec2<f32>(0.8, -0.5),
        vec2<f32>(0.8, 0.5),
        vec2<f32>(0.1, -0.5),
        vec2<f32>(0.8, 0.5),
        vec2<f32>(0.1, 0.5),
    );
    var colours = array<vec3<f32>, 9>(
        vec3<f32>(1.0, 0.0, 0.0),
        vec3<f32>(0.0, 1.0, 0.0),
        vec3<f32>(0.0, 0.0, 1.0),
        QUAD,
        QUAD,
        QUAD,
        QUAD,
        QUAD,
        QUAD,
    );
    var out: VertexOut;
    out.position = vec4<f32>(positions[index], 0.0, 1.0);
    out.colour = colours[index];
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    return vec4<f32>(in.colour, 1.0);
}
