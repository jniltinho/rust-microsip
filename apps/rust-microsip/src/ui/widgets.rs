//! Painted widgets that egui has no MicroSIP-like equivalent for: keypad
//! keys with the digit left-aligned, notebook tabs, the ▼ menu button and a
//! few line icons. Every interactive one reports a `WidgetInfo`, so AT-SPI
//! sees a named button.

use super::tokens::{self, Palette};
use eframe::egui::{self, FontId, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Ui, vec2};

/// Fill and border of a painted control in its current interaction state.
fn control_look(ui: &Ui, response: &Response, enabled: bool) -> (egui::Color32, Stroke) {
    let p = tokens::palette(ui.visuals().dark_mode);
    let look = ui.style().interact(response);
    let border = if response.has_focus() {
        Stroke::new(2.0, p.accent)
    } else {
        look.bg_stroke
    };
    let fill = if enabled { look.bg_fill } else { p.control };
    (fill, border)
}

fn sense(enabled: bool) -> Sense {
    if enabled {
        Sense::click()
    } else {
        Sense::hover()
    }
}

/// A keypad key as MicroSIP draws it: the large digit left-aligned with
/// padding and the letters small, right after it. With `center` the label
/// is centred instead (the R / + / C row).
pub fn key(
    ui: &mut Ui,
    size: egui::Vec2,
    digit: &str,
    letters: &str,
    name: &str,
    center: bool,
    enabled: bool,
) -> Response {
    let (rect, response) = ui.allocate_exact_size(size, sense(enabled));
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, name));
    let p = tokens::palette(ui.visuals().dark_mode);
    let (fill, border) = control_look(ui, &response, enabled);
    let painter = ui.painter();
    painter.rect(rect, 0.0, fill, border, StrokeKind::Inside);
    let color = if enabled { p.text } else { p.muted };
    let digit_font = FontId::proportional(if center {
        tokens::NUMBER_SIZE
    } else {
        tokens::DIGIT_SIZE
    });
    let digit = painter.layout_no_wrap(digit.to_owned(), digit_font, color);
    let letters = painter.layout_no_wrap(letters.to_owned(), FontId::proportional(10.0), p.muted);
    let width = digit.size().x
        + if letters.size().x > 0.0 {
            3.0 + letters.size().x
        } else {
            0.0
        };
    let left = if center {
        rect.center().x - width / 2.0
    } else {
        rect.left() + tokens::KEY_PAD
    };
    let top = rect.center().y - digit.size().y / 2.0;
    // Letters sit on the digit's baseline region, slightly below centre.
    let letters_top = top + digit.size().y * 0.62 - letters.size().y / 2.0;
    let digit_width = digit.size().x;
    painter.galley(Pos2::new(left, top), digit, color);
    painter.galley(
        Pos2::new(left + digit_width + 3.0, letters_top),
        letters,
        p.muted,
    );
    response
}

/// One notebook tab. The selected tab is raised and open at the bottom so
/// it joins the panel below; the others sit lower with a full border.
pub fn tab(ui: &mut Ui, label: &str, selected: bool) -> Response {
    let p = tokens::palette(ui.visuals().dark_mode);
    let font = egui::TextStyle::Body.resolve(ui.style());
    let galley = ui.painter().layout_no_wrap(label.to_owned(), font, p.text);
    let size = vec2(galley.size().x + 14.0, tokens::TAB_HEIGHT);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, label)
    });
    let painter = ui.painter();
    let border = Stroke::new(1.0, p.control_border);
    let body = if selected {
        rect
    } else {
        Rect::from_min_max(rect.min + vec2(0.0, 2.0), rect.max)
    };
    let fill = if selected {
        p.window
    } else if response.hovered() {
        p.surface_alt
    } else {
        p.control
    };
    painter.rect_filled(body, 0.0, fill);
    painter.line_segment([body.left_bottom(), body.left_top()], border);
    painter.line_segment([body.left_top(), body.right_top()], border);
    painter.line_segment([body.right_top(), body.right_bottom()], border);
    if !selected {
        painter.line_segment([body.left_bottom(), body.right_bottom()], border);
    }
    if response.has_focus() {
        painter.rect_stroke(
            body.shrink(2.0),
            0.0,
            Stroke::new(1.0, p.accent),
            StrokeKind::Inside,
        );
    }
    let pos = body.center() - galley.size() / 2.0 + vec2(0.0, if selected { -1.0 } else { 0.0 });
    painter.galley(pos, galley, p.text);
    response
}

