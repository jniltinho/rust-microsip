//! Calls and Contacts lists, laid out like MicroSIP's Logs and Contacts:
//! column headers, one row per call (or contact number) with an icon, and
//! the search box at the bottom with a magnifier. Click selects a row,
//! double-click calls, right-click opens the context menu; the selected row
//! also shows its actions as buttons so the keyboard reaches them.

use super::state::{self, Action, Tab, UiState, format_duration, outcome_text};
use super::{named, search_box, segmented, tokens, widgets};
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, StrokeKind, vec2};
use rust_microsip_core::Direction;
use rust_microsip_storage::{CallOutcome, CallRecord, HistoryFilter};
use std::time::Duration;

const ICON_COLUMN: f32 = 22.0;
const NUMBER_COLUMN: f32 = 68.0;
const TIME_COLUMN: f32 = 44.0;
const DURATION_COLUMN: f32 = 40.0;
const PAD: f32 = 6.0;

fn selection_id(tab: Tab) -> egui::Id {
    egui::Id::new(("rust_microsip_list_selection", tab.label()))
}

/// Number to put into a new contact after [`Action::NewContact`] opened the
/// editor (read back by the window after dispatching).
pub(super) fn prefill_id() -> egui::Id {
    egui::Id::new("rust_microsip_contact_prefill")
}

/// What the call row icon shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    /// Incoming and answered.
    Incoming,
    /// Outgoing and answered.
    Outgoing,
    /// Incoming, never answered.
    Missed,
    /// Outgoing, never answered (busy, canceled, failed...).
    Unanswered,
}

pub(super) const fn kind(call: &CallRecord) -> Kind {
    match (call.direction, call.outcome) {
        (Direction::Incoming, CallOutcome::Missed | CallOutcome::NoAnswer) => Kind::Missed,
        // Rejected or interrupted here: shown as incoming, outcome in the text.
        (Direction::Incoming, _) => Kind::Incoming,
        (Direction::Outgoing, CallOutcome::Answered) => Kind::Outgoing,
        (Direction::Outgoing, _) => Kind::Unanswered,
    }
}

/// Compact age for the time column ("now", "5 min", "3 h", "2 d").
pub(super) fn short_age(seconds: i64) -> String {
    match seconds.max(0) {
        s if s < 60 => "now".to_owned(),
        s if s < 3600 => format!("{} min", s / 60),
        s if s < 86_400 => format!("{} h", s / 3600),
        s => format!("{} d", s / 86_400),
    }
}

/// Screen-reader name of a call row: everything the columns and icon show.
pub(super) fn row_name(call: &CallRecord, name: Option<&str>, age: &str) -> String {
    let direction = match call.direction {
        Direction::Incoming => "incoming",
        Direction::Outgoing => "outgoing",
    };
    let who = match name {
        Some(name) => format!("{name} {}", call.remote),
        None => call.remote.clone(),
    };
    let mut text = format!(
        "{} {direction} call, {who}, {age}",
        outcome_text(call.outcome)
    );
    if let Some(ms) = call.duration_ms {
        text.push_str(&format!(", {}", format_duration(Duration::from_millis(ms))));
    }
    text
}

/// Contact name for a number, when a listed contact has it.
// ponytail: linear scan of the listed contacts per row; index by number if
// lists grow past a few hundred entries.
fn contact_name<'a>(st: &'a UiState, number: &str) -> Option<&'a str> {
    st.contacts
        .iter()
        .find(|c| c.numbers.iter().any(|n| n.number == number))
        .map(|c| c.name.as_str())
}

/// A row command, shared by the context menu and the selected-row strip.
#[derive(Clone, Debug)]
enum RowAction {
    Call(String),
    Copy(String),
    AddContact(String),
    Edit(i64),
    DeleteCall(i64),
    DeleteContact(i64),
}

