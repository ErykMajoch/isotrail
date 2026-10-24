use std::fs::{self, File};
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};

use isotrail_geo::{
    FeatureSource, Footprint, Grid, MINOR_WAY_MAX_CELL_M, OsmFeatures, OsmId, OsmKind,
    OverpassSource, RecordedFetcher, Ring,
};
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

fn closed(ring: &Ring) -> bool {
    ring.len() >= 4 && ring[0] == ring[ring.len() - 1]
}

/// (lat, lon) offsets of the hand-made fixture's 0.001° grid
fn at(x: f64, y: f64) -> (f64, f64) {
    (50.0 + y / 1000.0, 10.0 + x / 1000.0)
}

fn square(x: f64, y: f64, size: f64) -> Vec<(f64, f64)> {
    [(x, y), (x + size, y), (x + size, y + size), (x, y + size)]
        .into_iter()
        .map(|(x, y)| at(x, y))
        .collect()
}

/// Same vertices, wherever the ring starts and whichever way it runs
fn same_ring(ring: &Ring, expected: &[(f64, f64)]) -> bool {
    let close = |a: (f64, f64), b: (f64, f64)| (a.0 - b.0).abs() < 1e-9 && (a.1 - b.1).abs() < 1e-9;
    let open = &ring[..ring.len() - 1];
    open.len() == expected.len() && expected.iter().all(|&e| open.iter().any(|&p| close(p, e)))
}

#[test]
fn recorded_response_gives_typed_mountain_features() {
    let grid = snowdon_grid();
    let fetcher = RecordedFetcher::new(fixtures().join("osm"));
    let features = OverpassSource::new(fetcher).features(&grid).unwrap();

    let count = |key: &str, value: &str| {
        features
            .areas
            .iter()
            .map(|a| &a.tags)
            .chain(features.lines.iter().map(|l| &l.tags))
            .filter(|t| t.is(key, value))
            .count()
    };
    assert!(
        grid.cell_size_m > MINOR_WAY_MAX_CELL_M,
        "this grid is meant to be coarse"
    );
    assert_eq!(count("highway", "path") + count("waterway", "stream"), 0);
    assert!(count("natural", "cliff") >= 3, "{:?}", features.counts());
    for (key, value) in [
        ("highway", "primary"),
        ("natural", "scree"),
        ("natural", "bare_rock"),
        ("natural", "water"),
    ] {
        assert!(count(key, value) > 0, "no {key}={value}");
    }
    assert!(
        features
            .areas
            .iter()
            .any(|a| a.id.kind == OsmKind::Relation)
    );
    assert!(
        features
            .lines
            .iter()
            .all(|l| l.tags.get("highway").is_some()
                || l.tags.get("waterway").is_some()
                || l.tags.is("natural", "cliff"))
    );

    let rings = features
        .areas
        .iter()
        .flat_map(|a| &a.polygons)
        .flat_map(|p| std::iter::once(&p.outer).chain(&p.inner));
    let (cx, cy) = grid.footprint.center;
    for ring in rings {
        assert!(closed(ring), "{ring:?}");
        for &(lat, lon) in ring {
            let (x, y) = grid.footprint.projection.project(lat, lon);
            assert!((x - cx).hypot(y - cy) < 5_000.0, "{lat},{lon}");
        }
    }
}

#[test]
fn hand_made_multipolygons_assemble_into_rings_with_holes() {
    let bytes = fs::read(fixtures().join("osm/multipolygon.json")).unwrap();
    let features = OsmFeatures::from_overpass_json(&bytes).unwrap();
    let ids =
        |ids: &mut dyn Iterator<Item = OsmId>| ids.map(|id| id.to_string()).collect::<Vec<_>>();
    assert_eq!(
        ids(&mut features.areas.iter().map(|a| a.id)),
        ["relation/1", "relation/2", "way/100"]
    );
    assert_eq!(
        ids(&mut features.lines.iter().map(|l| l.id)),
        ["way/101", "way/102"]
    );

    let wood = &features.areas[0];
    assert!(wood.tags.is("natural", "wood") && wood.tags.is("leaf_type", "mixed"));
    assert_eq!(wood.polygons.len(), 2, "the unclosable outer is dropped");
    let split = &wood.polygons[0];
    let joined = [
        (0.0, 0.0),
        (10.0, 0.0),
        (10.0, 5.0),
        (10.0, 10.0),
        (0.0, 10.0),
    ];
    assert!(same_ring(&split.outer, &joined.map(|(x, y)| at(x, y))));
    assert_eq!(split.inner.len(), 2, "the orphan hole is dropped");
    assert!(same_ring(&split.inner[0], &square(2.0, 2.0, 2.0)));
    assert!(same_ring(&split.inner[1], &square(6.0, 6.0, 2.0)));
    let unlabelled = &wood.polygons[1];
    assert!(same_ring(&unlabelled.outer, &square(20.0, 0.0, 10.0)));
    assert_eq!(unlabelled.inner.len(), 1);
    assert!(same_ring(&unlabelled.inner[0], &square(22.0, 2.0, 2.0)));

    let island = &features.areas[1];
    assert!(island.tags.is("landuse", "forest"));
    let [shore, isle] = &island.polygons[..] else {
        panic!("{:?}", island.polygons);
    };
    assert!(same_ring(&shore.outer, &square(0.0, 20.0, 30.0)));
    assert_eq!(shore.inner.len(), 1);
    assert!(same_ring(&shore.inner[0], &square(5.0, 25.0, 20.0)));
    assert!(same_ring(&isle.outer, &square(10.0, 30.0, 10.0)));
    assert_eq!(isle.inner.len(), 1);
    assert!(same_ring(&isle.inner[0], &square(12.0, 32.0, 2.0)));

    for ring in features
        .areas
        .iter()
        .flat_map(|a| &a.polygons)
        .flat_map(|p| std::iter::once(&p.outer).chain(&p.inner))
    {
        assert!(closed(ring), "{ring:?}");
    }
    assert!(features.lines[1].tags.is("natural", "cliff"));
    assert_eq!(features.lines[0].points.len(), 3);
}
