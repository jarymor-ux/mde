//! Small built-in editor palettes. Themes do not require assets or disk reads.

use eframe::egui::{self, Color32, Stroke};

pub const THEMES: &[(&str, &str)] = &[
    ("catppuccin-mocha", "Catppuccin Mocha"),
    ("catppuccin-latte", "Catppuccin Latte"),
    ("dracula", "Dracula"),
    ("nord", "Nord"),
    ("gruvbox-dark", "Gruvbox Dark"),
    ("gruvbox-light", "Gruvbox Light"),
    ("tokyo-night", "Tokyo Night"),
    ("solarized-dark", "Solarized Dark"),
    ("solarized-light", "Solarized Light"),
];

#[derive(Clone, Copy)]
struct Palette {
    dark: bool,
    background: u32,
    window: u32,
    inset: u32,
    surface: u32,
    hover: u32,
    active: u32,
    border: u32,
    text: u32,
    muted: u32,
    accent: u32,
    warning: u32,
    error: u32,
}

fn palette(id: &str) -> Palette {
    match id {
        "catppuccin-latte" => Palette {
            dark: false,
            background: 0xeff1f5,
            window: 0xe6e9ef,
            inset: 0xf7f8fa,
            surface: 0xdce0e8,
            hover: 0xccd0da,
            active: 0xc4c8d4,
            border: 0x9ca0b0,
            text: 0x4c4f69,
            muted: 0x6c6f85,
            accent: 0x1e66f5,
            warning: 0x9a6500,
            error: 0xd20f39,
        },
        "dracula" => Palette {
            dark: true,
            background: 0x282a36,
            window: 0x30323f,
            inset: 0x21222c,
            surface: 0x343746,
            hover: 0x44475a,
            active: 0x55586c,
            border: 0x6272a4,
            text: 0xf8f8f2,
            muted: 0xb3bbd6,
            accent: 0xbd93f9,
            warning: 0xffb86c,
            error: 0xff5555,
        },
        "nord" => Palette {
            dark: true,
            background: 0x2e3440,
            window: 0x353c4a,
            inset: 0x272c36,
            surface: 0x3b4252,
            hover: 0x434c5e,
            active: 0x4c566a,
            border: 0x616e88,
            text: 0xeceff4,
            muted: 0xa8b2c4,
            accent: 0x88c0d0,
            warning: 0xebcb8b,
            error: 0xbf616a,
        },
        "gruvbox-dark" => Palette {
            dark: true,
            background: 0x282828,
            window: 0x32302f,
            inset: 0x1d2021,
            surface: 0x3c3836,
            hover: 0x45403d,
            active: 0x504945,
            border: 0x665c54,
            text: 0xebdbb2,
            muted: 0xbdae93,
            accent: 0x8ec07c,
            warning: 0xfabd2f,
            error: 0xfb4934,
        },
        "gruvbox-light" => Palette {
            dark: false,
            background: 0xfbf1c7,
            window: 0xf2e5bc,
            inset: 0xf9f5d7,
            surface: 0xebdbb2,
            hover: 0xe1d1a8,
            active: 0xd5c4a1,
            border: 0xa89984,
            text: 0x3c3836,
            muted: 0x665c54,
            accent: 0x427b58,
            warning: 0x8f6200,
            error: 0x9d0006,
        },
        "tokyo-night" => Palette {
            dark: true,
            background: 0x1a1b26,
            window: 0x1f2335,
            inset: 0x16161e,
            surface: 0x24283b,
            hover: 0x2f334d,
            active: 0x414868,
            border: 0x565f89,
            text: 0xc0caf5,
            muted: 0xa9b1d6,
            accent: 0x7aa2f7,
            warning: 0xe0af68,
            error: 0xf7768e,
        },
        "solarized-dark" => Palette {
            dark: true,
            background: 0x002b36,
            window: 0x073642,
            inset: 0x00232c,
            surface: 0x073642,
            hover: 0x093844,
            active: 0x0b3a46,
            border: 0x586e75,
            text: 0x93a1a1,
            muted: 0x839496,
            accent: 0x2aa198,
            warning: 0xb58900,
            error: 0xdc322f,
        },
        "solarized-light" => Palette {
            dark: false,
            background: 0xfdf6e3,
            window: 0xeee8d5,
            inset: 0xfffbef,
            surface: 0xeee8d5,
            hover: 0xe3ddcb,
            active: 0xd8d2c0,
            border: 0x93a1a1,
            text: 0x073642,
            muted: 0x657b83,
            accent: 0x006c92,
            warning: 0x8d6900,
            error: 0xdc322f,
        },
        // Includes unknown saved theme ids, ensuring a stable startup fallback.
        _ => Palette {
            dark: true,
            background: 0x1e1e2e,
            window: 0x242435,
            inset: 0x181825,
            surface: 0x313244,
            hover: 0x45475a,
            active: 0x585b70,
            border: 0x585b70,
            text: 0xcdd6f4,
            muted: 0xa6adc8,
            accent: 0x89b4fa,
            warning: 0xf9e2af,
            error: 0xf38ba8,
        },
    }
}

