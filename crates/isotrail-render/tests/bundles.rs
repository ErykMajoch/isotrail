use isotrail_bundle::{GridInfo, Landcover, MoonSample, SceneBundle, SunSample};
use isotrail_render::{
    Mesher, RenderError, RouteLine, SmoothMesher, SunState, VoxelMesher, Walk, mesh_within,
};
use isotrail_testkit::{bundle, gpu, grid, hike_bundle, synthetic_weather};
use miniz_oxide::deflate::compress_to_vec;
use miniz_oxide::inflate::decompress_to_vec;

const MAX_BYTES: u64 = 256 << 20;

/// A small slab with every part of a bundle filled in, so that each can be
/// damaged
fn full_bundle() -> SceneBundle {
    use Landcover::*;
    let classes = [
        Water, Grassland, Coniferous, Broadleaf, Rock, Scree, Scrub, Heath,
    ];
    let grid = GridInfo {
        rotation_rad: 0.3,
        exaggeration: 2.0,
        ..grid(16, 8, 30.0)
    };
    let heights: Vec<f32> = (0..128)
        .map(|i| 400.0 + ((i * 37) % 23) as f32 * 4.0)
        .collect();
    let landcover: Vec<Landcover> = (0..128).map(|i| classes[(i / 3) % 8]).collect();
    let mut bundle = bundle(grid, &heights, &landcover);
    bundle.terrain.lines[40] = isotrail_bundle::line_mask::PATH;
    bundle.route.points = (0..12)
        .map(|i| isotrail_bundle::RoutePoint {
            col: 1.5 + i as f32 * 1.1,
            row: 2.0 + (i % 4) as f32,
            distance_m: i as f32 * 40.0,
            time: Some(1_750_000_200 + i64::from(i) * 120),
            gps_ele_m: Some(410.0 + i as f32),
            draped_m: 405.0,
        })
        .collect();
    bundle.route.breaks = vec![5, 9];
    bundle.profile = (0..8)
        .map(|i| isotrail_bundle::ProfileSample {
            distance_m: i as f32 * 60.0,
            ele_m: 400.0 + i as f32 * 3.0,
        })
        .collect();
    bundle.season.leaf_day = Some(160);
    bundle.season.snow_line_m = Some(430.0);
    let start = 1_750_000_200;
    bundle.sun = (0..5)
        .map(|i| SunSample {
            time: start + i * 300,
            azimuth_deg: 120.0 + i as f32,
            elevation_deg: 30.0 + i as f32,
        })
        .collect();
    bundle.moon = bundle
        .sun
        .iter()
        .map(|s| MoonSample {
            time: s.time,
            azimuth_deg: 250.0,
            elevation_deg: -10.0,
            illuminated: 0.4,
        })
        .collect();
    bundle.weather = Some(synthetic_weather(1_749_999_600, 3));
    bundle
}

fn splitmix(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// Everything a still or a clip does with a bundle before the GPU
fn use_bundle(bundle: &SceneBundle) {
    let meshers: [&dyn Mesher; 2] = [&VoxelMesher::default(), &SmoothMesher];
    for mesher in meshers {
        let Ok(mut mesh) = mesh_within(mesher, bundle, MAX_BYTES) else {
            continue;
        };
        let metres_per_px = bundle.grid.cell_size_m as f32 / 2.0;
        mesh.add_props(bundle, metres_per_px);
        RouteLine::drape(bundle, &mesh.surface, metres_per_px / 2.0);
    }
    Walk::new(bundle);
    SunState::for_hike(bundle);
    if let Some(&first) = bundle.sun.first() {
        SunState::during(bundle, first.time as f64 + 450.0);
    }
}

/// Flips one byte of the payload at a time, keeping the file's header
fn fuzz(bytes: &[u8], mut state: u64) {
    let payload = decompress_to_vec(&bytes[6..]).unwrap();
    let (mut refused, mut read) = (0, 0);
    for _ in 0..2_000 {
        let mut damaged = payload.clone();
        let at = (splitmix(&mut state) % damaged.len() as u64) as usize;
        damaged[at] ^= (splitmix(&mut state) % 255 + 1) as u8;
        let file = [&bytes[..6], &compress_to_vec(&damaged, 6)].concat();
        match SceneBundle::from_bytes(&file) {
            Ok(bundle) => {
                use_bundle(&bundle);
                read += 1;
            }
            Err(err) => {
                assert!(
                    err.to_string().starts_with("the bundle is damaged"),
                    "{err}"
                );
                refused += 1;
            }
        }
    }
    assert!(
        refused > 100 && read > 100,
        "{refused} refused, {read} read"
    );
}

#[test]
fn damaged_payloads_are_refused_or_render_without_panicking() {
    fuzz(&full_bundle().to_bytes().unwrap(), 0x1507_2026);
}

#[test]
fn damaged_payloads_in_the_older_format_are_refused_or_render_without_panicking() {
    let path = format!(
        "{}/../../fixtures/bundles/format-{}.itb",
        env!("CARGO_MANIFEST_DIR"),
        isotrail_bundle::OLDEST_FORMAT_VERSION
    );
    let bytes = std::fs::read(path).unwrap();
    assert_eq!(
        isotrail_bundle::format_version(&bytes),
        Some(isotrail_bundle::OLDEST_FORMAT_VERSION)
    );
    fuzz(&bytes, 0x0505_2026);
}

/// A 1024 x 1024 slab of jagged peaks, as tall as the exaggeration goes
fn jagged() -> SceneBundle {
    let grid = GridInfo {
        exaggeration: 100.0,
        ..grid(1024, 1024, 10.0)
    };
    let heights: Vec<f32> = (0..grid.cells())
        .map(|i| (i as u64 * 7_919 % 1_000) as f32)
        .collect();
    bundle(grid, &heights, &vec![Landcover::Grassland; grid.cells()])
}

#[test]
fn a_mesh_too_big_for_the_gpu_is_refused_before_it_is_made() {
    let gpu = gpu();
    let max = gpu.max_buffer_size();
    let huge = jagged();
    let mesher = VoxelMesher { block_height: 0.25 };
    match mesh_within(&mesher, &huge, max) {
        Err(err @ RenderError::MeshTooLarge { .. }) => {
            let message = err.to_string();
            assert!(message.contains(&format!("({}MB)", max >> 20)), "{message}");
            assert!(message.contains("--exaggeration"), "{message}");
        }
        other => panic!("{:?}", other.map(|mesh| mesh.vertices.len())),
    }
    let small = hike_bundle();
    let mesh = mesh_within(&mesher, &small, max).unwrap();
    let bound = mesher.vertex_bound(&small, u64::MAX);
    assert!(mesh.vertices.len() as u64 <= bound);
}
