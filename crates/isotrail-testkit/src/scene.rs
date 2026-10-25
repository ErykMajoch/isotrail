use isotrail_bundle::{
    Bounds, GridInfo, HikeStats, Landcover, Metadata, Provenance, Route, RoutePoint, SceneBundle,
    Terrain, WaterBody, line_mask,
};

/// An unrotated grid centred on the origin
pub fn grid(cols: u32, rows: u32, cell_size_m: f64) -> GridInfo {
    GridInfo {
        cols,
        rows,
        cell_size_m,
        origin_lat: 0.0,
        origin_lon: 0.0,
        center_m: (0.0, 0.0),
        rotation_rad: 0.0,
        exaggeration: 1.0,
    }
}

/// A bundle with no route; water cells become one lake at their lowest height
///
/// # Panics
///
/// If the layers don't have one value per cell
pub fn bundle(grid: GridInfo, heights_m: &[f32], landcover: &[Landcover]) -> SceneBundle {
    assert_eq!(heights_m.len(), grid.cells(), "one height per cell");
    assert_eq!(landcover.len(), grid.cells(), "one class per cell");
    let mut terrain = Terrain::quantise(heights_m);
    terrain.landcover = landcover.iter().map(|&c| u8::from(c)).collect();
    terrain.lines = vec![0; grid.cells()];
    let lake: Vec<u32> = (0..grid.cells() as u32)
        .filter(|&i| landcover[i as usize] == Landcover::Water)
        .collect();
    let water = if lake.is_empty() {
        Vec::new()
    } else {
        let level_m = lake
            .iter()
            .map(|&i| terrain.height_m(i as usize))
            .fold(f32::INFINITY, f32::min);
        vec![WaterBody {
            level_m,
            cells: lake,
        }]
    };
    let mut bundle = SceneBundle {
        meta: Metadata {
            title: "Synthetic hill".into(),
            name: None,
            desc: None,
            timezone: None,
            local_date: None,
            stats: HikeStats {
                points: 0,
                segments: 0,
                distance_m: 0.0,
                gain_m: None,
                loss_m: None,
                min_ele_m: None,
                max_ele_m: None,
                bounds: Bounds {
                    min_lat: 0.0,
                    min_lon: 0.0,
                    max_lat: 0.0,
                    max_lon: 0.0,
                },
                start: None,
                end: None,
                elapsed_s: None,
                moving_s: None,
            },
            attribution: Vec::new(),
            provenance: Provenance {
                generator: "isotrail-testkit".into(),
                elevation: "synthetic".into(),
                map_data: "synthetic".into(),
            },
        },
        grid,
        terrain,
        water,
        prop_seed: 0,
        route: Route::default(),
        profile: Vec::new(),
    };
    bundle.prop_seed = bundle.content_seed();
    bundle
}

/// A turned 96 × 48 slab with a tilted plain, a rocky hill ringed by scree, a
/// levelled lake fed by a stream, two forests, a path and a road, and a
/// straight route from the lake shore to the summit
pub fn diorama_bundle() -> SceneBundle {
    let grid = GridInfo {
        rotation_rad: 0.4,
        exaggeration: 1.5,
        ..grid(96, 48, 25.0)
    };
    let (cols, rows) = (grid.cols as usize, grid.rows as usize);
    let height = |c: f32, r: f32| {
        let hill = 260.0 * (-((c - 64.0).powi(2) + (r - 20.0).powi(2)) / 220.0).exp();
        let bowl = 70.0 * (-((c - 24.0).powi(2) + (r - 30.0).powi(2)) / 60.0).exp();
        let ridge = 25.0 * (c / 7.0).sin() * (r / 11.0).cos();
        (300.0 + 1.2 * c + hill + ridge - bowl, hill)
    };
    // The lake fills the bowl up to a few metres above its floor
    let level = (0..cols * rows)
        .map(|i| height((i % cols) as f32 + 0.5, (i / cols) as f32 + 0.5).0)
        .fold(f32::INFINITY, f32::min)
        + 30.0;
    let mut heights = Vec::with_capacity(grid.cells());
    let mut classes = Vec::with_capacity(grid.cells());
    let mut lines = Vec::with_capacity(grid.cells());
    for row in 0..rows {
        for col in 0..cols {
            let (c, r) = (col as f32 + 0.5, row as f32 + 0.5);
            let (h, hill) = height(c, r);
            let class = if h < level && (c - 24.0).abs() < 14.0 {
                Landcover::Water
            } else if hill > 190.0 {
                Landcover::Rock
            } else if hill > 130.0 {
                Landcover::Scree
            } else if (c - 58.0).powi(2) / 2.0 + (r - 38.0).powi(2) < 60.0 {
                Landcover::Coniferous
            } else if (c - 12.0).powi(2) + (r - 10.0).powi(2) < 50.0 {
                Landcover::Broadleaf
            } else {
                Landcover::Grassland
            };
            // A stream into the lake, a path through the conifers, a road
            let line = match (col, row) {
                _ if class == Landcover::Water => 0,
                (90, _) => line_mask::ROAD,
                (44.., 38) => line_mask::PATH,
                (..24, 30) => line_mask::STREAM,
                _ => 0,
            };
            heights.push(if class == Landcover::Water { level } else { h });
            classes.push(class);
            lines.push(line);
        }
    }
    let mut bundle = bundle(grid, &heights, &classes);
    bundle.terrain.lines = lines;
    let (from, to) = ((34.0f32, 40.0f32), (64.0f32, 20.0f32));
    let length_m = (to.0 - from.0).hypot(to.1 - from.1) * grid.cell_size_m as f32;
    bundle.route.points = (0..=10)
        .map(|i| {
            let t = i as f32 / 10.0;
            let (col, row) = (from.0 + t * (to.0 - from.0), from.1 + t * (to.1 - from.1));
            RoutePoint {
                col,
                row,
                distance_m: t * length_m,
                time: None,
                gps_ele_m: None,
                draped_m: heights[row as usize * cols + col as usize],
            }
        })
        .collect();
    bundle.prop_seed = bundle.content_seed();
    bundle
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diorama_bundle_round_trips_and_has_every_feature() {
        let bundle = diorama_bundle();
        assert_eq!(SceneBundle::from_bytes(&bundle.to_bytes()).unwrap(), bundle);
        for class in [
            Landcover::Water,
            Landcover::Rock,
            Landcover::Scree,
            Landcover::Coniferous,
            Landcover::Broadleaf,
            Landcover::Grassland,
        ] {
            assert!(
                bundle.terrain.landcover.contains(&u8::from(class)),
                "{class:?}"
            );
        }
        for bit in [line_mask::PATH, line_mask::ROAD, line_mask::STREAM] {
            assert!(bundle.terrain.lines.iter().any(|&l| l & bit != 0), "{bit}");
        }
        let lake = &bundle.water[0];
        assert!(lake.cells.len() > 50);
        for &cell in &lake.cells {
            let height = bundle.terrain.height_m(cell as usize);
            assert!((height - lake.level_m).abs() < 0.01);
        }
    }
}
