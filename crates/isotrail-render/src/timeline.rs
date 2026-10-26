use std::f32::consts::PI;

use isotrail_bundle::{HikeStats, ProfileSample, Route, SceneBundle};

/// Shares of a clip showing the scene alone and the route drawing itself;
/// the rest holds on the finished hike
pub const INTRO_SHARE: f32 = 0.05;
pub const DRAW_SHARE: f32 = 0.8;
pub const DEFAULT_DURATION_S: f32 = 10.0;
pub const DEFAULT_FPS: u32 = 30;
pub const DEFAULT_ORBIT_DEG: f32 = 30.0;
/// Time pacing keeps at most this much of a stop
const PAUSE_S: f64 = 60.0;
/// Slower than this between two points counts as stopping
const CRAWL_M_PER_S: f64 = 0.25;

/// How long a clip runs and how often it changes
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Timeline {
    pub duration_s: f32,
    pub fps: u32,
    /// Distinct images per second, fewer than `fps` for choppy, game-like
    /// motion
    pub step_fps: Option<u32>,
}

impl Default for Timeline {
    fn default() -> Self {
        Timeline {
            duration_s: DEFAULT_DURATION_S,
            fps: DEFAULT_FPS,
            step_fps: None,
        }
    }
}

impl Timeline {
    pub fn frame_count(&self) -> u32 {
        ((self.duration_s * self.fps as f32).round() as u32).max(1)
    }

    /// Seconds into the clip that frame `index` shows
    pub fn frame_time(&self, index: u32) -> f32 {
        let fps = self.fps.max(1);
        match self.step_fps {
            Some(step) if step > 0 && step < fps => {
                (u64::from(index) * u64::from(step) / u64::from(fps)) as f32 / step as f32
            }
            _ => index as f32 / fps as f32,
        }
    }

    /// Share of the clip gone by `time_s`
    pub fn share(&self, time_s: f32) -> f32 {
        if self.duration_s > 0.0 {
            (time_s / self.duration_s).clamp(0.0, 1.0)
        } else {
            1.0
        }
    }

    /// Share of the route's drawing done by `time_s`: 0 through the intro,
    /// 1 through the hold
    pub fn draw_share(&self, time_s: f32) -> f32 {
        ((self.share(time_s) - INTRO_SHARE) / DRAW_SHARE).clamp(0.0, 1.0)
    }
}

/// An eased turn of the camera that ends on the still's view as the route
/// finishes drawing, and holds it from there
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Orbit {
    pub sweep_deg: f32,
}

impl Default for Orbit {
    fn default() -> Self {
        Orbit {
            sweep_deg: DEFAULT_ORBIT_DEG,
        }
    }
}

impl Orbit {
    /// Degrees the camera is turned from the still's view at clip share `share`
    pub fn yaw_offset_deg(&self, share: f32) -> f32 {
        let turned = ease(share / (INTRO_SHARE + DRAW_SHARE));
        if turned >= 1.0 {
            0.0
        } else {
            -self.sweep_deg * (1.0 - turned)
        }
    }
}

/// Slow at both ends, from 0 to 1
fn ease(x: f32) -> f32 {
    (1.0 - (PI * x.clamp(0.0, 1.0)).cos()) / 2.0
}

/// What sets the route's speed as it draws itself
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Pace {
    /// The same distance every frame
    #[default]
    Distance,
    /// The recorded times, with stops and the gaps between pieces cut to a
    /// minute
    Time,
}

/// Everything that moves in a clip
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Motion {
    pub timeline: Timeline,
    pub orbit: Orbit,
    pub pace: Pace,
}

/// What one frame of a clip shows
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Moment {
    pub time_s: f32,
    /// Added to the still's camera turn
    pub yaw_offset_deg: f32,
    /// Share of the route drawn, by distance
    pub progress: f32,
}

impl Motion {
    pub fn moment(&self, walk: &Walk, index: u32) -> Moment {
        let time_s = self.timeline.frame_time(index);
        Moment {
            time_s,
            yaw_offset_deg: self.orbit.yaw_offset_deg(self.timeline.share(time_s)),
            progress: walk.progress(self.pace, self.timeline.draw_share(time_s)),
        }
    }
}

/// The hike as it unfolds: how far along it is part-way through the
/// drawing, and its stats up to there
#[derive(Clone, Debug)]
pub struct Walk {
    stats: HikeStats,
    start_m: f32,
    end_m: f32,
    clock: Option<Clock>,
    profile: Vec<ProfileSample>,
    /// Over the whole profile
    totals: Climb,
}

