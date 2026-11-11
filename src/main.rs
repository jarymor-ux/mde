mod app;
mod autosave;
mod cli;
mod fonts;
mod live_preview;
mod markdown_syntax;
mod model;
mod syntax;
mod themes;

use std::time::Instant;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let started = Instant::now();
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let options = cli::parse(&args);
    if options.help {
        println!(
            "mde — native Markdown editor\n\nUsage: mde [OPTIONS] [FILE ... | FOLDER]\n\n  mde notes.md\n  mde one.md two.md three.md\n  mde .\n\nOpens the GUI in the background and immediately releases the terminal.\n\n--foreground  Run in the terminal (for debugging)\n--help        Show help\n--version     Show version\n--            Treat remaining arguments as paths\n\nMDE_STARTUP_LOG=1 mde --foreground prints time to first UI frame."
        );
        return Ok(());
    }
    if options.version {
        println!("mde {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if !options.foreground {
        cli::launch(&args)?;
        return Ok(());
    }
    let paths = options.paths;
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Glow,
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("mde")
            .with_inner_size([1200.0, 800.0])
            .with_min_inner_size([720.0, 420.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native(
        "mde",
        options,
        Box::new(move |cc| Ok(Box::new(app::MdeApp::new(cc, paths, started)))),
    )?;
    Ok(())
}