fn color(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

/// The active preset's accent, also suitable for local application highlights.
pub fn accent(id: &str) -> Color32 {
    color(palette(id).accent)
}

pub fn label(id: &str) -> &'static str {
    THEMES
        .iter()
        .find_map(|(theme_id, name)| (*theme_id == id).then_some(*name))
        .unwrap_or("Catppuccin Mocha")
}

/// Syntax roles shared by Markdown and fenced-code language highlighters.
#[derive(Clone, Copy, Debug)]
pub struct SyntaxColors {
    pub heading: Color32,
    pub marker: Color32,
    pub link: Color32,
    pub code: Color32,
    pub emphasis: Color32,
    pub comment: Color32,
    pub keyword: Color32,
    pub string: Color32,
    pub number: Color32,
    pub function: Color32,
    pub type_name: Color32,
    pub property: Color32,
    pub operator: Color32,
    pub variable: Color32,
}

/// Canonical hue families with darker variants where light themes need contrast.
pub fn syntax_colors(id: &str) -> SyntaxColors {
    let (blue, purple, green, orange, red, cyan, yellow) = match id {
        "catppuccin-latte" => (
            0x1e66f5, 0x8839ef, 0x2c8220, 0xcc4d0c, 0xd20f39, 0x179299, 0x9a6500,
        ),
        "dracula" => (
            0x8be9fd, 0xbd93f9, 0x50fa7b, 0xffb86c, 0xff79c6, 0x8be9fd, 0xf1fa8c,
        ),
        "nord" => (
            0x81a1c1, 0xb48ead, 0xa3be8c, 0xd08770, 0xbf616a, 0x88c0d0, 0xebcb8b,
        ),
        "gruvbox-dark" => (
            0x83a598, 0xd3869b, 0xb8bb26, 0xfe8019, 0xfb4934, 0x8ec07c, 0xfabd2f,
        ),
        "gruvbox-light" => (
            0x076678, 0x8f3f71, 0x79740e, 0xaf3a03, 0x9d0006, 0x427b58, 0xb57614,
        ),
        "tokyo-night" => (
            0x7aa2f7, 0xbb9af7, 0x9ece6a, 0xff9e64, 0xf7768e, 0x7dcfff, 0xe0af68,
        ),
        "solarized-dark" => (
            0x268bd2, 0x6c71c4, 0x859900, 0xcb4b16, 0xdc322f, 0x2aa198, 0xb58900,
        ),
        "solarized-light" => (
            0x268bd2, 0x6c71c4, 0x6f8100, 0xcb4b16, 0xdc322f, 0x16847d, 0x8d6900,
        ),
        _ => (
            0x89b4fa, 0xcba6f7, 0xa6e3a1, 0xfab387, 0xf38ba8, 0x94e2d5, 0xf9e2af,
        ),
    };
    let palette = palette(id);
    SyntaxColors {
        heading: color(blue),
        marker: color(purple),
        link: color(cyan),
        code: color(green),
        emphasis: color(red),
        comment: color(palette.muted),
        keyword: color(purple),
        string: color(green),
        number: color(orange),
        function: color(blue),
        type_name: color(yellow),
        property: color(cyan),
        operator: color(red),
        variable: color(palette.text),
    }
}

/// Apply the palette globally and return whether it is a dark theme.
pub fn apply(ctx: &egui::Context, id: &str) -> bool {
    let palette = palette(id);
    let mut style = egui::Style {
        visuals: if palette.dark {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        },
        ..Default::default()
    };
    let visuals = &mut style.visuals;
    let text = color(palette.text);
    let accent = color(palette.accent);
    let border = Stroke::new(1.0, color(palette.border));
    visuals.override_text_color = Some(text);
    visuals.weak_text_color = Some(color(palette.muted));
    visuals.panel_fill = color(palette.background);
    visuals.window_fill = color(palette.window);
    visuals.window_stroke = border;
    visuals.extreme_bg_color = color(palette.inset);
    visuals.text_edit_bg_color = Some(color(palette.inset));
    visuals.faint_bg_color = color(palette.window);
    visuals.code_bg_color = color(palette.surface);
    visuals.hyperlink_color = accent;
    visuals.warn_fg_color = color(palette.warning);
    visuals.error_fg_color = color(palette.error);
    // Opaque tinted backgrounds retain normal text contrast for both selected
    // text and selected controls. The cursor remains the recognizable accent.
    visuals.selection.bg_fill = tint(color(palette.background), accent);
    visuals.selection.stroke = Stroke::new(1.0, text);
    visuals.text_cursor.stroke = Stroke::new(2.0, accent);
    visuals.ime_composition.active_underline_stroke = Stroke::new(1.0, accent);
    visuals.ime_composition.inactive_underline_stroke = border;

    for (widget, fill, outline) in [
        (&mut visuals.widgets.noninteractive, palette.window, border),
        (&mut visuals.widgets.inactive, palette.surface, border),
        (
            &mut visuals.widgets.hovered,
            palette.hover,
            Stroke::new(1.0, accent),
        ),
        (
            &mut visuals.widgets.active,
            palette.active,
            Stroke::new(1.0, accent),
        ),
        (
            &mut visuals.widgets.open,
            palette.hover,
            Stroke::new(1.0, accent),
        ),
    ] {
        widget.bg_fill = color(fill);
        widget.weak_bg_fill = color(fill);
        widget.bg_stroke = outline;
        widget.fg_stroke = Stroke::new(1.0, text);
    }
    // Separators use noninteractive.bg_stroke; keep them in the palette too.
    style.spacing.item_spacing = egui::vec2(10.0, 7.0);
    style.spacing.button_padding = egui::vec2(9.0, 6.0);
    style.url_in_tooltip = true;
    ctx.set_theme(if palette.dark {
        egui::Theme::Dark
    } else {
        egui::Theme::Light
    });
    ctx.set_global_style(style);
    palette.dark
}

