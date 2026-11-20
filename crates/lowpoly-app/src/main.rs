//! Lowpoly, a minimalist low-poly mesh editor. Entry points for desktop and the web.

mod app;
mod render;

use app::LowpolyApp;

#[cfg(not(target_arch = "wasm32"))]
const APP_NAME: &str = "Lowpoly";

#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result {
    env_logger::init();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(APP_NAME)
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([640.0, 400.0]),
        ..Default::default()
    };
    eframe::run_native(
        APP_NAME,
        options,
        Box::new(|cc| Ok(Box::new(LowpolyApp::new(cc)?))),
    )
}

#[cfg(target_arch = "wasm32")]
fn main() {
    use eframe::{egui_wgpu, wgpu};

    // Forward `log` output to the browser console. Failing means a logger is already set.
    eframe::WebLogger::init(log::LevelFilter::Info).ok();

    // WebGL2 only for now: WebGPU support varies between browsers. Enabling it later is a
    // matter of adding `wgpu::Backends::BROWSER_WEBGPU` here.
    let mut web_options = eframe::WebOptions::default();
    if let egui_wgpu::WgpuSetup::CreateNew(setup) = &mut web_options.wgpu_options.wgpu_setup {
        setup.instance_descriptor.backends = wgpu::Backends::GL;
    }

    wasm_bindgen_futures::spawn_local(async {
        let Some(document) = eframe::web_sys::window().and_then(|window| window.document()) else {
            log::error!("not running in a browser page");
            return;
        };
        let result = start(&document, web_options).await;

        // Replace the loading text with the outcome.
        if let Some(status) = document.get_element_by_id("loading_text") {
            match result {
                Ok(()) => status.remove(),
                Err(error) => {
                    status.set_inner_html(
                        "<p>Lowpoly failed to start. See the browser console for details.</p>",
                    );
                    log::error!("failed to start: {error}");
                }
            }
        }
    });
}

/// Finds the canvas and starts the app on it.
#[cfg(target_arch = "wasm32")]
async fn start(
    document: &eframe::web_sys::Document,
    web_options: eframe::WebOptions,
) -> Result<(), String> {
    use eframe::wasm_bindgen::JsCast as _;

    let canvas = document
        .get_element_by_id("lowpoly_canvas")
        .ok_or("index.html has no #lowpoly_canvas element")?
        .dyn_into::<eframe::web_sys::HtmlCanvasElement>()
        .map_err(|_| "#lowpoly_canvas is not a <canvas>")?;
    eframe::WebRunner::new()
        .start(
            canvas,
            web_options,
            Box::new(|cc| Ok(Box::new(LowpolyApp::new(cc)?))),
        )
        .await
        .map_err(|error| format!("{error:?}"))
}
