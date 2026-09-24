//! Runs adjustment jobs on the GPU. See the module docs in `adjust`.

use std::num::NonZeroUsize;

use amalith_adjust::ColorLut;
use amalith_core::appearance::BlendMode;
use amalith_core::AdjustmentOp;
use vello::peniko::{Color, ImageData};
use vello::wgpu;
use vello::{AaConfig, RenderParams, Renderer, RendererOptions};

use super::LayerJob;

/// How many compiled lookup tables to keep. A cube is a few MB, so this
/// stays small; it only needs to cover what's on screen plus a slider drag.
const LUT_CACHE: usize = 8;

struct GpuLut {
    key: String,
    buffer: wgpu::Buffer,
    mode: u32,
    n: u32,
}

/// One per GPU device, next to the window renderers.
pub struct AdjustEngine {
    /// Renders job content. Separate from the window's renderer so the
    /// intermediate textures never take space in its image atlas.
    renderer: Renderer,
    pipeline: wgpu::ComputePipeline,
    layout: wgpu::BindGroupLayout,
    luts: Vec<GpuLut>,
}

/// The shader's blend-mode numbering (`blend` in `color.wgsl`).
pub fn blend_index(mode: BlendMode) -> u32 {
    match mode {
        BlendMode::Normal => 0,
        BlendMode::Multiply => 1,
        BlendMode::Screen => 2,
        BlendMode::Overlay => 3,
        BlendMode::Darken => 4,
        BlendMode::Lighten => 5,
        BlendMode::ColorDodge => 6,
        BlendMode::ColorBurn => 7,
        BlendMode::HardLight => 8,
        BlendMode::SoftLight => 9,
        BlendMode::Difference => 10,
        BlendMode::Exclusion => 11,
        BlendMode::Hue => 12,
        BlendMode::Saturation => 13,
        BlendMode::Color => 14,
        BlendMode::Luminosity => 15,
    }
}

pub const SHADER: &str = include_str!("color.wgsl");

fn texture(device: &wgpu::Device, width: u32, height: u32, label: &str) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::STORAGE_BINDING
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

fn bytes(entries: &[[f32; 4]]) -> Vec<u8> {
    entries.iter().flatten().flat_map(|v| v.to_le_bytes()).collect()
}

