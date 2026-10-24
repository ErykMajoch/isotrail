use glam::camera::rh::{proj, view};
use glam::{Mat4, Vec3};
use isotrail_bundle::GridInfo;

use crate::camera::slab_angle;

/// A late-morning sun, until lighting follows the hike's real time
pub const SUN_AZIMUTH_DEG: f64 = 150.0;
pub const SUN_ELEVATION_DEG: f64 = 40.0;
pub const SUN_COLOUR: Vec3 = Vec3::new(1.0, 0.96, 0.88);
pub const AMBIENT: f32 = 0.4;
/// Extra ambient, as a share of it, for faces turned towards the camera, so
/// sides facing away from the sun still read
pub const FILL: f32 = 0.6;
/// Steps the direct light is quantised to
pub const BANDS: u32 = 4;

/// The one directional light
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SunState {
    /// Unit vector towards the sun, in the slab frame
    pub direction: Vec3,
    pub colour: Vec3,
    pub intensity: f32,
    pub ambient: f32,
}

impl SunState {
    pub fn late_morning(grid: &GridInfo) -> SunState {
        let azimuth = slab_angle(grid, SUN_AZIMUTH_DEG);
        let elevation = SUN_ELEVATION_DEG.to_radians() as f32;
        SunState {
            direction: Vec3::new(
                azimuth.cos() * elevation.cos(),
                azimuth.sin() * elevation.cos(),
                elevation.sin(),
            ),
            colour: SUN_COLOUR,
            intensity: 1.0,
            ambient: AMBIENT,
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

#[cfg(test)]
mod tests {
    use isotrail_testkit::grid;

    use super::*;

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
