mod app;
mod cli;
#[cfg(target_os = "macos")]
mod desktop;

fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|s| s == "--version") {
        println!("FastAF Flow {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if args.iter().any(|s| s == "--list-models") {
        let settings = fastaf_flow::settings::Settings::load().unwrap_or_default();
        println!(
            "{}",
            serde_json::to_string_pretty(&fastaf_flow::catalog::all_models(&settings.model_roots))
                .unwrap()
        );
        return Ok(());
    }
    if args.len() > 1 {
        match cli::run(&args) {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error:#}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1020.0, 720.0])
            .with_min_inner_size([820.0, 610.0])
            .with_app_id("dev.fastaf.flow"),
        ..Default::default()
    };
    eframe::run_native(
        "FastAF Flow",
        options,
        Box::new(|cc| Ok(Box::new(app::FlowApp::new(cc)))),
    )
}
