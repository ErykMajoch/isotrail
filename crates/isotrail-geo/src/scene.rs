use chrono::{DateTime, Utc};
use isotrail_bundle::{
    Bounds, GridInfo, HikeStats, Metadata, MoonSample, ProfileSample, Provenance, Route,
    RoutePoint, SceneBundle, Season, SunSample, Terrain,
};
use isotrail_track::{Track, TrackPoint, TrackStats, haversine_m, moon_position, sun_position};
use tracing::warn;

use crate::{GeoError, Grid, Heightmap, LandcoverMap, leaf_day, vertical_exaggeration};

/// Consecutive segments further apart than this are drawn with a gap
pub const ROUTE_BREAK_M: f64 = 50.0;
/// Simplification tolerance for the drawn route, in cells
pub const ROUTE_TOLERANCE_CELLS: f64 = 0.25;
pub const PROFILE_SAMPLES: usize = 256;
pub const OSM_ATTRIBUTION: &str = "© OpenStreetMap contributors";
pub const DEM_ATTRIBUTION: &str = "Terrain Tiles (AWS)";
pub const SUN_STEP_S: i64 = 300;
/// Longer hikes get no sun, moon or weather
pub const MAX_TIMED_SPAN_DAYS: i64 = 31;

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
        sun: Vec::new(),
        moon: Vec::new(),
        weather: None,
        season: Season {
            leaf_day: stats
                .start_local()
                .and_then(|t| leaf_day(t.date_naive(), origin_lat)),
            ..Season::default()
        },
    };
    (bundle.sun, bundle.moon) = sky_track(stats.start, stats.end, origin_lat, origin_lon);
    bundle.prop_seed = bundle.content_seed();
    Ok(bundle)
}

/// Gives the route, the start and end, and the sun and moon the times of
/// `track`, the bundle's own track timed another way. The seed, the date,
/// and the moving and elapsed times stay as they were
pub fn retime(bundle: &mut SceneBundle, track: &Track) {
    let kept = track.simplified_indices(ROUTE_TOLERANCE_CELLS * bundle.grid.cell_size_m);
    let times = track.segments.iter().zip(kept).flat_map(|(segment, kept)| {
        kept.into_iter()
            .map(|i| segment.points[i].time.map(|t| t.timestamp()))
    });
    for (point, time) in bundle.route.points.iter_mut().zip(times) {
        point.time = time;
    }
    let times = || track.points().filter_map(|point| point.time);
    let (start, end) = (times().min(), times().max());
    let stats = &mut bundle.meta.stats;
    stats.start = start.map(|t| t.timestamp());
    stats.end = end.map(|t| t.timestamp());
    let grid = &bundle.grid;
    (bundle.sun, bundle.moon) = sky_track(start, end, grid.origin_lat, grid.origin_lon);
}

