//! The eframe application: panel layout and viewport input.

use std::fmt;

use egui::{Color32, PointerButton, Rect, Sense, pos2};
use glam::DVec2;
use lowpoly_editor::{OrbitCamera, Projection};

use crate::render::ViewportRenderer;

/// Zoom factor per point of scroll; one wheel notch (about 50 points) zooms by about 10%.
const ZOOM_PER_SCROLL_POINT: f64 = 0.002;

/// Why the app could not start.
#[derive(Debug)]
pub(crate) enum StartError {
    /// eframe started without a wgpu renderer.
    NoWgpu,
}

impl fmt::Display for StartError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoWgpu => f.write_str("the wgpu renderer is not available"),
        }
    }
}

impl std::error::Error for StartError {}

/// The editor application.
#[derive(Debug)]
pub(crate) struct LowpolyApp {
    camera: OrbitCamera,
    viewport: ViewportRenderer,
}

impl LowpolyApp {
    pub(crate) fn new(cc: &eframe::CreationContext<'_>) -> Result<Self, StartError> {
        let render_state = cc.wgpu_render_state.as_ref().ok_or(StartError::NoWgpu)?;
        let info = render_state.adapter.get_info();
        log::info!("rendering with {} ({:?})", info.name, info.backend);
        cc.egui_ctx.set_theme(egui::Theme::Dark);
        Ok(Self {
            camera: OrbitCamera::default(),
            viewport: ViewportRenderer::new(render_state),
        })
    }

    fn viewport_ui(&mut self, ui: &mut egui::Ui) {
        let rect = ui.max_rect();
        let response = ui.allocate_rect(rect, Sense::click_and_drag());
        self.navigate(ui, &response, rect);

        // Draw the image on whole physical pixels: it is sampled with nearest filtering at 1:1,
        // so a fractional rect would double or drop rows.
        let pixels_per_point = ui.ctx().pixels_per_point();
        let snap = |p: egui::Pos2| (p.to_vec2() * pixels_per_point).round().to_pos2();
        let (min, max) = (snap(rect.min), snap(rect.max));
        let size = [
            physical_pixels(max.x - min.x),
            physical_pixels(max.y - min.y),
        ];
        if let Some(texture) = self.viewport.render(&self.camera, size) {
            let image_rect = Rect::from_min_max(
                (min.to_vec2() / pixels_per_point).to_pos2(),
                (max.to_vec2() / pixels_per_point).to_pos2(),
            );
            let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
            ui.painter().image(texture, image_rect, uv, Color32::WHITE);
        }
    }

    /// Camera navigation: middle-drag orbits, Shift+middle or right-drag pans, the wheel (or a
    /// pinch) zooms towards the pointer.
    fn navigate(&mut self, ui: &egui::Ui, response: &egui::Response, rect: Rect) {
        let height = f64::from(rect.height());
        let delta = to_dvec2(response.drag_delta());
        let shift = ui.input(|i| i.modifiers.shift);
        if response.dragged_by(PointerButton::Middle) && !shift {
            self.camera.orbit(delta, height);
        } else if response.dragged_by(PointerButton::Secondary)
            || response.dragged_by(PointerButton::Middle)
        {
            self.camera.pan(delta, height);
        }

        if let Some(pointer) = response.hover_pos() {
            let (scroll, pinch, scrolling) =
                ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta(), i.is_scrolling()));
            if scrolling {
                // egui spreads each wheel notch over several frames; keep them coming until
                // the zoom finishes.
                ui.ctx().request_repaint();
            }
            let factor = (-f64::from(scroll) * ZOOM_PER_SCROLL_POINT).exp() / f64::from(pinch);
            if (factor - 1.0).abs() > f64::EPSILON {
                let relative = (pointer - rect.min) / rect.size();
                let ndc = DVec2::new(
                    f64::from(relative.x) * 2.0 - 1.0,
                    1.0 - f64::from(relative.y) * 2.0,
                );
                let aspect = f64::from(rect.width()) / height;
                self.camera.zoom_at(factor, ndc, aspect);
            }
        }

        if response.has_focus() || response.hovered() {
            let toggle = ui.input(|i| i.key_pressed(egui::Key::Num5));
            if toggle {
                let next = match self.camera.projection() {
                    Projection::Perspective => Projection::Orthographic,
                    Projection::Orthographic => Projection::Perspective,
                };
                self.camera.set_projection(next);
            }
        }
    }
}

impl eframe::App for LowpolyApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::Panel::top("toolbar").show(ui, toolbar);
        egui::Panel::bottom("status").show(ui, |ui| self.status_bar(ui));
        egui::Panel::left("sidebar")
            .resizable(true)
            .default_size(240.0)
            .show(ui, sidebar);
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| self.viewport_ui(ui));
    }
}

impl LowpolyApp {
    fn status_bar(&self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Middle-drag: orbit · Shift+middle or right-drag: pan · Wheel: zoom · 5: perspective/ortho");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let projection = match self.camera.projection() {
                    Projection::Perspective => "Perspective",
                    Projection::Orthographic => "Orthographic",
                };
                ui.label(projection);
            });
        });
    }
}

fn toolbar(ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.strong("Lowpoly");
        ui.separator();
        ui.weak("Tools arrive in later milestones");
    });
}

fn sidebar(ui: &mut egui::Ui) {
    ui.heading("Outliner");
    ui.weak("No objects");
    ui.separator();
    ui.heading("Properties");
    ui.weak("Nothing selected");
}

fn to_dvec2(v: egui::Vec2) -> DVec2 {
    DVec2::new(f64::from(v.x), f64::from(v.y))
}

/// Converts a whole number of physical pixels held in an `f32` to `u32`.
fn physical_pixels(pixels: f32) -> u32 {
    // Saturating float-to-int conversion: negative and NaN become 0.
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "rounded, clamped viewport size"
    )]
    let whole = pixels.round().max(0.0) as u32;
    whole
}
