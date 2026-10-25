use geo::{Coord, LineString, SimplifyIdx};
use glam::{Vec2, Vec3};
use isotrail_bundle::{GridInfo, RoutePoint, SceneBundle};
use wgpu::util::DeviceExt;

use crate::mesh::slab_position;
use crate::post::{DEPTH_FORMAT, GBuffer, ROUTE_FORMAT};
use crate::{Camera, Gpu, Surface};

/// How far the line is pulled towards the camera, in internal pixels, so the
/// surface it lies on doesn't hide it
const LIFT_PX: f32 = 3.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RouteVertex {
    /// In the slab frame
    pub position: Vec3,
    /// Along the recorded track
    pub distance_m: f32,
}

/// The bundle's route laid on a terrain surface, one piece between each of
/// the route's gaps, climbing step edges straight up
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RouteLine {
    pieces: Vec<Vec<RouteVertex>>,
}

impl RouteLine {
    /// Leaves out points that would move the line less than `tolerance_m`
    /// sideways
    pub fn drape(bundle: &SceneBundle, surface: &Surface, tolerance_m: f32) -> RouteLine {
        let route = &bundle.route;
        let len = route.points.len();
        let mut starts: Vec<usize> = route
            .breaks
            .iter()
            .map(|&b| b as usize)
            .filter(|&b| b > 0 && b < len)
            .collect();
        starts.insert(0, 0);
        starts.push(len);
        starts.dedup();
        let tolerance = f64::from(tolerance_m) / bundle.grid.cell_size_m;
        let pieces = starts
            .windows(2)
            .map(|w| drape_piece(&route.points[w[0]..w[1]], &bundle.grid, surface, tolerance))
            .filter(|piece| piece.len() >= 2)
            .collect();
        RouteLine { pieces }
    }

    pub fn pieces(&self) -> &[Vec<RouteVertex>] {
        &self.pieces
    }

    pub fn start(&self) -> Option<&RouteVertex> {
        self.pieces.first()?.first()
    }

    pub fn finish(&self) -> Option<&RouteVertex> {
        self.pieces.last()?.last()
    }

    /// Distance along the track reached at `progress`, from 0 at the start to
    /// 1 at the finish
    pub fn distance_at(&self, progress: f32) -> f32 {
        match (self.start(), self.finish()) {
            (Some(start), Some(finish)) => {
                let progress = progress.clamp(0.0, 1.0);
                start.distance_m + (finish.distance_m - start.distance_m) * progress
            }
            _ => 0.0,
        }
    }

    /// Where the line has got to at `distance_m`; within a gap, the end of
    /// the piece before it
    pub fn point_at(&self, distance_m: f32) -> Option<Vec3> {
        let mut previous: Option<Vec3> = None;
        for piece in &self.pieces {
            if piece[0].distance_m >= distance_m {
                return Some(previous.unwrap_or(piece[0].position));
            }
            for pair in piece.windows(2) {
                let [a, b] = [pair[0], pair[1]];
                if b.distance_m >= distance_m {
                    let t = (distance_m - a.distance_m) / (b.distance_m - a.distance_m);
                    return Some(a.position.lerp(b.position, t));
                }
            }
            previous = piece.last().map(|v| v.position);
        }
        previous
    }
}

fn drape_piece(
    points: &[RoutePoint],
    grid: &GridInfo,
    surface: &Surface,
    tolerance: f64,
) -> Vec<RouteVertex> {
    let line: LineString = points
        .iter()
        .map(|p| Coord {
            x: p.col.into(),
            y: p.row.into(),
        })
        .collect();
    let kept = line.simplify_idx(tolerance);
    let mut vertices: Vec<RouteVertex> = Vec::new();
    for pair in kept.windows(2) {
        let (a, b) = (&points[pair[0]], &points[pair[1]]);
        let (from, to) = (Vec2::new(a.col, a.row), Vec2::new(b.col, b.row));
        let mut ts = surface.crossings(from, to);
        ts.insert(0, 0.0);
        ts.push(1.0);
        for w in ts.windows(2) {
            let inside = from.lerp(to, (w[0] + w[1]) / 2.0);
            for t in [w[0], w[1]] {
                let at = from.lerp(to, t);
                let vertex = RouteVertex {
                    position: slab_position(grid, at.x.into(), at.y.into())
                        .extend(surface.height(inside, at)),
                    distance_m: a.distance_m + (b.distance_m - a.distance_m) * t,
                };
                let repeated = vertices
                    .last()
                    .is_some_and(|last| last.position.distance_squared(vertex.position) < 1e-6);
                if repeated {
                    continue;
                }
                if let [.., a, b] = vertices[..]
                    && in_line(a.position, b.position, vertex.position)
                {
                    vertices.pop();
                }
                vertices.push(vertex);
            }
        }
    }
    vertices
}

