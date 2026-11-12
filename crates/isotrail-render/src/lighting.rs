use chrono::DateTime;
use glam::camera::rh::{proj, view};
use glam::{Mat4, Vec3};
use isotrail_bundle::{GridInfo, MoonSample, SceneBundle, SunSample};
use isotrail_track::{moon_position, sun_position};

use crate::camera::slab_angle;
use crate::{Atmosphere, ForcedWeather, SKY_BOTTOM, SKY_TOP, WeatherEffects};

/// The late-morning sun of hikes without times
pub const SUN_AZIMUTH_DEG: f64 = 150.0;
pub const SUN_ELEVATION_DEG: f64 = 40.0;
pub const SUN_COLOUR: Vec3 = Vec3::new(1.0, 0.96, 0.88);
pub const AMBIENT: f32 = 0.4;
/// Extra ambient, as a share of it, for faces turned towards the camera, so
/// sides facing away from the sun still read
pub const FILL: f32 = 0.6;
/// Steps the direct light is quantised to
pub const BANDS: u32 = 4;
/// The light never comes from lower than this, so a setting sun casts long
/// but bounded shadows and twilight still models the terrain
pub const MIN_LIGHT_ELEVATION_DEG: f64 = 10.0;
/// Direct light from a full moon high in a dark sky
pub const MOONLIGHT: f32 = 0.4;
const MOON_COLOUR: Vec3 = Vec3::new(0.72, 0.82, 1.0);

/// The light and sky at one sun elevation
#[derive(Clone, Copy)]
struct Key {
    elevation_deg: f32,
    colour: Vec3,
    intensity: f32,
    ambient: f32,
    ambient_colour: Vec3,
    sky: [[u8; 3]; 2],
}

/// Night, blue hour, sunset, golden hour and day; between two keys the light
/// blends linearly with the sun's elevation
const RAMP: [Key; 5] = [
    Key {
        elevation_deg: -18.0,
        colour: Vec3::new(0.55, 0.65, 1.0),
        intensity: 0.1,
        ambient: 0.3,
        ambient_colour: Vec3::new(0.45, 0.55, 0.95),
        sky: [[14, 18, 40], [30, 40, 74]],
    },
    Key {
        elevation_deg: -6.0,
        colour: Vec3::new(0.6, 0.68, 1.0),
        intensity: 0.35,
        ambient: 0.28,
        ambient_colour: Vec3::new(0.6, 0.66, 1.0),
        sky: [[32, 44, 92], [112, 110, 160]],
    },
    Key {
        elevation_deg: 0.0,
        colour: Vec3::new(1.0, 0.55, 0.32),
        intensity: 0.75,
        ambient: 0.32,
        ambient_colour: Vec3::new(0.8, 0.74, 0.92),
        sky: [[70, 90, 150], [240, 150, 100]],
    },
    Key {
        elevation_deg: 6.0,
        colour: Vec3::new(1.0, 0.78, 0.52),
        intensity: 0.95,
        ambient: 0.36,
        ambient_colour: Vec3::new(1.0, 0.9, 0.82),
        sky: [[110, 150, 200], [244, 200, 150]],
    },
    Key {
        elevation_deg: 20.0,
        colour: SUN_COLOUR,
        intensity: 1.0,
        ambient: AMBIENT,
        ambient_colour: Vec3::ONE,
        sky: [SKY_TOP, SKY_BOTTOM],
    },
];

/// The one directional light, and the sky it shines from
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SunState {
    /// Unit vector towards the light, in the slab frame
    pub direction: Vec3,
    pub colour: Vec3,
    pub intensity: f32,
    pub ambient: f32,
    /// Tints the ambient light
    pub ambient_colour: Vec3,
    /// The sky's colour at the top and bottom of the frame
    pub sky: [[u8; 3]; 2],
    /// How brightly the stars show, from 0 by day to 1 at night
    pub stars: f32,
    /// The moon's disc, while it shows in the sky
    pub moon: Option<MoonDisc>,
    /// `None` under a clear sky with nothing falling
    pub weather: Option<Atmosphere>,
}

