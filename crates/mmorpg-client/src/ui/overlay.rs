//! External egui composition: winit input translation (`egui-winit`) and wgpu
//! painting (`egui-wgpu`) of the tessellated interface into the presented frame.
use crate::{
    ClientError,
    graphics::{Overlay, OverlayTarget},
};

/// One tessellated interface frame plus the texture changes not yet uploaded.
/// Texture deltas accumulate until a frame is actually painted, so a skipped
/// surface frame never loses the font atlas.
#[derive(Default)]
pub struct EguiPaint {
    renderer: Option<(egui_wgpu::Renderer, wgpu::TextureFormat)>,
    primitives: Vec<egui::ClippedPrimitive>,
    textures: egui::TexturesDelta,
    pixels_per_point: f32,
}

impl EguiPaint {
    /// Tessellates one finished egui pass for the next painted frame and
    /// returns its platform output (cursor, text input) for the window.
    pub fn prepare(
        &mut self,
        context: &egui::Context,
        output: egui::FullOutput,
    ) -> egui::PlatformOutput {
        let egui::FullOutput {
            platform_output,
            textures_delta,
            shapes,
            pixels_per_point,
            ..
        } = output;
        self.pixels_per_point = pixels_per_point;
        self.primitives = context.tessellate(shapes, pixels_per_point);
        self.textures.append(textures_delta);
        platform_output
    }
}

impl Drop for EguiPaint {
    /// Texture changes never painted die with their GPU renderer.
    fn drop(&mut self) {
        self.textures.clear();
    }
}

impl Overlay for EguiPaint {
    fn paint(&mut self, target: OverlayTarget<'_>) -> Result<(), ClientError> {
        if self
            .renderer
            .as_ref()
            .is_none_or(|(_, format)| *format != target.format)
        {
            let renderer = egui_wgpu::Renderer::new(
                target.device,
                target.format,
                egui_wgpu::RendererOptions::default(),
            );
            // A new renderer has none of the earlier textures.
            self.renderer = Some((renderer, target.format));
        }
        let Some((renderer, _)) = self.renderer.as_mut() else {
            return Ok(());
        };
        let mut textures = std::mem::take(&mut self.textures);
        for (id, delta) in &textures.set {
            for delta in delta {
                renderer.update_texture(target.device, target.queue, *id, delta);
            }
        }
        let screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels: target.size,
            pixels_per_point: self.pixels_per_point.max(0.1),
        };
        let mut encoder = target
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("interface"),
            });
        let commands = renderer.update_buffers(
            target.device,
            target.queue,
            &mut encoder,
            &self.primitives,
            &screen,
        );
        {
            let mut pass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("interface"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: target.view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    ..Default::default()
                })
                .forget_lifetime();
            renderer.render(&mut pass, &self.primitives, &screen);
        }
        target
            .queue
            .submit(commands.into_iter().chain([encoder.finish()]));
        for id in &textures.free {
            renderer.free_texture(id);
        }
        textures.clear();
        Ok(())
    }
}

/// The window's egui context and its winit input state.
pub struct EguiWindow {
    pub context: egui::Context,
    pub state: egui_winit::State,
    pub paint: EguiPaint,
}

impl EguiWindow {
    #[must_use]
    pub fn new(window: &winit::window::Window) -> Self {
        let context = egui::Context::default();
        let state = egui_winit::State::new(
            context.clone(),
            egui::ViewportId::ROOT,
            window,
            Some(window.scale_factor() as f32),
            None,
            None,
        );
        Self {
            context,
            state,
            paint: EguiPaint::default(),
        }
    }
}
