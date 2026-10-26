use glam::camera::rh::{proj, view};
use glam::{Mat4, Vec2, Vec3};
use isotrail_bundle::{GridInfo, SceneBundle};

use crate::mesh::slab_position;

pub const PITCH_DEG: f32 = 30.0;
/// How far the camera turns from the hike's heading, so the trail recedes
/// up and to the right
pub const DEFAULT_YAW_OFFSET_DEG: f32 = 20.0;
/// A hike that ends closer to its start than this share of its furthest
/// reach counts as a loop
const LOOP_SHARE: f32 = 0.25;
pub const MAX_ZOOM: f32 = 3.0;

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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Aspect {
    /// 9:16, for Instagram stories
    #[default]
    Story,
    /// 4:5, for feed posts
    Feed,
    Square,
    /// 16:9, for websites
    Wide,
}

impl Aspect {
    pub const ALL: [Aspect; 4] = [Aspect::Story, Aspect::Feed, Aspect::Square, Aspect::Wide];

    pub fn name(self) -> &'static str {
        match self {
            Aspect::Story => "story",
            Aspect::Feed => "feed",
            Aspect::Square => "square",
            Aspect::Wide => "wide",
        }
    }

    pub fn output_size(self) -> (u32, u32) {
        match self {
            Aspect::Story => (1080, 1920),
            Aspect::Feed => (1080, 1350),
            Aspect::Square => (1080, 1080),
            Aspect::Wide => (1920, 1080),
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
            Aspect::Feed | Aspect::Square | Aspect::Wide => Insets {
                top: 0.05,
                bottom: 0.05,
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
    /// Size of an internal pixel on a plane facing the camera
    pub metres_per_px: f32,
}

impl Camera {
    /// Looks along slab-frame angle `heading`, [`PITCH_DEG`] down, scaled and
    /// centred so that every point fits the framing's safe area
    pub fn diorama(
        heading: f32,
        points: impl IntoIterator<Item = Vec3>,
        framing: &Framing,
    ) -> Camera {
        let fit = Fit::new(heading, points);
        let per_pixel = fit.scale(framing);
        fit.camera(framing, per_pixel, Vec2::ZERO, false)
    }

    /// Like [`Camera::diorama`] at a set `metres_per_px`, as an animation
    /// keeps the same scale while it turns, and moved to whole internal
    /// pixels so that the terrain doesn't shimmer
    pub fn at_scale(
        heading: f32,
        points: impl IntoIterator<Item = Vec3>,
        framing: &Framing,
        metres_per_px: f32,
    ) -> Camera {
        Fit::new(heading, points).camera(framing, metres_per_px.max(f32::EPSILON), Vec2::ZERO, true)
    }

    /// The `metres_per_px` at which `points` fit the framing's safe area
    /// from every one of `headings`
    pub fn fit_scale(headings: &[f32], points: &[Vec3], framing: &Framing) -> f32 {
        headings
            .iter()
            .map(|&heading| Fit::new(heading, points.iter().copied()).scale(framing))
            .fold(f32::EPSILON, f32::max)
    }
}

/// Points seen from a heading, in view space
struct Fit {
    view: Mat4,
    forward: Vec3,
    min: Vec3,
    max: Vec3,
}

impl Fit {
    fn new(heading: f32, points: impl IntoIterator<Item = Vec3>) -> Fit {
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
        Fit {
            view,
            forward,
            min,
            max,
        }
    }

    fn scale(&self, framing: &Framing) -> f32 {
        let (width, height) = (framing.width as f32, framing.height as f32);
        let safe = framing.safe;
        let safe_width = width * (1.0 - safe.left - safe.right);
        let safe_height = height * (1.0 - safe.top - safe.bottom);
        let extent = self.max - self.min;
        (extent.x / safe_width)
            .max(extent.y / safe_height)
            .max(f32::EPSILON)
    }

    /// `shift` moves the view-space point shown at the safe area's centre
    /// away from the middle of the points
    fn camera(&self, framing: &Framing, per_pixel: f32, shift: Vec2, snap: bool) -> Camera {
        let (min, max) = (self.min, self.max);
        let (width, height) = (framing.width as f32, framing.height as f32);
        let extent = max - min;

        let centre = safe_centre(framing);
        let mut left = (min.x + max.x) / 2.0 + shift.x - centre.x * per_pixel;
        let mut top = (min.y + max.y) / 2.0 + shift.y + centre.y * per_pixel;
        if snap {
            left = (left / per_pixel).round() * per_pixel;
            top = (top / per_pixel).round() * per_pixel;
        }
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
            view_proj: projection * self.view,
            forward: self.forward,
            metres_per_px: per_pixel,
        }
    }

    /// Where a ground offset in the slab frame moves on screen, in view-space
    /// metres
    fn to_screen(&self, ground: Vec2) -> Vec2 {
        self.view.transform_vector3(ground.extend(0.0)).truncate()
    }

    fn to_ground(&self, screen: Vec2) -> Vec2 {
        let along = self.forward.truncate().normalize_or(Vec2::X);
        let right = Vec2::new(along.y, -along.x);
        right * screen.x + along * (screen.y / PITCH_DEG.to_radians().sin())
    }
}

/// Pixel position of the safe area's centre, from the top-left corner
fn safe_centre(framing: &Framing) -> Vec2 {
    let safe = framing.safe;
    Vec2::new(
        framing.width as f32 * (safe.left + (1.0 - safe.left - safe.right) / 2.0),
        framing.height as f32 * (safe.top + (1.0 - safe.top - safe.bottom) / 2.0),
    )
}

/// How a viewer has turned, magnified and moved the diorama
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    /// Added to the still's camera turn
    pub yaw_offset_deg: f32,
    /// 1 shows the whole slab, up to [`MAX_ZOOM`]
    pub zoom: f32,
    /// Slab-frame metres across the ground from the middle of the whole-slab
    /// view to the middle of this one, so it turns with the slab
    pub focus: Vec2,
}

