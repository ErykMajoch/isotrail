struct Uniforms {
    inv_view_proj: mat4x4<f32>,
    sky_top: vec4<f32>,
    sky_bottom: vec4<f32>,
    // silhouette, depth-break and crease strengths, then the depth-break
    // threshold in metres
    edges: vec4<f32>,
    // x the crease threshold as a cosine, y the sky's dither spread, z the
    // palette size (0 for none), w how much the route's border darkens
    look: vec4<f32>,
    // rgb the route's colour, a its opacity where hidden
    route: vec4<f32>,
    // x the route's width in pixels, y the sprite count, z the background:
    // 0 sky, 1 a flat colour kept as it is, 2 transparent, w the stars' seed
    counts: vec4<u32>,
    // x how brightly the stars show, y how far they have moved in pixels,
    // z the moon's opacity, w its radius in pixels
    night: vec4<f32>,
    // xy the moon's centre in pixels, zw towards its sunlit side
    moon: vec4<f32>,
    // x where the terminator crosses the moon's axis, from -1 (full) to 1 (new)
    moon_phase: vec4<f32>,
    // x the noise level clouds start at (above 1 for none), y how far they
    // have drifted in pixels, z how hazy the air is, w seconds into the clip
    weather: vec4<f32>,
    // x how thickly rain falls and y snow, z and w their sideways pixels for
    // each pixel of fall
    falling: vec4<f32>,
    cloud_lit: vec4<f32>,
    cloud_shade: vec4<f32>,
    haze_colour: vec4<f32>,
    // xy each sprite's top-left pixel, zw its size
    sprite_frame: array<vec4<i32>, 4>,
    // xy each sprite's top-left texel in the atlas
    sprite_atlas: array<vec4<i32>, 4>,
    // xyz OKLab
    palette_lab: array<vec4<f32>, 256>,
    // rgb sRGB
    palette_rgb: array<vec4<f32>, 256>,
}

@group(0) @binding(0) var<uniform> uniforms: Uniforms;
@group(0) @binding(1) var colour_map: texture_2d<f32>;
@group(0) @binding(2) var normal_map: texture_2d<f32>;
@group(0) @binding(3) var id_map: texture_2d<u32>;
// A plain float texture read through a nearest sampler: GLSL can't load
// texels from depth textures, and WebGL2 reads zeros from one without a
// sampler that turns filtering off
@group(0) @binding(4) var depth_map: texture_2d<f32>;
// 0 no route, 1 hidden, 2 visible
@group(0) @binding(5) var route_map: texture_2d<u32>;
@group(0) @binding(6) var sprite_map: texture_2d<f32>;
@group(0) @binding(7) var depth_sampler: sampler;

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
    s.depth = textureSampleLevel(depth_map, depth_sampler, (vec2<f32>(p) + 0.5) / vec2<f32>(size), 0.0).r;
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

// The opaque texel of the frontmost sprite covering the pixel, if any
fn sprite_at(pixel: vec2<i32>) -> vec4<f32> {
    var colour = vec4<f32>(0.0);
    for (var i = 0u; i < uniforms.counts.y; i++) {
        let frame = uniforms.sprite_frame[i];
        let local = pixel - frame.xy;
        if all(local >= vec2<i32>(0)) && all(local < frame.zw) {
            let texel = textureLoad(sprite_map, uniforms.sprite_atlas[i].xy + local, 0);
            if texel.a > 0.5 {
                colour = texel;
            }
        }
    }
    return colour;
}

// The route's line widened down and to the right, visible winning over hidden
fn route_at(pixel: vec2<i32>) -> u32 {
    let size = vec2<i32>(textureDimensions(route_map));
    let width = i32(uniforms.counts.x);
    var mark = 0u;
    for (var dy = 0; dy < width; dy++) {
        for (var dx = 0; dx < width; dx++) {
            let p = pixel - vec2<i32>(dx, dy);
            if all(p >= vec2<i32>(0)) && all(p < size) {
                mark = max(mark, textureLoad(route_map, p, 0).r);
            }
        }
    }
    return mark;
}

fn borders_route(pixel: vec2<i32>) -> bool {
    let offsets = array<vec2<i32>, 4>(vec2<i32>(1, 0), vec2<i32>(-1, 0), vec2<i32>(0, 1), vec2<i32>(0, -1));
    for (var i = 0; i < 4; i++) {
        if route_at(pixel + offsets[i]) == 2u {
            return true;
        }
    }
    return false;
}

