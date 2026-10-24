use std::fs::File;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};

use isotrail_geo::{
    DemSource, FeatureSource, Footprint, Grid, Landcover, LandcoverMap, OverpassSource,
    RecordedFetcher, TerrariumSource,
};
use isotrail_track::{GpxReader, TrackReader};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

#[test]
fn recorded_mountain_gets_rock_water_and_a_level_lake() {
    let mut file = File::open(fixtures().join("gpx/snowdon.gpx")).unwrap();
    let track = GpxReader.read(&mut file).unwrap().clean().unwrap();
    let grid = Grid::new(
        &Footprint::from_track(&track).unwrap(),
        NonZeroU32::new(32).unwrap(),
    );
    let mut heightmap = TerrariumSource::new(RecordedFetcher::new(fixtures().join("dem")))
        .heightmap(&grid)
        .unwrap();
    let features = OverpassSource::new(RecordedFetcher::new(fixtures().join("osm")))
        .features(&grid)
        .unwrap();
    let before = heightmap.clone();

    let map = LandcoverMap::classify(&grid, &features, &mut heightmap);
    assert_eq!(map.classes.len(), (grid.cols * grid.rows) as usize);
    let count = |class| map.classes.iter().filter(|&&c| c == class).count();
    assert!(count(Landcover::Water) > 0, "{:?}", map.shares());
    assert!(
        count(Landcover::Rock) + count(Landcover::Scree) > 0,
        "{:?}",
        map.shares()
    );

    assert!(!map.water.is_empty());
    for body in &map.water {
        let lowest = body
            .cells
            .iter()
            .map(|&c| before.heights_m[c as usize])
            .fold(f32::INFINITY, f32::min);
        assert!(body.level_m >= lowest);
        for &cell in &body.cells {
            assert_eq!(map.classes[cell as usize], Landcover::Water);
            assert_eq!(heightmap.heights_m[cell as usize], body.level_m);
        }
    }
    let levelled: usize = map.water.iter().map(|b| b.cells.len()).sum();
    let changed = heightmap
        .heights_m
        .iter()
        .zip(&before.heights_m)
        .filter(|(a, b)| a != b)
        .count();
    assert!(changed <= levelled);
}
