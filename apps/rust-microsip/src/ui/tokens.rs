//! Visual tokens, in one place: sizes and the color roles of each theme
//! (see `openspec/changes/refine-native-ui/design.md`). Widgets take colors
//! from [`Palette`] or from the egui visuals built by [`visuals`].

use eframe::egui::{self, Color32, CornerRadius, Shadow, Stroke};

pub const KEY_HEIGHT: f32 = 36.0;
pub const CONTROL_HEIGHT: f32 = 28.0;
pub const CALL_HEIGHT: f32 = 34.0;
pub const TOGGLE_HEIGHT: f32 = 22.0;
pub const SPACING: f32 = 4.0;
pub const NUMBER_SIZE: f32 = 18.0;
pub const DIGIT_SIZE: f32 = 20.0;
/// Left padding of the digit inside a keypad key.
pub const KEY_PAD: f32 = 10.0;
/// Notebook tab height.
pub const TAB_HEIGHT: f32 = 22.0;
/// Width of the End button beside the wide Call button.
pub const END_WIDTH: f32 = 60.0;
/// One history row (icon, name, time, duration).
pub const ROW_HEIGHT: f32 = 30.0;
/// Segmented buttons (filters, theme, transport).
pub const SEGMENT_HEIGHT: f32 = 24.0;
/// Status bar line.
pub const STATUS_HEIGHT: f32 = 22.0;
/// Radius of the round presence icon in the status bar.
pub const STATUS_DOT: f32 = 5.5;
/// Width of the label column in settings forms.
pub const LABEL_WIDTH: f32 = 90.0;
/// Section title size in settings.
pub const SECTION_SIZE: f32 = 14.0;
/// Accent underline of the selected tab or segment.
pub const UNDERLINE: f32 = 2.0;

/// Color roles of one theme.
pub struct Palette {
    pub window: Color32,
    pub surface: Color32,
    pub surface_alt: Color32,
    /// Buttons and keypad keys at rest.
    pub control: Color32,
    /// Border of buttons, keys, tabs and fields.
    pub control_border: Color32,
    pub pressed: Color32,
    pub hairline: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub accent: Color32,
    /// Selection background (selected rows, segments, tabs).
    pub accent_soft: Color32,
    /// Text on an accent fill (primary buttons).
    pub on_accent: Color32,
    pub ok: Color32,
    pub warn: Color32,
    pub error: Color32,
}

pub const fn palette(dark: bool) -> Palette {
    const fn rgb(v: u32) -> Color32 {
        Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
    }
    if dark {
        Palette {
            window: rgb(0x111316),
            surface: rgb(0x1A1D21),
            surface_alt: rgb(0x2A2F36),
            control: rgb(0x1E2125),
            control_border: rgb(0x3A4048),
            pressed: rgb(0x2C3137),
            hairline: rgb(0x30353C),
            text: rgb(0xE9EBEE),
            muted: rgb(0x9BA4AE),
            accent: rgb(0x6AAEFF),
            accent_soft: rgb(0x1C3352),
            on_accent: rgb(0x0B0D10),
            ok: rgb(0x3FB950),
            warn: rgb(0xFFB74D),
            error: rgb(0xFF7A75),
        }
    } else {
        Palette {
            // Close to the classic Windows look MicroSIP uses.
            window: rgb(0xF0F0F0),
            surface: rgb(0xFFFFFF),
            surface_alt: rgb(0xE5F1FB),
            control: rgb(0xE1E1E1),
            control_border: rgb(0xADADAD),
            pressed: rgb(0xCCE4F7),
            hairline: rgb(0xD0D0D0),
            text: rgb(0x16191D),
            muted: rgb(0x59626D),
            accent: rgb(0x0A57B5),
            accent_soft: rgb(0xD9E7FA),
            on_accent: rgb(0xFFFFFF),
            ok: rgb(0x17692C),
            warn: rgb(0x7A4200),
            error: rgb(0xB01E22),
        }
    }
}

