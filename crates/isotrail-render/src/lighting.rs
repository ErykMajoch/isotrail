use glam::camera::rh::{proj, view};
use glam::{Mat4, Vec3};
use isotrail_bundle::{GridInfo, SceneBundle};

use crate::camera::slab_angle;
use crate::{SKY_BOTTOM, SKY_TOP};

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
        intensity: 0.25,
        ambient: 0.22,
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
}

impl SunState {
    pub fn late_morning(grid: &GridInfo) -> SunState {
        SunState::at(grid, SUN_AZIMUTH_DEG, SUN_ELEVATION_DEG)
    }

    /// The light of a sun standing at this compass bearing and elevation
    pub fn at(grid: &GridInfo, azimuth_deg: f64, elevation_deg: f64) -> SunState {
        let azimuth = slab_angle(grid, azimuth_deg);
        let light = elevation_deg.max(MIN_LIGHT_ELEVATION_DEG).to_radians() as f32;
        let key = blend(elevation_deg as f32);
        SunState {
            direction: Vec3::new(
                azimuth.cos() * light.cos(),
                azimuth.sin() * light.cos(),
                light.sin(),
            ),
            colour: key.colour,
            intensity: key.intensity,
            ambient: key.ambient,
            ambient_colour: key.ambient_colour,
            sky: key.sky,
        }
    }

    /// The sun at the start of the hike, or the late-morning one when the
    /// track has no times
    pub fn for_hike(bundle: &SceneBundle) -> SunState {
        match bundle.sun.first() {
            Some(sun) => SunState::at(
                &bundle.grid,
                sun.azimuth_deg.into(),
                sun.elevation_deg.into(),
            ),
            None => SunState::late_morning(&bundle.grid),
        }
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
    use isotrail_bundle::{Landcover, SunSample};
    use isotrail_testkit::{bundle, grid};

    use super::*;

    fn luminance(rgb: Vec3) -> f32 {
        rgb.dot(Vec3::new(0.2126, 0.7152, 0.0722))
    }

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