fn tint(background: Color32, accent: Color32) -> Color32 {
    let mix = |base: u8, highlight: u8| ((u16::from(base) * 7 + u16::from(highlight)) / 8) as u8;
    Color32::from_rgb(
        mix(background.r(), accent.r()),
        mix(background.g(), accent.g()),
        mix(background.b(), accent.b()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn luminance(color: Color32) -> f32 {
        let linear = |value: u8| {
            let value = f32::from(value) / 255.0;
            if value <= 0.04045 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * linear(color.r()) + 0.7152 * linear(color.g()) + 0.0722 * linear(color.b())
    }

    fn contrast(a: Color32, b: Color32) -> f32 {
        let (a, b) = (luminance(a), luminance(b));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    #[test]
    fn presets_have_legible_document_inline_code_and_selected_text() {
        let context = egui::Context::default();
        for &(id, name) in THEMES {
            let dark = apply(&context, id);
            let style = context.style_of(if dark {
                egui::Theme::Dark
            } else {
                egui::Theme::Light
            });
            let visuals = &style.visuals;
            let text = visuals.text_color();
            assert_eq!(visuals.dark_mode, dark);
            assert_eq!(label(id), name);
            for (surface, background) in [
                ("panel", visuals.panel_fill),
                ("window", visuals.window_fill),
                ("editor", visuals.text_edit_bg_color.unwrap()),
                ("inline code", visuals.code_bg_color),
                ("selection", visuals.selection.bg_fill),
                ("inactive widget", visuals.widgets.inactive.bg_fill),
                ("hovered widget", visuals.widgets.hovered.bg_fill),
                ("active widget", visuals.widgets.active.bg_fill),
            ] {
                assert!(
                    contrast(text, background) >= 4.5,
                    "{name}: {surface} text contrast {}",
                    contrast(text, background)
                );
            }
            assert!(
                contrast(visuals.weak_text_color(), visuals.panel_fill) >= 3.0,
                "{name}: weak text contrast"
            );
            assert_eq!(visuals.hyperlink_color, accent(id));
        }
    }

    #[test]
    fn unknown_ids_apply_the_default_palette() {
        let context = egui::Context::default();
        assert!(apply(&context, "unknown"));
        let fallback = context.style_of(egui::Theme::Dark).visuals.clone();
        assert!(apply(&context, "catppuccin-mocha"));
        assert_eq!(fallback, context.style_of(egui::Theme::Dark).visuals);
        assert_eq!(accent("unknown"), accent("catppuccin-mocha"));
        assert_eq!(label("unknown"), "Catppuccin Mocha");
    }

    fn syntax_roles(colors: SyntaxColors) -> [(&'static str, Color32); 14] {
        [
            ("heading", colors.heading),
            ("marker", colors.marker),
            ("link", colors.link),
            ("code", colors.code),
            ("emphasis", colors.emphasis),
            ("comment", colors.comment),
            ("keyword", colors.keyword),
            ("string", colors.string),
            ("number", colors.number),
            ("function", colors.function),
            ("type", colors.type_name),
            ("property", colors.property),
            ("operator", colors.operator),
            ("variable", colors.variable),
        ]
    }

    #[test]
    fn syntax_colors_are_readable_on_each_theme_background() {
        for &(id, name) in THEMES {
            let palette = palette(id);
            for (role, foreground) in syntax_roles(syntax_colors(id)) {
                for background in [palette.background, palette.inset] {
                    let ratio = contrast(foreground, color(background));
                    assert!(ratio >= 3.0, "{name}: {role} contrast {ratio}");
                }
            }
        }
    }

    #[test]
    fn unknown_syntax_theme_uses_mocha_colors() {
        assert_eq!(
            syntax_roles(syntax_colors("unknown")),
            syntax_roles(syntax_colors("catppuccin-mocha"))
        );
    }
}
