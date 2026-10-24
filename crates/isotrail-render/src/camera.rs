use glam::camera::rh::{proj, view};
use glam::{Mat4, Vec3};
use isotrail_bundle::{GridInfo, SceneBundle};

use crate::mesh::slab_position;

pub const PITCH_DEG: f32 = 30.0;
/// How far the camera turns from the hike's heading, so the trail recedes
/// up and to the right
pub const DEFAULT_YAW_OFFSET_DEG: f32 = 20.0;
/// A hike that ends closer to its start than this share of its furthest
/// reach counts as a loop
const LOOP_SHARE: f32 = 0.25;

/// Shares of the frame, per side, that the diorama must stay out of
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Insets {
    pub top: f32,
    pub bottom: f32,
    pub left: f32,
    pub right: f32,
}

/// Internal frame size in pixels and its safe area
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Framing {
    pub width: u32,
    pub height: u32,
    pub safe: Insets,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Aspect {
    /// 9:16, for Instagram stories
    #[default]
    Story,
}

impl Aspect {
    pub fn output_size(self) -> (u32, u32) {
        match self {
            Aspect::Story => (1080, 1920),
        }
    }

    /// Story keeps clear of Instagram's header and reply bar
    pub fn safe_area(self) -> Insets {
        match self {
            Aspect::Story => Insets {
                top: 0.14,
                bottom: 0.20,
                left: 0.05,
                right: 0.05,
            },
        }
    }

    /// `None` if `scale` doesn't divide the output size
    pub fn framing(self, scale: u32) -> Option<Framing> {
        let (width, height) = self.output_size();
        (scale > 0 && width % scale == 0 && height % scale == 0).then(|| Framing {
            width: width / scale,
            height: height / scale,
            safe: self.safe_area(),
        })
    }
}

/// An orthographic camera over the slab frame the meshers use
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    pub view_proj: Mat4,
    /// Unit vector the camera looks along
    pub forward: Vec3,
}

impl Camera {
    /// Looks along slab-frame angle `heading`, [`PITCH_DEG`] down, scaled and
    /// centred so that every point fits the framing's safe area
    pub fn diorama(
        heading: f32,
        points: impl IntoIterator<Item = Vec3>,
        framing: &Framing,
    ) -> Camera {
        let pitch = PITCH_DEG.to_radians();
        let forward = Vec3::new(
            heading.cos() * pitch.cos(),
            heading.sin() * pitch.cos(),
            -pitch.sin(),
        );
        let view = view::look_to_mat4(Vec3::ZERO, forward, Vec3::Z);

        let (mut min, mut max) = (Vec3::INFINITY, Vec3::NEG_INFINITY);
        for point in points {
            let p = view.transform_point3(point);
            (min, max) = (min.min(p), max.max(p));
        }
        if min.x > max.x {
            (min, max) = (Vec3::splat(-1.0), Vec3::ONE);
        }

        let (width, height) = (framing.width as f32, framing.height as f32);
        let safe = framing.safe;
        let safe_width = width * (1.0 - safe.left - safe.right);
        let safe_height = height * (1.0 - safe.top - safe.bottom);
        let extent = max - min;
        let per_pixel = (extent.x / safe_width)
            .max(extent.y / safe_height)
            .max(f32::EPSILON);

        // Pixel position of the safe area's centre, from the top-left corner
        let centre_x = width * (safe.left + (1.0 - safe.left - safe.right) / 2.0);
        let centre_y = height * (safe.top + (1.0 - safe.top - safe.bottom) / 2.0);
        let left = (min.x + max.x) / 2.0 - centre_x * per_pixel;
        let top = (min.y + max.y) / 2.0 + centre_y * per_pixel;
        let depth_pad = extent.z * 0.01 + 1.0;
        // wgpu's clip space has y up and depth in [0, 1], like Direct3D
        let projection = proj::directx::orthographic(
            left,
            left + width * per_pixel,
            top - height * per_pixel,
            top,
            -max.z - depth_pad,
            -min.z + depth_pad,
        );
        Camera {
            view_proj: projection * view,
            forward,
        }
    }
}