/// Where the moon stands, as the sky shows it
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MoonDisc {
    /// Unit vector towards the moon, in the slab frame
    pub direction: Vec3,
    /// Unit vector towards the sun, which lights one side of the disc
    pub sun_direction: Vec3,
    /// Share of the disc in sunlight
    pub illuminated: f32,
    /// How much the disc stands out from the sky, rising as the sky darkens
    pub opacity: f32,
}

/// The moon's compass bearing, elevation and phase
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Moon {
    pub azimuth_deg: f64,
    pub elevation_deg: f64,
    /// Share of the disc in sunlight
    pub illuminated: f64,
}

impl From<&MoonSample> for Moon {
    fn from(sample: &MoonSample) -> Moon {
        Moon {
            azimuth_deg: sample.azimuth_deg.into(),
            elevation_deg: sample.elevation_deg.into(),
            illuminated: sample.illuminated.into(),
        }
    }
}

impl SunState {
    pub fn late_morning(grid: &GridInfo) -> SunState {
        SunState::at(grid, SUN_AZIMUTH_DEG, SUN_ELEVATION_DEG)
    }

    /// The light of a sun standing at this compass bearing and elevation, on
    /// a moonless night
    pub fn at(grid: &GridInfo, azimuth_deg: f64, elevation_deg: f64) -> SunState {
        SunState::with_moon(grid, azimuth_deg, elevation_deg, None)
    }

    /// The light of the sun and, once the sky is dark, the moon
    pub fn with_moon(
        grid: &GridInfo,
        azimuth_deg: f64,
        elevation_deg: f64,
        moon: Option<Moon>,
    ) -> SunState {
        let key = blend(elevation_deg as f32);
        let mut sun = SunState {
            direction: light_direction(grid, azimuth_deg, elevation_deg),
            colour: key.colour,
            intensity: key.intensity,
            ambient: key.ambient,
            ambient_colour: key.ambient_colour,
            sky: key.sky,
            stars: smoothstep(-6.0, -14.0, elevation_deg as f32),
            moon: None,
            weather: None,
        };
        let Some(moon) = moon.filter(|moon| moon.elevation_deg > -2.0) else {
            return sun;
        };
        let opacity = smoothstep(6.0, -4.0, elevation_deg as f32);
        if opacity > 0.0 {
            sun.moon = Some(MoonDisc {
                direction: towards(grid, moon.azimuth_deg, moon.elevation_deg),
                sun_direction: towards(grid, azimuth_deg, elevation_deg),
                illuminated: moon.illuminated as f32,
                opacity,
            });
        }
        // A half moon gives about a tenth of a full one's light
        let moonlight = MOONLIGHT
            * (moon.illuminated as f32).powi(2)
            * smoothstep(-1.0, 8.0, moon.elevation_deg as f32)
            * smoothstep(-4.0, -12.0, elevation_deg as f32);
        if moonlight > 0.0 {
            let (from_sun, from_moon) = (key.colour * key.intensity, MOON_COLOUR * moonlight);
            let total = from_sun + from_moon;
            sun.intensity = total.max_element();
            sun.colour = total / sun.intensity;
            let towards_moon = light_direction(grid, moon.azimuth_deg, moon.elevation_deg);
            sun.direction = (sun.direction * luminance(from_sun)
                + towards_moon * luminance(from_moon))
            .normalize_or(sun.direction);
        }
        sun
    }

    /// The sun, moon and weather over the slab at `unix_s`, which need not
    /// fall within the hike
    pub fn at_instant(bundle: &SceneBundle, unix_s: i64) -> SunState {
        SunState::seen(bundle, When::Instant(unix_s), &Conditions::default())
    }

    /// The sun, moon and weather at the start of the hike, or the
    /// late-morning sun when the track has no times
    pub fn for_hike(bundle: &SceneBundle) -> SunState {
        SunState::seen(bundle, When::Start, &Conditions::default())
    }