@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let pixel = vec2<i32>(position.xy);
    let sprite = sprite_at(pixel);
    if sprite.a > 0.5 {
        return vec4<f32>(quantise(sprite.rgb), 1.0);
    }
    let mark = route_at(pixel);
    let background = uniforms.counts.z;
    if background != 0u && mark == 0u && textureLoad(id_map, pixel, 0).r == 0u && !borders_route(pixel) {
        if background == 2u {
            return vec4<f32>(0.0);
        }
        return vec4<f32>(uniforms.sky_top.rgb, 1.0);
    }
    var colour = scene(pixel);
    if mark == 2u {
        colour = uniforms.route.rgb;
    } else {
        if mark == 1u {
            colour = mix(colour, uniforms.route.rgb, uniforms.route.a);
        }
        if borders_route(pixel) {
            colour *= 1.0 - uniforms.look.w;
        }
    }
    return vec4<f32>(quantise(falling(pixel, colour)), 1.0);
}

fn hash(x: u32, y: u32, seed: u32) -> u32 {
    var h = x * 374761393u + y * 668265263u + seed * 2246822519u;
    h = (h ^ (h >> 13u)) * 1274126177u;
    return h ^ (h >> 16u);
}

fn unit(h: u32) -> f32 {
    return f32(h & 0xffffu) / 65535.0;
}