/// The recorded times along the route, and the same with stops cut short
#[derive(Clone, Debug)]
struct Clock {
    distance_m: Vec<f32>,
    paced_s: Vec<f64>,
    real_s: Vec<f64>,
}

#[derive(Clone, Copy, Debug, Default)]
struct Climb {
    up: f32,
    down: f32,
    high: f32,
    low: f32,
}

impl Walk {
    pub fn new(bundle: &SceneBundle) -> Walk {
        let points = &bundle.route.points;
        let (start_m, end_m) = match (points.first(), points.last()) {
            (Some(first), Some(last)) if points.len() >= 2 => (first.distance_m, last.distance_m),
            _ => (0.0, bundle.meta.stats.distance_m as f32),
        };
        let profile = if bundle.profile.len() >= 2 {
            bundle.profile.clone()
        } else {
            Vec::new()
        };
        let mut walk = Walk {
            stats: bundle.meta.stats.clone(),
            start_m,
            end_m,
            clock: Clock::new(&bundle.route),
            profile,
            totals: Climb::default(),
        };
        walk.totals = walk.climb_to(f32::INFINITY);
        walk
    }

    /// Whether the route has the times that time pacing needs
    pub fn is_timed(&self) -> bool {
        self.clock.is_some()
    }

    /// Share of the route's distance reached `draw_share` of the way through
    /// the drawing
    pub fn progress(&self, pace: Pace, draw_share: f32) -> f32 {
        let share = draw_share.clamp(0.0, 1.0);
        match (pace, &self.clock) {
            (Pace::Time, Some(clock)) if self.end_m > self.start_m => {
                let distance = clock.distance_at(share);
                ((distance - self.start_m) / (self.end_m - self.start_m)).clamp(0.0, 1.0)
            }
            _ => share,
        }
    }

    /// The stats over the first `progress` share of the route: distance,
    /// climb and time so far, and the highest and lowest points passed
    pub fn stats(&self, progress: f32) -> HikeStats {
        if progress >= 1.0 {
            return self.stats.clone();
        }
        let progress = progress.max(0.0);
        let share = f64::from(progress);
        let distance = self.start_m + (self.end_m - self.start_m) * progress;
        let total = &self.stats;
        let so_far = match (self.profile.first(), self.profile.last()) {
            (Some(first), Some(last)) => {
                self.climb_to(first.distance_m + (last.distance_m - first.distance_m) * progress)
            }
            _ => Climb::default(),
        };
        let part = |done: f32, all: f32| {
            if all > 0.0 {
                f64::from(done / all)
            } else {
                share
            }
        };
        let time_share = self
            .clock
            .as_ref()
            .map_or(share, |clock| clock.paced_share(distance));
        let start = self.profile.first().map(|p| f64::from(p.ele_m));
        let first = self.profile.first().map_or(0.0, |p| p.ele_m);
        HikeStats {
            distance_m: total.distance_m * share,
            gain_m: total.gain_m.map(|m| m * part(so_far.up, self.totals.up)),
            loss_m: total
                .loss_m
                .map(|m| m * part(so_far.down, self.totals.down)),
            moving_s: total
                .moving_s
                .map(|s| (s as f64 * time_share).round() as i64),
            elapsed_s: total.elapsed_s.map(|s| match &self.clock {
                Some(clock) => (clock.real_at(distance).round() as i64).min(s),
                None => (s as f64 * share).round() as i64,
            }),
            max_ele_m: total.max_ele_m.map(|max| match start {
                Some(start) if self.totals.high > first => {
                    let start = start.min(max);
                    let climbed = (so_far.high - first) / (self.totals.high - first);
                    start + (max - start) * f64::from(climbed)
                }
                _ => max,
            }),
            min_ele_m: total.min_ele_m.map(|min| match start {
                Some(start) if self.totals.low < first => {
                    let start = start.max(min);
                    let dropped = (first - so_far.low) / (first - self.totals.low);
                    start + (min - start) * f64::from(dropped)
                }
                _ => min,
            }),
            ..total.clone()
        }
    }

    /// The profile's climb, descent and extremes up to `distance_m`
    fn climb_to(&self, distance_m: f32) -> Climb {
        let Some(first) = self.profile.first() else {
            return Climb::default();
        };
        let mut climb = Climb {
            high: first.ele_m,
            low: first.ele_m,
            ..Climb::default()
        };
        for pair in self.profile.windows(2) {
            let [a, b] = [pair[0], pair[1]];
            if a.distance_m >= distance_m {
                break;
            }
            let ele = if b.distance_m > distance_m {
                let t = (distance_m - a.distance_m) / (b.distance_m - a.distance_m);
                a.ele_m + (b.ele_m - a.ele_m) * t
            } else {
                b.ele_m
            };
            let rise = ele - a.ele_m;
            climb.up += rise.max(0.0);
            climb.down += (-rise).max(0.0);
            climb.high = climb.high.max(ele);
            climb.low = climb.low.min(ele);
        }
        climb
    }
}