    /// The light over the slab at `when`, changed as `conditions` say
    pub fn seen(bundle: &SceneBundle, when: When, conditions: &Conditions) -> SunState {
        let (azimuth_deg, elevation_deg, moon, weather_time) = position(bundle, when);
        let [azimuth_deg, elevation_deg] = conditions.sun.unwrap_or([azimuth_deg, elevation_deg]);
        let sun = SunState::with_moon(&bundle.grid, azimuth_deg, elevation_deg, moon);
        let recorded = weather_time.and_then(|time_s| {
            bundle
                .weather
                .as_ref()
                .filter(|weather| !weather.samples.is_empty())
                .map(|weather| weather.at(time_s))
        });
        let state = match conditions.weather {
            Some(forced) => Some(forced.state(recorded.as_ref())),
            None => recorded,
        };
        sun.with_weather(
            state.map(|state| Atmosphere::new(&bundle.grid, &state).showing(conditions.effects)),
        )
    }

    /// The light under `weather`: cloud dims and greys the sun and the sky,
    /// and hides the stars and the moon
    pub fn with_weather(mut self, weather: Option<Atmosphere>) -> SunState {
        self.weather = weather;
        let Some(weather) = weather else {
            return self;
        };
        let overcast = weather.dimming();
        self.intensity *= 1.0 - 0.65 * overcast;
        self.colour = self
            .colour
            .lerp(Vec3::splat(luminance(self.colour)), 0.6 * overcast);
        self.ambient_colour = self
            .ambient_colour
            .lerp(Vec3::splat(luminance(self.ambient_colour)), 0.5 * overcast);
        self.sky = self.sky.map(|rgb| {
            let colour = Vec3::from_array(rgb.map(f32::from));
            let grey = Vec3::splat(luminance(colour) * 0.92) * Vec3::new(0.97, 1.0, 1.04);
            colour
                .lerp(grey, 0.85 * overcast)
                .round()
                .clamp(Vec3::ZERO, Vec3::splat(255.0))
                .to_array()
                .map(|c| c as u8)
        });
        let clear = 1.0 - weather.visible_cover();
        self.stars *= clear;
        if let Some(moon) = &mut self.moon {
            moon.opacity *= clear;
            if moon.opacity <= 0.0 {
                self.moon = None;
            }
        }
        self
    }

    /// The light under the bundle's weather at Unix time `time_s`
    pub fn with_weather_at(self, bundle: &SceneBundle, time_s: f64) -> SunState {
        let weather = bundle
            .weather
            .as_ref()
            .filter(|weather| !weather.samples.is_empty())
            .map(|weather| Atmosphere::new(&bundle.grid, &weather.at(time_s)));
        self.with_weather(weather)
    }

    /// The sun, moon and weather at Unix time `time_s`, between the
    /// bundle's samples; before the first and after the last, they stand as
    /// at those
    pub fn during(bundle: &SceneBundle, time_s: f64) -> SunState {
        SunState::seen(bundle, When::During(time_s), &Conditions::default())
    }

    /// An orthographic projection from the sun that holds every point
    pub fn shadow_view_proj(&self, points: impl IntoIterator<Item = Vec3>) -> Mat4 {
        let forward = -self.direction.normalize_or(Vec3::Z);
        let up = if forward.z.abs() > 0.99 {
            Vec3::Y
        } else {
            Vec3::Z
        };
        let view = view::look_to_mat4(Vec3::ZERO, forward, up);
        let (mut min, mut max) = (Vec3::INFINITY, Vec3::NEG_INFINITY);
        for point in points {
            let p = view.transform_point3(point);
            (min, max) = (min.min(p), max.max(p));
        }
        if min.x > max.x {
            (min, max) = (Vec3::splat(-1.0), Vec3::ONE);
        }
        let pad = (max - min).max_element() * 0.01 + 1.0;
        let projection = proj::directx::orthographic(
            min.x - pad,
            max.x + pad,
            min.y - pad,
            max.y + pad,
            -max.z - pad,
            -min.z + pad,
        );
        projection * view
    }
}

/// When over the hike a light is seen
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum When {
    /// The hike's start, or late morning for a track without times
    Start,
    /// Unix seconds, between the bundle's sky samples
    During(f64),
    /// Unix seconds, which need not fall within the hike
    Instant(i64),
}

/// Changes to the light the bundle gives
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Conditions {
    /// A sun set by hand: compass bearing and elevation in degrees
    pub sun: Option<[f64; 2]>,
    /// A sky in place of the recorded weather
    pub weather: Option<ForcedWeather>,
    pub effects: WeatherEffects,
}

