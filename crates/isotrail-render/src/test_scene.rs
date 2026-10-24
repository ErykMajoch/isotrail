use crate::{Gpu, OffscreenTarget, TARGET_FORMAT};

/// Clears to `[32, 48, 80, 255]`, then draws a red-green-blue triangle on the
/// left and a flat `[240, 200, 64, 255]` rectangle on the right, for checking
/// that the GPU path works end to end
pub fn draw_test_scene(gpu: &Gpu, target: &OffscreenTarget) {
    let device = gpu.device();
    let module = device.create_shader_module(wgpu::include_wgsl!("test_scene.wgsl"));
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("test scene"),
        layout: None,
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(TARGET_FORMAT.into())],
        }),
        multiview_mask: None,
        cache: None,
    });

    let mut encoder =
        device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("test scene"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target.view(),
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 32.0 / 255.0,
                        g: 48.0 / 255.0,
                        b: 80.0 / 255.0,
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        pass.set_pipeline(&pipeline);
        pass.draw(0..9, 0..1);
    }
    gpu.queue().submit([encoder.finish()]);
}