/// `(menu label, strip label, accessible name, action)`.
type RowItem = (&'static str, &'static str, &'static str, RowAction);

fn run(ctx: &egui::Context, action: RowAction, out: &mut Vec<Action>) {
    match action {
        RowAction::Call(number) => out.push(Action::DialNumber(number)),
        RowAction::Copy(number) => ctx.copy_text(number),
        RowAction::AddContact(number) => {
            ctx.data_mut(|d| d.insert_temp(prefill_id(), number));
            out.push(Action::NewContact);
        }
        RowAction::Edit(id) => out.push(Action::EditContact(id)),
        RowAction::DeleteCall(id) => out.push(Action::DeleteCall(id)),
        RowAction::DeleteContact(id) => out.push(Action::DeleteContact(id)),
    }
}

/// One text cell: text, color, right-aligned.
type Cell = (String, Color32, bool);

/// Column rectangles: the first column takes the room the fixed ones leave.
fn columns(rect: Rect, fixed: &[f32]) -> Vec<Rect> {
    let mut right = rect.right() - PAD;
    let mut out = Vec::with_capacity(fixed.len() + 1);
    for width in fixed.iter().rev() {
        out.push(Rect::from_x_y_ranges(
            (right - width)..=right,
            rect.y_range(),
        ));
        right -= width + PAD;
    }
    let left = rect.left() + ICON_COLUMN;
    out.push(Rect::from_x_y_ranges(
        left..=right.max(left + 1.0),
        rect.y_range(),
    ));
    out.reverse();
    out
}

/// Draw `text` on one line inside `rect`, truncated with an ellipsis.
fn text_in(
    painter: &egui::Painter,
    rect: Rect,
    text: &str,
    font: egui::FontId,
    color: Color32,
    right: bool,
) {
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_owned(), font, color);
    job.wrap = egui::text::TextWrapping::truncate_at_width(rect.width().max(1.0));
    let galley = painter.layout_job(job);
    let x = if right {
        rect.right() - galley.size().x
    } else {
        rect.left()
    };
    painter.galley(
        Pos2::new(x, rect.center().y - galley.size().y / 2.0),
        galley,
        color,
    );
}

/// Column headers (MicroSIP's "Name | Number ..."), with column rules.
fn header(ui: &mut egui::Ui, palette: &tokens::Palette, titles: &[&str], fixed: &[f32]) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(width, 20.0), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, palette.surface);
    let font = egui::TextStyle::Small.resolve(ui.style());
    let rule = Stroke::new(1.0, palette.hairline);
    for (index, (column, title)) in columns(rect, fixed).into_iter().zip(titles).enumerate() {
        text_in(&painter, column, title, font.clone(), palette.muted, false);
        if index > 0 {
            painter.vline(column.left() - PAD / 2.0, rect.y_range(), rule);
        }
    }
    painter.hline(rect.x_range(), rect.bottom() - 0.5, rule);
}

/// A selectable list row: background states, icon, cells, a11y name.
fn row(
    ui: &mut egui::Ui,
    id: egui::Id,
    selected: bool,
    name: &str,
    fixed: &[f32],
    cells: &[Cell],
    icon: impl FnOnce(&egui::Painter, Rect),
) -> egui::Response {
    let palette = &tokens::palette(ui.visuals().dark_mode);
    let width = ui.available_width();
    // A stable id keeps focus and the context menu on the same row when the
    // action strip shifts the rows below the selection.
    let (_, rect) = ui.allocate_space(vec2(width, tokens::ROW_HEIGHT));
    let response = ui.interact(rect, id, Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, selected, name)
    });
    let painter = ui.painter_at(rect);
    if selected {
        painter.rect_filled(rect, 0.0, palette.accent_soft);
    } else if response.hovered() {
        painter.rect_filled(rect, 0.0, palette.surface_alt);
    }
    if response.has_focus() {
        painter.rect_stroke(
            rect,
            0.0,
            Stroke::new(2.0, palette.accent),
            StrokeKind::Inside,
        );
    }
    painter.hline(
        rect.x_range(),
        rect.bottom() - 0.5,
        Stroke::new(1.0, palette.hairline),
    );
    let icon_rect = Rect::from_center_size(
        Pos2::new(rect.left() + ICON_COLUMN / 2.0, rect.center().y),
        vec2(10.0, 10.0),
    );
    icon(&painter, icon_rect);
    let body = egui::TextStyle::Body.resolve(ui.style());
    let small = egui::TextStyle::Small.resolve(ui.style());
    for (index, (column, (text, color, right))) in
        columns(rect, fixed).into_iter().zip(cells).enumerate()
    {
        let font = if index == 0 {
            body.clone()
        } else {
            small.clone()
        };
        text_in(&painter, column, text, font, *color, *right);
    }
    response
}

