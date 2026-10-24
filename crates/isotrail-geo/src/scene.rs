use isotrail_bundle::{
    Bounds, GridInfo, HikeStats, Metadata, ProfileSample, Provenance, Route, RoutePoint,
    SceneBundle, Terrain,
};
use isotrail_track::{Track, TrackPoint, TrackStats, haversine_m};

use crate::{GeoError, Grid, Heightmap, LandcoverMap, vertical_exaggeration};

/// Consecutive segments further apart than this are drawn with a gap
pub const ROUTE_BREAK_M: f64 = 50.0;
/// Simplification tolerance for the drawn route, in cells
pub const ROUTE_TOLERANCE_CELLS: f64 = 0.25;
pub const PROFILE_SAMPLES: usize = 256;
pub const OSM_ATTRIBUTION: &str = "© OpenStreetMap contributors";
pub const DEM_ATTRIBUTION: &str = "Terrain Tiles (AWS)";

/// Packs the prepared terrain and the route into a bundle; `heightmap` is
/// the one [`LandcoverMap::classify`] levelled
pub fn scene_bundle(
    track: &Track,
    title: &str,
    grid: &Grid,
    heightmap: &Heightmap,
    landcover: &LandcoverMap,
    provenance: Provenance,
) -> Result<SceneBundle, GeoError> {
    let stats = track.stats()?;
    let (min, max) = heightmap.range();
    let footprint = &grid.footprint;
    let (origin_lat, origin_lon) = footprint.projection.origin();

    let mut terrain = Terrain::quantise(&heightmap.heights_m);
    terrain.landcover = landcover.classes.iter().map(|&c| u8::from(c)).collect();
    terrain.lines = landcover.lines.clone();

    let along = distances(track);
    let route = route(track, &along, grid, heightmap);
    let profile = profile(track, &along, &route, stats.distance_m);

    let mut bundle = SceneBundle {
        meta: Metadata {
            title: title.to_owned(),
            name: track.name.clone(),
            desc: track.desc.clone(),
            timezone: stats.timezone.map(|tz| tz.name().to_owned()),
            local_date: stats
                .start_local()
                .map(|t| t.format("%Y-%m-%d").to_string()),
            stats: hike_stats(&stats),
            attribution: vec![OSM_ATTRIBUTION.into(), DEM_ATTRIBUTION.into()],
            provenance,
        },
        grid: GridInfo {
            cols: grid.cols,
            rows: grid.rows,
            cell_size_m: grid.cell_size_m,
            origin_lat,
            origin_lon,
            center_m: footprint.center,
            rotation_rad: footprint.rotation_rad,
            exaggeration: vertical_exaggeration(footprint.length_m, f64::from(max - min)),
        },
        terrain,
        water: landcover.water.clone(),
        prop_seed: 0,
        route,
        profile,
    };
    bundle.prop_seed = bundle.content_seed();
    Ok(bundle)
}

fn hike_stats(stats: &TrackStats) -> HikeStats {
    HikeStats {
        points: stats.points as u32,
        segments: stats.segments as u32,
        distance_m: stats.distance_m,
        gain_m: stats.gain_m,
        loss_m: stats.loss_m,
        min_ele_m: stats.min_ele_m,
        max_ele_m: stats.max_ele_m,
        bounds: Bounds {
            min_lat: stats.bounds.min_lat,
            min_lon: stats.bounds.min_lon,
            max_lat: stats.bounds.max_lat,
            max_lon: stats.bounds.max_lon,
        },
        start: stats.start.map(|t| t.timestamp()),
        end: stats.end.map(|t| t.timestamp()),
        elapsed_s: stats.elapsed.map(|d| d.num_seconds()),
        moving_s: stats.moving.map(|d| d.num_seconds()),
    }
}

/// Distance along the track at every point, summed the same way as
/// [`TrackStats::distance_m`]
fn distances(track: &Track) -> Vec<f64> {
    let mut previous: Option<&TrackPoint> = None;
    let mut along = 0.0;
    track
        .points()
        .map(|point| {
            if let Some(previous) = previous {
                along += haversine_m(previous, point);
            }
            previous = Some(point);
            along
        })
        .collect()
}