/// Slab-frame angle, counter-clockwise from +x, that the hike heads in: from
/// start to finish, or from the start to the halfway point for loops and
/// out-and-backs
pub fn route_heading(bundle: &SceneBundle) -> Option<f32> {
    let grid = &bundle.grid;
    let points = &bundle.route.points;
    let position = |i: usize| slab_position(grid, points[i].col.into(), points[i].row.into());
    let (first, last) = (points.first()?, points.last()?);
    let start = position(0);
    let reach = (0..points.len())
        .map(|i| position(i).distance(start))
        .fold(0.0, f32::max);
    let mut direction = position(points.len() - 1) - start;
    if direction.length() < LOOP_SHARE * reach {
        let halfway = (first.distance_m + last.distance_m) / 2.0;
        let i = points.iter().position(|p| p.distance_m >= halfway)?;
        direction = position(i) - start;
    }
    (direction.length() > grid.cell_size_m as f32 / 2.0).then(|| direction.y.atan2(direction.x))
}

/// Slab-frame angle the camera looks along: the hike's heading turned by
/// `offset_deg`, or the long side on a 2:1 diagonal for bundles without a route
pub fn view_heading(bundle: &SceneBundle, offset_deg: f32) -> f32 {
    let grid = &bundle.grid;
    route_heading(bundle).map_or_else(
        || slab_angle(grid, grid.camera_yaw_deg()),
        |heading| heading + offset_deg.to_radians(),
    )
}

/// Angle in the slab frame, counter-clockwise from +x, of compass bearing `bearing_deg`
pub(crate) fn slab_angle(grid: &GridInfo, bearing_deg: f64) -> f32 {
    ((90.0 - bearing_deg).to_radians() - grid.rotation_rad) as f32
}

