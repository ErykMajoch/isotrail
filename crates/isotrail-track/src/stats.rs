use chrono::{DateTime, TimeDelta, Utc};
use chrono_tz::Tz;
use tracing::warn;

use crate::geodesy::haversine_m;
use crate::{Segment, Track, TrackError, TrackPoint, timezone_at};

/// Width of the along-track moving average applied to GPS elevation before counting gain
pub const SMOOTHING_WINDOW_M: f64 = 100.0;
/// Elevation change needed before a climb or descent counts
pub const GAIN_THRESHOLD_M: f64 = 3.0;
/// Longer gaps between consecutive points count as stopped time
pub const PAUSE_THRESHOLD: TimeDelta = TimeDelta::seconds(60);

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bounds {
    pub min_lat: f64,
    pub min_lon: f64,
    pub max_lat: f64,
    pub max_lon: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TrackStats {
    pub points: usize,
    pub segments: usize,
    /// Includes the straight line between consecutive segments
    pub distance_m: f64,
    pub gain_m: Option<f64>,
    pub loss_m: Option<f64>,
    pub min_ele_m: Option<f64>,
    pub max_ele_m: Option<f64>,
    pub bounds: Bounds,
    pub start: Option<DateTime<Utc>>,
    pub end: Option<DateTime<Utc>>,
    pub elapsed: Option<TimeDelta>,
    /// Time inside segments, without gaps over [`PAUSE_THRESHOLD`]
    pub moving: Option<TimeDelta>,
    /// Zone at the first point, for display only
    pub timezone: Option<Tz>,
}

impl TrackStats {
    pub fn start_local(&self) -> Option<DateTime<Tz>> {
        Some(self.start?.with_timezone(&self.timezone?))
    }

    pub fn end_local(&self) -> Option<DateTime<Tz>> {
        Some(self.end?.with_timezone(&self.timezone?))
    }
}

impl Track {
    pub fn stats(&self) -> Result<TrackStats, TrackError> {
        let points: Vec<&TrackPoint> = self.points().collect();
        let first = **points.first().ok_or(TrackError::EmptyTrack)?;

        let distance_m = points.windows(2).map(|w| haversine_m(w[0], w[1])).sum();

        let elevations = || points.iter().filter_map(|p| p.ele);
        let min_ele_m = elevations().reduce(f64::min);
        let max_ele_m = elevations().reduce(f64::max);
        let smoothed: Vec<f64> = self
            .segments
            .iter()
            .flat_map(|segment| smoothed_elevation(segment, SMOOTHING_WINDOW_M))
            .collect();
        let (gain_m, loss_m) = climb(&smoothed, GAIN_THRESHOLD_M).unzip();
        if gain_m.is_none() {
            warn!("the track has no elevation data");
        }

        let start = points.iter().find_map(|p| p.time);
        let end = points.iter().rev().find_map(|p| p.time);
        let moving = self
            .segments
            .iter()
            .flat_map(|segment| segment.points.windows(2))
            .filter_map(|w| Some(w[1].time? - w[0].time?))
            .filter(|dt| *dt > TimeDelta::zero() && *dt <= PAUSE_THRESHOLD)
            .reduce(|a, b| a + b);
        if start.is_none() {
            warn!("the track has no timestamps, so time-based features are off");
        }

        let bounds = points.iter().fold(
            Bounds {
                min_lat: first.lat,
                min_lon: first.lon,
                max_lat: first.lat,
                max_lon: first.lon,
            },
            |b, p| Bounds {
                min_lat: b.min_lat.min(p.lat),
                min_lon: b.min_lon.min(p.lon),
                max_lat: b.max_lat.max(p.lat),
                max_lon: b.max_lon.max(p.lon),
            },
        );

        Ok(TrackStats {
            points: points.len(),
            segments: self.segments.len(),
            distance_m,
            gain_m,
            loss_m,
            min_ele_m,
            max_ele_m,
            bounds,
            start,
            end,
            elapsed: start.zip(end).map(|(start, end)| end - start),
            moving,
            timezone: timezone_at(first.lat, first.lon),
        })
    }
}

/// Centred moving average over a window measured along the track, so the
/// smoothing does not depend on how often the device logged a point
fn smoothed_elevation(segment: &Segment, window_m: f64) -> Vec<f64> {
    let mut along = 0.0;
    let mut samples = Vec::with_capacity(segment.points.len());
    for (i, point) in segment.points.iter().enumerate() {
        if i > 0 {
            along += haversine_m(&segment.points[i - 1], point);
        }
        if let Some(ele) = point.ele {
            samples.push((along, ele));
        }
    }

    let mut prefix = Vec::with_capacity(samples.len() + 1);
    prefix.push(0.0);
    for (_, ele) in &samples {
        prefix.push(prefix[prefix.len() - 1] + ele);
    }

    let half = window_m / 2.0;
    let (mut lo, mut hi) = (0, 0);
    samples
        .iter()
        .map(|&(at, _)| {
            while at - samples[lo].0 > half {
                lo += 1;
            }
            while hi < samples.len() && samples[hi].0 - at <= half {
                hi += 1;
            }
            (prefix[hi] - prefix[lo]) / (hi - lo) as f64
        })
        .collect()
}

fn climb(series: &[f64], threshold: f64) -> Option<(f64, f64)> {
    let (&first, rest) = series.split_first()?;
    let (mut gain, mut loss, mut anchor) = (0.0, 0.0, first);
    for &ele in rest {
        if ele - anchor >= threshold {
            gain += ele - anchor;
            anchor = ele;
        } else if anchor - ele >= threshold {
            loss += anchor - ele;
            anchor = ele;
        }
    }
    Some((gain, loss))
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    const STEP_DEG: f64 = 0.001;
    const STEP_M: f64 = 111.195_08;

    fn point(i: usize, ele: Option<f64>, secs: Option<i64>) -> TrackPoint {
        TrackPoint {
            lat: 46.5 + i as f64 * STEP_DEG,
            lon: 10.0,
            ele,
            time: secs.map(|s| Utc.timestamp_opt(1_750_000_000 + s, 0).unwrap()),
        }
    }

    fn track(segments: Vec<Vec<TrackPoint>>) -> Track {
        Track {
            segments: segments
                .into_iter()
                .map(|points| Segment { points })
                .collect(),
            ..Track::default()
        }
    }

    /// Points far enough apart that the smoothing window holds only one
    fn sparse(eles: &[f64]) -> Vec<TrackPoint> {
        eles.iter()
            .enumerate()
            .map(|(i, &e)| point(i, Some(e), None))
            .collect()
    }

    #[test]
    fn distance_sums_haversine_including_segment_joins() {
        let t = track(vec![
            vec![point(0, None, None), point(1, None, None)],
            vec![point(2, None, None), point(3, None, None)],
        ]);
        let stats = t.stats().unwrap();
        assert!((stats.distance_m - 3.0 * STEP_M).abs() < 1e-2);
        assert_eq!((stats.points, stats.segments), (4, 2));
    }

    #[test]
    fn hysteresis_counts_swings_at_or_above_the_threshold() {
        let stats = track(vec![sparse(&[0.0, 10.0, 20.0, 15.0, 25.0])])
            .stats()
            .unwrap();
        assert_eq!((stats.gain_m, stats.loss_m), (Some(30.0), Some(5.0)));
        assert_eq!((stats.min_ele_m, stats.max_ele_m), (Some(0.0), Some(25.0)));
    }

    #[test]
    fn swings_below_the_threshold_are_ignored() {
        let stats = track(vec![sparse(&[100.0, 102.0, 100.0, 102.5, 100.5])])
            .stats()
            .unwrap();
        assert_eq!((stats.gain_m, stats.loss_m), (Some(0.0), Some(0.0)));
    }

    #[test]
    fn hysteresis_carries_across_segments() {
        let stats = track(vec![sparse(&[0.0, 2.0]), sparse(&[2.0, 4.0])])
            .stats()
            .unwrap();
        assert_eq!(stats.gain_m, Some(4.0));
    }

    #[test]
    fn smoothing_suppresses_alternating_gps_noise() {
        let step = 0.0001;
        let noisy: Vec<_> = (0..200)
            .map(|i| TrackPoint {
                lat: 46.5 + i as f64 * step,
                lon: 10.0,
                ele: Some(if i % 2 == 0 { 1005.0 } else { 995.0 }),
                time: None,
            })
            .collect();
        let raw_gain = climb(
            &noisy.iter().filter_map(|p| p.ele).collect::<Vec<_>>(),
            GAIN_THRESHOLD_M,
        );
        assert_eq!(raw_gain, Some((990.0, 1000.0)));
        let stats = track(vec![noisy]).stats().unwrap();
        assert_eq!((stats.gain_m, stats.loss_m), (Some(0.0), Some(0.0)));
    }

    #[test]
    fn moving_time_skips_long_gaps_and_segment_joins() {
        let t = track(vec![
            vec![
                point(0, None, Some(0)),
                point(1, None, Some(3)),
                point(2, None, Some(6)),
                point(3, None, Some(126)),
                point(4, None, Some(129)),
            ],
            vec![point(5, None, Some(1000)), point(6, None, Some(1003))],
        ]);
        let stats = t.stats().unwrap();
        assert_eq!(stats.moving, Some(TimeDelta::seconds(12)));
        assert_eq!(stats.elapsed, Some(TimeDelta::seconds(1003)));
        assert_eq!(
            stats.start,
            Some(Utc.timestamp_opt(1_750_000_000, 0).unwrap())
        );
        assert_eq!(
            stats.end,
            Some(Utc.timestamp_opt(1_750_001_003, 0).unwrap())
        );
    }

    #[test]
    fn a_gap_of_exactly_the_threshold_still_counts_as_moving() {
        let stats = track(vec![vec![
            point(0, None, Some(0)),
            point(1, None, Some(60)),
        ]])
        .stats()
        .unwrap();
        assert_eq!(stats.moving, Some(PAUSE_THRESHOLD));
    }

    #[test]
    fn missing_elevation_and_time_give_none() {
        let stats = track(vec![vec![point(0, None, None), point(1, None, None)]])
            .stats()
            .unwrap();
        assert_eq!((stats.gain_m, stats.loss_m), (None, None));
        assert_eq!((stats.min_ele_m, stats.max_ele_m), (None, None));
        assert_eq!((stats.start, stats.end), (None, None));
        assert_eq!((stats.elapsed, stats.moving), (None, None));
        assert_eq!(stats.start_local(), None);
    }

    #[test]
    fn bounds_and_local_time() {
        let mut points = vec![point(0, Some(1.0), Some(0)), point(1, Some(2.0), Some(3))];
        points[1].lon = 9.99;
        let stats = track(vec![points]).stats().unwrap();
        assert_eq!(
            stats.bounds,
            Bounds {
                min_lat: 46.5,
                min_lon: 9.99,
                max_lat: 46.501,
                max_lon: 10.0,
            }
        );
        let tz = stats.timezone.expect("a zone over land");
        assert_eq!(
            stats.start_local(),
            Some(stats.start.unwrap().with_timezone(&tz))
        );
        assert_eq!(
            stats.end_local().unwrap().naive_utc(),
            stats.end.unwrap().naive_utc()
        );
    }

    #[test]
    fn empty_track_is_an_error() {
        assert!(matches!(
            Track::default().stats(),
            Err(TrackError::EmptyTrack)
        ));
    }
}