/// Whether `b` lies on the way from `a` to `c`
fn in_line(a: Vec3, b: Vec3, c: Vec3) -> bool {
    (c - a).cross(b - a).length() <= 1e-3 * (c - a).length() && (b - a).dot(c - b) > 0.0
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct GpuVertex {
    position: [f32; 3],
    distance_m: f32,
    /// Along the line on screen, in internal pixels
    length_px: f32,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    view_proj: [f32; 16],
    lift: [f32; 4],
    route: [f32; 4],
}

/// Marks where the route shows in front of the terrain and where it hides
/// behind it
pub(crate) struct RoutePass {
    device: wgpu::Device,
    visible: wgpu::RenderPipeline,
    hidden: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
}

impl RoutePass {
    pub fn new(gpu: &Gpu) -> RoutePass {
        let device = gpu.device();
        let module = device.create_shader_module(wgpu::include_wgsl!("route.wgsl"));
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("route"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("route"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = |entry_point, depth_compare| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("route"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vs_main"),
                    compilation_options: Default::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: size_of::<GpuVertex>() as wgpu::BufferAddress,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &wgpu::vertex_attr_array![
                            0 => Float32x3,
                            1 => Float32,
                            2 => Float32,
                        ],
                    })],
                },
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::LineList,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(false),
                    depth_compare: Some(depth_compare),
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(entry_point),
                    compilation_options: Default::default(),
                    targets: &[Some(ROUTE_FORMAT.into())],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        RoutePass {
            device: device.clone(),
            visible: pipeline("fs_visible", wgpu::CompareFunction::LessEqual),
            hidden: pipeline("fs_hidden", wgpu::CompareFunction::Greater),
            layout,
        }
    }

    /// Draws the line up to `distance_m` into the g-buffer's route target,
    /// hidden stretches dashed every `dash_px`
    pub fn encode(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        gbuffer: &GBuffer,
        line: Option<&RouteLine>,
        distance_m: f32,
        camera: &Camera,
        dash_px: f32,
    ) {
        let device = &self.device;
        let vertices = line.map_or_else(Vec::new, |line| {
            line_vertices(line, camera, gbuffer.width, gbuffer.height)
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("route"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &gbuffer.route,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &gbuffer.depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            ..Default::default()
        });
        if vertices.is_empty() {
            return;
        }
        let uniforms = Uniforms {
            view_proj: camera.view_proj.to_cols_array(),
            lift: (-camera.forward * LIFT_PX * camera.metres_per_px)
                .extend(0.0)
                .to_array(),
            route: [distance_m, dash_px, 0.0, 0.0],
        };
        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("route uniforms"),
            contents: bytemuck::bytes_of(&uniforms),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("route vertices"),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("route"),
            layout: &self.layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });
        pass.set_bind_group(0, &bind_group, &[]);
        pass.set_vertex_buffer(0, vertex_buffer.slice(..));
        // Drawn last, so overlapping stretches show as visible
        for pipeline in [&self.hidden, &self.visible] {
            pass.set_pipeline(pipeline);
            pass.draw(0..vertices.len() as u32, 0..1);
        }
    }
}

/// Pixel position of a slab-frame point in a `width × height` frame, from
/// the top-left corner
pub(crate) fn to_pixel(camera: &Camera, point: Vec3, width: u32, height: u32) -> Vec2 {
    let ndc = camera.view_proj.project_point3(point);
    Vec2::new(
        (ndc.x + 1.0) / 2.0 * width as f32,
        (1.0 - ndc.y) / 2.0 * height as f32,
    )
}

/// Each piece's segments as vertex pairs
fn line_vertices(line: &RouteLine, camera: &Camera, width: u32, height: u32) -> Vec<GpuVertex> {
    let mut vertices = Vec::new();
    let mut length_px = 0.0;
    for piece in line.pieces() {
        let mut previous: Option<(GpuVertex, Vec2)> = None;
        for vertex in piece {
            let pixel = to_pixel(camera, vertex.position, width, height);
            if let Some((_, last)) = previous {
                length_px += pixel.distance(last);
            }
            let current = GpuVertex {
                position: vertex.position.to_array(),
                distance_m: vertex.distance_m,
                length_px,
            };
            if let Some((last, _)) = previous {
                vertices.extend([last, current]);
            }
            previous = Some((current, pixel));
        }
    }
    vertices
}

#[cfg(test)]
mod tests {
    use isotrail_bundle::{Landcover, Route};
    use isotrail_testkit::{bundle, grid};

    use super::*;
    use crate::{Mesher, SmoothMesher, VoxelMesher};

    /// A 10 × 10 slab of 10 m cells, `heights` per cell, with a route through
    /// the (col, row) points
    fn routed(heights: &[f32], points: &[(f32, f32)], breaks: Vec<u32>) -> SceneBundle {
        let mut bundle = bundle(grid(10, 10, 10.0), heights, &[Landcover::Grassland; 100]);
        let mut distance_m = 0.0;
        let mut previous = points[0];
        bundle.route = Route {
            points: points
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
            breaks,
        };
        bundle
    }