fn route(track: &Track, along: &[f64], grid: &Grid, heightmap: &Heightmap) -> Route {
    let kept = track.simplified_indices(ROUTE_TOLERANCE_CELLS * grid.cell_size_m);
    let mut route = Route::default();
    let mut first = 0;
    let mut previous: Option<&TrackPoint> = None;
    for (segment, kept) in track.segments.iter().zip(kept) {
        if let (Some(previous), Some(start)) = (previous, segment.points.first())
            && haversine_m(previous, start) > ROUTE_BREAK_M
        {
            route.breaks.push(route.points.len() as u32);
        }
        for i in kept {
            let point = &segment.points[i];
            let (x, y) = grid.footprint.projection.project(point.lat, point.lon);
            let (col, row) = grid.local_to_cell(x, y);
            route.points.push(RoutePoint {
                col: col as f32,
                row: row as f32,
                distance_m: along[first + i] as f32,
                time: point.time.map(|t| t.timestamp()),
                gps_ele_m: point.ele.map(|e| e as f32),
                draped_m: heightmap.sample(col, row),
            });
        }
        first += segment.points.len();
        previous = segment.points.last().or(previous);
    }
    route
}

/// Evenly spaced by distance, from GPS elevation, or from the ground under
/// the route when the track has none
fn profile(track: &Track, along: &[f64], route: &Route, total_m: f64) -> Vec<ProfileSample> {
    let mut series: Vec<(f64, f64)> = track
        .points()
        .zip(along)
        .filter_map(|(p, &d)| Some((d, p.ele?)))
        .collect();
    if series.is_empty() {
        series = route
            .points
            .iter()
            .map(|p| (f64::from(p.distance_m), f64::from(p.draped_m)))
            .collect();
    }
    (0..PROFILE_SAMPLES)
        .map(|i| {
            let distance_m = total_m * i as f64 / (PROFILE_SAMPLES - 1) as f64;
            ProfileSample {
                distance_m: distance_m as f32,
                ele_m: interpolate(&series, distance_m) as f32,
            }
        })
        .collect()
}