/// The square ▼ button at the right of the tab row; `name` is its
/// accessible name.
pub fn menu_button(ui: &mut Ui, name: &str) -> Response {
    let size = vec2(tokens::TAB_HEIGHT + 2.0, tokens::TAB_HEIGHT - 4.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, name));
    let p = tokens::palette(ui.visuals().dark_mode);
    let (fill, border) = control_look(ui, &response, true);
    let painter = ui.painter();
    painter.rect(rect, 0.0, fill, border, StrokeKind::Inside);
    let c = rect.center();
    let triangle = vec![
        c + vec2(-4.0, -2.0),
        c + vec2(4.0, -2.0),
        c + vec2(0.0, 3.0),
    ];
    painter.add(egui::Shape::convex_polygon(triangle, p.text, Stroke::NONE));
    response
}

/// The size grip in the bottom-right corner of the status bar (drawn only;
/// the window manager resizes from the border).
pub fn grip(painter: &egui::Painter, corner: Pos2, palette: &Palette) {
    for (dx, dy) in [
        (1.0, 1.0),
        (2.0, 1.0),
        (1.0, 2.0),
        (3.0, 1.0),
        (2.0, 2.0),
        (1.0, 3.0),
    ] {
        let center = corner - vec2(dx * 4.0, dy * 4.0) + vec2(2.0, 2.0);
        painter.rect_filled(
            Rect::from_center_size(center, vec2(2.0, 2.0)),
            0.0,
            palette.muted,
        );
    }
}

/// Magnifier icon in a square of `size`.
pub fn magnifier(ui: &mut Ui, size: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(size, size), Sense::hover());
    let p = tokens::palette(ui.visuals().dark_mode);
    let stroke = Stroke::new(1.6, p.muted);
    let center = rect.center() + vec2(-2.0, -2.0);
    let radius = size * 0.28;
    ui.painter().circle_stroke(center, radius, stroke);
    let start = center + vec2(radius * 0.7, radius * 0.7);
    ui.painter()
        .line_segment([start, start + vec2(radius, radius)], stroke);
}

/// Which line icon [`icon_box`] draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Glyph {
    /// A loudspeaker with one wave.
    Speaker,
    /// A microphone capsule on a stand.
    Microphone,
}

/// A bordered square with a line icon (decorative; not focusable).
pub fn icon_box(ui: &mut Ui, size: f32, glyph: Glyph) {
    let (rect, _) = ui.allocate_exact_size(vec2(size, size), Sense::hover());
    let p = tokens::palette(ui.visuals().dark_mode);
    let painter = ui.painter();
    painter.rect(
        rect,
        0.0,
        p.control,
        Stroke::new(1.0, p.control_border),
        StrokeKind::Inside,
    );
    let c = rect.center();
    let stroke = Stroke::new(1.4, p.text);
    match glyph {
        Glyph::Speaker => {
            let body = vec![
                c + vec2(-5.0, -2.0),
                c + vec2(-2.0, -2.0),
                c + vec2(2.0, -5.0),
                c + vec2(2.0, 5.0),
                c + vec2(-2.0, 2.0),
                c + vec2(-5.0, 2.0),
            ];
            painter.add(egui::Shape::convex_polygon(body, p.text, Stroke::NONE));
            painter.line_segment([c + vec2(4.5, -3.0), c + vec2(5.5, 0.0)], stroke);
            painter.line_segment([c + vec2(5.5, 0.0), c + vec2(4.5, 3.0)], stroke);
        }
        Glyph::Microphone => {
            let capsule = Rect::from_center_size(c + vec2(0.0, -2.0), vec2(5.0, 8.0));
            painter.rect_filled(capsule, 2.5, p.text);
            painter.line_segment([c + vec2(-4.0, 1.0), c + vec2(-3.0, 3.5)], stroke);
            painter.line_segment([c + vec2(4.0, 1.0), c + vec2(3.0, 3.5)], stroke);
            painter.line_segment([c + vec2(-3.0, 3.5), c + vec2(3.0, 3.5)], stroke);
            painter.line_segment([c + vec2(0.0, 3.5), c + vec2(0.0, 6.0)], stroke);
        }
    }
}