// Value noise whose lattice repeats every `period` cells across
fn wrapped_noise(p: vec2<f32>, period: i32, seed: u32) -> f32 {
    let i = vec2<i32>(floor(p));
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let x0 = u32(((i.x % period) + period) % period);
    let x1 = u32((((i.x + 1) % period) + period) % period);
    let y0 = bitcast<u32>(i.y);
    let y1 = bitcast<u32>(i.y + 1);
    let a = unit(hash(x0, y0, seed));
    let b = unit(hash(x1, y0, seed));
    let c = unit(hash(x0, y1, seed));
    let d = unit(hash(x1, y1, seed));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

// The clouds wrap after a whole turn of the view, like the stars
fn cloud_noise(x: f32, y: f32) -> f32 {
    let cell = 2.0 * f32(textureDimensions(id_map).x) / 10.0;
    let p = vec2<f32>(x / cell, y / (cell * 0.3));
    let seed = uniforms.counts.w + 101u;
    return 0.65 * wrapped_noise(p, 10, seed) + 0.35 * wrapped_noise(p * 2.0 + vec2<f32>(0.0, 7.0), 20, seed + 1u);
}

// Clouds over the sky's colour, lit on top and shaded underneath, thinning
// out towards the horizon
fn clouds(pixel: vec2<i32>, sky: vec3<f32>, t: f32) -> vec3<f32> {
    let level = uniforms.weather.x + smoothstep(0.2, 0.42, t);
    if level >= 1.0 {
        return sky;
    }
    let x = f32(pixel.x) - uniforms.night.y - uniforms.weather.y;
    let y = f32(pixel.y);
    if cloud_noise(x, y) <= level {
        return sky;
    }
    if cloud_noise(x, y - 2.0) <= level {
        return uniforms.cloud_lit.rgb;
    }
    if cloud_noise(x, y + 2.0) <= level {
        return uniforms.cloud_shade.rgb;
    }
    return mix(uniforms.cloud_lit.rgb, uniforms.cloud_shade.rgb, 0.3);
}

// Whether a `width` × `length` particle covers the pixel. Particles fall
// through a grid of `cell`-sized cells, `fallen` pixels so far and `slant`
// pixels sideways for each pixel down, and a cell holds one with a chance
// of `chance`
fn particle(pixel: vec2<i32>, cell: vec2<i32>, width: i32, length: i32, fallen: f32, slant: f32, chance: f32, salt: u32) -> bool {
    let p = vec2<f32>(pixel) + 0.5;
    // Sheared so that each particle falls straight down its column of cells
    let q = vec2<i32>(floor(vec2<f32>(p.x - slant * p.y, p.y - fallen)));
    let c = vec2<i32>(floor(vec2<f32>(q) / vec2<f32>(cell)));
    let local = q - c * cell;
    let seed = uniforms.counts.w + salt;
    if unit(hash(bitcast<u32>(c.x), bitcast<u32>(c.y), seed)) >= chance {
        return false;
    }
    let h = hash(bitcast<u32>(c.x), bitcast<u32>(c.y), seed + 7u);
    let at = vec2<i32>(
        i32((h & 0xffffu) % u32(cell.x - width + 1)),
        i32((h >> 16u) % u32(cell.y - length + 1)),
    );
    return all(local >= at) && all(local < at + vec2<i32>(width, length));
}

// Rain streaks and snowflakes in front of everything, the far layer fainter
fn falling(pixel: vec2<i32>, colour: vec3<f32>) -> vec3<f32> {
    var out = colour;
    let rain = uniforms.falling.x;
    let snow = uniforms.falling.y;
    let height = f32(textureDimensions(id_map).y);
    let time = uniforms.weather.w;
    let lit = uniforms.cloud_lit.rgb;
    if rain > 0.0 {
        let slant = uniforms.falling.z;
        // Drops catch the light against dark ground and darken a light sky
        let luma = vec3<f32>(0.2126, 0.7152, 0.0722);
        let dark = dot(out, luma) < 0.6 * dot(lit, luma);
        let drop = select(lit * vec3<f32>(0.55, 0.6, 0.7), min(lit * vec3<f32>(1.0, 1.05, 1.15), vec3<f32>(1.0)), dark);
        if particle(pixel, vec2<i32>(5, 19), 1, 2, height * 1.2 * time, slant, rain * 0.6, 11u) {
            out = mix(out, drop, 0.45);
        }
        if particle(pixel, vec2<i32>(7, 26), 1, 4, height * 1.6 * time, slant, rain * 0.8, 23u) {
            out = mix(out, drop, 0.7);
        }
    }
    if snow > 0.0 {
        let slant = uniforms.falling.w;
        let flake = mix(lit, vec3<f32>(1.0), 0.6);
        if particle(pixel, vec2<i32>(6, 7), 1, 1, height * 0.14 * time, slant, snow * 0.5, 37u) {
            out = mix(out, flake, 0.7);
        }
        if particle(pixel, vec2<i32>(9, 10), 2, 2, height * 0.22 * time, slant, snow * 0.7, 41u) {
            out = flake;
        }
    }
    return out;
}

// Distant terrain and the low sky fade into the haze
fn hazed(colour: vec3<f32>, distance: f32) -> vec3<f32> {
    return mix(colour, uniforms.haze_colour.rgb, uniforms.weather.z * distance);
}

// The stars and the moon over the sky's colour, `t` down from the top
fn night_sky(pixel: vec2<i32>, sky: vec3<f32>, t: f32) -> vec3<f32> {
    var colour = sky;
    let stars = uniforms.night.x;
    if stars > 0.0 {
        // The stars repeat after a whole turn of the view
        let turn = 2 * i32(textureDimensions(id_map).x);
        let x = ((pixel.x - i32(uniforms.night.y)) % turn + turn) % turn;
        let h = hash(u32(x), u32(pixel.y), uniforms.counts.w);
        let density = 0.007 * (1.0 - smoothstep(0.5, 0.85, t));
        if f32(h & 0xffffu) / 65535.0 < density {
            let brightness = 0.75 + 0.25 * f32(h >> 16u) / 65535.0;
            colour = mix(colour, vec3<f32>(0.92, 0.95, 1.0) * brightness, stars);
        }
    }
    let opacity = uniforms.night.z;
    if opacity > 0.0 {
        let d = (vec2<f32>(pixel) + 0.5 - uniforms.moon.xy) / uniforms.night.w;
        if dot(d, d) <= 1.0 {
            let side = uniforms.moon.zw;
            let along = dot(d, side);
            let across = dot(d, vec2<f32>(-side.y, side.x));
            let edge = uniforms.moon_phase.x * sqrt(max(1.0 - across * across, 0.0));
            let lit = vec3<f32>(0.95, 0.93, 0.82);
            // Earthshine keeps the dark side just visible
            let face = select(mix(colour, lit, 0.12), lit, along >= edge);
            colour = mix(colour, face, opacity);
        }
    }
    return colour;
}

// The lit, outlined terrain or the dithered sky, before quantising
fn scene(pixel: vec2<i32>) -> vec3<f32> {
    let centre = load(pixel);
    if centre.id == 0u {
        let size = textureDimensions(id_map);
        let t = (f32(pixel.y) + 0.5) / f32(size.y);
        // Rounded like the terrain's colour target, so both quantise the same way
        let sky = round(mix(uniforms.sky_top.rgb, uniforms.sky_bottom.rgb, t) * 255.0) / 255.0;
        let night = night_sky(pixel, sky + bayer(pixel) * uniforms.look.y, t);
        return hazed(clouds(pixel, night, t), smoothstep(0.15, 0.6, t));
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
    return hazed(colour, mix(0.3, 1.0, centre.depth));
}