/// Linear in a series sorted by distance, held flat past its ends
fn interpolate(series: &[(f64, f64)], at: f64) -> f64 {
    let i = series.partition_point(|&(d, _)| d < at);
    match (i.checked_sub(1).and_then(|j| series.get(j)), series.get(i)) {
        (Some(&(d0, e0)), Some(&(d1, e1))) if d1 > d0 => e0 + (e1 - e0) * (at - d0) / (d1 - d0),
        (_, Some(&(_, e))) | (Some(&(_, e)), None) => e,
        (None, None) => 0.0,
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use chrono::{TimeZone, Utc};
    use isotrail_track::Segment;

    use super::*;
    use crate::{Footprint, LocalProjection, OsmFeatures};

    const ORIGIN: (f64, f64) = (46.5, 10.0);

    fn provenance() -> Provenance {
        Provenance {
            generator: "test".into(),
            elevation: "synthetic".into(),
            map_data: "none".into(),
        }
    }

    /// East-west pieces at y = 0, every 100 m, climbing 1 m per 10 m east
    fn track(pieces: &[(f64, f64)], with_ele: bool) -> Track {
        let projection = LocalProjection::new(ORIGIN.0, ORIGIN.1);
        let mut secs = 0;
        let segments = pieces
            .iter()
            .map(|&(from, to)| {
                let steps = ((to - from) / 100.0).round() as i32;
                let points = (0..=steps)
                    .map(|i| {
                        let x = from + (to - from) * f64::from(i) / f64::from(steps);
                        let (lat, lon) = projection.unproject(x, 0.0);
                        secs += 30;
                        TrackPoint {
                            lat,
                            lon,
                            ele: with_ele.then_some(1000.0 + x / 10.0),
                            time: Some(Utc.timestamp_opt(1_750_000_000 + secs, 0).unwrap()),
                        }
                    })
                    .collect();
                Segment { points }
            })
            .collect();
        Track {
            name: Some("Synthetic Traverse".into()),
            desc: None,
            segments,
        }
    }

    /// The grid, and ground rising 1 m per 20 m east
    fn terrain(track: &Track) -> (Grid, Heightmap, LandcoverMap) {
        let grid = Grid::new(
            &Footprint::from_track(track).unwrap(),
            NonZeroU32::new(52).unwrap(),
        );
        let heights_m = (0..grid.rows)
            .flat_map(|row| (0..grid.cols).map(move |col| (col, row)))
            .map(|(col, row)| {
                let (lat, lon) = grid.cell_center_lat_lon(col, row);
                500.0 + (test_x(lat, lon) / 20.0) as f32
            })
            .collect();
        let mut heightmap = Heightmap {
            cols: grid.cols,
            rows: grid.rows,
            heights_m,
        };
        let features = OsmFeatures {
            areas: vec![],
            lines: vec![],
        };
        let landcover = LandcoverMap::classify(&grid, &features, &mut heightmap);
        (grid, heightmap, landcover)
    }

    fn bundle(track: &Track) -> SceneBundle {
        let (grid, heightmap, landcover) = terrain(track);
        scene_bundle(
            track,
            "Traverse",
            &grid,
            &heightmap,
            &landcover,
            provenance(),
        )
        .unwrap()
    }

    fn local_x(bundle: &SceneBundle, point: &RoutePoint) -> f64 {
        let grid = &bundle.grid;
        let u =
            f64::from(point.col) * grid.cell_size_m - f64::from(grid.cols) * grid.cell_size_m / 2.0;
        let v =
            f64::from(grid.rows) * grid.cell_size_m / 2.0 - f64::from(point.row) * grid.cell_size_m;
        let (sin, cos) = grid.rotation_rad.sin_cos();
        let (x, y) = (
            grid.center_m.0 + u * cos - v * sin,
            grid.center_m.1 + u * sin + v * cos,
        );
        let (lat, lon) = LocalProjection::new(grid.origin_lat, grid.origin_lon).unproject(x, y);
        test_x(lat, lon)
    }

    /// East of the test origin, in the coordinates [`track`] uses
    fn test_x(lat: f64, lon: f64) -> f64 {
        LocalProjection::new(ORIGIN.0, ORIGIN.1).project(lat, lon).0
    }

    #[test]
    fn route_breaks_only_after_long_gaps() {
        let track = track(&[(0.0, 3000.0), (3030.0, 5000.0), (5500.0, 8000.0)], true);
        let bundle = bundle(&track);
        let route = &bundle.route;
        assert_eq!(route.points.len(), 6, "straight pieces keep their ends");
        assert_eq!(route.breaks, [4]);
        let stats = track.stats().unwrap();
        assert_eq!(bundle.meta.stats.distance_m, stats.distance_m);
        assert_eq!(route.points[5].distance_m, stats.distance_m as f32);
        assert_eq!(route.points[0].distance_m, 0.0);
        assert!(
            route
                .points
                .windows(2)
                .all(|w| w[0].distance_m < w[1].distance_m)
        );
        for (point, x) in route
            .points
            .iter()
            .zip([0.0, 3000.0, 3030.0, 5000.0, 5500.0, 8000.0])
        {
            assert!((local_x(&bundle, point) - x).abs() < 0.01, "{point:?}");
            assert_eq!(point.gps_ele_m, Some((1000.0 + x / 10.0) as f32));
        }
        assert_eq!(route.points[1].time, Some(1_750_000_000 + 31 * 30));
    }

    #[test]
    fn points_are_draped_on_the_ground() {
        let track = track(&[(0.0, 8000.0)], true);
        let (grid, heightmap, landcover) = terrain(&track);
        let bundle =
            scene_bundle(&track, "T", &grid, &heightmap, &landcover, provenance()).unwrap();
        for point in &bundle.route.points {
            let x = local_x(&bundle, point);
            assert_eq!(
                point.draped_m,
                heightmap.sample(f64::from(point.col), f64::from(point.row))
            );
            assert!(
                (f64::from(point.draped_m) - (500.0 + x / 20.0)).abs() < 0.01,
                "{point:?}"
            );
        }
        for (i, &h) in heightmap.heights_m.iter().enumerate() {
            assert!((bundle.terrain.height_m(i) - h).abs() <= bundle.terrain.height_scale_m);
        }
        assert_eq!(bundle.terrain.landcover.len(), bundle.grid.cells());
        let (min, max) = heightmap.range();
        assert_eq!(
            bundle.grid.exaggeration,
            vertical_exaggeration(grid.footprint.length_m, f64::from(max - min))
        );
        assert!((grid.footprint.length_m - 10_400.0).abs() < 1e-6);
    }

    #[test]
    fn profile_follows_gps_elevation_or_falls_back_to_the_ground() {
        let pieces = [(0.0, 4000.0), (4000.0, 8000.0)];
        for (with_ele, start, end) in [(true, 1000.0, 1800.0), (false, 500.0, 900.0)] {
            let bundle = bundle(&track(&pieces, with_ele));
            let profile = &bundle.profile;
            assert_eq!(profile.len(), PROFILE_SAMPLES);
            assert_eq!(profile[0].distance_m, 0.0);
            assert_eq!(
                profile[PROFILE_SAMPLES - 1].distance_m,
                bundle.meta.stats.distance_m as f32
            );
            let ele = |i: usize| f64::from(profile[i].ele_m);
            assert!((ele(0) - start).abs() < 0.5, "{}", ele(0));
            assert!((ele(PROFILE_SAMPLES - 1) - end).abs() < 0.5);
            assert!((ele(PROFILE_SAMPLES / 2) - (start + end) / 2.0).abs() < 5.0);
            assert!(profile.windows(2).all(|w| w[0].ele_m <= w[1].ele_m));
        }
    }

    #[test]
    fn bundles_are_deterministic_and_carry_the_metadata() {
        let track = track(&[(0.0, 2000.0), (2600.0, 8000.0)], true);
        let bundle = bundle(&track);
        assert_eq!(bundle.to_bytes(), self::bundle(&track).to_bytes());
        assert_eq!(bundle.prop_seed, bundle.content_seed());
        assert_eq!(SceneBundle::from_bytes(&bundle.to_bytes()).unwrap(), bundle);

        let meta = &bundle.meta;
        assert_eq!(meta.title, "Traverse");
        assert_eq!(meta.name.as_deref(), Some("Synthetic Traverse"));
        let stats = track.stats().unwrap();
        assert_eq!(meta.timezone.as_deref(), stats.timezone.map(|tz| tz.name()));
        assert_eq!(meta.local_date.as_deref(), Some("2025-06-15"));
        assert_eq!(meta.stats.points, 76);
        assert_eq!(meta.stats.segments, 2);
        assert_eq!(meta.stats.start, Some(1_750_000_030));
        assert_eq!(meta.stats.elapsed_s, Some(75 * 30));
        assert_eq!(meta.stats.gain_m, stats.gain_m);
        assert_eq!(meta.attribution, [OSM_ATTRIBUTION, DEM_ATTRIBUTION]);
    }

    #[test]
    fn interpolation_is_linear_and_flat_past_the_ends() {
        let series = [(0.0, 10.0), (10.0, 20.0), (10.0, 30.0), (20.0, 10.0)];
        assert_eq!(interpolate(&series, -5.0), 10.0);
        assert_eq!(interpolate(&series, 5.0), 15.0);
        assert_eq!(interpolate(&series, 10.0), 20.0);
        assert_eq!(interpolate(&series, 15.0), 20.0);
        assert_eq!(interpolate(&series, 25.0), 10.0);
        assert_eq!(interpolate(&[], 1.0), 0.0);
    }
}