impl Default for View {
    fn default() -> Self {
        View::turned(0.0)
    }
}

impl View {
    pub fn turned(yaw_offset_deg: f32) -> View {
        View {
            yaw_offset_deg,
            zoom: 1.0,
            focus: Vec2::ZERO,
        }
    }
}

/// A slab seen at one scale from any heading, as a viewer turns, zooms and
/// pans it without ever seeing past the whole-slab view
#[derive(Clone, Debug)]
pub struct Turntable {
    heading: f32,
    points: Vec<Vec3>,
    framing: Framing,
    metres_per_px: f32,
}

impl Turntable {
    /// `heading` is the still's, and the scale fits `points` from every turn
    /// of `yaw_offsets_deg` from it
    pub fn new(
        heading: f32,
        points: Vec<Vec3>,
        framing: &Framing,
        yaw_offsets_deg: &[f32],
    ) -> Turntable {
        let mut headings: Vec<f32> = yaw_offsets_deg
            .iter()
            .map(|offset| heading + offset.to_radians())
            .collect();
        headings.dedup();
        let metres_per_px = Camera::fit_scale(&headings, &points, framing);
        Turntable {
            heading,
            points,
            framing: *framing,
            metres_per_px,
        }
    }

    /// At 1×
    pub fn metres_per_px(&self) -> f32 {
        self.metres_per_px
    }

    /// Moved to whole internal pixels at the view's zoom, like
    /// [`Camera::at_scale`]
    pub fn camera(&self, view: &View) -> Camera {
        let fit = self.fit(view);
        fit.camera(
            &self.framing,
            self.metres_per_px / view.zoom.max(1.0),
            fit.to_screen(view.focus),
            true,
        )
    }

    /// Magnified `factor` times, keeping the ground under internal pixel
    /// `pixel` (from the top-left) where it is
    pub fn zoom_at(&self, view: View, factor: f32, pixel: Vec2) -> View {
        let zoom = (view.zoom * factor).clamp(1.0, MAX_ZOOM);
        let fit = self.fit(&view);
        let (before, after) = (
            self.metres_per_px / view.zoom.max(1.0),
            self.metres_per_px / zoom,
        );
        let centre = safe_centre(&self.framing);
        let towards = Vec2::new(pixel.x - centre.x, centre.y - pixel.y);
        let shift = fit.to_screen(view.focus) + towards * (before - after);
        self.clamp(View {
            zoom,
            focus: fit.to_ground(shift),
            ..view
        })
    }