impl Clock {
    /// `None` unless every point has a time
    fn new(route: &Route) -> Option<Clock> {
        let points = &route.points;
        let first = points.first()?.time?;
        if points.len() < 2 {
            return None;
        }
        let mut clock = Clock {
            distance_m: vec![points[0].distance_m],
            paced_s: vec![0.0],
            real_s: vec![0.0],
        };
        let mut paced = 0.0;
        for (i, pair) in points.windows(2).enumerate() {
            let (a, b) = (pair[0], pair[1]);
            let dt = (b.time? - a.time?).max(0) as f64;
            let gap = route.breaks.binary_search(&(i as u32 + 1)).is_ok();
            let walk_s = f64::from((b.distance_m - a.distance_m).max(0.0)) / CRAWL_M_PER_S;
            paced += if gap {
                dt.min(PAUSE_S)
            } else {
                dt.min(PAUSE_S + walk_s)
            };
            clock.distance_m.push(b.distance_m);
            clock.paced_s.push(paced);
            clock.real_s.push((b.time? - first) as f64);
        }
        (paced > 0.0).then_some(clock)
    }

    fn total_s(&self) -> f64 {
        self.paced_s.last().copied().unwrap_or(0.0)
    }

    /// Distance reached once `share` of the paced time has passed
    fn distance_at(&self, share: f32) -> f32 {
        let target = f64::from(share) * self.total_s();
        let i = self.paced_s.partition_point(|&s| s < target);
        if i == 0 {
            return self.distance_m[0];
        }
        if i == self.paced_s.len() {
            return self.distance_m[i - 1];
        }
        let (a, b) = (self.paced_s[i - 1], self.paced_s[i]);
        let t = ((target - a) / (b - a)) as f32;
        self.distance_m[i - 1] + (self.distance_m[i] - self.distance_m[i - 1]) * t
    }

    fn paced_share(&self, distance_m: f32) -> f64 {
        self.at(distance_m, &self.paced_s) / self.total_s()
    }

    /// Seconds since the start when the hike reached `distance_m`
    fn real_at(&self, distance_m: f32) -> f64 {
        self.at(distance_m, &self.real_s)
    }

    fn at(&self, distance_m: f32, series: &[f64]) -> f64 {
        let i = self.distance_m.partition_point(|&d| d < distance_m);
        if i == 0 {
            return series[0];
        }
        if i == series.len() {
            return series[i - 1];
        }
        let (a, b) = (self.distance_m[i - 1], self.distance_m[i]);
        if b <= a {
            return series[i - 1];
        }
        let t = f64::from((distance_m - a) / (b - a));
        series[i - 1] + (series[i] - series[i - 1]) * t
    }
}

#[cfg(test)]
mod tests {
    use isotrail_bundle::{Landcover, RoutePoint};
    use isotrail_testkit::{bundle, grid};

    use super::*;

    /// A straight 1 km route along a flat slab, `(distance, seconds)` per
    /// point, with a profile that climbs 100 m to the middle and drops 50 m
    fn hike(points: &[(f32, Option<i64>)], breaks: Vec<u32>) -> SceneBundle {
        let mut bundle = bundle(
            grid(10, 10, 100.0),
            &[0.0; 100],
            &[Landcover::Grassland; 100],
        );
        bundle.route = Route {
            points: points
                .iter()
                .map(|&(distance_m, time)| RoutePoint {
                    col: 0.5 + distance_m / 100.0 * 0.9,
                    row: 5.0,
                    distance_m,
                    time: time.map(|s| 1_750_000_000 + s),
                    gps_ele_m: None,
                    draped_m: 0.0,
                })
                .collect(),
            breaks,
        };
        bundle.profile = [(0.0, 500.0), (500.0, 600.0), (1000.0, 550.0)]
            .map(|(distance_m, ele_m)| ProfileSample { distance_m, ele_m })
            .to_vec();
        let stats = &mut bundle.meta.stats;
        stats.distance_m = 1000.0;
        (stats.gain_m, stats.loss_m) = (Some(110.0), Some(60.0));
        (stats.max_ele_m, stats.min_ele_m) = (Some(605.0), Some(495.0));
        (stats.moving_s, stats.elapsed_s) = (Some(1_000), Some(4_600));
        bundle
    }