/// Flat, square-cornered egui visuals built from [`palette`].
pub fn visuals(dark: bool) -> egui::Visuals {
    let p = palette(dark);
    let mut v = if dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    let hairline = Stroke::new(1.0, p.hairline);
    v.panel_fill = p.window;
    v.window_fill = p.surface;
    v.extreme_bg_color = p.surface;
    v.text_edit_bg_color = Some(p.surface);
    v.faint_bg_color = p.surface_alt;
    v.code_bg_color = p.surface_alt;
    v.window_stroke = hairline;
    v.window_shadow = Shadow::NONE;
    v.popup_shadow = Shadow::NONE;
    v.window_corner_radius = CornerRadius::ZERO;
    v.menu_corner_radius = CornerRadius::ZERO;
    v.hyperlink_color = p.accent;
    v.warn_fg_color = p.warn;
    v.error_fg_color = p.error;
    v.weak_text_color = Some(p.muted);
    v.selection.bg_fill = p.accent_soft;
    v.selection.stroke = Stroke::new(1.0, p.accent);
    v.text_cursor.stroke = Stroke::new(2.0, p.accent);
    let w = &mut v.widgets;
    for (state, fill, border) in [
        (&mut w.noninteractive, p.window, p.hairline),
        (&mut w.inactive, p.control, p.control_border),
        // Hover and keyboard focus share these visuals: an accent border.
        (&mut w.hovered, p.surface_alt, p.accent),
        (&mut w.active, p.pressed, p.accent),
        (&mut w.open, p.surface_alt, p.hairline),
    ] {
        state.bg_fill = fill;
        state.weak_bg_fill = fill;
        state.bg_stroke = Stroke::new(1.0, border);
        state.fg_stroke = Stroke::new(1.0, p.text);
        state.corner_radius = CornerRadius::ZERO;
        state.expansion = 0.0;
    }
    v
}

/// Apply [`visuals`] to both egui themes.
pub fn install(ctx: &egui::Context) {
    ctx.set_visuals_of(egui::Theme::Dark, visuals(true));
    ctx.set_visuals_of(egui::Theme::Light, visuals(false));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WCAG 2.x contrast ratio between two opaque colors.
    fn contrast(a: Color32, b: Color32) -> f32 {
        let luminance = |c: Color32| {
            let channel = |v: u8| {
                let v = f32::from(v) / 255.0;
                if v <= 0.040_45 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * channel(c.r()) + 0.7152 * channel(c.g()) + 0.0722 * channel(c.b())
        };
        let (x, y) = (luminance(a), luminance(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    #[test]
    fn every_role_pair_meets_wcag_aa_in_both_themes() {
        for dark in [false, true] {
            let p = palette(dark);
            let v = visuals(dark);
            for (bg_name, background) in [
                ("window", p.window),
                ("surface", p.surface),
                ("surface_alt", p.surface_alt),
                ("control", p.control),
                ("pressed", p.pressed),
                ("panel", v.panel_fill),
                ("popup", v.window_fill),
            ] {
                for (name, color) in [
                    ("text", p.text),
                    ("egui text", v.text_color()),
                    ("muted", p.muted),
                    ("accent", p.accent),
                    ("ok", p.ok),
                    ("warn", p.warn),
                    ("error", p.error),
                ] {
                    let ratio = contrast(color, background);
                    assert!(
                        ratio >= 4.5,
                        "{name} on {bg_name} dark={dark}: {ratio:.2}:1"
                    );
                }
            }
            for (name, fg, bg) in [
                (
                    "selected text",
                    v.selection.stroke.color,
                    v.selection.bg_fill,
                ),
                ("selected muted", p.muted, p.accent_soft),
                ("selected error", p.error, p.accent_soft),
                ("primary button", p.on_accent, p.accent),
                ("pressed text", p.text, p.pressed),
            ] {
                let ratio = contrast(fg, bg);
                assert!(ratio >= 4.5, "{name} dark={dark}: {ratio:.2}:1");
            }
            // Hairlines and icons are non-text: 3:1 against the window for
            // the status icon colors and the focus/hover border.
            for (name, color) in [
                ("accent border", p.accent),
                ("ok icon", p.ok),
                ("muted icon", p.muted),
            ] {
                let ratio = contrast(color, p.window);
                assert!(ratio >= 3.0, "{name} dark={dark}: {ratio:.2}:1");
            }
        }
    }

    #[test]
    fn corners_are_square_and_flat() {
        for dark in [false, true] {
            let v = visuals(dark);
            assert_eq!(v.window_corner_radius, CornerRadius::ZERO);
            assert_eq!(v.widgets.inactive.corner_radius, CornerRadius::ZERO);
            assert_eq!(v.popup_shadow, Shadow::NONE);
        }
    }
}
