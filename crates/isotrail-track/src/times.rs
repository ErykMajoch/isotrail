use chrono::{DateTime, Offset};

use crate::{Track, haversine_m, timezone_at};

const HALF_HOUR_S: i64 = 1800;
const QUARTER_HOUR_S: i64 = 900;

impl Track {
    /// The same points timed at an even pace by distance: from the start
    /// rounded to the nearest half hour in local time, for the length
    /// rounded to the nearest quarter hour (at least one). A track without
    /// times stays as it is
    pub fn coarse_times(&self) -> Track {
        let times = || self.points().filter_map(|point| point.time);
        let (Some(start), Some(end), Some(first)) =
            (times().min(), times().max(), self.points().next())
        else {
            return self.clone();
        };
        let offset = timezone_at(first.lat, first.lon).map_or(0, |tz| {
            i64::from(start.with_timezone(&tz).offset().fix().local_minus_utc())
        });
        let start_s = nearest(start.timestamp() + offset, HALF_HOUR_S) - offset;
        let length_s = nearest((end - start).num_seconds(), QUARTER_HOUR_S).max(QUARTER_HOUR_S);

        let mut along = vec![0.0];
        let points: Vec<_> = self.points().collect();
        for pair in points.windows(2) {
            along.push(along[along.len() - 1] + haversine_m(pair[0], pair[1]));
        }
        let total = along[along.len() - 1];
        let last = (points.len() - 1).max(1) as f64;

        let mut track = self.clone();
        let points = track.segments.iter_mut().flat_map(|s| &mut s.points);
        for (i, (point, done)) in points.zip(along).enumerate() {
            let share = if total > 0.0 {
                done / total
            } else {
                i as f64 / last
            };
            let at = start_s + (length_s as f64 * share).round() as i64;
            point.time = DateTime::from_timestamp(at, 0);
        }
        track
    }
}

fn nearest(value: i64, step: i64) -> i64 {
    (value + step / 2).div_euclid(step) * step
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Timelike, Utc};

    use super::*;
    use crate::{Segment, TrackPoint};

    /// Points every 0.001° north from `lat`, `lon`, `step_s` apart from `start`
    fn track(lat: f64, lon: f64, start: i64, steps_s: &[i64]) -> Track {
        let mut at = start;
        let points = std::iter::once(0)
            .chain(steps_s.iter().copied())
            .enumerate()
            .map(|(i, step)| {
                at += step;
                TrackPoint {
                    lat: lat + i as f64 * 0.001,
                    lon,
                    ele: Some(1000.0),
                    time: Utc.timestamp_opt(at, 0).single(),
                }
            })
            .collect();
        Track {
            segments: vec![Segment { points }],
            ..Track::default()
        }
    }

    fn times(track: &Track) -> Vec<i64> {
        track
            .points()
            .map(|point| point.time.unwrap().timestamp())
            .collect()
    }

    #[test]
    fn the_start_and_length_are_rounded_and_times_follow_distance() {
        // 2025-06-15 10:14:07 UTC, in a zone a whole number of hours away
        let start = Utc.with_ymd_and_hms(2025, 6, 15, 10, 14, 7).unwrap();
        let recorded = track(46.5, 10.0, start.timestamp(), &[600, 30, 4000, 5000]);
        let coarse = recorded.coarse_times();
        let times = times(&coarse);
        let first = Utc.timestamp_opt(times[0], 0).unwrap();
        assert_eq!((first.hour(), first.minute(), first.second()), (10, 0, 0));
        // 9630s recorded rounds to 9900s, 2h45m
        assert_eq!(times[4] - times[0], 9900);
        assert_eq!(times[1] - times[0], 2475);
        assert!(times.windows(2).all(|w| w[0] < w[1]));
        assert!(times.iter().all(|t| !self::times(&recorded).contains(t)));
        assert_eq!(
            coarse.points().map(|p| (p.lat, p.lon)).collect::<Vec<_>>(),
            recorded
                .points()
                .map(|p| (p.lat, p.lon))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn every_point_gets_a_time_and_short_hikes_last_a_quarter_hour() {
        let mut recorded = track(46.5, 10.0, 1_750_000_000, &[60, 60, 60]);
        recorded.segments[0].points[1].time = None;
        let coarse = recorded.coarse_times();
        let times = times(&coarse);
        assert_eq!(times.len(), 4);
        assert_eq!(times[3] - times[0], 900);
    }

    #[test]
    fn the_start_is_rounded_in_local_time() {
        // Kathmandu is 5h45m ahead of UTC: 04:02 UTC is 09:47 local
        let start = Utc.with_ymd_and_hms(2025, 4, 2, 4, 2, 0).unwrap();
        let coarse = track(27.7, 85.3, start.timestamp(), &[3600]).coarse_times();
        let first = Utc.timestamp_opt(times(&coarse)[0], 0).unwrap();
        assert_eq!((first.hour(), first.minute()), (4, 15));
    }

    #[test]
    fn a_track_without_times_stays_as_it_is() {
        let mut untimed = track(46.5, 10.0, 0, &[60]);
        for point in untimed.segments.iter_mut().flat_map(|s| &mut s.points) {
            point.time = None;
        }
        assert_eq!(untimed.coarse_times(), untimed);
    }
}