    /// 1 km in 1000 s of walking with an hour's stop halfway
    fn paused() -> SceneBundle {
        hike(
            &[
                (0.0, Some(0)),
                (500.0, Some(500)),
                (500.0, Some(4_100)),
                (1000.0, Some(4_600)),
            ],
            Vec::new(),
        )
    }

    #[track_caller]
    fn assert_near(actual: f32, expected: f32) {
        assert!((actual - expected).abs() < 1e-4, "{actual} vs {expected}");
    }

    #[test]
    fn frames_fill_the_duration() {
        let timeline = Timeline::default();
        assert_eq!(timeline.frame_count(), 300);
        assert_near(timeline.frame_time(150), 5.0);
        let short = Timeline {
            duration_s: 0.01,
            ..timeline
        };
        assert_eq!(short.frame_count(), 1);
    }

    #[test]
    fn step_fps_holds_each_image_for_several_frames() {
        let timeline = Timeline {
            step_fps: Some(12),
            ..Timeline::default()
        };
        let times: Vec<f32> = (0..6).map(|i| timeline.frame_time(i)).collect();
        // 30 frames a second over 12 images: 3, 2, 3, 2…
        let twelfth = 1.0 / 12.0;
        assert_eq!(times, [0.0, 0.0, 0.0, twelfth, twelfth, 2.0 / 12.0]);
        let changes = (0..300)
            .filter(|&i| i == 0 || timeline.frame_time(i) != timeline.frame_time(i - 1))
            .count();
        assert_eq!(changes, 120);
        // As fast as the frames or faster is no step at all
        let even = Timeline {
            step_fps: Some(30),
            ..timeline
        };
        assert_near(even.frame_time(7), 7.0 / 30.0);
    }

    #[test]
    fn the_route_draws_between_the_intro_and_the_hold() {
        let timeline = Timeline::default();
        assert_eq!(timeline.draw_share(0.0), 0.0);
        assert_eq!(timeline.draw_share(0.5), 0.0);
        assert_near(timeline.draw_share(4.5), 0.5);
        assert_eq!(timeline.draw_share(8.5), 1.0);
        assert_eq!(timeline.draw_share(10.0), 1.0);
        // The intro, drawing and hold stretch with the duration
        let long = Timeline {
            duration_s: 20.0,
            ..timeline
        };
        assert_eq!(long.draw_share(1.0), 0.0);
        assert_near(long.draw_share(9.0), 0.5);
    }

    #[test]
    fn orbits_ease_back_to_the_still_view_by_the_hold() {
        let orbit = Orbit::default();
        assert_eq!(orbit.sweep_deg, 30.0);
        assert_near(orbit.yaw_offset_deg(0.0), -30.0);
        let drawn = INTRO_SHARE + DRAW_SHARE;
        assert_near(orbit.yaw_offset_deg(drawn / 2.0), -15.0);
        assert_near(orbit.yaw_offset_deg(drawn), 0.0);
        assert_eq!(orbit.yaw_offset_deg(0.95), 0.0);
        assert_eq!(orbit.yaw_offset_deg(1.0), 0.0);
        // Eased: slow at the ends, fastest in the middle
        let step = |share: f32| orbit.yaw_offset_deg(share + 0.01) - orbit.yaw_offset_deg(share);
        assert!(step(0.0) < step(0.2) && step(0.2) < step(drawn / 2.0 - 0.005));
        assert!(step(drawn - 0.01) < step(drawn / 2.0 - 0.005));

        let turn = Orbit { sweep_deg: 360.0 };
        assert_near(turn.yaw_offset_deg(0.0), -360.0);
        assert_eq!(Orbit { sweep_deg: 0.0 }.yaw_offset_deg(0.3), 0.0);
    }

    #[test]
    fn distance_pacing_ignores_the_clock() {
        let walk = Walk::new(&paused());
        assert!(walk.is_timed());
        assert_eq!(walk.progress(Pace::Distance, 0.3), 0.3);
        assert_eq!(walk.progress(Pace::Distance, 1.5), 1.0);
    }

    #[test]
    fn time_pacing_cuts_stops_to_a_minute() {
        let walk = Walk::new(&paused());
        // 500 s walking, 60 s of the stop, 500 s walking
        let total = 1_060.0;
        assert_near(walk.progress(Pace::Time, 250.0 / total), 0.25);
        assert_near(walk.progress(Pace::Time, 530.0 / total), 0.5);
        assert_near(walk.progress(Pace::Time, 810.0 / total), 0.75);
        assert_eq!(walk.progress(Pace::Time, 1.0), 1.0);
    }