/// The sun's bearing and elevation, the moon, and the time the weather is
/// read at, if the track has times
fn position(bundle: &SceneBundle, when: When) -> (f64, f64, Option<Moon>, Option<f64>) {
    let grid = &bundle.grid;
    let track = &bundle.sun;
    let sample = |i: usize| match track.get(i) {
        Some(&SunSample {
            azimuth_deg,
            elevation_deg,
            ..
        }) => (
            azimuth_deg.into(),
            elevation_deg.into(),
            bundle.moon.get(i).map(Moon::from),
        ),
        None => (SUN_AZIMUTH_DEG, SUN_ELEVATION_DEG, None),
    };
    let start = |(azimuth, elevation, moon): (f64, f64, Option<Moon>)| {
        (
            azimuth,
            elevation,
            moon,
            track.first().map(|first| first.time as f64),
        )
    };
    match when {
        When::Start => start(sample(0)),
        When::Instant(unix_s) => {
            let Some(time) = DateTime::from_timestamp(unix_s, 0) else {
                return start(sample(0));
            };
            let sun = sun_position(grid.origin_lat, grid.origin_lon, time);
            let moon = moon_position(grid.origin_lat, grid.origin_lon, time);
            (
                sun.azimuth_deg,
                sun.elevation_deg,
                Some(Moon {
                    azimuth_deg: moon.azimuth_deg,
                    elevation_deg: moon.elevation_deg,
                    illuminated: moon.illuminated,
                }),
                Some(unix_s as f64),
            )
        }
        When::During(time_s) => {
            let weather_time = (!track.is_empty()).then_some(time_s);
            let i = track.partition_point(|s| (s.time as f64) <= time_s);
            if i == 0 || i == track.len() {
                let (azimuth, elevation, moon) = sample(i.saturating_sub(1));
                return (azimuth, elevation, moon, weather_time);
            }
            let (from, to) = (&track[i - 1], &track[i]);
            let t = (time_s - from.time as f64) / (to.time - from.time) as f64;
            let lerp = |x: f32, y: f32| f64::from(x) + (f64::from(y) - f64::from(x)) * t;
            let turn = |x: f32, y: f32| {
                let delta = (f64::from(y) - f64::from(x) + 180.0).rem_euclid(360.0) - 180.0;
                (f64::from(x) + delta * t).rem_euclid(360.0)
            };
            let moon = match (bundle.moon.get(i - 1), bundle.moon.get(i)) {
                (Some(x), Some(y)) => Some(Moon {
                    azimuth_deg: turn(x.azimuth_deg, y.azimuth_deg),
                    elevation_deg: lerp(x.elevation_deg, y.elevation_deg),
                    illuminated: lerp(x.illuminated, y.illuminated),
                }),
                _ => None,
            };
            (
                turn(from.azimuth_deg, to.azimuth_deg),
                lerp(from.elevation_deg, to.elevation_deg),
                moon,
                weather_time,
            )
        }
    }
}

/// Unit vector towards a point in the sky, in the slab frame
fn towards(grid: &GridInfo, azimuth_deg: f64, elevation_deg: f64) -> Vec3 {
    let azimuth = slab_angle(grid, azimuth_deg);
    let elevation = elevation_deg.to_radians() as f32;
    Vec3::new(
        azimuth.cos() * elevation.cos(),
        azimuth.sin() * elevation.cos(),
        elevation.sin(),
    )
}

/// Towards a light in the sky, never lower than [`MIN_LIGHT_ELEVATION_DEG`]
fn light_direction(grid: &GridInfo, azimuth_deg: f64, elevation_deg: f64) -> Vec3 {
    towards(
        grid,
        azimuth_deg,
        elevation_deg.max(MIN_LIGHT_ELEVATION_DEG),
    )
}

fn luminance(rgb: Vec3) -> f32 {
    rgb.dot(Vec3::new(0.2126, 0.7152, 0.0722))
}

