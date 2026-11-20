//! Offscreen rendering of the 3D scene into a texture that egui shows in the central panel.

use std::sync::Arc;

use eframe::egui_wgpu::{self, RenderState};
use eframe::wgpu::{self, util::DeviceExt as _};
use egui::mutex::RwLock;
use glam::Mat4;
use lowpoly_editor::OrbitCamera;

use super::colors;
use super::grid::{GridGeometry, LineVertex};

/// Colour format of the viewport image. egui samples native textures as gamma-space
/// `Rgba8Unorm`, so shaders write sRGB-encoded colour.
const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
/// MSAA sample count when the adapter supports it for both attachments.
const MSAA_SAMPLES: u32 = 4;

/// Renders the scene for the viewport and keeps the result registered as an egui texture.
pub(crate) struct ViewportRenderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    egui_renderer: Arc<RwLock<egui_wgpu::Renderer>>,
    sample_count: u32,
    uniforms: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    grid: GridPass,
    target: Option<Target>,
    /// What the current target image shows, so an unchanged frame isn't re-rendered.
    rendered: Option<FrameKey>,
}

impl std::fmt::Debug for ViewportRenderer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ViewportRenderer")
            .field("sample_count", &self.sample_count)
            .field("size", &self.target.as_ref().map(|t| t.size))
            .finish_non_exhaustive()
    }
}

/// The inputs that determine the rendered image.
#[derive(Clone, Copy, PartialEq)]
struct FrameKey {
    size: [u32; 2],
    view_proj: Mat4,
}

/// Offscreen attachments for one viewport size.
struct Target {
    size: [u32; 2],
    /// Single-sample image that egui samples (the MSAA resolve target when multisampling).
    color_view: wgpu::TextureView,
    msaa_view: Option<wgpu::TextureView>,
    depth_view: wgpu::TextureView,
    texture_id: egui::TextureId,
}

/// Pipelines and geometry for the ground grid and axes.
struct GridPass {
    vertices: wgpu::Buffer,
    geometry: GridGeometry,
    ground: wgpu::RenderPipeline,
    vertical: wgpu::RenderPipeline,
}