/// Select on click, run the context menu, and show the action strip under
/// the selected row.
fn interact(
    ui: &mut egui::Ui,
    response: &egui::Response,
    tab: Tab,
    id: i64,
    selected: bool,
    items: Vec<RowItem>,
    out: &mut Vec<Action>,
) {
    if response.clicked() || response.secondary_clicked() {
        ui.data_mut(|d| d.insert_temp(selection_id(tab), id));
    }
    if response.double_clicked()
        && let Some((_, _, _, RowAction::Call(number))) = items.first()
    {
        out.push(Action::DialNumber(number.clone()));
    }
    response.context_menu(|ui| {
        for (label, _, _, item) in &items {
            if ui.button(*label).clicked() {
                run(ui.ctx(), item.clone(), out);
                ui.close();
            }
        }
    });
    if !selected {
        return;
    }
    let gap = ui.spacing().item_spacing.x;
    let count = items.len() as f32;
    let width = (ui.available_width() - ICON_COLUMN - (count - 1.0) * gap) / count;
    ui.horizontal(|ui| {
        ui.add_space(ICON_COLUMN - gap);
        for (_, label, name, item) in items {
            let button = egui::Button::new(egui::RichText::new(label).small())
                .min_size(vec2(width, tokens::TOGGLE_HEIGHT));
            if named(ui.add(button), name).on_hover_text(name).clicked() {
                run(ui.ctx(), item, out);
            }
        }
    });
    ui.add_space(tokens::SPACING);
}

/// Bottom bar of the Calls and Contacts tabs: magnifier and search field,
/// then Clear (Calls) or Add and Import (Contacts).
pub(super) fn footer(ui: &mut egui::Ui, st: &mut UiState, out: &mut Vec<Action>) {
    ui.add_space(tokens::SPACING);
    ui.horizontal(|ui| {
        let height = ui.spacing().interact_size.y;
        widgets::magnifier(ui, height);
        let buttons = if st.tab == Tab::Calls { 52.0 } else { 110.0 };
        let width = (ui.available_width() - buttons).max(60.0);
        if st.tab == Tab::Calls {
            if search_box(ui, &mut st.history_search, width) {
                out.push(Action::SearchHistory);
            }
            if ui
                .add_enabled(!st.history.is_empty(), egui::Button::new("Clear"))
                .on_hover_text("Delete the whole history")
                .clicked()
            {
                out.push(Action::ClearHistory);
            }
        } else {
            if search_box(ui, &mut st.contact_search, width) {
                out.push(Action::SearchContacts);
            }
            if ui.button("Add").on_hover_text("New contact").clicked() {
                out.push(Action::NewContact);
            }
            if ui
                .button("Import")
                .on_hover_text("Import contacts from a CSV file")
                .clicked()
            {
                out.push(Action::OpenImport);
            }
        }
    });
    ui.add_space(tokens::SPACING);
}

fn empty(ui: &mut egui::Ui, palette: &tokens::Palette, text: &str) {
    ui.add_space(tokens::SPACING * 4.0);
    ui.vertical_centered(|ui| {
        ui.label(egui::RichText::new(text).color(palette.muted));
    });
}