/// 0 at `from` and 1 at `to`, eased between; `to` may be the smaller
pub(crate) fn smoothstep(from: f32, to: f32, x: f32) -> f32 {
    let t = ((x - from) / (to - from)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn blend(elevation_deg: f32) -> Key {
    let above = RAMP.partition_point(|key| key.elevation_deg <= elevation_deg);
    let (low, high) = match (above.checked_sub(1), RAMP.get(above)) {
        (Some(below), Some(high)) => (&RAMP[below], high),
        (Some(_), None) => return RAMP[RAMP.len() - 1],
        (None, _) => return RAMP[0],
    };
    let t = (elevation_deg - low.elevation_deg) / (high.elevation_deg - low.elevation_deg);
    let lerp = |a: f32, b: f32| a + (b - a) * t;
    let sky = |i: usize| {
        std::array::from_fn(|c| {
            lerp(f32::from(low.sky[i][c]), f32::from(high.sky[i][c])).round() as u8
        })
    };
    Key {
        elevation_deg,
        colour: low.colour.lerp(high.colour, t),
        intensity: lerp(low.intensity, high.intensity),
        ambient: lerp(low.ambient, high.ambient),
        ambient_colour: low.ambient_colour.lerp(high.ambient_colour, t),
        sky: [sky(0), sky(1)],
    }
}

#[cfg(test)]
mod tests {
    use isotrail_bundle::Landcover;
    use isotrail_testkit::{bundle, grid};

    use super::*;

    fn brightness(sun: &SunState) -> f32 {
        sun.intensity * (1.0 - sun.ambient) * luminance(sun.colour)
            + sun.ambient * luminance(sun.ambient_colour)
    }

    fn sky_luminance(sun: &SunState) -> f32 {
        sun.sky
            .iter()
            .map(|rgb| luminance(Vec3::from_array(rgb.map(f32::from))))
            .sum()
    }

    #[test]
    fn the_sun_stands_south_south_east_whatever_the_slab_turn() {
        for rotation_rad in [0.0, 0.7, -2.0] {
            let grid = GridInfo {
                rotation_rad,
                ..grid(10, 10, 10.0)
            };
            let sun = SunState::late_morning(&grid);
            assert!((sun.direction.length() - 1.0).abs() < 1e-5);
            assert!((sun.direction.z - 40f32.to_radians().sin()).abs() < 1e-5);
            let bearing = 90.0
                - (f64::from(sun.direction.y.atan2(sun.direction.x)) + rotation_rad).to_degrees();
            assert!(
                (bearing.rem_euclid(360.0) - 150.0).abs() < 1e-3,
                "{bearing}"
            );
        }
    }

    #[test]
    fn a_high_sun_gives_the_day_light() {
        let grid = grid(10, 10, 10.0);
        for elevation in [20.0, 40.0, 75.0] {
            let sun = SunState::at(&grid, 150.0, elevation);
            assert_eq!(
                (sun.colour, sun.intensity, sun.ambient),
                (SUN_COLOUR, 1.0, AMBIENT)
            );
            assert_eq!(sun.ambient_colour, Vec3::ONE);
            assert_eq!(sun.sky, [SKY_TOP, SKY_BOTTOM]);
        }
    }

    #[test]
    fn light_and_sky_darken_as_the_sun_goes_down() {
        let grid = grid(10, 10, 10.0);
        let suns: Vec<SunState> = (-40..=60)
            .rev()
            .map(|half_deg| SunState::at(&grid, 270.0, f64::from(half_deg) / 2.0))
            .collect();
        for pair in suns.windows(2) {
            assert!(brightness(&pair[1]) <= brightness(&pair[0]) + 1e-6);
            assert!(sky_luminance(&pair[1]) <= sky_luminance(&pair[0]) + 1.0);
        }
        let (day, night) = (&suns[0], &suns[suns.len() - 1]);
        assert!(brightness(night) < brightness(day) * 0.35);
        assert!(sky_luminance(night) < sky_luminance(day) * 0.25);
    }

    #[test]
    fn low_suns_are_warm_and_night_is_blue() {
        let grid = grid(10, 10, 10.0);
        let warmth = |sun: SunState| sun.colour.x / sun.colour.z;
        let day = warmth(SunState::at(&grid, 270.0, 40.0));
        assert!(warmth(SunState::at(&grid, 270.0, 3.0)) > day * 1.5);
        let night = SunState::at(&grid, 270.0, -25.0);
        assert!(warmth(night) < 1.0);
        assert!(night.ambient_colour.z > night.ambient_colour.x);
    }

    #[test]
    fn the_light_never_comes_from_below_the_clamp() {
        let grid = grid(10, 10, 10.0);
        let floor = (MIN_LIGHT_ELEVATION_DEG as f32).to_radians().sin();
        for elevation in [-30.0, -3.0, 0.0, 5.0] {
            let sun = SunState::at(&grid, 290.0, elevation);
            assert!((sun.direction.z - floor).abs() < 1e-6, "{elevation}");
            assert!((sun.direction.length() - 1.0).abs() < 1e-5);
        }
        assert!(SunState::at(&grid, 290.0, 30.0).direction.z > 0.49);
    }

    #[test]
    fn hikes_start_under_their_first_sun_sample() {
        let mut bundle = bundle(grid(2, 2, 10.0), &[0.0; 4], &[Landcover::Grassland; 4]);
        let grid = bundle.grid;
        assert_eq!(SunState::for_hike(&bundle), SunState::late_morning(&grid));
        let sample = |time, azimuth_deg, elevation_deg| SunSample {
            time,
            azimuth_deg,
            elevation_deg,
        };
        bundle.sun = vec![sample(0, 290.0, 3.0), sample(300, 291.0, 2.5)];
        assert_eq!(SunState::for_hike(&bundle), SunState::at(&grid, 290.0, 3.0));
        bundle.moon = vec![
            moon_sample(0, 100.0, 4.0, 0.9),
            moon_sample(300, 101.0, 5.0, 0.9),
        ];
        let moon = Moon {
            azimuth_deg: 100.0,
            elevation_deg: 4.0,
            illuminated: 0.9,
        };
        assert_eq!(
            SunState::for_hike(&bundle),
            SunState::with_moon(&grid, 290.0, 3.0, Some(moon))
        );
    }

    fn moon_sample(
        time: i64,
        azimuth_deg: f32,
        elevation_deg: f32,
        illuminated: f32,
    ) -> MoonSample {
        MoonSample {
            time,
            azimuth_deg,
            elevation_deg,
            illuminated,
        }
    }

    fn full_moon(elevation_deg: f64) -> Option<Moon> {
        Some(Moon {
            azimuth_deg: 180.0,
            elevation_deg,
            illuminated: 1.0,
        })
    }

    #[test]
    fn stars_come_out_as_the_sky_darkens() {
        let grid = grid(10, 10, 10.0);
        let stars = |elevation| SunState::at(&grid, 300.0, elevation).stars;
        assert_eq!(stars(10.0), 0.0);
        assert_eq!(stars(-6.0), 0.0);
        assert!(stars(-10.0) > 0.2 && stars(-10.0) < 0.8);
        assert_eq!(stars(-14.0), 1.0);
        assert_eq!(stars(-40.0), 1.0);
    }

    #[test]
    fn the_moon_shows_once_the_sun_is_low_and_lights_the_night() {
        let grid = grid(10, 10, 10.0);
        // By day the moon neither shows nor lights anything
        let day = SunState::with_moon(&grid, 120.0, 40.0, full_moon(30.0));
        assert_eq!(day, SunState::at(&grid, 120.0, 40.0));

        let moonless = SunState::at(&grid, 0.0, -25.0);
        let moonlit = SunState::with_moon(&grid, 0.0, -25.0, full_moon(30.0));
        assert!(brightness(&moonlit) > brightness(&moonless) * 1.3);
        assert_eq!(moonlit.ambient, moonless.ambient);
        assert!(moonlit.colour.z >= moonlit.colour.x, "{:?}", moonlit.colour);
        let disc = moonlit.moon.unwrap();
        assert_eq!((disc.illuminated, disc.opacity), (1.0, 1.0));
        // The moon lights from the south, the sun is below the north horizon
        let towards_moon = towards(&grid, 180.0, 30.0);
        assert!(
            moonlit.direction.dot(towards_moon) > 0.95,
            "{:?}",
            moonlit.direction
        );
        assert!((disc.direction - towards_moon).length() < 1e-5);
        assert!((disc.sun_direction - towards(&grid, 0.0, -25.0)).length() < 1e-5);

        // A thin crescent hardly lights anything, and a set moon nothing
        let crescent = SunState::with_moon(
            &grid,
            0.0,
            -25.0,
            Some(Moon {
                illuminated: 0.1,
                ..full_moon(30.0).unwrap()
            }),
        );
        assert!(brightness(&crescent) < brightness(&moonless) * 1.02);
        assert!(crescent.moon.is_some());
        let set = SunState::with_moon(&grid, 0.0, -25.0, full_moon(-5.0));
        assert_eq!(set, moonless);
    }

    #[test]
    fn the_sky_between_samples_is_interpolated() {
        let mut bundle = bundle(grid(2, 2, 10.0), &[0.0; 4], &[Landcover::Grassland; 4]);
        let grid = bundle.grid;
        let sample = |time, azimuth_deg, elevation_deg| SunSample {
            time,
            azimuth_deg,
            elevation_deg,
        };
        bundle.sun = vec![sample(1_000, 350.0, -20.0), sample(1_300, 10.0, -16.0)];
        bundle.moon = vec![
            moon_sample(1_000, 170.0, 20.0, 0.5),
            moon_sample(1_300, 174.0, 22.0, 0.52),
        ];
        let moon = |azimuth_deg, elevation_deg, illuminated| {
            Some(Moon {
                azimuth_deg,
                elevation_deg,
                illuminated,
            })
        };
        // The bearing turns the short way through north
        let middle = SunState::during(&bundle, 1_150.0);
        let expected = SunState::with_moon(&grid, 0.0, -18.0, moon(172.0, 21.0, 0.51));
        assert!((middle.direction - expected.direction).length() < 1e-4);
        assert!(
            (middle.moon.unwrap().direction - expected.moon.unwrap().direction).length() < 1e-4
        );
        assert!((middle.moon.unwrap().illuminated - 0.51).abs() < 1e-4);
        assert!((middle.intensity - expected.intensity).abs() < 1e-4);
        // Outside the track the ends hold
        assert_eq!(
            SunState::during(&bundle, 0.0),
            SunState::with_moon(&grid, 350.0, -20.0, moon(170.0, 20.0, 0.5))
        );
        assert_eq!(
            SunState::during(&bundle, 9_999.0),
            SunState::during(&bundle, 1_300.0)
        );
        bundle.sun.clear();
        bundle.moon.clear();
        assert_eq!(
            SunState::during(&bundle, 1_150.0),
            SunState::late_morning(&grid)
        );
    }

    fn weather(cloud_cover: f32, overcast: f32) -> Option<Atmosphere> {
        Some(Atmosphere {
            cloud_cover,
            solid: 1.0,
            overcast,
            haze: 0.0,
            precipitation: isotrail_bundle::Precipitation::None,
            density: 0.0,
            wind: glam::Vec2::ZERO,
            shown: WeatherEffects::ALL,
        })
    }

    #[test]
    fn cloud_dims_the_light_and_greys_the_sky() {
        let grid = grid(10, 10, 10.0);
        let day = SunState::at(&grid, 150.0, 40.0);
        assert_eq!(day.with_weather(None), day);
        let suns: Vec<SunState> = (0..=4)
            .map(|i| day.with_weather(weather(i as f32 / 4.0, i as f32 / 4.0)))
            .collect();
        for pair in suns.windows(2) {
            assert!(brightness(&pair[1]) < brightness(&pair[0]));
            assert!(sky_luminance(&pair[1]) <= sky_luminance(&pair[0]) + 1.0);
        }
        let blue = |sun: &SunState| f32::from(sun.sky[0][2]) - f32::from(sun.sky[0][0]);
        assert!(blue(&suns[4]) < blue(&day) * 0.3);
        assert_eq!(suns[2].direction, day.direction);
    }

    #[test]
    fn cloud_hides_the_stars_and_the_moon() {
        let grid = grid(10, 10, 10.0);
        let night = SunState::with_moon(&grid, 0.0, -25.0, full_moon(30.0));
        let broken = night.with_weather(weather(0.5, 0.0));
        assert_eq!(broken.stars, 0.5);
        assert_eq!(broken.moon.unwrap().opacity, 0.5);
        let covered = night.with_weather(weather(1.0, 1.0));
        assert_eq!((covered.stars, covered.moon), (0.0, None));
    }

    #[test]
    fn the_bundle_weather_follows_the_clock() {
        let mut bundle = bundle(grid(2, 2, 10.0), &[0.0; 4], &[Landcover::Grassland; 4]);
        bundle.sun = vec![SunSample {
            time: 3_600,
            azimuth_deg: 150.0,
            elevation_deg: 40.0,
        }];
        assert_eq!(SunState::for_hike(&bundle).weather, None);
        let mut weather = isotrail_testkit::synthetic_weather(0, 3);
        weather.samples[1].cloud_cover_pct = 100.0;
        weather.samples[2].cloud_cover_pct = 0.0;
        bundle.weather = Some(weather);
        let cover = |sun: SunState| sun.weather.unwrap().cloud_cover;
        assert_eq!(cover(SunState::for_hike(&bundle)), 1.0);
        assert_eq!(cover(SunState::during(&bundle, 5_400.0)), 0.5);
        bundle.sun.clear();
        assert_eq!(SunState::during(&bundle, 5_400.0).weather, None);
    }

    #[test]
    fn conditions_set_the_sun_and_the_sky_by_hand() {
        let mut bundle = bundle(grid(2, 2, 10.0), &[0.0; 4], &[Landcover::Grassland; 4]);
        bundle.sun = vec![SunSample {
            time: 3_600,
            azimuth_deg: 150.0,
            elevation_deg: 40.0,
        }];
        let mut weather = isotrail_testkit::synthetic_weather(0, 3);
        weather.samples[1].cloud_cover_pct = 100.0;
        weather.samples[1].cloud_low_pct = 100.0;
        bundle.weather = Some(weather);
        let seen = |when, conditions: Conditions| SunState::seen(&bundle, when, &conditions);
        let usual = Conditions::default();
        assert_eq!(seen(When::Start, usual), SunState::for_hike(&bundle));
        assert_eq!(
            seen(When::During(4_000.0), usual),
            SunState::during(&bundle, 4_000.0)
        );

        let low_sun = Conditions {
            sun: Some([270.0, 5.0]),
            ..usual
        };
        let expected = SunState::at(&bundle.grid, 270.0, 5.0)
            .with_weather(SunState::for_hike(&bundle).weather);
        assert_eq!(seen(When::Start, low_sun), expected);

        let clear = Conditions {
            weather: Some(ForcedWeather::Clear),
            ..usual
        };
        let cleared = seen(When::Start, clear);
        assert_eq!(cleared.weather.unwrap().cloud_cover, 0.0);
        assert!(brightness(&cleared) > brightness(&seen(When::Start, usual)));

        let unclouded = Conditions {
            effects: WeatherEffects::NONE,
            ..usual
        };
        let plain = SunState::at(&bundle.grid, 150.0, 40.0);
        let shown = seen(When::Start, unclouded);
        assert_eq!(
            (shown.intensity, shown.colour, shown.sky),
            (plain.intensity, plain.colour, plain.sky)
        );
        assert!(!shown.weather.unwrap().moves());
    }

    #[test]
    fn the_shadow_projection_holds_every_point() {
        let points: Vec<Vec3> = (0..27)
            .map(|i| Vec3::new((i % 3) as f32, (i / 3 % 3) as f32, (i / 9) as f32) * 500.0 - 400.0)
            .collect();
        for direction in [Vec3::new(1.0, -2.0, 1.5), Vec3::Z] {
            let sun = SunState {
                direction: direction.normalize(),
                ..SunState::late_morning(&grid(10, 10, 10.0))
            };
            let view_proj = sun.shadow_view_proj(points.iter().copied());
            for &p in &points {
                let clip = view_proj.project_point3(p);
                assert!(clip.x.abs() < 1.0 && clip.y.abs() < 1.0, "{clip}");
                assert!(clip.z > 0.0 && clip.z < 1.0, "{clip}");
            }
        }
    }
}