impl ViewportRenderer {
    pub(crate) fn new(render_state: &RenderState) -> Self {
        let device = render_state.device.clone();
        let sample_count = supported_sample_count(&render_state.adapter);

        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("viewport uniforms"),
            size: size_of::<Mat4>() as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("viewport uniforms"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("viewport uniforms"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniforms.as_entire_binding(),
            }],
        });
        let grid = GridPass::new(&device, &bind_group_layout, sample_count);

        Self {
            device,
            queue: render_state.queue.clone(),
            egui_renderer: Arc::clone(&render_state.renderer),
            sample_count,
            uniforms,
            bind_group,
            grid,
            target: None,
            rendered: None,
        }
    }

    /// Renders the scene at `size` physical pixels, if anything changed, and returns the egui
    /// texture holding the image. Returns `None` for an empty viewport.
    pub(crate) fn render(
        &mut self,
        camera: &OrbitCamera,
        size: [u32; 2],
    ) -> Option<egui::TextureId> {
        if size.contains(&0) {
            return None;
        }
        let aspect = f64::from(size[0]) / f64::from(size[1]);
        let key = FrameKey {
            size,
            view_proj: camera.view_projection(aspect).as_mat4(),
        };
        if self.rendered == Some(key) {
            return self.target.as_ref().map(|t| t.texture_id);
        }

        if self.target.as_ref().is_none_or(|t| t.size != size) {
            self.resize(size);
        }
        let target = self.target.as_ref().expect("target was just created");

        self.queue.write_buffer(
            &self.uniforms,
            0,
            bytemuck::bytes_of(&key.view_proj.to_cols_array()),
        );
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("viewport"),
            });
        {
            let [r, g, b, _] = colors::rgba(colors::BACKGROUND).map(f64::from);
            let (view, resolve_target) = match &target.msaa_view {
                Some(msaa) => (msaa, Some(&target.color_view)),
                None => (&target.color_view, None),
            };
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("viewport"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color { r, g, b, a: 1.0 }),
                        store: if resolve_target.is_some() {
                            wgpu::StoreOp::Discard
                        } else {
                            wgpu::StoreOp::Store
                        },
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &target.depth_view,
                    // Reverse-Z: 0 is the far plane.
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(0.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &self.bind_group, &[]);
            self.grid.draw(&mut pass);
        }
        self.queue.submit([encoder.finish()]);

        self.rendered = Some(key);
        Some(target.texture_id)
    }

    /// Recreates the attachments for a new size, keeping the same egui texture id.
    fn resize(&mut self, size: [u32; 2]) {
        let extent = wgpu::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        };
        let texture = |label, format, sample_count, usage| {
            self.device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: extent,
                    mip_level_count: 1,
                    sample_count,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage,
                    view_formats: &[],
                })
                .create_view(&wgpu::TextureViewDescriptor::default())
        };
        let color_view = texture(
            "viewport color",
            COLOR_FORMAT,
            1,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        );
        let msaa_view = (self.sample_count > 1).then(|| {
            texture(
                "viewport msaa",
                COLOR_FORMAT,
                self.sample_count,
                wgpu::TextureUsages::RENDER_ATTACHMENT,
            )
        });
        let depth_view = texture(
            "viewport depth",
            DEPTH_FORMAT,
            self.sample_count,
            wgpu::TextureUsages::RENDER_ATTACHMENT,
        );

        let mut egui_renderer = self.egui_renderer.write();
        let filter = wgpu::FilterMode::Nearest; // the image is shown at 1:1 physical pixels
        let texture_id = match &self.target {
            Some(old) => {
                egui_renderer.update_egui_texture_from_wgpu_texture(
                    &self.device,
                    &color_view,
                    filter,
                    old.texture_id,
                );
                old.texture_id
            }
            None => egui_renderer.register_native_texture(&self.device, &color_view, filter),
        };
        self.target = Some(Target {
            size,
            color_view,
            msaa_view,
            depth_view,
            texture_id,
        });
        self.rendered = None;
    }
}

/// The MSAA sample count to use: [`MSAA_SAMPLES`] if both attachment formats support it, else 1.
fn supported_sample_count(adapter: &wgpu::Adapter) -> u32 {
    let supports = |format| {
        adapter
            .get_texture_format_features(format)
            .flags
            .sample_count_supported(MSAA_SAMPLES)
    };
    if supports(COLOR_FORMAT) && supports(DEPTH_FORMAT) {
        MSAA_SAMPLES
    } else {
        1
    }
}

impl GridPass {
    fn new(
        device: &wgpu::Device,
        bind_group_layout: &wgpu::BindGroupLayout,
        sample_count: u32,
    ) -> Self {
        let geometry = GridGeometry::new();
        let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("grid vertices"),
            contents: bytemuck::cast_slice(&geometry.vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("grid"),
            source: wgpu::ShaderSource::Wgsl(include_str!("grid.wgsl").into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("grid"),
            bind_group_layouts: &[Some(bind_group_layout)],
            immediate_size: 0,
        });
        let pipeline = |label, depth_write_enabled| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    buffers: &[Some(LineVertex::LAYOUT)],
                },
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::LineList,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(depth_write_enabled),
                    depth_compare: Some(wgpu::CompareFunction::GreaterEqual),
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState {
                    count: sample_count,
                    ..Default::default()
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_main"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: COLOR_FORMAT,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        Self {
            vertices,
            ground: pipeline("grid ground", false),
            vertical: pipeline("grid vertical", true),
            geometry,
        }
    }

    fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        pass.set_vertex_buffer(0, self.vertices.slice(..));
        pass.set_pipeline(&self.ground);
        pass.draw(self.geometry.ground.clone(), 0..1);
        pass.set_pipeline(&self.vertical);
        pass.draw(self.geometry.vertical.clone(), 0..1);
    }
}