impl AdjustEngine {
    pub fn new(device: &wgpu::Device) -> Option<Self> {
        let renderer = Renderer::new(
            device,
            RendererOptions {
                use_cpu: false,
                antialiasing_support: vello::AaSupport::area_only(),
                num_init_threads: NonZeroUsize::new(1),
                pipeline_cache: None,
            },
        )
        .ok()?;
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("adjustment color pass"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let entry = |binding, ty| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty,
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("adjustment color pass"),
            entries: &[
                entry(
                    0,
                    wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                ),
                entry(
                    1,
                    wgpu::BindingType::StorageTexture {
                        access: wgpu::StorageTextureAccess::WriteOnly,
                        format: wgpu::TextureFormat::Rgba8Unorm,
                        view_dimension: wgpu::TextureViewDimension::D2,
                    },
                ),
                entry(
                    2,
                    wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                ),
                entry(
                    3,
                    wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                ),
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("adjustment color pass"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("adjustment color pass"),
            layout: Some(&pipeline_layout),
            module: &module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        Some(Self { renderer, pipeline, layout, luts: Vec::new() })
    }

    /// Renders every job and points each job's placeholder at its result on
    /// `target` — the renderer that will draw the scene the placeholders
    /// are in. `expired` placeholders are released first.
    pub fn run(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        jobs: Vec<LayerJob>,
        expired: &[ImageData],
        target: &mut Renderer,
    ) {
        for image in expired {
            target.override_image(image, None);
            self.renderer.override_image(image, None);
        }
        for job in jobs {
            let last = job.steps.len().saturating_sub(1);
            for (i, step) in job.steps.into_iter().enumerate() {
                let input = texture(device, job.width, job.height, "adjustment input");
                let input_view = input.create_view(&wgpu::TextureViewDescriptor::default());
                let _ = self.renderer.render_to_texture(
                    device,
                    queue,
                    &step.content,
                    &input_view,
                    &RenderParams {
                        base_color: Color::TRANSPARENT,
                        width: job.width,
                        height: job.height,
                        antialiasing_method: AaConfig::Area,
                    },
                );
                let result = match self.lut(device, queue, &step.op) {
                    Some(lut) => {
                        let output = texture(device, job.width, job.height, "adjustment output");
                        self.dispatch(device, queue, &input, &output, lut, step.blend, step.opacity);
                        output
                    }
                    // Not a color op: pass the content through unchanged.
                    None => input,
                };
                let base = wgpu::TexelCopyTextureInfoBase {
                    texture: result,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                };
                if i == last {
                    target.override_image(&step.output, Some(base));
                } else {
                    self.renderer.override_image(&step.output, Some(base));
                }
            }
        }
    }

    /// The compiled table for `op`, uploaded; `None` for non-color ops.
    fn lut(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, op: &AdjustmentOp) -> Option<usize> {
        let key = format!("{op:?}");
        if let Some(i) = self.luts.iter().position(|l| l.key == key) {
            let hit = self.luts.remove(i);
            self.luts.push(hit);
            return Some(self.luts.len() - 1);
        }
        let (mode, n, data) = match amalith_adjust::compile(op)? {
            ColorLut::OneD(t) => {
                let entries: Vec<[f32; 4]> = (0..256).map(|i| [t[0][i], t[1][i], t[2][i], 0.0]).collect();
                (0, 256, entries)
            }
            ColorLut::Cube { n, data } => (1, n as u32, data),
        };
        let contents = bytes(&data);
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("adjustment lut"),
            size: contents.len() as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&buffer, 0, &contents);
        if self.luts.len() >= LUT_CACHE {
            self.luts.remove(0);
        }
        self.luts.push(GpuLut { key, buffer, mode, n });
        Some(self.luts.len() - 1)
    }

    /// Runs the color pass from `input` into `output` (same size).
    #[allow(clippy::too_many_arguments)]
    fn dispatch(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        input: &wgpu::Texture,
        output: &wgpu::Texture,
        lut: usize,
        blend: BlendMode,
        opacity: f32,
    ) {
        let lut = &self.luts[lut];
        let (width, height) = (input.width(), input.height());
        let mut uniform = Vec::with_capacity(32);
        for word in [width, height, lut.mode, lut.n, blend_index(blend), opacity.to_bits(), 0, 0] {
            uniform.extend_from_slice(&word.to_le_bytes());
        }
        let params = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("adjustment params"),
            size: uniform.len() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&params, 0, &uniform);
        let input_view = input.create_view(&wgpu::TextureViewDescriptor::default());
        let output_view = output.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("adjustment color pass"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&input_view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&output_view) },
                wgpu::BindGroupEntry { binding: 2, resource: params.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 3, resource: lut.buffer.as_entire_binding() },
            ],
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("adjustment") });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("adjustment color pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.dispatch_workgroups(width.div_ceil(8), height.div_ceil(8), 1);
        }
        queue.submit([encoder.finish()]);
    }

    /// Applies one color op to `pixels` (straight RGBA8) on the GPU and
    /// reads the result back — for the parity test against the CPU
    /// reference.
    #[cfg(test)]
    fn apply_pixels(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pixels: &[u8],
        width: u32,
        height: u32,
        op: &AdjustmentOp,
        blend: BlendMode,
        opacity: f32,
    ) -> Vec<u8> {
        let input = texture(device, width, height, "parity input");
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &input,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            pixels,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(width * 4), rows_per_image: Some(height) },
            wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        );
        let output = texture(device, width, height, "parity output");
        let lut = self.lut(device, queue, op).expect("color op");
        self.dispatch(device, queue, &input, &output, lut, blend, opacity);
        self.read_back(device, queue, &output)
    }

    /// Reads a texture back as tightly packed RGBA8 rows (tests only).
    #[cfg(test)]
    fn read_back(&self, device: &wgpu::Device, queue: &wgpu::Queue, output: &wgpu::Texture) -> Vec<u8> {
        let (width, height) = (output.width(), output.height());
        let padded = (width * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: (padded * height) as u64,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: output,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(padded), rows_per_image: Some(height) },
            },
            wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        );
        queue.submit([encoder.finish()]);
        let slice = buffer.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
        let mapped = slice.get_mapped_range();
        let mut out = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height as usize {
            let row = y * padded as usize;
            out.extend_from_slice(&mapped[row..row + width as usize * 4]);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use amalith_core::{AdjustmentKind, ColorBalanceParams, HueSaturationParams, LevelsParams};

    /// The shader must parse, validate, and translate for every backend we
    /// ship — including HLSL for Windows (DX12), which the Mac never runs.
    #[test]
    fn color_shader_validates_and_translates_for_every_backend() {
        let module = naga::front::wgsl::parse_str(SHADER).expect("WGSL parses");
        let info = naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all())
            .validate(&module)
            .expect("WGSL validates");
        let mut hlsl = String::new();
        naga::back::hlsl::Writer::new(&mut hlsl, &naga::back::hlsl::Options::default(), &Default::default())
            .write(&module, &info, None)
            .expect("translates to HLSL (Windows)");
        naga::back::msl::write_string(&module, &info, &naga::back::msl::Options::default(), &Default::default())
            .expect("translates to MSL (macOS)");
        assert!(hlsl.contains("main"));
    }

    #[test]
    fn blend_numbering_covers_every_mode_once() {
        use BlendMode::*;
        let all = [
            Normal, Multiply, Screen, Overlay, Darken, Lighten, ColorDodge, ColorBurn, HardLight, SoftLight,
            Difference, Exclusion, Hue, Saturation, Color, Luminosity,
        ];
        let mut seen: Vec<u32> = all.iter().map(|m| blend_index(*m)).collect();
        seen.sort();
        assert_eq!(seen, (0..16).collect::<Vec<_>>());
    }

    /// End to end through the export path, on a real GPU: an Invert
    /// adjustment changes what's beneath it in its own layer, and nothing
    /// above it or in another layer.
    /// `cargo test -p amalith-shell -- --ignored adjusts_only`
    #[test]
    #[ignore = "requires a GPU"]
    fn an_adjustment_adjusts_only_beneath_it_in_its_own_layer() {
        use amalith_commands::{Command, CommandOutcome, Editor, LayerOptions};
        use amalith_core::{Color as Rgba, Document, LayerKind, Paint, Rect};

        let mut editor = Editor::new(Document::new("e2e"));
        let mut layer = |editor: &mut Editor, raster: bool| {
            let CommandOutcome::Layer(id) = editor.execute(Command::CreateLayer { name: "L".into(), index: None }).unwrap() else { panic!() };
            if raster {
                let l = editor.document().layer(id).unwrap();
                let options = LayerOptions {
                    name: l.name.clone(), color: l.color, visible: true, locked: false, template: false,
                    print: true, preview: true, dim_images_to: None, kind: LayerKind::Raster,
                };
                editor.execute(Command::SetLayerOptions { id, options }).unwrap();
            }
            id
        };
        let rect = |editor: &mut Editor, layer, r: Rect, c: Rgba| {
            let CommandOutcome::Object(id) = editor.execute(Command::CreateRect { parent: amalith_core::ObjectParent::Layer(layer), rect: r, name: None }).unwrap() else { panic!() };
            editor.execute(Command::SetFill { objects: vec![id], paint: Paint::Solid(c) }).unwrap();
            editor.execute(Command::SetStroke { objects: vec![id], paint: Paint::None }).unwrap();
        };
        let other = layer(&mut editor, false);
        rect(&mut editor, other, Rect::new(0., 0., 20., 20.), Rgba::rgb(0., 0., 1.)); // blue, other layer
        let photo = layer(&mut editor, true);
        rect(&mut editor, photo, Rect::new(20., 0., 40., 20.), Rgba::rgb(1., 0., 0.)); // red, beneath
        editor
            .execute(Command::CreateAdjustment {
                layer: photo,
                index: None,
                name: None,
                data: amalith_core::AdjustmentData::new(AdjustmentOp::Invert),
            })
            .unwrap();
        rect(&mut editor, photo, Rect::new(40., 0., 60., 20.), Rgba::rgb(0., 1., 0.)); // green, above

        let mut context = vello::util::RenderContext::new();
        let index = pollster::block_on(context.device(None)).expect("GPU adapter");
        let dev = &context.devices[index];
        let mut engine = AdjustEngine::new(&dev.device).expect("engine");
        let mut renderer = Renderer::new(&dev.device, Default::default()).unwrap();

        let (w, h) = (60u32, 20u32);
        let mut adjust = super::super::AdjustCollector::disabled();
        adjust.begin_frame(true, 1.0);
        let scene = crate::canvas::export_scene(
            editor.document(),
            vello::kurbo::Rect::new(0., 0., 60., 20.),
            1.0,
            None,
            &std::collections::HashMap::new(),
            false,
            &mut crate::text::TextContext::new(),
            Color::BLACK,
            &mut adjust,
        );
        let (jobs, _) = adjust.finish();
        assert_eq!(jobs.len(), 1);
        engine.run(&dev.device, &dev.queue, jobs, &[], &mut renderer);

        let target = texture(&dev.device, w, h, "e2e target");
        renderer
            .render_to_texture(
                &dev.device,
                &dev.queue,
                &scene,
                &target.create_view(&Default::default()),
                &RenderParams { base_color: Color::TRANSPARENT, width: w, height: h, antialiasing_method: AaConfig::Area },
            )
            .unwrap();
        let pixels = engine.read_back(&dev.device, &dev.queue, &target);
        let at = |x: u32| &pixels[((10 * w + x) * 4) as usize..((10 * w + x) * 4 + 4) as usize];
        assert_eq!(at(10), &[0, 0, 255, 255], "other layer untouched");
        assert_eq!(at(30), &[0, 255, 255, 255], "red beneath the adjustment is inverted to cyan");
        assert_eq!(at(50), &[0, 255, 0, 255], "green above the adjustment untouched");
    }

    /// Needs a real GPU: `cargo test -p amalith-shell -- --ignored parity`.
    #[test]
    #[ignore]
    fn gpu_matches_the_cpu_reference() {
        let instance = wgpu::Instance::default();
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).expect("adapter");
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).expect("device");
        let mut engine = AdjustEngine::new(&device).expect("engine");

        let (w, h) = (64u32, 64u32);
        let mut pixels = Vec::with_capacity((w * h * 4) as usize);
        let mut s = 0x2545F4914F6CDD1Du64;
        for i in 0..w * h {
            for _ in 0..3 {
                s ^= s << 13;
                s ^= s >> 7;
                s ^= s << 17;
                pixels.push((s >> 24) as u8);
            }
            pixels.push(if i % 17 == 0 { 0 } else { 255 });
        }
        let mut hs = HueSaturationParams::default();
        hs.adjustments[0].hue = 40.0;
        hs.adjustments[0].saturation = 20.0;
        let mut cb = ColorBalanceParams::default();
        cb.midtones = [30.0, -20.0, 10.0];
        let mut levels = LevelsParams::default();
        levels.ranges[0].gamma = 1.6;
        let cases = [
            (AdjustmentOp::Invert, BlendMode::Normal, 1.0),
            (AdjustmentOp::Levels(levels), BlendMode::Normal, 0.6),
            (AdjustmentOp::HueSaturation(hs), BlendMode::Normal, 1.0),
            (AdjustmentOp::ColorBalance(cb), BlendMode::Multiply, 0.8),
            (AdjustmentKind::BlackWhite.default_op(), BlendMode::Luminosity, 1.0),
        ];
        for (op, blend, opacity) in cases {
            let gpu = engine.apply_pixels(&device, &queue, &pixels, w, h, &op, blend, opacity);
            let mut cpu = pixels.clone();
            amalith_adjust::cpu::apply(&mut cpu, &amalith_adjust::compile(&op).unwrap(), blend, opacity, None);
            let worst = gpu.iter().zip(&cpu).map(|(a, b)| (*a as i32 - *b as i32).abs()).max().unwrap();
            assert!(worst <= 1, "{:?} {blend:?}: GPU and CPU differ by up to {worst}/255", op.kind());
        }
    }
}
