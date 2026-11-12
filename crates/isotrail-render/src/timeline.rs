use std::f32::consts::PI;

use isotrail_bundle::{HikeStats, ProfileSample, Route, SceneBundle};

use crate::{Aim, Conditions, SunState};

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
/// A stop at least this long is a night: the route waits while the clock
/// runs through it
const NIGHT_S: f64 = 6.0 * 3600.0;
/// Share of the drawing each night takes, and all of them together at most
const NIGHT_SHARE: f32 = 0.06;
const MAX_NIGHTS_SHARE: f32 = 0.3;

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

    /// Seconds into the clip where the hold on the finished hike begins
    pub fn hold_s(&self) -> f32 {
        self.duration_s * (INTRO_SHARE + DRAW_SHARE)
    }

    /// Whether frame `index` falls in the hold. The last frame always does,
    /// so a short or choppy clip still ends on the finished hike
    pub fn holds(&self, index: u32) -> bool {
        index + 1 >= self.frame_count() || index as f32 / self.fps.max(1) as f32 >= self.hold_s()
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
    /// Holds this sun through the clip instead of the hike's own
    pub sun: Option<SunState>,
    /// How the hike's own sun and sky are changed
    pub conditions: Conditions,
    /// The view the orbit settles on
    pub aim: Aim,
}

/// What one frame of a clip shows
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Moment {
    pub time_s: f32,
    /// Added to the still's camera turn
    pub yaw_offset_deg: f32,
    /// Share of the route drawn, by distance
    pub progress: f32,
    /// Unix seconds at the route's head, for routes with times
    pub hike_time_s: Option<f64>,
}

impl Motion {
    pub fn moment(&self, walk: &Walk, index: u32) -> Moment {
        let time_s = self.timeline.frame_time(index);
        if self.timeline.holds(index) {
            // A held image from before the hold would show the route unfinished
            return Moment {
                time_s: time_s.max(self.timeline.hold_s()),
                yaw_offset_deg: 0.0,
                progress: 1.0,
                hike_time_s: walk.step(self.pace, 1.0).time_s,
            };
        }
        let step = walk.step(self.pace, self.timeline.draw_share(time_s));
        Moment {
            time_s,
            yaw_offset_deg: self.orbit.yaw_offset_deg(self.timeline.share(time_s)),
            progress: step.progress,
            hike_time_s: step.time_s,
        }
    }
}

