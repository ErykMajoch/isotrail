use crate::{Track, TrackPoint, haversine_m};

/// 1990-01-01: earlier times come from a device clock that was never set
const EARLIEST_S: i64 = 631_152_000;
const GLITCH_GAP_S: i64 = 30 * 86_400;
/// Times further apart than this can't belong to one hike
const CLOCK_BREAK_S: i64 = 31 * 86_400;
const STRAY_SPEED_M_S: f64 = 100.0;
/// Untimed legs longer than this many median steps are suspect
const STRAY_STEP_FACTOR: f64 = 50.0;

/// Clears times before 1990, and times more than 30 days from both timed
/// neighbours and from the median time; returns how many were cleared
pub fn clear_glitched_times(track: &mut Track) -> usize {
    let mut times: Vec<Option<i64>> = track
        .points()
        .map(|p| p.time.map(|t| t.timestamp()).filter(|&t| t >= EARLIEST_S))
        .collect();
    let timed: Vec<(usize, i64)> = times
        .iter()
        .enumerate()
        .filter_map(|(i, t)| Some((i, (*t)?)))
        .collect();
    if let Some(median) = median_i64(timed.iter().map(|&(_, t)| t).collect()) {
        let far = |t: i64, other: Option<i64>| other.is_none_or(|o| (t - o).abs() > GLITCH_GAP_S);
        for (k, &(i, t)) in timed.iter().enumerate() {
            let previous = k.checked_sub(1).map(|k| timed[k].1);
            let next = timed.get(k + 1).map(|&(_, t)| t);
            if far(t, previous) && far(t, next) && far(t, Some(median)) {
                times[i] = None;
            }
        }
    }
    keep_one_clock(&mut times);
    let mut cleared = 0;
    for (point, time) in points_mut(track).zip(&times) {
        if point.time.is_some() && time.is_none() {
            point.time = None;
            cleared += 1;
        }
    }
    cleared
}

/// Splits the times, in order, wherever two are more than 31 days apart, and
/// keeps only the group with the most of them (the earliest on a tie): a
/// pair of glitched times close to each other still lies weeks away from the
/// rest
fn keep_one_clock(times: &mut [Option<i64>]) {
    let mut sorted: Vec<i64> = times.iter().flatten().copied().collect();
    sorted.sort_unstable();
    let mut groups: Vec<&[i64]> = sorted
        .chunk_by(|a, b| b.saturating_sub(*a) <= CLOCK_BREAK_S)
        .collect();
    if groups.len() < 2 {
        return;
    }
    groups.sort_by_key(|group| std::cmp::Reverse(group.len()));
    let kept = groups[0];
    let (first, last) = (kept[0], kept[kept.len() - 1]);
    for time in times.iter_mut() {
        if time.is_some_and(|t| t < first || t > last) {
            *time = None;
        }
    }
}

/// Drops points far from both neighbours while those neighbours lie close
/// together: legs faster than 100m/s, or for untimed legs longer than 50
/// median steps. A first or last point goes when its one leg is suspect and
/// the next one isn't, and for an untimed leg only when it is also 50 times
/// longer than the rest of the track. Returns how many were dropped
pub fn drop_stray_points(track: &mut Track) -> usize {
    let points: Vec<TrackPoint> = track.points().copied().collect();
    let n = points.len();
    if n < 3 {
        return 0;
    }
    let steps: Vec<f64> = points
        .windows(2)
        .map(|w| haversine_m(&w[0], &w[1]))
        .collect();
    let total: f64 = steps.iter().sum();
    let median_step = median_f64(steps.clone()).max(1.0);
    let timed = |k: usize| points[k].time.is_some() && points[k + 1].time.is_some();
    let suspect: Vec<bool> = (0..n - 1)
        .map(|k| match (points[k].time, points[k + 1].time) {
            (Some(a), Some(b)) => {
                let dt_s = ((b - a).num_milliseconds().abs() as f64 / 1000.0).max(1.0);
                steps[k] / dt_s > STRAY_SPEED_M_S
            }
            _ => steps[k] > STRAY_STEP_FACTOR * median_step,
        })
        .collect();
    let end_stray = |leg: usize, next_leg: usize| {
        suspect[leg]
            && !suspect[next_leg]
            && (timed(leg) || steps[leg] > STRAY_STEP_FACTOR * (total - steps[leg]))
    };
    let stray: Vec<bool> = (0..n)
        .map(|i| match i {
            0 => end_stray(0, 1),
            i if i == n - 1 => end_stray(n - 2, n - 3),
            i => {
                let shortcut = haversine_m(&points[i - 1], &points[i + 1]);
                suspect[i - 1] && suspect[i] && shortcut < steps[i - 1].min(steps[i]) / 2.0
            }
        })
        .collect();
    let mut index = 0;
    for segment in &mut track.segments {
        segment.points.retain(|_| {
            index += 1;
            !stray[index - 1]
        });
    }
    stray.iter().filter(|&&s| s).count()
}

