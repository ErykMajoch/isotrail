use std::fs::File;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};

use isotrail_geo::{DemSource, Footprint, Grid, RecordedFetcher, TerrariumSource, TileId};
use isotrail_track::{GpxReader, TrackReader};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

fn snowdon_grid() -> Grid {
    let mut file = File::open(fixtures().join("gpx/snowdon.gpx")).unwrap();
    let track = GpxReader.read(&mut file).unwrap().clean().unwrap();
    let footprint = Footprint::from_track(&track).unwrap();
    Grid::new(&footprint, NonZeroU32::new(32).unwrap())
}

#[test]
fn recorded_tiles_give_a_plausible_mountain() {
    let grid = snowdon_grid();
    let tiles = TerrariumSource::<RecordedFetcher>::tiles(&grid);
    assert_eq!(
        tiles,
        [
            TileId {
                z: 11,
                x: 1000,
                y: 666
            },
            TileId {
                z: 11,
                x: 1001,
                y: 666
            }
        ]
    );

    let fetcher = RecordedFetcher::new(fixtures().join("dem"));
    let heightmap = TerrariumSource::new(fetcher).heightmap(&grid).unwrap();
    assert_eq!((heightmap.cols, heightmap.rows), (grid.cols, grid.rows));
    assert!(heightmap.heights_m.iter().all(|h| h.is_finite()));
    let (min, max) = heightmap.range();
    assert!((950.0..1100.0).contains(&max), "summit at {max} m");
    assert!((100.0..600.0).contains(&min), "valley floor at {min} m");

    let summit = (0..grid.rows)
        .flat_map(|row| (0..grid.cols).map(move |col| (col, row)))
        .max_by(|&a, &b| heightmap.get(a.0, a.1).total_cmp(&heightmap.get(b.0, b.1)))
        .unwrap();
    let (lat, lon) = grid.cell_center_lat_lon(summit.0, summit.1);
    let (x, y) = grid.footprint.projection.project(lat, lon);
    let (sx, sy) = grid.footprint.projection.project(53.0685, -4.0763);
    let off = (x - sx).hypot(y - sy);
    assert!(
        off < 2.0 * grid.cell_size_m,
        "highest cell {off:.0} m from the summit"
    );
}

#[test]
fn neighbouring_cells_stay_continuous_across_the_seam() {
    let grid = snowdon_grid();
    let fetcher = RecordedFetcher::new(fixtures().join("dem"));
    let heightmap = TerrariumSource::new(fetcher).heightmap(&grid).unwrap();
    let mut steepest = 0.0f32;
    for row in 0..grid.rows {
        for col in 1..grid.cols {
            steepest = steepest.max((heightmap.get(col, row) - heightmap.get(col - 1, row)).abs());
        }
    }
    let grade = f64::from(steepest) / grid.cell_size_m;
    assert!(
        grade < 1.5,
        "a {steepest} m jump between neighbouring cells"
    );
}