    /// Cells in columns 5 and up are 10 m higher
    fn step() -> Vec<f32> {
        (0..100)
            .map(|i| if i % 10 >= 5 { 10.0 } else { 0.0 })
            .collect()
    }

    fn drape(bundle: &SceneBundle, mesher: &dyn Mesher) -> RouteLine {
        RouteLine::drape(bundle, &mesher.mesh(bundle).surface, 0.1)
    }

    #[test]
    fn lines_climb_steps_straight_up() {
        let bundle = routed(&step(), &[(2.5, 5.5), (7.5, 5.5)], Vec::new());
        let line = drape(&bundle, &VoxelMesher::default());
        assert_eq!(line.pieces().len(), 1);
        let zs: Vec<f32> = line.pieces()[0].iter().map(|v| v.position.z).collect();
        let xs: Vec<f32> = line.pieces()[0].iter().map(|v| v.position.x).collect();
        assert_eq!(zs.first(), Some(&0.0));
        assert_eq!(zs.last(), Some(&10.0));
        // The climb happens at the column edge, x = 0 in the slab frame
        let climb = zs.windows(2).position(|w| w[0] != w[1]).unwrap();
        assert_eq!((xs[climb], xs[climb + 1]), (0.0, 0.0));
        assert_eq!((zs[climb], zs[climb + 1]), (0.0, 10.0));
        for v in &line.pieces()[0] {
            assert_eq!(v.position.y, -5.0);
        }
    }

    #[test]
    fn smooth_lines_follow_the_slope() {
        let bundle = routed(&step(), &[(2.5, 5.5), (7.5, 5.5)], Vec::new());
        let line = drape(&bundle, &SmoothMesher);
        // Flat, a ramp across the corners between the levels, flat again, with
        // the cell and diagonal crossings along each straight run merged
        let corners: Vec<(f32, f32)> = line.pieces()[0]
            .iter()
            .map(|v| (v.position.x, v.position.z))
            .collect();
        assert_eq!(
            corners,
            [(-25.0, 0.0), (-10.0, 0.0), (10.0, 10.0), (25.0, 10.0)]
        );
        // Halfway up where the corners between the two levels average them
        let middle = line.point_at(25.0).unwrap();
        assert!(
            (middle.x).abs() < 1e-4 && (middle.z - 5.0).abs() < 1e-4,
            "{middle}"
        );
    }

    #[test]
    fn gaps_split_the_line() {
        let points = [(1.5, 1.5), (3.5, 1.5), (6.5, 1.5), (8.5, 1.5)];
        let bundle = routed(&[0.0; 100], &points, vec![2]);
        let line = drape(&bundle, &VoxelMesher::default());
        assert_eq!(line.pieces().len(), 2);
        assert_eq!(line.start().unwrap().position.x, -35.0);
        assert_eq!(line.pieces()[0].last().unwrap().position.x, -15.0);
        assert_eq!(line.pieces()[1][0].position.x, 15.0);
        assert_eq!(line.finish().unwrap().position.x, 35.0);
        // Within the gap the line waits at the end of the first piece
        let gap = line.point_at(35.0).unwrap();
        assert_eq!(gap.x, -15.0);
    }

    #[test]
    fn single_point_pieces_are_dropped() {
        let points = [(1.5, 1.5), (3.5, 1.5), (6.5, 1.5)];
        let line = drape(
            &routed(&[0.0; 100], &points, vec![2]),
            &VoxelMesher::default(),
        );
        assert_eq!(line.pieces().len(), 1);
        assert!(
            drape(
                &routed(&[0.0; 100], &[(1.0, 1.0)], Vec::new()),
                &SmoothMesher
            )
            .pieces()
            .is_empty()
        );
    }

    #[test]
    fn straight_runs_simplify_to_their_ends() {
        let points: Vec<(f32, f32)> = (0..=40).map(|i| (1.0 + i as f32 * 0.2, 4.2)).collect();
        let bundle = routed(&[0.0; 100], &points, Vec::new());
        let line = drape(&bundle, &VoxelMesher::default());
        // Flat ground: nothing but the two ends survives
        assert_eq!(line.pieces()[0].len(), 2);
    }

    #[test]
    fn progress_walks_along_the_track() {
        let bundle = routed(&[0.0; 100], &[(1.5, 5.5), (8.5, 5.5)], Vec::new());
        let line = drape(&bundle, &VoxelMesher::default());
        assert_eq!(line.distance_at(0.0), 0.0);
        assert_eq!(line.distance_at(1.0), 70.0);
        assert_eq!(line.distance_at(2.0), 70.0);
        let half = line.point_at(line.distance_at(0.5)).unwrap();
        assert!((half - Vec3::new(0.0, -5.0, 0.0)).length() < 1e-4, "{half}");
        assert_eq!(line.point_at(-1.0), Some(line.start().unwrap().position));
        assert_eq!(line.point_at(1e9), Some(line.finish().unwrap().position));
        assert_eq!(RouteLine::default().point_at(0.0), None);
    }
}