/// Places where a time is earlier than the one before it
pub fn backward_steps(track: &Track) -> usize {
    let times: Vec<_> = track.points().filter_map(|p| p.time).collect();
    times.windows(2).filter(|w| w[1] < w[0]).count()
}

fn points_mut(track: &mut Track) -> impl Iterator<Item = &mut TrackPoint> {
    track.segments.iter_mut().flat_map(|s| &mut s.points)
}

fn median_i64(mut values: Vec<i64>) -> Option<i64> {
    values.sort_unstable();
    let mid = values.len() / 2;
    match values.len() {
        0 => None,
        len if len % 2 == 1 => Some(values[mid]),
        _ => Some(values[mid - 1] + (values[mid] - values[mid - 1]) / 2),
    }
}

fn median_f64(mut values: Vec<f64>) -> f64 {
    values.sort_unstable_by(f64::total_cmp);
    let mid = values.len() / 2;
    match values.len() {
        0 => 0.0,
        len if len % 2 == 1 => values[mid],
        _ => (values[mid - 1] + values[mid]) / 2.0,
    }
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};

    use super::*;
    use crate::Segment;

    const START_S: i64 = 1_750_000_000;
    /// About 11m north per step
    const STEP_DEG: f64 = 0.0001;

    fn walk(n: usize, every_s: Option<i64>) -> Vec<TrackPoint> {
        (0..n)
            .map(|i| TrackPoint {
                lat: 53.0 + i as f64 * STEP_DEG,
                lon: -1.7,
                ele: None,
                time: every_s.map(|s| Utc.timestamp_opt(START_S + i as i64 * s, 0).unwrap()),
            })
            .collect()
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

    fn at(s: i64) -> Option<chrono::DateTime<Utc>> {
        Utc.timestamp_opt(s, 0).single()
    }

    #[test]
    fn times_before_1990_are_cleared() {
        let mut points = walk(5, Some(3));
        points[0].time = at(0);
        let mut t = track(vec![points]);
        assert_eq!(clear_glitched_times(&mut t), 1);
        assert_eq!(t.segments[0].points[0].time, None);
        assert!(t.segments[0].points[1..].iter().all(|p| p.time.is_some()));
    }

    #[test]
    fn a_first_time_years_early_or_one_in_the_future_is_cleared() {
        let mut points = walk(6, Some(3));
        points[0].time = Utc.with_ymd_and_hms(2005, 3, 1, 8, 0, 0).single();
        points[3].time = Utc.with_ymd_and_hms(2040, 1, 1, 0, 0, 0).single();
        let mut t = track(vec![points]);
        assert_eq!(clear_glitched_times(&mut t), 2);
        let cleared: Vec<bool> = t.points().map(|p| p.time.is_none()).collect();
        assert_eq!(cleared, [true, false, false, true, false, false]);
    }

    #[test]
    fn a_pair_of_times_far_in_the_future_is_cleared() {
        let mut points = walk(4, Some(1));
        let far = Utc.with_ymd_and_hms(9999, 1, 1, 0, 0, 0).single().unwrap();
        points[2].time = Some(far);
        points[3].time = Some(far + chrono::TimeDelta::seconds(1));
        let mut t = track(vec![points]);
        assert_eq!(clear_glitched_times(&mut t), 2);
        let cleared: Vec<bool> = t.points().map(|p| p.time.is_none()).collect();
        assert_eq!(cleared, [false, false, true, true]);

        // The larger group is kept, wherever it lies
        let mut points = walk(5, Some(1));
        points[0].time = at(START_S - 400 * 86_400);
        points[1].time = at(START_S - 400 * 86_400 + 1);
        let mut t = track(vec![points]);
        assert_eq!(clear_glitched_times(&mut t), 2);
        assert!(t.points().skip(2).all(|p| p.time.is_some()));
    }

    #[test]
    fn long_but_steady_tracks_keep_their_times() {
        let mut t = track(vec![walk(2, Some(31 * 86_400))]);
        assert_eq!(clear_glitched_times(&mut t), 0);
        let mut t = track(vec![walk(100, Some(86_400))]);
        assert_eq!(clear_glitched_times(&mut t), 0);
    }

    #[test]
    fn a_point_at_zero_zero_is_dropped_timed_or_not() {
        for every_s in [Some(3), None] {
            let mut points = walk(10, every_s);
            (points[4].lat, points[4].lon) = (0.0, 0.0);
            let mut t = track(vec![points]);
            assert_eq!(drop_stray_points(&mut t), 1, "{every_s:?}");
            assert_eq!(t.point_count(), 9);
            assert!(t.points().all(|p| p.lat > 50.0));
        }
    }

    #[test]
    fn a_far_jump_is_dropped_at_either_end_too() {
        for i in [0, 5, 9] {
            for every_s in [Some(60), None] {
                let mut points = walk(10, every_s);
                points[i].lon += 3.0;
                let mut t = track(vec![points]);
                assert_eq!(drop_stray_points(&mut t), 1, "point {i}, {every_s:?}");
                assert!(t.points().all(|p| p.lon < 0.0));
            }
        }
    }

    #[test]
    fn strays_are_found_across_segment_joins() {
        let mut second = walk(10, Some(3));
        second.drain(..5);
        let mut first = walk(5, Some(3));
        first[4].lat = 0.0;
        let mut t = track(vec![first, second]);
        assert_eq!(drop_stray_points(&mut t), 1);
        assert_eq!(t.point_count(), 9);
    }

    #[test]
    fn fast_legs_and_single_long_jumps_are_kept() {
        // A ferry at 15m/s
        let mut t = track(vec![
            walk(10, Some(1))
                .into_iter()
                .map(|mut p| {
                    p.lat = 53.0 + (p.lat - 53.0) * 1.5;
                    p
                })
                .collect(),
        ]);
        assert_eq!(drop_stray_points(&mut t), 0);
        // A drive with the recording off: one leg far and fast, then walking
        let mut points = walk(10, Some(3));
        for p in &mut points[5..] {
            p.lon += 1.0;
        }
        let mut t = track(vec![points.clone()]);
        assert_eq!(drop_stray_points(&mut t), 0);
        // The same untimed, and a planned route with long straights and a corner
        for p in &mut points {
            p.time = None;
        }
        let mut t = track(vec![points]);
        assert_eq!(drop_stray_points(&mut t), 0);
        let mut route = walk(20, None);
        route.push(TrackPoint {
            lat: 53.05,
            ..route[19]
        });
        route.push(TrackPoint {
            lat: 53.05,
            lon: -1.6,
            ..route[19]
        });
        let mut t = track(vec![route]);
        assert_eq!(drop_stray_points(&mut t), 0);
    }

    #[test]
    fn backward_steps_are_counted() {
        let mut later = walk(4, Some(3));
        for p in &mut later {
            p.time = p.time.map(|t| t + chrono::TimeDelta::days(1));
        }
        let t = track(vec![later, walk(4, Some(3))]);
        assert_eq!(backward_steps(&t), 1);
        assert_eq!(backward_steps(&track(vec![walk(4, Some(3))])), 0);
    }

    #[test]
    fn medians() {
        assert_eq!(median_i64(vec![]), None);
        assert_eq!(median_i64(vec![5, 1, 3]), Some(3));
        assert_eq!(median_i64(vec![4, 0]), Some(2));
        assert_eq!(median_f64(vec![2.0, 1.0, 4.0, 3.0]), 2.5);
    }
}
