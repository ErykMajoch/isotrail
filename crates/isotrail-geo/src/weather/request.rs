use isotrail_track::{Track, TrackPoint, haversine_m};

/// One weather location per this much of the route's extent
pub const LOCATION_SPACING_M: f64 = 20_000.0;
pub const MAX_LOCATIONS: usize = 8;
const HOUR_S: i64 = 3600;

/// The hours of a hike and where the hiker was in each
#[derive(Debug, Clone, PartialEq)]
pub struct WeatherRequest {
    /// In order along the route
    pub locations: Vec<WeatherLocation>,
    /// Unix seconds on the hour, from the hour before the start to the hour
    /// after the end
    pub hours: Vec<i64>,
    /// Index into `locations` for each hour
    pub hour_locations: Vec<usize>,
    /// Unix seconds when the weather is looked up
    pub now: i64,
}

/// The middle of a stretch of the route, rounded to 0.01°
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WeatherLocation {
    pub lat: f64,
    pub lon: f64,
    /// Mean track elevation over the stretch, rounded to 10 m, so the
    /// temperature is for the height walked rather than the model's terrain
    pub ele_m: Option<f64>,
}

impl WeatherRequest {
    /// The route is split into equal stretches, one per location, and each
    /// hour reads the weather in the middle of the stretch the hiker was on;
    /// none when the track has no times
    pub fn from_track(track: &Track, now: i64) -> Option<WeatherRequest> {
        let points: Vec<&TrackPoint> = track.points().collect();
        let along: Vec<f64> = points
            .iter()
            .scan((0.0, None), |(total, last), &p| {
                if let Some(prev) = last.replace(p) {
                    *total += haversine_m(prev, p);
                }
                Some(*total)
            })
            .collect();
        let timed: Vec<(i64, f64)> = points
            .iter()
            .zip(&along)
            .filter_map(|(p, &d)| Some((p.time?.timestamp(), d)))
            .collect();
        let start = timed.iter().map(|&(t, _)| t).min()?;
        let end = timed.iter().map(|&(t, _)| t).max()?;
        let first = start.div_euclid(HOUR_S) * HOUR_S - HOUR_S;
        let last = (end + HOUR_S - 1).div_euclid(HOUR_S) * HOUR_S + HOUR_S;
        let hours: Vec<i64> = (first..=last).step_by(HOUR_S as usize).collect();

        let total = along.last().copied().unwrap_or_default();
        let count = locations_for(&points);
        let stretch = |d: f64| {
            if total > 0.0 {
                ((d / total * count as f64) as usize).min(count - 1)
            } else {
                0
            }
        };
        let locations = (0..count)
            .map(|k| {
                let (lat, lon) = point_at(&points, &along, total * (k as f64 + 0.5) / count as f64);
                let elevations: Vec<f64> = points
                    .iter()
                    .zip(&along)
                    .filter(|&(_, &d)| stretch(d) == k)
                    .filter_map(|(p, _)| p.ele)
                    .collect();
                let ele_m = (!elevations.is_empty()).then(|| {
                    let mean = elevations.iter().sum::<f64>() / elevations.len() as f64;
                    (mean / 10.0).round() * 10.0
                });
                WeatherLocation {
                    lat: round(lat),
                    lon: round(lon),
                    ele_m,
                }
            })
            .collect();
        let hour_locations = hours
            .iter()
            .map(|&hour| stretch(distance_at(&timed, hour)))
            .collect();
        Some(WeatherRequest {
            locations,
            hours,
            hour_locations,
            now,
        })
    }

    pub fn last_hour(&self) -> i64 {
        self.hours.last().copied().unwrap_or_default()
    }
}

fn locations_for(points: &[&TrackPoint]) -> usize {
    let corner = |pick: fn(f64, f64) -> f64| {
        let lat = points
            .iter()
            .map(|p| p.lat)
            .reduce(pick)
            .unwrap_or_default();
        let lon = points
            .iter()
            .map(|p| p.lon)
            .reduce(pick)
            .unwrap_or_default();
        TrackPoint {
            lat,
            lon,
            ele: None,
            time: None,
        }
    };
    let diagonal = haversine_m(&corner(f64::min), &corner(f64::max));
    ((diagonal / LOCATION_SPACING_M).ceil() as usize).clamp(1, MAX_LOCATIONS)
}

