//! onepaint desktop app.

fn main() -> eframe::Result {
    env_logger::init();
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport: egui::ViewportBuilder::default()
            .with_title("onepaint")
            .with_inner_size([1400.0, 900.0]),
        ..Default::default()
    };
    eframe::run_native("onepaint", options, Box::new(|_cc| Ok(Box::new(App))))
}

struct App;

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ui, |ui| ui.label("onepaint"));
    }
}