/// How far the drawing has got: along the route, and on the hike's clock
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Step {
    /// Share of the route's distance drawn
    pub progress: f32,
    /// Unix seconds at the route's head, for routes with times
    pub time_s: Option<f64>,
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
/// and nights left out
#[derive(Clone, Debug)]
struct Clock {
    /// Unix seconds at the first point
    start_s: f64,
    distance_m: Vec<f32>,
    paced_s: Vec<f64>,
    real_s: Vec<f64>,
    /// The points that a night follows
    nights: Vec<usize>,
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
        self.step(pace, draw_share).progress
    }

    /// Where the route's head is `draw_share` of the way through the
    /// drawing, and the time there. Each night takes an equal slice of the
    /// drawing, the head waiting while the clock runs through it
    pub fn step(&self, pace: Pace, draw_share: f32) -> Step {
        let share = draw_share.clamp(0.0, 1.0);
        let span = self.end_m - self.start_m;
        let Some(clock) = &self.clock else {
            return Step {
                progress: share,
                time_s: None,
            };
        };
        if share >= 1.0 {
            return Step {
                progress: 1.0,
                time_s: Some(clock.start_s + clock.real_s.last().copied().unwrap_or(0.0)),
            };
        }
        let timed = pace == Pace::Time && span > 0.0;
        let walked = |at: usize| -> f32 {
            if timed {
                (clock.paced_s[at] / clock.total_s()) as f32
            } else {
                self.share_of(clock.distance_m[at])
            }
        };
        let nights = &clock.nights;
        let night_share = if nights.is_empty() {
            0.0
        } else {
            NIGHT_SHARE.min(MAX_NIGHTS_SHARE / nights.len() as f32)
        };
        let walking = 1.0 - night_share * nights.len() as f32;
        let (mut cursor, mut from, mut dawn) = (0.0, 0.0, None);
        for &before in nights {
            let to = walked(before);
            let stretch = (to - from).max(0.0) * walking;
            if share < cursor + stretch {
                break;
            }
            cursor += stretch;
            if share < cursor + night_share {
                let t = f64::from((share - cursor) / night_share);
                let (dusk, dawn) = (clock.real_s[before], clock.real_s[before + 1]);
                return Step {
                    progress: self.share_of(clock.distance_m[before]),
                    time_s: Some(clock.start_s + dusk + (dawn - dusk) * t),
                };
            }
            cursor += night_share;
            from = to;
            dawn = Some(clock.real_s[before + 1]);
        }
        let walked_share = (from + (share - cursor) / walking).clamp(0.0, 1.0);
        let (distance, real) = if timed {
            let paced = f64::from(walked_share) * clock.total_s();
            (clock.distance_at(walked_share), clock.real_at_paced(paced))
        } else {
            let distance = self.start_m + span * walked_share;
            (distance, clock.real_at(distance))
        };
        // Past a night, the clock never runs back into it
        let real = dawn.map_or(real, |dawn: f64| real.max(dawn));
        Step {
            progress: self.share_of(distance),
            time_s: Some(clock.start_s + real),
        }
    }

    fn share_of(&self, distance_m: f32) -> f32 {
        if self.end_m > self.start_m {
            ((distance_m - self.start_m) / (self.end_m - self.start_m)).clamp(0.0, 1.0)
        } else {
            1.0
        }
    }

    /// The stats at a frame of a clip, the elapsed time following the clock
    /// through nights
    pub fn stats_at(&self, moment: &Moment) -> HikeStats {
        let mut stats = self.stats(moment.progress);
        if let (Some(clock), Some(time_s), Some(total)) =
            (&self.clock, moment.hike_time_s, self.stats.elapsed_s)
        {
            stats.elapsed_s =
                Some(((time_s - clock.start_s).round() as i64).clamp(0, total.max(0)));
        }
        stats
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
            start_s: first as f64,
            distance_m: vec![points[0].distance_m],
            paced_s: vec![0.0],
            real_s: vec![0.0],
            nights: Vec::new(),
        };
        let (mut paced, mut real) = (0.0, 0.0);
        for (i, pair) in points.windows(2).enumerate() {
            let (a, b) = (pair[0], pair[1]);
            // Times that run backwards hold the clock where it was
            let reached = ((b.time? - first) as f64).max(real);
            let dt = reached - real;
            real = reached;
            let gap = route.breaks.binary_search(&(i as u32 + 1)).is_ok();
            let walk_s = f64::from((b.distance_m - a.distance_m).max(0.0)) / CRAWL_M_PER_S;
            paced += if dt >= NIGHT_S {
                clock.nights.push(i);
                0.0
            } else if gap {
                dt.min(PAUSE_S)
            } else {
                dt.min(PAUSE_S + walk_s)
            };
            clock.distance_m.push(b.distance_m);
            clock.paced_s.push(paced);
            clock.real_s.push(real);
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

    /// Seconds since the start once `paced_s` of the paced time has passed
    fn real_at_paced(&self, paced_s: f64) -> f64 {
        let i = self.paced_s.partition_point(|&s| s < paced_s);
        if i == 0 {
            return self.real_s[0];
        }
        if i == self.paced_s.len() {
            return self.real_s[i - 1];
        }
        let (a, b) = (self.paced_s[i - 1], self.paced_s[i]);
        if b <= a {
            return self.real_s[i];
        }
        let t = (paced_s - a) / (b - a);
        self.real_s[i - 1] + (self.real_s[i] - self.real_s[i - 1]) * t
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
        // Two hours between the two pieces
        let walk = Walk::new(&hike(
            &[
                (0.0, Some(0)),
                (400.0, Some(400)),
                (600.0, Some(7_600)),
                (1000.0, Some(8_000)),
            ],
            vec![2],
        ));
        let total = 860.0;
        assert_near(walk.progress(Pace::Time, 400.0 / total), 0.4);
        assert_near(walk.progress(Pace::Time, 460.0 / total), 0.6);
    }

    /// 400 m on the first evening, a night until 50 000 s, then 600 m
    fn overnight() -> SceneBundle {
        hike(
            &[
                (0.0, Some(0)),
                (400.0, Some(400)),
                (600.0, Some(50_000)),
                (1000.0, Some(50_400)),
            ],
            vec![2],
        )
    }

    const START: f64 = 1_750_000_000.0;

    #[track_caller]
    fn assert_step(step: Step, progress: f32, time_s: f64) {
        assert_near(step.progress, progress);
        let time = step.time_s.unwrap() - START;
        assert!((time - time_s).abs() < 0.5, "{time} vs {time_s}");
    }

    #[test]
    fn nights_hold_the_head_while_the_clock_runs() {
        let walk = Walk::new(&overnight());
        // The night takes 6% of the drawing, at 40% of the distance
        let dusk = 0.4 * 0.94;
        assert_step(walk.step(Pace::Distance, 0.0), 0.0, 0.0);
        assert_step(walk.step(Pace::Distance, 0.2 * 0.94), 0.2, 200.0);
        assert_step(
            walk.step(Pace::Distance, dusk + 0.001),
            0.4,
            400.0 + 49_600.0 / 60.0,
        );
        assert_step(walk.step(Pace::Distance, dusk + 0.03), 0.4, 25_200.0);
        assert_step(
            walk.step(Pace::Distance, dusk + 0.059),
            0.4,
            50_000.0 - 49_600.0 / 60.0,
        );
        // Past the night the morning never runs back, even across the
        // distance between the pieces
        assert_step(
            walk.step(Pace::Distance, dusk + 0.06 + 0.1 * 0.94),
            0.5,
            50_000.0,
        );
        assert_step(
            walk.step(Pace::Distance, dusk + 0.06 + 0.4 * 0.94),
            0.8,
            50_200.0,
        );
        assert_step(walk.step(Pace::Distance, 1.0), 1.0, 50_400.0);

        // By time the night is halfway through 800 s of walking
        assert_step(walk.step(Pace::Time, 0.25 * 0.94), 0.2, 200.0);
        assert_step(walk.step(Pace::Time, 0.5 * 0.94 + 0.03), 0.4, 25_200.0);
        assert_step(
            walk.step(Pace::Time, 0.5 * 0.94 + 0.06 + 0.25 * 0.94),
            0.8,
            50_200.0,
        );
        assert_step(walk.step(Pace::Time, 1.0), 1.0, 50_400.0);
    }

    #[test]
    fn the_clock_never_runs_backwards() {
        let walk = Walk::new(&overnight());
        for pace in [Pace::Distance, Pace::Time] {
            let mut previous = (0.0, START);
            for i in 0..=1000 {
                let step = walk.step(pace, i as f32 / 1000.0);
                let time = step.time_s.unwrap();
                assert!(
                    step.progress >= previous.0 && time >= previous.1,
                    "{pace:?} {i}"
                );
                previous = (step.progress, time);
            }
        }
    }

    #[test]
    fn many_nights_share_at_most_thirty_percent_of_the_drawing() {
        // Ten days of 100 m, each followed by a night
        let points: Vec<(f32, Option<i64>)> = (0..20)
            .map(|i| {
                let day = i / 2;
                let distance = (day * 100 + (i % 2) * 100) as f32;
                (distance, Some(day as i64 * 86_400 + (i % 2) as i64 * 1_000))
            })
            .collect();
        let walk = Walk::new(&hike(&points, (1..10).map(|d| d * 2).collect()));
        let held = (0..=10_000)
            .map(|i| walk.step(Pace::Distance, i as f32 / 10_000.0))
            .collect::<Vec<_>>()
            .windows(2)
            .filter(|w| w[0].progress == w[1].progress)
            .count();
        assert!((2_900..=3_050).contains(&held), "{held}");
    }

    #[test]
    fn elapsed_time_counts_through_the_night() {
        let mut bundle = overnight();
        bundle.meta.stats.elapsed_s = Some(50_400);
        let walk = Walk::new(&bundle);
        let at = |draw_share: f32| {
            let step = walk.step(Pace::Distance, draw_share);
            let moment = Moment {
                time_s: 0.0,
                yaw_offset_deg: 0.0,
                progress: step.progress,
                hike_time_s: step.time_s,
            };
            walk.stats_at(&moment)
        };
        let night = at(0.4 * 0.94 + 0.03);
        assert_eq!(night.elapsed_s, Some(25_200));
        assert!((night.distance_m - 400.0).abs() < 1e-3);
        assert_eq!(at(1.0), bundle.meta.stats);
    }

    #[test]
    fn routes_without_times_pace_by_distance() {
        let walk = Walk::new(&hike(&[(0.0, Some(0)), (1000.0, None)], Vec::new()));
        assert!(!walk.is_timed());
        assert_eq!(walk.progress(Pace::Time, 0.4), 0.4);
        assert_eq!(walk.step(Pace::Distance, 0.4).time_s, None);
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
        assert_eq!(first.hike_time_s, Some(START));
        assert_near(first.yaw_offset_deg, -30.0);
        let last = motion.moment(&walk, motion.timeline.frame_count() - 1);
        assert_eq!(last.progress, 1.0);
        assert_eq!(last.hike_time_s, Some(START + 4_600.0));
        assert_eq!(last.yaw_offset_deg, 0.0);
    }

    /// Six days of 100 m, each but the last followed by a night
    fn six_days() -> SceneBundle {
        let points: Vec<(f32, Option<i64>)> = (0..12)
            .map(|i| {
                let day = i / 2;
                let distance = (day * 100 + (i % 2) * 100) as f32;
                (distance, Some(day as i64 * 86_400 + (i % 2) as i64 * 2_000))
            })
            .collect();
        hike(&points, (1..6).map(|d| d * 2).collect())
    }

    #[test]
    fn every_clip_ends_on_the_finished_hike() {
        for bundle in [paused(), six_days()] {
            let walk = Walk::new(&bundle);
            let finish =
                START + bundle.route.points.last().unwrap().time.unwrap() as f64 - 1_750_000_000.0;
            for pace in [Pace::Distance, Pace::Time] {
                for duration_s in 1..=60 {
                    for fps in [1, 12, 30, 50] {
                        for step_fps in [None, Some(1), Some(6), Some(12), Some(fps)] {
                            let motion = Motion {
                                timeline: Timeline {
                                    duration_s: duration_s as f32,
                                    fps,
                                    step_fps,
                                },
                                pace,
                                ..Motion::default()
                            };
                            let count = motion.timeline.frame_count();
                            let last = motion.moment(&walk, count - 1);
                            let case = format!("{duration_s} s, {fps} fps, {step_fps:?}, {pace:?}");
                            assert_eq!(last.progress, 1.0, "{case}");
                            assert_eq!(last.yaw_offset_deg, 0.0, "{case}");
                            assert_eq!(last.hike_time_s, Some(finish), "{case}");
                            // Every frame from the hold on shows the same
                            let hold = (0..count).find(|&i| motion.timeline.holds(i)).unwrap();
                            for i in hold..count {
                                assert_eq!(motion.moment(&walk, i).progress, 1.0, "{case}");
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn a_choppy_clip_holds_its_last_image_on_the_finish() {
        // Five seconds at one image a second used to end at 94%
        let walk = Walk::new(&paused());
        let motion = Motion {
            timeline: Timeline {
                duration_s: 5.0,
                fps: 30,
                step_fps: Some(1),
            },
            ..Motion::default()
        };
        let moments: Vec<Moment> = (0..150).map(|i| motion.moment(&walk, i)).collect();
        assert!(moments[119].progress < 1.0);
        for moment in &moments[128..] {
            assert_eq!(moment.time_s, 4.25);
            assert_eq!(moment.progress, 1.0);
        }
        // One image for the whole hold, so it is rendered once
        let images = moments
            .windows(2)
            .filter(|w| w[0].time_s != w[1].time_s)
            .count();
        assert_eq!(images, 5);
    }

    #[test]
    fn times_running_backwards_never_turn_the_clock_back() {
        // The second half stamped a day before the first, as written by a
        // device whose clock jumped
        let mut bundle = hike(
            &[
                (0.0, Some(86_400)),
                (250.0, Some(86_650)),
                (500.0, Some(86_900)),
                (750.0, Some(0)),
                (1000.0, Some(250)),
            ],
            vec![3],
        );
        bundle.meta.stats.elapsed_s = Some(-86_650);
        let walk = Walk::new(&bundle);
        assert!(walk.is_timed());
        for pace in [Pace::Distance, Pace::Time] {
            let motion = Motion {
                pace,
                ..Motion::default()
            };
            let mut last = f64::MIN;
            for index in 0..motion.timeline.frame_count() {
                let moment = motion.moment(&walk, index);
                let time = moment.hike_time_s.unwrap();
                assert!(time >= last, "{pace:?} frame {index}");
                last = time;
                let stats = walk.stats_at(&moment);
                assert!(stats.elapsed_s.unwrap() >= 0, "{pace:?} frame {index}");
            }
        }
    }

    #[test]
    fn time_pace_keeps_a_slow_logging_interval_whole() {
        let points: Vec<(f32, Option<i64>)> = (0..=10)
            .map(|i| (i as f32 * 100.0, Some(i64::from(i) * 61)))
            .collect();
        let walk = Walk::new(&hike(&points, Vec::new()));
        for share in [0.1, 0.25, 0.5, 0.9] {
            assert_near(walk.progress(Pace::Time, share), share);
        }
    }
}