    /// Dragged `pixels` internal pixels, right and down
    pub fn pan(&self, view: View, pixels: Vec2) -> View {
        let fit = self.fit(&view);
        let per_pixel = self.metres_per_px / view.zoom.max(1.0);
        let shift = fit.to_screen(view.focus) + Vec2::new(-pixels.x, pixels.y) * per_pixel;
        self.clamp(View {
            focus: fit.to_ground(shift),
            ..view
        })
    }

    /// Moved back inside the whole-slab view, as a turn can push it out
    pub fn clamp(&self, view: View) -> View {
        let zoom = view.zoom.clamp(1.0, MAX_ZOOM);
        let fit = self.fit(&view);
        let centre = safe_centre(&self.framing);
        let (width, height) = (self.framing.width as f32, self.framing.height as f32);
        let room = self.metres_per_px * (1.0 - 1.0 / zoom);
        let shift = fit.to_screen(view.focus);
        let shift = Vec2::new(
            shift.x.clamp(-centre.x * room, (width - centre.x) * room),
            shift.y.clamp(-(height - centre.y) * room, centre.y * room),
        );
        View {
            zoom,
            focus: fit.to_ground(shift),
            ..view
        }
    }

    fn fit(&self, view: &View) -> Fit {
        Fit::new(
            self.heading + view.yaw_offset_deg.to_radians(),
            self.points.iter().copied(),
        )
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
    fn every_preset_divides_by_every_pixel_scale() {
        for aspect in Aspect::ALL {
            for scale in [2, 3, 5, 6, 10] {
                let framing = aspect.framing(scale).unwrap();
                let (width, height) = aspect.output_size();
                assert_eq!(
                    (framing.width * scale, framing.height * scale),
                    (width, height)
                );
            }
        }
        assert_eq!(Aspect::Wide.framing(3).unwrap().width, 640);
        assert_eq!(Aspect::Feed.framing(3).unwrap().height, 450);
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
    fn one_scale_fits_every_heading() {
        let grid = grid(200, 100, 30.0);
        let framing = Aspect::Story.framing(3).unwrap();
        let points = slab_corners(&grid, 800.0);
        let headings = [0.2, 0.5, 0.9];
        let scale = Camera::fit_scale(&headings, &points, &framing);
        let fits: Vec<f32> = headings
            .iter()
            .map(|&h| Camera::diorama(h, points.clone(), &framing).metres_per_px)
            .collect();
        assert!(fits.iter().all(|&fit| fit <= scale));
        assert!(fits.contains(&scale));
    }

    #[test]
    fn fixed_scale_cameras_sit_on_whole_pixels() {
        let grid = grid(200, 100, 30.0);
        let framing = Aspect::Story.framing(3).unwrap();
        let points = slab_corners(&grid, 800.0);
        for heading in [0.1, 0.37, 1.3] {
            let camera = Camera::at_scale(heading, points.clone(), &framing, 9.3);
            assert_eq!(camera.metres_per_px, 9.3);
            let origin = to_pixels(&camera, &framing, Vec3::ZERO);
            assert!((origin.x - origin.x.round()).abs() < 1e-3, "{origin}");
            assert!((origin.y - origin.y.round()).abs() < 1e-3, "{origin}");
            // Still centred on the safe area, to within a pixel
            let pixels: Vec<Vec3> = points
                .iter()
                .map(|&p| to_pixels(&camera, &framing, p))
                .collect();
            let (min, max) = pixels
                .iter()
                .fold((Vec3::INFINITY, Vec3::NEG_INFINITY), |(min, max), &p| {
                    (min.min(p), max.max(p))
                });
            let centre = (min + max) / 2.0;
            let safe = framing.safe;
            let (w, h) = (framing.width as f32, framing.height as f32);
            let safe_x = w * (safe.left + (1.0 - safe.left - safe.right) / 2.0);
            let safe_y = h * (safe.top + (1.0 - safe.top - safe.bottom) / 2.0);
            assert!((centre.x - safe_x).abs() <= 0.5 + 1e-3, "{centre}");
            assert!((centre.y - safe_y).abs() <= 0.5 + 1e-3, "{centre}");
        }
    }

    fn turntable() -> (Turntable, Framing) {
        let framing = Aspect::Wide.framing(3).unwrap();
        let points = slab_corners(&grid(200, 100, 30.0), 800.0);
        let turn: Vec<f32> = (0..360).step_by(5).map(|d| d as f32).collect();
        (Turntable::new(0.4, points, &framing, &turn), framing)
    }

    /// Where the window shown at `view` sits in the whole-slab view, in its
    /// pixels, from a point seen in both
    fn window_corner(turntable: &Turntable, framing: &Framing, view: &View) -> Vec2 {
        let whole = turntable.camera(&View::turned(view.yaw_offset_deg));
        let zoomed = turntable.camera(view);
        let point = Vec3::new(120.0, -40.0, 300.0);
        let (a, b) = (
            to_pixels(&whole, framing, point),
            to_pixels(&zoomed, framing, point),
        );
        a.truncate() - b.truncate() / view.zoom
    }

    #[test]
    fn unzoomed_turntables_match_fixed_scale_cameras() {
        let (turntable, framing) = turntable();
        for yaw in [0.0, 37.0, -120.0] {
            let camera = Camera::at_scale(
                0.4 + f32::to_radians(yaw),
                turntable.points.iter().copied(),
                &framing,
                turntable.metres_per_px(),
            );
            assert_eq!(turntable.camera(&View::turned(yaw)), camera);
        }
    }

    #[test]
    fn zooming_keeps_the_ground_under_the_pointer() {
        let (turntable, framing) = turntable();
        let point = Vec3::new(500.0, 300.0, 0.0);
        let mut view = View::turned(15.0);
        for factor in [1.5, 1.6, 0.8] {
            let pixel = to_pixels(&turntable.camera(&view), &framing, point).truncate();
            view = turntable.zoom_at(view, factor, pixel);
            let after = to_pixels(&turntable.camera(&view), &framing, point).truncate();
            assert!(pixel.distance(after) <= 0.75, "{pixel} vs {after}");
        }
        assert!((view.zoom - 1.92).abs() < 1e-4, "{}", view.zoom);
    }

    #[test]
    fn zoom_stays_between_the_whole_slab_and_its_limit() {
        let (turntable, _) = turntable();
        let centre = Vec2::new(320.0, 180.0);
        let close = turntable.zoom_at(View::default(), 10.0, centre);
        assert_eq!(close.zoom, MAX_ZOOM);
        let whole = turntable.zoom_at(close, 0.01, Vec2::new(10.0, 10.0));
        assert_eq!(whole.zoom, 1.0);
        assert!(whole.focus.length() < 1e-3, "{}", whole.focus);
    }

    #[test]
    fn whole_slab_views_do_not_pan() {
        let (turntable, _) = turntable();
        let view = turntable.pan(View::default(), Vec2::new(40.0, -25.0));
        assert!(view.focus.length() < 1e-3, "{}", view.focus);
    }

    #[test]
    fn panning_stops_at_the_edges_of_the_whole_slab_view() {
        let (turntable, framing) = turntable();
        let (width, height) = (framing.width as f32, framing.height as f32);
        let zoomed = turntable.zoom_at(View::turned(-30.0), 2.0, Vec2::new(320.0, 180.0));
        let middle = window_corner(&turntable, &framing, &zoomed);
        assert!(
            middle.distance(Vec2::new(width, height) / 4.0) < 1.0,
            "{middle}"
        );

        for (drag, corner) in [
            (Vec2::new(5000.0, 5000.0), Vec2::ZERO),
            (Vec2::new(-5000.0, -5000.0), Vec2::new(width, height) / 2.0),
        ] {
            let view = turntable.pan(zoomed, drag);
            let actual = window_corner(&turntable, &framing, &view);
            assert!(actual.distance(corner) < 1.0, "{actual} vs {corner}");
        }
    }

    #[test]
    fn the_middle_of_the_view_turns_with_the_slab() {
        let (turntable, framing) = turntable();
        let view = turntable.zoom_at(View::default(), 3.0, Vec2::new(300.0, 170.0));
        let view = turntable.pan(view, Vec2::new(30.0, 12.0));
        assert!(view.focus.length() > 50.0, "{}", view.focus);
        let middle = view.focus.extend(400.0);
        let centre = safe_centre(&framing);
        for yaw in [0.0, 25.0, 60.0] {
            let turned = View {
                yaw_offset_deg: yaw,
                ..view
            };
            let pixel = to_pixels(&turntable.camera(&turned), &framing, middle).truncate();
            assert!(pixel.distance(centre) <= 0.75, "{yaw}°: {pixel}");
        }
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
