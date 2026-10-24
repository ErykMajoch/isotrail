struct Uniforms {
    inv_view_proj: mat4x4<f32>,
    sky_top: vec4<f32>,
    sky_bottom: vec4<f32>,
    // silhouette, depth-break and crease strengths, then the depth-break
    // threshold in metres
    edges: vec4<f32>,
    // x the crease threshold as a cosine, y the sky's dither spread, z the
    // palette size (0 for none)
    look: vec4<f32>,
    // xyz OKLab
    palette_lab: array<vec4<f32>, 256>,
    // rgb sRGB
    palette_rgb: array<vec4<f32>, 256>,
}

@group(0) @binding(0) var<uniform> uniforms: Uniforms;
@group(0) @binding(1) var colour_map: texture_2d<f32>;
@group(0) @binding(2) var normal_map: texture_2d<f32>;
@group(0) @binding(3) var id_map: texture_2d<u32>;
@group(0) @binding(4) var depth_map: texture_depth_2d;

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
}

struct Sample {
    id: u32,
    normal: vec3<f32>,
    depth: f32,
    world: vec3<f32>,
}

fn load(pixel: vec2<i32>) -> Sample {
    let size = vec2<i32>(textureDimensions(id_map));
    let p = clamp(pixel, vec2<i32>(0), size - 1);
    var s: Sample;
    s.id = textureLoad(id_map, p, 0).r;
    s.normal = normalize(textureLoad(normal_map, p, 0).xyz * 2.0 - 1.0);
    s.depth = textureLoad(depth_map, p, 0);
    let ndc = vec2<f32>(
        (f32(p.x) + 0.5) / f32(size.x) * 2.0 - 1.0,
        1.0 - (f32(p.y) + 0.5) / f32(size.y) * 2.0,
    );
    let world = uniforms.inv_view_proj * vec4<f32>(ndc, s.depth, 1.0);
    s.world = world.xyz / world.w;
    return s;
}

fn bayer(pixel: vec2<i32>) -> f32 {
    let m = array<f32, 16>(0.0, 8.0, 2.0, 10.0, 12.0, 4.0, 14.0, 6.0, 3.0, 11.0, 1.0, 9.0, 15.0, 7.0, 13.0, 5.0);
    return (m[(pixel.y & 3) * 4 + (pixel.x & 3)] + 0.5) / 16.0 - 0.5;
}

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    return select(pow((c + 0.055) / 1.055, vec3<f32>(2.4)), c / 12.92, c <= vec3<f32>(0.04045));
}

fn oklab(srgb: vec3<f32>) -> vec3<f32> {
    let c = srgb_to_linear(clamp(srgb, vec3<f32>(0.0), vec3<f32>(1.0)));
    let lms = vec3<f32>(
        dot(c, vec3<f32>(0.41222146, 0.53633255, 0.051445995)),
        dot(c, vec3<f32>(0.2119035, 0.6806995, 0.10739696)),
        dot(c, vec3<f32>(0.08830246, 0.28171885, 0.6299787)),
    );
    let r = pow(lms, vec3<f32>(1.0 / 3.0));
    return vec3<f32>(
        dot(r, vec3<f32>(0.21045426, 0.7936178, -0.004072047)),
        dot(r, vec3<f32>(1.9779985, -2.4285922, 0.4505937)),
        dot(r, vec3<f32>(0.025904037, 0.78277177, -0.80867577)),
    );
}

fn quantise(srgb: vec3<f32>) -> vec3<f32> {
    let count = u32(uniforms.look.z);
    if count == 0u {
        return srgb;
    }
    let lab = oklab(srgb);
    var best = 0u;
    var best_distance = 1e9;
    for (var i = 0u; i < count; i++) {
        let d = lab - uniforms.palette_lab[i].xyz;
        let distance = dot(d, d);
        if distance < best_distance {
            best = i;
            best_distance = distance;
        }
    }
    return uniforms.palette_rgb[best].rgb;
}

@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let pixel = vec2<i32>(position.xy);
    let centre = load(pixel);
    if centre.id == 0u {
        let size = textureDimensions(id_map);
        let t = (f32(pixel.y) + 0.5) / f32(size.y);
        // Rounded like the terrain's colour target, so both quantise the same way
        let sky = round(mix(uniforms.sky_top.rgb, uniforms.sky_bottom.rgb, t) * 255.0) / 255.0;
        return vec4<f32>(quantise(sky + bayer(pixel) * uniforms.look.y), 1.0);
    }

    var silhouette = false;
    var depth_break = false;
    var crease = false;
    let offsets = array<vec2<i32>, 4>(vec2<i32>(1, 0), vec2<i32>(-1, 0), vec2<i32>(0, 1), vec2<i32>(0, -1));
    for (var i = 0; i < 4; i++) {
        let other = load(pixel + offsets[i]);
        if other.id != centre.id {
            silhouette = true;
            continue;
        }
        let off_plane = dot(centre.normal, other.world - centre.world);
        if other.depth > centre.depth && abs(off_plane) > uniforms.edges.w {
            depth_break = true;
            continue;
        }
        // Each side lies on or below the other's plane, and the upper face takes the line
        let convex = off_plane <= uniforms.edges.w * 0.25
            && dot(other.normal, centre.world - other.world) <= uniforms.edges.w * 0.25;
        if convex && dot(centre.normal, other.normal) < uniforms.look.x
            && centre.normal.z >= other.normal.z - 1e-3 {
            crease = true;
        }
    }

    var colour = textureLoad(colour_map, pixel, 0).rgb;
    if silhouette {
        colour *= 1.0 - uniforms.edges.x;
    } else if depth_break {
        colour *= 1.0 - uniforms.edges.y;
    } else if crease {
        colour = min(colour * (1.0 + uniforms.edges.z), vec3<f32>(1.0));
    }
    return vec4<f32>(quantise(colour), 1.0);
}