#[cfg(test)]
mod tests {
    use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, TAU};

    use isotrail_bundle::{Landcover, Route, RoutePoint};
    use isotrail_testkit::{bundle, grid};

    use super::*;

    fn slab_corners(grid: &GridInfo, top: f32) -> Vec<Vec3> {
        let (u, v) = (grid.length_m() as f32 / 2.0, grid.width_m() as f32 / 2.0);
        let mut corners = Vec::new();
        for z in [0.0, top] {
            for (x, y) in [(-u, -v), (u, -v), (u, v), (-u, v)] {
                corners.push(Vec3::new(x, y, z));
            }
        }
        corners
    }

    /// Pixel position, from the top-left, and depth
    fn to_pixels(camera: &Camera, framing: &Framing, point: Vec3) -> Vec3 {
        let ndc = camera.view_proj.project_point3(point);
        Vec3::new(
            (ndc.x + 1.0) / 2.0 * framing.width as f32,
            (1.0 - ndc.y) / 2.0 * framing.height as f32,
            ndc.z,
        )
    }

    /// A flat 20 × 20 slab of 10 m cells with a route through the given
    /// (col, row) positions
    fn with_route(cells: &[(f32, f32)]) -> SceneBundle {
        let mut bundle = bundle(
            grid(20, 20, 10.0),
            &[0.0; 400],
            &[Landcover::Grassland; 400],
        );
        let mut distance_m = 0.0;
        let mut previous = cells.first().copied().unwrap_or_default();
        bundle.route = Route {
            points: cells
                .iter()
                .map(|&(col, row)| {
                    distance_m += (col - previous.0).hypot(row - previous.1) * 10.0;
                    previous = (col, row);
                    RoutePoint {
                        col,
                        row,
                        distance_m,
                        time: None,
                        gps_ele_m: None,
                        draped_m: 0.0,
                    }
                })
                .collect(),
            breaks: Vec::new(),
        };
        bundle
    }

    #[track_caller]
    fn assert_angle(actual: Option<f32>, expected: f32) {
        let actual = actual.expect("a heading");
        let diff = (actual - expected).rem_euclid(TAU);
        assert!(diff.min(TAU - diff) < 1e-4, "{actual} vs {expected}");
    }

    #[test]
    fn story_framing_divides_by_the_scale() {
        let framing = Aspect::Story.framing(3).unwrap();
        assert_eq!((framing.width, framing.height), (360, 640));
        assert_eq!(Aspect::Story.framing(6).unwrap().width, 180);
        assert_eq!(Aspect::Story.framing(7), None);
        assert_eq!(Aspect::Story.framing(0), None);
    }

    #[test]
    fn slab_fills_the_safe_area_width() {
        let grid = grid(200, 100, 30.0);
        let framing = Aspect::Story.framing(3).unwrap();
        let points = slab_corners(&grid, 800.0);
        let camera = Camera::diorama(0.4, points.clone(), &framing);
        let pixels: Vec<Vec3> = points
            .iter()
            .map(|&p| to_pixels(&camera, &framing, p))
            .collect();

        let safe = framing.safe;
        let (w, h) = (framing.width as f32, framing.height as f32);
        for p in &pixels {
            assert!(p.x >= w * safe.left - 0.01 && p.x <= w * (1.0 - safe.right) + 0.01);
            assert!(p.y >= h * safe.top - 0.01 && p.y <= h * (1.0 - safe.bottom) + 0.01);
            assert!(p.z > 0.0 && p.z < 1.0, "depth {}", p.z);
        }
        let min_x = pixels.iter().map(|p| p.x).fold(f32::INFINITY, f32::min);
        let max_x = pixels.iter().map(|p| p.x).fold(0.0, f32::max);
        assert!((min_x - w * safe.left).abs() < 0.01, "{min_x}");
        assert!((max_x - w * (1.0 - safe.right)).abs() < 0.01, "{max_x}");
    }

    #[test]
    fn straight_hikes_head_from_start_to_finish() {
        // Rows grow towards -y
        let bundle = with_route(&[(2.0, 18.0), (6.0, 15.0), (10.0, 12.0)]);
        assert_angle(route_heading(&bundle), 3f32.atan2(4.0));
        let back = with_route(&[(10.0, 12.0), (2.0, 18.0)]);
        assert_angle(route_heading(&back), (-3f32).atan2(-4.0));
    }

    #[test]
    fn loops_head_for_their_halfway_point() {
        let out_and_back = with_route(&[(2.0, 10.0), (18.0, 10.0), (2.5, 10.0)]);
        assert_angle(route_heading(&out_and_back), 0.0);
        let square = with_route(&[
            (2.0, 2.0),
            (18.0, 2.0),
            (18.0, 18.0),
            (2.0, 18.0),
            (2.0, 2.0),
        ]);
        assert_angle(route_heading(&square), -FRAC_PI_4);
    }

    #[test]
    fn tiny_or_missing_routes_have_no_heading() {
        assert_eq!(route_heading(&with_route(&[])), None);
        assert_eq!(route_heading(&with_route(&[(5.0, 5.0)])), None);
        assert_eq!(route_heading(&with_route(&[(5.0, 5.0), (5.2, 5.1)])), None);
    }

    #[test]
    fn hikes_recede_up_and_to_the_right() {
        // Heading towards +y
        let bundle = with_route(&[(10.0, 18.0), (10.0, 2.0)]);
        let heading = view_heading(&bundle, 30.0);
        assert_angle(Some(heading), FRAC_PI_2 + 30f32.to_radians());
        let framing = Aspect::Story.framing(3).unwrap();
        let camera = Camera::diorama(heading, slab_corners(&bundle.grid, 0.0), &framing);
        let pixel = |p: Vec3| to_pixels(&camera, &framing, p);
        let along = pixel(Vec3::Y * 100.0) - pixel(Vec3::ZERO);
        assert!(along.x > 0.0 && along.y < 0.0, "{along}");
    }

    #[test]
    fn routeless_bundles_put_the_long_side_on_a_two_to_one_diagonal() {
        let bundle = bundle(grid(64, 32, 10.0), &[0.0; 2048], &[Landcover::Rock; 2048]);
        let heading = view_heading(&bundle, 30.0);
        assert_angle(Some(heading), FRAC_PI_4);
        let framing = Aspect::Story.framing(3).unwrap();
        let camera = Camera::diorama(heading, slab_corners(&bundle.grid, 0.0), &framing);
        let pixel = |p: Vec3| to_pixels(&camera, &framing, p);

        // The far end of the long side is up and to the right
        let along = pixel(Vec3::X * 100.0) - pixel(Vec3::ZERO);
        assert!(along.x > 0.0 && along.y < 0.0, "{along}");
        assert!((along.x / -along.y - 2.0).abs() < 1e-3, "{along}");
        // The short side runs up and to the left, also at 2:1
        let across = pixel(Vec3::Y * 100.0) - pixel(Vec3::ZERO);
        assert!(across.x < 0.0 && across.y < 0.0, "{across}");
        assert!((across.x / across.y - 2.0).abs() < 1e-3, "{across}");
        // Up is up
        let up = pixel(Vec3::Z * 100.0) - pixel(Vec3::ZERO);
        assert!(up.x.abs() < 1e-3 && up.y < 0.0);
    }

    #[test]
    fn compass_bearings_turn_with_the_slab() {
        let grid = GridInfo {
            rotation_rad: 0.3,
            ..grid(10, 10, 10.0)
        };
        assert_angle(Some(slab_angle(&grid, 0.0)), FRAC_PI_2 - 0.3);
        assert_angle(Some(slab_angle(&grid, 90.0)), -0.3);
    }
}