/// Leaves out every time and what follows from them, as for a track
/// without times; the seed stays
pub fn untime(bundle: &mut SceneBundle) {
    for point in &mut bundle.route.points {
        point.time = None;
    }
    let meta = &mut bundle.meta;
    meta.local_date = None;
    meta.provenance.weather = None;
    let stats = &mut meta.stats;
    (stats.start, stats.end, stats.elapsed_s, stats.moving_s) = (None, None, None, None);
    bundle.sun.clear();
    bundle.moon.clear();
    bundle.weather = None;
    bundle.season = Season::default();
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

/// The sun and moon over the slab every [`SUN_STEP_S`] from the start, and
/// at the end; none past [`MAX_TIMED_SPAN_DAYS`]
fn sky_track(
    start: Option<DateTime<Utc>>,
    end: Option<DateTime<Utc>>,
    lat: f64,
    lon: f64,
) -> (Vec<SunSample>, Vec<MoonSample>) {
    let (Some(start), Some(end)) = (start, end) else {
        return Default::default();
    };
    let days = (end - start).num_days();
    if days > MAX_TIMED_SPAN_DAYS {
        warn!(
            "the hike spans {days} days, more than {MAX_TIMED_SPAN_DAYS}, so the sun and moon are left out"
        );
        return Default::default();
    }
    // Samples are stored in whole seconds, so sampling from fractional
    // times could give the last two the same second
    let whole = |t: DateTime<Utc>| DateTime::from_timestamp(t.timestamp(), 0).unwrap_or(t);
    let (start, end) = (whole(start), whole(end));
    let step = chrono::TimeDelta::seconds(SUN_STEP_S);
    std::iter::successors(Some(start), |&t| Some(t + step))
        .take_while(|&t| t < end)
        .chain([end])
        .map(|time| {
            let sun = sun_position(lat, lon, time);
            let moon = moon_position(lat, lon, time);
            (
                SunSample {
                    time: time.timestamp(),
                    azimuth_deg: sun.azimuth_deg as f32,
                    elevation_deg: sun.elevation_deg as f32,
                },
                MoonSample {
                    time: time.timestamp(),
                    azimuth_deg: moon.azimuth_deg as f32,
                    elevation_deg: moon.elevation_deg as f32,
                    illuminated: moon.illuminated as f32,
                },
            )
        })
        .unzip()
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
    let mut series = gps_elevations(track, along);
    if series.is_empty() {
        series = route
            .points
            .iter()
            .map(|p| (f64::from(p.distance_m), f64::from(p.draped_m)))
            .collect();
    }
    samples(&series, total_m)
}

/// The profile a bundle of `track` would hold, without the ground to fall
/// back on: `None` when the track has no elevations
pub fn elevation_profile(track: &Track, total_m: f64) -> Option<Vec<ProfileSample>> {
    let series = gps_elevations(track, &distances(track));
    (!series.is_empty()).then(|| samples(&series, total_m))
}

fn gps_elevations(track: &Track, along: &[f64]) -> Vec<(f64, f64)> {
    track
        .points()
        .zip(along)
        .filter_map(|(p, &d)| Some((d, p.ele?)))
        .collect()
}

fn samples(series: &[(f64, f64)], total_m: f64) -> Vec<ProfileSample> {
    (0..PROFILE_SAMPLES)
        .map(|i| {
            let distance_m = total_m * i as f64 / (PROFILE_SAMPLES - 1) as f64;
            ProfileSample {
                distance_m: distance_m as f32,
                ele_m: interpolate(series, distance_m) as f32,
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
            weather: None,
        }
    }

    /// East-west pieces at y = 0, every 100m, climbing 1m per 10m east
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

    /// The grid, and ground rising 1m per 20m east
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

            let track = track(&pieces, with_ele);
            let alone = elevation_profile(&track, bundle.meta.stats.distance_m);
            assert_eq!(alone.as_ref(), with_ele.then_some(profile));
        }
    }

    #[test]
    fn bundles_are_deterministic_and_carry_the_metadata() {
        let track = track(&[(0.0, 2000.0), (2600.0, 8000.0)], true);
        let bundle = bundle(&track);
        assert_eq!(
            bundle.to_bytes().unwrap(),
            self::bundle(&track).to_bytes().unwrap()
        );
        assert_eq!(bundle.prop_seed, bundle.content_seed());
        assert_eq!(
            SceneBundle::from_bytes(&bundle.to_bytes().unwrap()).unwrap(),
            bundle
        );

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
    fn the_sun_and_moon_are_sampled_from_start_to_end() {
        let track = track(&[(0.0, 2000.0), (2600.0, 8000.0)], true);
        let sky = bundle(&track);
        let sun = sky.sun;
        let times: Vec<i64> = sun.iter().map(|s| s.time - 1_750_000_030).collect();
        assert_eq!(times, [0, 300, 600, 900, 1200, 1500, 1800, 2100, 2250]);
        let start = sun_position(ORIGIN.0, ORIGIN.1, track.stats().unwrap().start.unwrap());
        // The slab's origin is within a few kilometres of the track's
        assert!((f64::from(sun[0].elevation_deg) - start.elevation_deg).abs() < 0.1);
        assert!((f64::from(sun[0].azimuth_deg) - start.azimuth_deg).abs() < 0.1);
        let moon = sky.moon;
        assert_eq!(moon.len(), sun.len());
        assert!(moon.iter().zip(&sun).all(|(m, s)| m.time == s.time));
        let rise = moon_position(ORIGIN.0, ORIGIN.1, track.stats().unwrap().start.unwrap());
        assert!((f64::from(moon[0].elevation_deg) - rise.elevation_deg).abs() < 0.1);
        assert!((f64::from(moon[0].illuminated) - rise.illuminated).abs() < 1e-3);

        let mut untimed = track.clone();
        for point in untimed.segments.iter_mut().flat_map(|s| &mut s.points) {
            point.time = None;
        }
        let untimed = bundle(&untimed);
        assert!(untimed.sun.is_empty() && untimed.moon.is_empty());
    }

    #[test]
    fn coarse_times_replace_every_recorded_time_and_keep_the_rest() {
        let track = track(&[(0.0, 2000.0), (2600.0, 8000.0)], true);
        let recorded = bundle(&track);
        let coarse = track.coarse_times();
        let mut retimed = recorded.clone();
        retime(&mut retimed, &coarse);

        let recorded_times: Vec<i64> = track
            .points()
            .filter_map(|p| p.time)
            .map(|t| t.timestamp())
            .collect();
        let route = &retimed.route.points;
        assert_eq!(route.len(), recorded.route.points.len());
        assert!(route.iter().all(|p| p.time.is_some()));
        let stored = route
            .iter()
            .filter_map(|p| p.time)
            .chain(retimed.sun.iter().map(|s| s.time))
            .chain(retimed.moon.iter().map(|m| m.time));
        for time in stored {
            assert!(!recorded_times.contains(&time), "{time}");
        }
        let stats = &retimed.meta.stats;
        let (start, end) = (stats.start.unwrap(), stats.end.unwrap());
        assert_eq!(start % 1800, 0);
        assert_eq!((end - start) % 900, 0);
        assert_eq!(retimed.sun.first().map(|s| s.time), Some(start));
        assert_eq!(retimed.sun.last().map(|s| s.time), Some(end));
        assert_eq!(route[0].time, Some(start));
        assert_eq!(route[route.len() - 1].time, Some(end));

        assert_eq!(retimed.prop_seed, recorded.prop_seed);
        assert_ne!(retimed.content_seed(), recorded.content_seed());
        let kept = &recorded.meta.stats;
        assert_eq!(
            (stats.elapsed_s, stats.moving_s),
            (kept.elapsed_s, kept.moving_s)
        );
        assert_eq!(retimed.meta.local_date, recorded.meta.local_date);
        assert_eq!(retimed.meta.timezone, recorded.meta.timezone);
        assert_eq!(retimed.season, recorded.season);
        retimed.validate().unwrap();
    }

    #[test]
    fn untimed_bundles_match_a_track_without_times_but_for_the_seed() {
        let track = track(&[(0.0, 2000.0), (2600.0, 8000.0)], true);
        let mut untimed = bundle(&track);
        let seed = untimed.prop_seed;
        untime(&mut untimed);
        assert_eq!(untimed.prop_seed, seed);

        let mut without = track.clone();
        for point in without.segments.iter_mut().flat_map(|s| &mut s.points) {
            point.time = None;
        }
        let mut expected = bundle(&without);
        assert_ne!(expected.prop_seed, seed);
        expected.prop_seed = seed;
        assert_eq!(untimed, expected);
    }

    #[test]
    fn a_glitched_first_time_leaves_a_normal_sky_track() {
        let mut glitched = track(&[(0.0, 2000.0), (2600.0, 8000.0)], true);
        glitched.segments[0].points[0].time = Utc.timestamp_opt(0, 0).single();
        let sky = bundle(&glitched.clean().unwrap());
        assert_eq!(sky.sun.len(), 9);
        assert_eq!(sky.meta.stats.start, Some(1_750_000_060));
    }

    #[test]
    fn fractional_start_and_end_times_give_distinct_whole_seconds() {
        let mut track = track(&[(0.0, 2000.0), (2600.0, 8000.0)], true);
        let points: Vec<_> = track
            .segments
            .iter_mut()
            .flat_map(|s| &mut s.points)
            .collect();
        let last = points.len() - 1;
        for (i, point) in points.into_iter().enumerate() {
            let millis = if i == last {
                3_600_700
            } else {
                200 + i as i64 * 1000
            };
            point.time = Utc
                .timestamp_millis_opt(1_750_000_000_000 + millis)
                .single();
        }
        let bundle = bundle(&track);
        let times: Vec<i64> = bundle.sun.iter().map(|s| s.time - 1_750_000_000).collect();
        assert!(times.windows(2).all(|w| w[0] < w[1]), "{times:?}");
        assert_eq!(times.first(), Some(&0));
        assert_eq!(times.last(), Some(&3600));
        assert_eq!(times.len(), 13);
        let bytes = bundle.to_bytes().unwrap();
        assert_eq!(SceneBundle::from_bytes(&bytes).unwrap(), bundle);
    }

    #[test]
    fn hikes_longer_than_a_month_get_no_sky_track() {
        let mut long = track(&[(0.0, 2000.0), (2600.0, 8000.0)], true);
        let points = long.segments.iter_mut().flat_map(|s| &mut s.points);
        for (i, point) in points.enumerate() {
            point.time = Utc
                .timestamp_opt(1_750_000_000 + i as i64 * 86_400, 0)
                .single();
        }
        let sky = bundle(&long);
        assert!(sky.meta.stats.elapsed_s.unwrap() > 31 * 86_400);
        assert!(sky.sun.is_empty() && sky.moon.is_empty());
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