fn point_at(points: &[&TrackPoint], along: &[f64], distance: f64) -> (f64, f64) {
    let i = along.partition_point(|&d| d < distance);
    if i == 0 || i == points.len() {
        let p = points[i.min(points.len() - 1)];
        return (p.lat, p.lon);
    }
    let (a, b) = (points[i - 1], points[i]);
    let t = (distance - along[i - 1]) / (along[i] - along[i - 1]);
    (a.lat + (b.lat - a.lat) * t, a.lon + (b.lon - a.lon) * t)
}

fn distance_at(timed: &[(i64, f64)], time: i64) -> f64 {
    let i = timed.partition_point(|&(t, _)| t <= time);
    match i {
        0 => timed[0].1,
        n if n == timed.len() => timed[n - 1].1,
        n => {
            let ((t0, d0), (t1, d1)) = (timed[n - 1], timed[n]);
            d0 + (d1 - d0) * (time - t0) as f64 / (t1 - t0) as f64
        }
    }
}

fn round(degrees: f64) -> f64 {
    (degrees * 100.0).round() / 100.0
}

#[cfg(test)]
mod tests {
    use chrono::DateTime;
    use isotrail_track::Segment;

    use super::*;

    const NOON: i64 = 1_750_075_200;

    /// Northward along a meridian from 50°N, a point every `step_s`
    fn walk(km: f64, points: usize, step_s: i64) -> Track {
        let degrees = km * 1000.0 / 111_195.0;
        let at = |i: usize| TrackPoint {
            lat: 50.0 + degrees * i as f64 / (points - 1) as f64,
            lon: 10.0,
            ele: Some(1000.0 + i as f64),
            time: DateTime::from_timestamp(NOON + 1234 + i as i64 * step_s, 0),
        };
        Track {
            segments: vec![Segment {
                points: (0..points).map(at).collect(),
            }],
            ..Track::default()
        }
    }

    #[test]
    fn hours_reach_one_past_each_end() {
        let request = WeatherRequest::from_track(&walk(5.0, 11, 600), 0).unwrap();
        assert_eq!(request.hours.first(), Some(&(NOON - 3600)));
        assert_eq!(request.last_hour(), NOON + 4 * 3600);
        assert_eq!(request.hours.len(), 6);
        assert!(request.hours.windows(2).all(|w| w[1] - w[0] == 3600));
    }

    #[test]
    fn a_day_hike_reads_one_place_in_the_middle() {
        let request = WeatherRequest::from_track(&walk(5.0, 11, 600), 0).unwrap();
        let expected = WeatherLocation {
            lat: 50.02,
            lon: 10.0,
            ele_m: Some(1010.0),
        };
        assert_eq!(request.locations, vec![expected]);
        assert!(request.hour_locations.iter().all(|&i| i == 0));
    }

    #[test]
    fn a_long_walk_reads_the_stretch_the_hiker_is_on() {
        let request = WeatherRequest::from_track(&walk(70.0, 71, 3600), 0).unwrap();
        assert_eq!(request.locations.len(), 4);
        let lats: Vec<f64> = request.locations.iter().map(|l| l.lat).collect();
        assert_eq!(lats, vec![50.08, 50.24, 50.39, 50.55]);
        let heights: Vec<Option<f64>> = request.locations.iter().map(|l| l.ele_m).collect();
        assert_eq!(
            heights,
            vec![Some(1010.0), Some(1030.0), Some(1040.0), Some(1060.0)]
        );
        let first = request.hour_locations[..3].to_vec();
        assert_eq!(first, vec![0, 0, 0]);
        let middle = request.hour_locations[request.hours.len() / 2];
        assert!(middle == 1 || middle == 2, "{middle}");
        assert_eq!(request.hour_locations.last(), Some(&3));
        assert!(request.hour_locations.windows(2).all(|w| w[0] <= w[1]));
    }

    #[test]
    fn very_long_routes_are_capped() {
        let request = WeatherRequest::from_track(&walk(400.0, 41, 3600), 0).unwrap();
        assert_eq!(request.locations.len(), MAX_LOCATIONS);
    }

    #[test]
    fn tracks_without_times_have_no_hours() {
        let mut track = walk(5.0, 3, 60);
        for p in &mut track.segments[0].points {
            p.time = None;
            p.ele = None;
        }
        assert_eq!(WeatherRequest::from_track(&track, 0), None);
    }

    #[test]
    fn tracks_without_elevations_leave_it_to_the_model() {
        let mut track = walk(5.0, 3, 60);
        for p in &mut track.segments[0].points {
            p.ele = None;
        }
        let request = WeatherRequest::from_track(&track, 0).unwrap();
        assert_eq!(request.locations[0].ele_m, None);
    }
}