pub(super) fn calls(
    ui: &mut egui::Ui,
    st: &mut UiState,
    palette: &tokens::Palette,
    out: &mut Vec<Action>,
) {
    let filters = [
        (HistoryFilter::All, "All"),
        (HistoryFilter::Incoming, "In"),
        (HistoryFilter::Outgoing, "Out"),
        (HistoryFilter::Missed, "Missed"),
    ];
    let options: Vec<_> = filters
        .iter()
        .map(|(f, label)| (*label, st.history_filter == *f, None))
        .collect();
    if let Some(index) = segmented(ui, palette, &options) {
        out.push(Action::SetHistoryFilter(filters[index].0));
    }
    let fixed = [NUMBER_COLUMN, TIME_COLUMN, DURATION_COLUMN];
    header(ui, palette, &["Name", "Number", "Time", "Duration"], &fixed);
    if st.history.is_empty() {
        return empty(ui, palette, "No calls.");
    }
    let selected = ui.data(|d| d.get_temp::<i64>(selection_id(Tab::Calls)));
    let now = state::utc_now();
    for entry in &st.history {
        let call = &entry.call;
        let name = contact_name(st, &call.remote);
        let age = now - call.started_utc;
        let kind = kind(call);
        let main = if kind == Kind::Missed {
            palette.error
        } else {
            palette.text
        };
        let duration = call.duration_ms.map_or_else(
            || "\u{2014}".to_owned(),
            |ms| format_duration(Duration::from_millis(ms)),
        );
        let cells = [
            (name.unwrap_or(&call.remote).to_owned(), main, false),
            (call.remote.clone(), palette.muted, false),
            (short_age(age), palette.muted, true),
            (duration, palette.muted, true),
        ];
        let is_selected = selected == Some(entry.id);
        let label = row_name(call, name, &state::age_text(age));
        let response = row(
            ui,
            egui::Id::new(("rust_microsip_call_row", entry.id)),
            is_selected,
            &label,
            &fixed,
            &cells,
            |painter, rect| {
                direction_icon(painter, rect, kind, palette);
            },
        );
        let secure = if call.secure { "SRTP" } else { "RTP" };
        let response = response.on_hover_text(format!(
            "{} \u{00b7} {} \u{00b7} {secure}\nDouble-click: call back. Right-click: more actions",
            outcome_text(call.outcome),
            state::age_text(age)
        ));
        let number = &call.remote;
        let items = vec![
            ("Call", "Call", "Call back", RowAction::Call(number.clone())),
            (
                "Copy number",
                "Copy",
                "Copy number",
                RowAction::Copy(number.clone()),
            ),
            (
                "Add to contacts",
                "Add",
                "Add to contacts",
                RowAction::AddContact(number.clone()),
            ),
            (
                "Delete\u{2026}",
                "Delete",
                "Delete entry",
                RowAction::DeleteCall(entry.id),
            ),
        ];
        interact(ui, &response, Tab::Calls, entry.id, is_selected, items, out);
    }
}

pub(super) fn contacts(
    ui: &mut egui::Ui,
    st: &mut UiState,
    palette: &tokens::Palette,
    out: &mut Vec<Action>,
) {
    let number_width = (ui.available_width() * 0.4).clamp(NUMBER_COLUMN, 140.0);
    let fixed = [number_width];
    header(ui, palette, &["Name", "Number"], &fixed);
    if st.contacts.is_empty() {
        return empty(ui, palette, "No contacts.");
    }
    let selected = ui.data(|d| d.get_temp::<i64>(selection_id(Tab::Contacts)));
    // Row ids: contact id and number index, packed (at most 255 numbers).
    for contact in &st.contacts {
        for (index, number) in contact.numbers.iter().enumerate() {
            let row_id = contact.id * 256 + i64::try_from(index).unwrap_or(0);
            let is_selected = selected == Some(row_id);
            let label = match &number.label {
                Some(label) if !label.is_empty() => {
                    format!("{} {} ({label})", contact.name, number.number)
                }
                _ => format!("{} {}", contact.name, number.number),
            };
            let name = if contact.favorite {
                format!("Favorite, {label}")
            } else {
                label
            };
            let cells = [
                (contact.name.clone(), palette.text, false),
                (number.number.clone(), palette.muted, false),
            ];
            let favorite = contact.favorite;
            let response = row(
                ui,
                egui::Id::new(("rust_microsip_contact_row", row_id)),
                is_selected,
                &name,
                &fixed,
                &cells,
                |painter, rect| {
                    person_icon(painter, rect, favorite, palette);
                },
            )
            .on_hover_text("Double-click: call. Right-click: more actions");
            let items = vec![
                (
                    "Call",
                    "Call",
                    "Call number",
                    RowAction::Call(number.number.clone()),
                ),
                (
                    "Copy number",
                    "Copy",
                    "Copy number",
                    RowAction::Copy(number.number.clone()),
                ),
                (
                    "Edit\u{2026}",
                    "Edit",
                    "Edit contact",
                    RowAction::Edit(contact.id),
                ),
                (
                    "Delete\u{2026}",
                    "Delete",
                    "Delete contact",
                    RowAction::DeleteContact(contact.id),
                ),
            ];
            interact(
                ui,
                &response,
                Tab::Contacts,
                row_id,
                is_selected,
                items,
                out,
            );
        }
    }
}

