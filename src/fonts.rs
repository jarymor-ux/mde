use eframe::egui::{self, FontData, FontDefinitions, FontFamily};
use std::path::PathBuf;

/// Kitty on this Mac resolves its configured Fira Code to Menlo-Regular.
/// Read that one file directly; never run kitty or scan fonts at app startup.
pub fn install(ctx: &egui::Context) {
    let path = std::env::var_os("MDE_FONT_PATH")
        .map(PathBuf::from)
        .or_else(|| {
            cfg!(target_os = "macos").then(|| PathBuf::from("/System/Library/Fonts/Menlo.ttc"))
        });
    let Some(path) = path else {
        return;
    };
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!(
                "mde: cannot load font {}: {error}; using bundled fonts",
                path.display()
            );
            return;
        }
    };
    let mut definitions = FontDefinitions::default();
    let mut data = FontData::from_owned(bytes);
    data.index = std::env::var("MDE_FONT_INDEX")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    definitions.font_data.insert("kitty".into(), data.into());
    for family in [FontFamily::Monospace, FontFamily::Proportional] {
        definitions
            .families
            .entry(family)
            .or_default()
            .insert(0, "kitty".into());
    }
    ctx.set_fonts(definitions);
}