    #[test]
    fn time_pacing_follows_the_walking_speed() {
        // The first half took three times as long as the second
        let walk = Walk::new(&hike(
            &[(0.0, Some(0)), (500.0, Some(1_500)), (1000.0, Some(2_000))],
            Vec::new(),
        ));
        assert_near(walk.progress(Pace::Time, 0.75), 0.5);
        assert_near(walk.progress(Pace::Time, 0.375), 0.25);
    }

    #[test]
    fn gaps_between_pieces_count_as_a_minute() {
        // Overnight between the two pieces
        let walk = Walk::new(&hike(
            &[
                (0.0, Some(0)),
                (400.0, Some(400)),
                (600.0, Some(50_000)),
                (1000.0, Some(50_400)),
            ],
            vec![2],
        ));
        let total = 860.0;
        assert_near(walk.progress(Pace::Time, 400.0 / total), 0.4);
        assert_near(walk.progress(Pace::Time, 460.0 / total), 0.6);
    }

    #[test]
    fn routes_without_times_pace_by_distance() {
        let walk = Walk::new(&hike(&[(0.0, Some(0)), (1000.0, None)], Vec::new()));
        assert!(!walk.is_timed());
        assert_eq!(walk.progress(Pace::Time, 0.4), 0.4);
        let still = Walk::new(&hike(&[(0.0, Some(9)), (1000.0, Some(9))], Vec::new()));
        assert!(!still.is_timed());
    }

    #[test]
    fn stats_count_up_to_their_totals() {
        let bundle = paused();
        let walk = Walk::new(&bundle);
        let start = walk.stats(0.0);
        assert_eq!(start.distance_m, 0.0);
        assert_eq!((start.gain_m, start.loss_m), (Some(0.0), Some(0.0)));
        assert_eq!((start.moving_s, start.elapsed_s), (Some(0), Some(0)));
        assert_eq!(start.max_ele_m, Some(500.0));

        // At the top: all the climb, none of the descent, and the stop
        // still ahead in elapsed time
        let top = walk.stats(0.5);
        assert_eq!(top.distance_m, 500.0);
        assert_eq!((top.gain_m, top.loss_m), (Some(110.0), Some(0.0)));
        assert_eq!(
            top.moving_s,
            Some((1_000.0 * 500.0 / 1_060.0f64).round() as i64)
        );
        assert_eq!(top.elapsed_s, Some(500));
        assert_eq!(top.max_ele_m, Some(605.0));

        let late = walk.stats(0.75);
        assert_eq!(late.loss_m, Some(30.0));
        assert_eq!(late.elapsed_s, Some(4_350));
        // The profile never dips below its start, so the low point stands
        assert_eq!(late.min_ele_m, Some(495.0));

        assert_eq!(walk.stats(1.0), bundle.meta.stats);
        let mut previous = walk.stats(0.0);
        for i in 1..=100 {
            let stats = walk.stats(i as f32 / 100.0);
            assert!(stats.distance_m >= previous.distance_m);
            assert!(stats.gain_m >= previous.gain_m && stats.loss_m >= previous.loss_m);
            assert!(stats.moving_s >= previous.moving_s);
            assert!(stats.elapsed_s >= previous.elapsed_s);
            assert!(stats.max_ele_m >= previous.max_ele_m);
            previous = stats;
        }
    }

    #[test]
    fn missing_stats_stay_missing() {
        let mut bundle = hike(&[(0.0, None), (1000.0, None)], Vec::new());
        bundle.profile.clear();
        let stats = &mut bundle.meta.stats;
        (stats.gain_m, stats.moving_s, stats.elapsed_s) = (None, None, None);
        let walked = Walk::new(&bundle).stats(0.5);
        assert_eq!(walked.distance_m, 500.0);
        assert_eq!(walked.loss_m, Some(30.0));
        assert_eq!(
            (walked.gain_m, walked.moving_s, walked.elapsed_s),
            (None, None, None)
        );
        // Without a profile the extremes show from the start
        assert_eq!(
            (walked.max_ele_m, walked.min_ele_m),
            (Some(605.0), Some(495.0))
        );
    }

    #[test]
    fn moments_combine_the_timeline_orbit_and_pace() {
        let walk = Walk::new(&paused());
        let motion = Motion {
            pace: Pace::Time,
            ..Motion::default()
        };
        let first = motion.moment(&walk, 0);
        assert_eq!((first.time_s, first.progress), (0.0, 0.0));
        assert_near(first.yaw_offset_deg, -30.0);
        let last = motion.moment(&walk, motion.timeline.frame_count() - 1);
        assert_eq!(last.progress, 1.0);
        assert_eq!(last.yaw_offset_deg, 0.0);
    }
}