/// A diagonal arrow: down-left for incoming, up-right for outgoing; missed
/// adds a short bar under the arrow, so the state never rests on color alone.
fn direction_icon(painter: &egui::Painter, rect: Rect, kind: Kind, palette: &tokens::Palette) {
    let (color, incoming) = match kind {
        Kind::Incoming => (palette.ok, true),
        Kind::Outgoing => (palette.accent, false),
        Kind::Missed => (palette.error, true),
        Kind::Unanswered => (palette.muted, false),
    };
    let stroke = Stroke::new(1.6, color);
    let head = rect.width() * 0.55;
    let (from, to) = if incoming {
        (rect.right_top(), rect.left_bottom())
    } else {
        (rect.left_bottom(), rect.right_top())
    };
    painter.line_segment([from, to], stroke);
    let back = if incoming { 1.0 } else { -1.0 };
    painter.line_segment([to, to + vec2(back * head, 0.0)], stroke);
    painter.line_segment([to, to + vec2(0.0, -back * head)], stroke);
    if kind == Kind::Missed {
        painter.hline(rect.x_range(), rect.bottom() + 3.0, stroke);
    }
}

/// A head-and-shoulders outline; favorites are filled in the accent color.
fn person_icon(painter: &egui::Painter, rect: Rect, favorite: bool, palette: &tokens::Palette) {
    let color = if favorite {
        palette.accent
    } else {
        palette.muted
    };
    let head = Pos2::new(rect.center().x, rect.top() + 2.5);
    let shoulders = Rect::from_min_max(
        Pos2::new(rect.left(), rect.top() + 6.0),
        Pos2::new(rect.right(), rect.bottom() + 1.0),
    );
    if favorite {
        painter.circle_filled(head, 2.8, color);
        painter.rect_filled(shoulders, 3.0, color);
    } else {
        let stroke = Stroke::new(1.3, color);
        painter.circle_stroke(head, 2.8, stroke);
        painter.rect_stroke(shoulders, 3.0, stroke, StrokeKind::Inside);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(direction: Direction, outcome: CallOutcome, duration_ms: Option<u64>) -> CallRecord {
        CallRecord {
            session_key: "test:1".into(),
            started_utc: 0,
            answered_utc: None,
            ended_utc: None,
            direction,
            remote: "1002".into(),
            outcome,
            duration_ms,
            secure: false,
        }
    }

    #[test]
    fn icon_kind_follows_direction_and_outcome() {
        use CallOutcome::*;
        for (direction, outcome, expected) in [
            (Direction::Incoming, Answered, Kind::Incoming),
            (Direction::Incoming, Missed, Kind::Missed),
            (Direction::Incoming, NoAnswer, Kind::Missed),
            (Direction::Incoming, Rejected, Kind::Incoming),
            (Direction::Outgoing, Answered, Kind::Outgoing),
            (Direction::Outgoing, Busy, Kind::Unanswered),
            (Direction::Outgoing, Canceled, Kind::Unanswered),
        ] {
            assert_eq!(
                kind(&record(direction, outcome, None)),
                expected,
                "{outcome:?}"
            );
        }
    }

    #[test]
    fn short_age_fits_the_time_column() {
        assert_eq!(short_age(-5), "now");
        assert_eq!(short_age(59), "now");
        assert_eq!(short_age(60 * 14), "14 min");
        assert_eq!(short_age(3600 * 3), "3 h");
        assert_eq!(short_age(86_400 * 2), "2 d");
    }

    #[test]
    fn row_name_reads_every_column() {
        let call = record(Direction::Outgoing, CallOutcome::Answered, Some(84_000));
        assert_eq!(
            row_name(&call, Some("Ana Lima"), "5 min ago"),
            "Answered outgoing call, Ana Lima 1002, 5 min ago, 1:24"
        );
        let missed = record(Direction::Incoming, CallOutcome::Missed, None);
        assert_eq!(
            row_name(&missed, None, "just now"),
            "Missed incoming call, 1002, just now"
        );
    }

    #[test]
    fn columns_give_the_first_column_the_rest() {
        let rect = Rect::from_min_size(Pos2::ZERO, vec2(300.0, 30.0));
        let cols = columns(rect, &[NUMBER_COLUMN, TIME_COLUMN]);
        assert_eq!(cols.len(), 3);
        assert!(cols[0].left() >= ICON_COLUMN && cols[0].right() < cols[1].left());
        assert!((cols[2].right() - (300.0 - PAD)).abs() < 0.01);
        assert!(cols[1].right() < cols[2].left());
    }
}
