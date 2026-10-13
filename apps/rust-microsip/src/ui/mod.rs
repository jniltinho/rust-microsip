//! Native desktop window (eframe/egui, wgpu) in the compact MicroSIP layout.
//!
//! The window talks to the engine only through [`Backend`]: commands go out
//! with [`Backend::send`], events come back through [`Backend::poll`].
//! Presentation logic lives in the GPU-free `state` module; this module only
//! lays out widgets and routes clicks and keys into it.

mod state;

pub use state::{Theme, UiSettings};

use rust_microsip_audio::AudioDevices;
use rust_microsip_storage::{HistoryFilter, Store};

use eframe::egui;
use rust_microsip_core::{
    CallPhase, Command, Direction, Event, MediaSecurity, Notice, RegistrationState, Transport,
};
use state::{
    Action, ContactForm, ImportForm, MAX_NUMBER_CHARS, SaveFn, Tab, UiState, View, format_duration,
    outcome_text, phase_text, registration_text, transfer_text,
};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Engine connection used by the window. Both calls must not block.
pub trait Backend {
    /// Queue a command.
    ///
    /// # Errors
    /// Returns [`Notice::Overloaded`] when the queue is full, or another
    /// notice when the command is refused immediately.
    fn send(&self, command: Command) -> Result<(), Notice>;
    /// Drain the events available now, in order.
    fn poll(&self) -> Vec<Event>;
    /// Receive the egui context once, so the engine thread can call
    /// [`egui::Context::request_repaint`] after queueing events.
    fn attach_context(&self, ctx: &egui::Context) {
        let _ = ctx;
    }
    /// List audio devices for settings; `None` when unavailable. Called on
    /// opening settings, never per frame.
    fn audio_devices(&self) -> Option<AudioDevices> {
        None
    }
    /// Requests from outside the window (a second instance or a `sip:` link).
    fn external(&self) -> Vec<External> {
        Vec::new()
    }
}

/// A request from outside the window. A dial request only fills the number:
/// the user still confirms with Call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum External {
    /// Bring the window to the front.
    Activate,
    /// Bring the window to the front and fill this number.
    Dial(rust_microsip_core::DialTarget),
}

/// Persists secret-free settings; returns a user-safe message on failure.
pub type SaveSettings = Box<SaveFn>;

/// Application name and window title.
pub const APP_NAME: &str = "rust-microsip";
const FONT_KEY: &str = "noto-sans-regular";
const FONT: &[u8] = include_bytes!("../../assets/NotoSans-Regular.ttf");
const FONT_NOTICE: &str =
    "Noto Sans Regular 2.008. Copyright 2015-2021 Google LLC. All Rights Reserved.";
const FONT_LICENSE: &str = include_str!("../../assets/OFL.txt");
const PROJECT_NOTICE: &str = "rust-microsip is free software: you can redistribute it and/or modify it \
under the terms of the GNU General Public License as published by the Free Software Foundation, \
either version 3 of the License, or (at your option) any later version (GPL-3.0-or-later). It is \
distributed WITHOUT ANY WARRANTY. The embedded font is licensed separately under the SIL Open \
Font License 1.1.";
const PASSWORD_NOTE: &str = "The password is kept in memory for this session only; \
remembering it is not available in this release.";
const KEYPAD: [char; 12] = ['1', '2', '3', '4', '5', '6', '7', '8', '9', '*', '0', '#'];
const KEYPAD_LETTERS: [&str; 12] = [
    "", "ABC", "DEF", "GHI", "JKL", "MNO", "PQRS", "TUV", "WXYZ", "", "", "",
];

/// Visual tokens, in one place. Text colors keep at least 4.5:1 contrast on
/// the default egui panel backgrounds of each theme.
mod tokens {
    use eframe::egui::Color32;
    pub const KEY_HEIGHT: f32 = 36.0;
    pub const CONTROL_HEIGHT: f32 = 28.0;
    pub const CALL_HEIGHT: f32 = 34.0;
    pub const TOGGLE_HEIGHT: f32 = 22.0;
    pub const SPACING: f32 = 4.0;
    pub const NUMBER_SIZE: f32 = 18.0;
    pub const DIGIT_SIZE: f32 = 20.0;

    /// Semantic colors for one theme.
    pub struct Palette {
        pub ok: Color32,
        pub warn: Color32,
        pub error: Color32,
    }
    pub const fn palette(dark: bool) -> Palette {
        if dark {
            Palette {
                ok: Color32::from_rgb(0x81, 0xC7, 0x84),
                warn: Color32::from_rgb(0xFF, 0xB7, 0x4D),
                error: Color32::from_rgb(0xEF, 0x9A, 0x9A),
            }
        } else {
            Palette {
                ok: Color32::from_rgb(0x1B, 0x5E, 0x20),
                warn: Color32::from_rgb(0x8A, 0x4B, 0x00),
                error: Color32::from_rgb(0xB0, 0x00, 0x20),
            }
        }
    }
}

fn number_id() -> egui::Id {
    egui::Id::new("rust_microsip_number")
}
fn password_id() -> egui::Id {
    egui::Id::new("rust_microsip_password")
}
fn transfer_id() -> egui::Id {
    egui::Id::new("rust_microsip_transfer")
}

fn theme_preference(theme: Theme) -> egui::ThemePreference {
    match theme {
        Theme::System => egui::ThemePreference::System,
        Theme::Light => egui::ThemePreference::Light,
        Theme::Dark => egui::ThemePreference::Dark,
    }
}

fn install_font(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::empty();
    fonts.font_data.insert(
        FONT_KEY.to_owned(),
        Arc::new(egui::FontData::from_static(FONT)),
    );
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts.families.insert(family, vec![FONT_KEY.to_owned()]);
    }
    ctx.set_fonts(fonts);
}

/// Window options: 300x460 compact window, resizable, wgpu renderer.
pub fn native_options() -> eframe::NativeOptions {
    eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(APP_NAME)
            .with_app_id(APP_NAME)
            .with_inner_size([300.0, 460.0])
            // Smaller than the default size so 200% scaling still fits
            // 1024x768; the content scrolls.
            .with_min_inner_size([260.0, 360.0])
            .with_resizable(true),
        renderer: eframe::Renderer::Wgpu,
        persist_window: false,
        ..Default::default()
    }
}

/// History/contacts database for the window. Without a store nothing is
/// recorded; `warning` stays visible in the Calls and Contacts tabs.
#[derive(Debug, Default)]
pub struct Storage {
    /// Open store (on disk, or in memory as a fallback).
    pub store: Option<Store>,
    /// User-safe text explaining degraded persistence.
    pub warning: Option<String>,
}

/// Open the window and block until it closes.
///
/// # Errors
/// Returns the eframe error when the native runtime fails to start.
pub fn run(
    backend: Box<dyn Backend>,
    settings: UiSettings,
    save: SaveSettings,
    storage: Storage,
) -> eframe::Result {
    eframe::run_native(
        APP_NAME,
        native_options(),
        Box::new(move |cc| {
            Ok(Box::new(SoftphoneApp::new(
                cc, backend, settings, save, storage,
            )))
        }),
    )
}

/// The softphone window.
pub struct SoftphoneApp {
    backend: Box<dyn Backend>,
    save: SaveSettings,
    state: UiState,
    applied_theme: Theme,
    focus_number: bool,
}

impl SoftphoneApp {
    /// Build the window: installs the embedded font, applies the theme and
    /// hands the egui context to the backend for repaint requests.
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        backend: Box<dyn Backend>,
        settings: UiSettings,
        save: SaveSettings,
        storage: Storage,
    ) -> Self {
        Self::with_context(&cc.egui_ctx, backend, settings, save, storage)
    }

    fn with_context(
        ctx: &egui::Context,
        backend: Box<dyn Backend>,
        settings: UiSettings,
        save: SaveSettings,
        storage: Storage,
    ) -> Self {
        install_font(ctx);
        ctx.options_mut(|o| o.fallback_theme = egui::Theme::Light);
        ctx.set_theme(theme_preference(settings.theme));
        backend.attach_context(ctx);
        let applied_theme = settings.theme;
        let mut state = UiState::new(settings);
        state.attach_store(storage.store, storage.warning, Instant::now());
        Self {
            backend,
            save,
            applied_theme,
            state,
            focus_number: true,
        }
    }

    fn dispatch(&mut self, ctx: &egui::Context, action: Action) {
        let leaving_settings = self.state.view == View::Settings;
        self.state
            .dispatch(action, &*self.backend, &mut *self.save, Instant::now());
        if leaving_settings && self.state.view != View::Settings {
            // Drop egui's copy (undo history) of the masked password.
            ctx.data_mut(|d| d.remove::<egui::text_edit::TextEditState>(password_id()));
        }
        if self.state.theme != self.applied_theme {
            self.applied_theme = self.state.theme;
            ctx.set_theme(theme_preference(self.state.theme));
        }
        if self.state.quitting {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn poll(&mut self, ctx: &egui::Context) {
        let now = Instant::now();
        for event in self.backend.poll() {
            self.state.apply_event(event, now);
        }
        for request in self.backend.external() {
            if let External::Dial(target) = request
                && self.state.call.is_none()
            {
                self.state.number = target.as_str().to_owned();
                self.dispatch(ctx, Action::SelectTab(Tab::Phone));
                self.focus_number = true;
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }
        if ctx.input(|i| i.viewport().close_requested()) && !self.state.quitting {
            self.dispatch(ctx, Action::Quit);
        }
        if self
            .state
            .call
            .as_ref()
            .is_some_and(|c| c.established_at.is_some())
        {
            ctx.request_repaint_after(Duration::from_secs(1));
        }
        if let Some(left) = self.state.notice_remaining(now) {
            ctx.request_repaint_after(left);
        }
    }

    fn keyboard(&mut self, ctx: &egui::Context) -> Vec<Action> {
        let mut out = Vec::new();
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::L)) {
            out.push(Action::SelectTab(Tab::Phone));
            self.focus_number = true;
        }
        let focused = ctx.memory(|m| m.focused());
        let typing_elsewhere = ctx.text_edit_focused() && focused != Some(number_id());
        if self.state.view != View::Main {
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
                out.push(Action::CloseView);
            }
            return out;
        }
        if typing_elsewhere {
            return out;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)) {
            out.push(Action::Call);
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            out.push(Action::End);
        }
        if !ctx.text_edit_focused() {
            ctx.input(|i| {
                for event in &i.events {
                    if let egui::Event::Text(text) = event {
                        out.extend(
                            text.chars()
                                .filter(|c| c.is_ascii_digit() || matches!(c, '*' | '#'))
                                .map(Action::Key),
                        );
                    }
                }
            });
        }
        out
    }

    /// Lay out one frame. Kept separate from [`eframe::App::ui`] so it can be
    /// exercised headlessly.
    fn draw(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let now = Instant::now();
        let mut out = self.keyboard(&ctx);
        let st = &mut self.state;
        let palette = tokens::palette(ui.visuals().dark_mode);
        ui.spacing_mut().item_spacing = egui::vec2(tokens::SPACING, tokens::SPACING);
        egui::Panel::top("rust_microsip_tabs").show(ui, |ui| tab_strip(ui, st, &mut out));
        egui::Panel::bottom("rust_microsip_status").show(ui, |ui| status_bar(ui, st, &palette));
        let focus_number = std::mem::take(&mut self.focus_number);
        egui::CentralPanel::default().show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                incoming_banner(ui, st, &mut out);
                call_status(ui, st, &palette, now);
                if let Some(text) = st.notice(now) {
                    ui.label(egui::RichText::new(text).color(palette.warn));
                }
                if let Some(confirm) = st.confirm {
                    ui.group(|ui| {
                        ui.label(egui::RichText::new(confirm.question()).strong());
                        ui.horizontal(|ui| {
                            if ui.button("Delete").clicked() {
                                out.push(Action::Confirm);
                            }
                            if ui.button("Cancel").clicked() {
                                out.push(Action::CancelConfirm);
                            }
                        });
                    });
                }
                if matches!(st.tab, Tab::Calls | Tab::Contacts)
                    && !matches!(st.view, View::Settings | View::About)
                    && let Some(text) = &st.storage_warning
                {
                    ui.label(egui::RichText::new(text).small().color(palette.warn));
                }
                match st.view {
                    View::Settings => settings(ui, st, &palette, &mut out),
                    View::About => about(ui, &mut out),
                    View::ContactEdit => contact_editor(ui, st, &mut out),
                    View::Import => import(ui, st, &palette, &mut out),
                    View::Main => match st.tab {
                        Tab::Phone => phone(ui, st, focus_number, &mut out),
                        Tab::Calls => calls(ui, st, &mut out),
                        Tab::Contacts => contacts(ui, st, &mut out),
                    },
                }
            });
        });
        for action in out {
            self.dispatch(&ctx, action);
        }
    }
}

impl eframe::App for SoftphoneApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.draw(ui);
    }
}

/// Button that is disabled with a hover reason when `enabled` is an error.
fn guarded(ui: &mut egui::Ui, button: egui::Button<'_>, enabled: Result<(), &str>) -> bool {
    match enabled {
        Ok(()) => ui.add(button).clicked(),
        Err(reason) => {
            ui.add_enabled(false, button).on_disabled_hover_text(reason);
            false
        }
    }
}

fn tab_strip(ui: &mut egui::Ui, st: &UiState, out: &mut Vec<Action>) {
    ui.horizontal(|ui| {
        for tab in Tab::ALL {
            let selected =
                st.tab == tab && matches!(st.view, View::Main | View::ContactEdit | View::Import);
            if ui
                .add(egui::Button::new(tab.label()).selected(selected))
                .clicked()
            {
                out.push(Action::SelectTab(tab));
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.menu_button("Menu", |ui| {
                for (label, action) in [
                    ("Settings\u{2026}", Action::OpenSettings),
                    ("Unregister", Action::Unregister),
                    ("About and licenses\u{2026}", Action::OpenAbout),
                    ("Quit", Action::Quit),
                ] {
                    if ui.button(label).clicked() {
                        out.push(action);
                        ui.close();
                    }
                }
            });
        });
    });
}

fn status_bar(ui: &mut egui::Ui, st: &UiState, palette: &tokens::Palette) {
    let color = match st.registration {
        RegistrationState::Registered => palette.ok,
        RegistrationState::Failed(_) => palette.error,
        RegistrationState::Registering | RegistrationState::Disabled => ui.visuals().text_color(),
    };
    ui.horizontal_wrapped(|ui| {
        ui.label(egui::RichText::new("\u{2022}").color(color));
        ui.label(egui::RichText::new(registration_text(st.registration)).color(color));
        if st.dnd {
            ui.label(egui::RichText::new("DND on").strong());
        }
    });
    ui.label(egui::RichText::new(st.account_text()).small());
}

fn incoming_banner(ui: &mut egui::Ui, st: &UiState, out: &mut Vec<Action>) {
    let Some(call) = st.call.as_ref().filter(|c| c.is_incoming_ringing()) else {
        return;
    };
    egui::Frame::group(ui.style())
        .fill(ui.visuals().selection.bg_fill)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(
                egui::RichText::new("Incoming call")
                    .strong()
                    .color(ui.visuals().selection.stroke.color),
            );
            ui.label(
                egui::RichText::new(&call.snapshot.remote)
                    .size(tokens::NUMBER_SIZE)
                    .color(ui.visuals().selection.stroke.color),
            );
            ui.horizontal(|ui| {
                let size = egui::vec2(0.0, tokens::CONTROL_HEIGHT);
                if ui
                    .add(egui::Button::new("Answer").min_size(size))
                    .on_hover_text("Enter")
                    .clicked()
                {
                    out.push(Action::Call);
                }
                if ui
                    .add(egui::Button::new("Reject").min_size(size))
                    .on_hover_text("Esc")
                    .clicked()
                {
                    out.push(Action::End);
                }
            });
        });
}

fn call_status(ui: &mut egui::Ui, st: &UiState, palette: &tokens::Palette, now: Instant) {
    let Some(call) = st.call.as_ref().filter(|c| !c.is_incoming_ringing()) else {
        return;
    };
    let snap = &call.snapshot;
    ui.group(|ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            let arrow = if snap.direction == Direction::Outgoing {
                "Out"
            } else {
                "In"
            };
            ui.label(egui::RichText::new(arrow).small());
            ui.label(egui::RichText::new(&snap.remote).strong());
            if let Some(d) = call.duration(now) {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(format_duration(d));
                });
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.label(phase_text(snap));
            if snap.muted {
                ui.label(egui::RichText::new("Muted").strong());
            }
            if snap.secure {
                ui.label(egui::RichText::new("Secure (SRTP)").color(palette.ok))
                    .on_hover_text("Media is encrypted with SRTP");
            } else {
                ui.label(egui::RichText::new("Unencrypted (RTP)").color(palette.warn))
                    .on_hover_text("Audio is not encrypted");
            }
        });
        if let Some(transfer) = snap.transfer {
            ui.label(transfer_text(transfer));
        }
    });
}

fn phone(ui: &mut egui::Ui, st: &mut UiState, focus_number: bool, out: &mut Vec<Action>) {
    let dtmf = st.keypad_sends_dtmf();
    let in_call = st.call.is_some();
    let edit = egui::TextEdit::singleline(&mut st.number)
        .id(number_id())
        .char_limit(MAX_NUMBER_CHARS)
        .font(egui::FontId::proportional(tokens::NUMBER_SIZE))
        .desired_width(f32::INFINITY)
        .hint_text("Number");
    // During a call the status panel above shows the number; hiding the
    // field keeps the compact window free of scrolling.
    if !in_call {
        let response = ui.add(edit).on_hover_text("Number to call (Ctrl+L)");
        if focus_number {
            response.request_focus();
            response.scroll_to_me(None);
        }
    }
    let gap = ui.spacing().item_spacing.x;
    let width = (ui.available_width() - 2.0 * gap) / 3.0;
    for row in KEYPAD.chunks(3).zip(KEYPAD_LETTERS.chunks(3)) {
        ui.horizontal(|ui| {
            for (key, letters) in row.0.iter().zip(row.1) {
                let label = (
                    egui::RichText::new(key.to_string()).size(tokens::DIGIT_SIZE),
                    egui::RichText::new(*letters).small().weak(),
                );
                let button = ui.add_sized([width, tokens::KEY_HEIGHT], egui::Button::new(label));
                let button = if dtmf {
                    button.on_hover_text("Send DTMF")
                } else {
                    button
                };
                if button.clicked() {
                    out.push(Action::Key(*key));
                }
            }
        });
    }
    ui.horizontal(|ui| {
        let size = [width, tokens::CONTROL_HEIGHT];
        for (text, hover, action) in [
            ("Back", "Delete the last character", Action::Backspace),
            ("+", "Add +", Action::Plus),
            ("C", "Clear the number", Action::ClearNumber),
        ] {
            if ui
                .add_enabled(!in_call, egui::Button::new(text).min_size(size.into()))
                .on_hover_text(hover)
                .clicked()
            {
                out.push(action);
            }
        }
    });
    ui.add_space(tokens::SPACING);
    ui.horizontal(|ui| {
        let side = width;
        let call_width = ui.available_width() - side - gap;
        let call = egui::Button::new(st.call_label())
            .min_size(egui::vec2(call_width, tokens::CALL_HEIGHT));
        if guarded(ui, call, st.can_call()) {
            out.push(Action::Call);
        }
        let end = egui::Button::new(st.end_label()).min_size(egui::vec2(side, tokens::CALL_HEIGHT));
        if guarded(ui, end, st.can_end().map(|_| ())) {
            out.push(Action::End);
        }
    });
    ui.add_space(tokens::SPACING);
    for (text, hover) in [
        ("Spk", "Speaker volume arrives in a later release"),
        ("Mic", "Microphone volume arrives in a later release"),
    ] {
        ui.horizontal(|ui| {
            ui.label(text);
            ui.spacing_mut().slider_width = ui.available_width();
            let mut level = 1.0_f32;
            ui.add_enabled(
                false,
                egui::Slider::new(&mut level, 0.0..=1.0).show_value(false),
            )
            .on_disabled_hover_text(hover);
        });
    }
    let connected = st
        .connected_call()
        .map(|c| (c.muted, c.phase == CallPhase::Held));
    let reason = connected.as_ref().map(|_| ()).map_err(|r| *r);
    let (muted, held) = connected.unwrap_or((false, false));
    // Bound the row to one button height: inside the scroll area a centered
    // right-to-left layout would otherwise center in unbounded space.
    let row = egui::vec2(ui.available_width(), tokens::TOGGLE_HEIGHT);
    ui.allocate_ui_with_layout(
        row,
        egui::Layout::right_to_left(egui::Align::Center),
        |ui| {
            let size = egui::vec2(0.0, tokens::TOGGLE_HEIGHT);
            // Right-to-left: reverse visual order.
            if guarded(
                ui,
                egui::Button::new("XFER")
                    .selected(st.transfer_target.is_some())
                    .min_size(size),
                reason,
            ) {
                out.push(Action::OpenTransfer);
            }
            if guarded(
                ui,
                egui::Button::new("HOLD").selected(held).min_size(size),
                reason,
            ) {
                out.push(Action::ToggleHold);
            }
            if guarded(
                ui,
                egui::Button::new("MUTE").selected(muted).min_size(size),
                reason,
            ) {
                out.push(Action::ToggleMute);
            }
            let dnd = ui
                .add(egui::Button::new("DND").selected(st.dnd).min_size(size))
                .on_hover_text("Do not disturb: reject new incoming calls");
            if dnd.clicked() {
                out.push(Action::ToggleDnd);
            }
        },
    );
    if let Some(target) = st.transfer_target.as_mut() {
        ui.group(|ui| {
            ui.label("Transfer to");
            let response = ui.add(
                egui::TextEdit::singleline(target)
                    .id(transfer_id())
                    .char_limit(MAX_NUMBER_CHARS)
                    .desired_width(f32::INFINITY)
                    .hint_text("Number"),
            );
            if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                out.push(Action::ConfirmTransfer);
            }
            ui.horizontal(|ui| {
                if ui.button("Transfer").clicked() {
                    out.push(Action::ConfirmTransfer);
                }
                if ui.button("Cancel").clicked() {
                    out.push(Action::CancelTransfer);
                }
            });
        });
    }
}

/// A row of `height` whose children are laid out right to left. Bounded so
/// a centered layout inside the scroll area does not take unbounded space.
fn right_row(ui: &mut egui::Ui, height: f32, add: impl FnOnce(&mut egui::Ui)) {
    let size = egui::vec2(ui.available_width(), height);
    ui.allocate_ui_with_layout(size, egui::Layout::right_to_left(egui::Align::Center), add);
}

fn search_box(ui: &mut egui::Ui, text: &mut String, width: f32) -> bool {
    ui.add(
        egui::TextEdit::singleline(text)
            .hint_text("Search")
            .char_limit(MAX_NUMBER_CHARS)
            .desired_width(width),
    )
    .on_hover_text("Searches local data only")
    .changed()
}

fn calls(ui: &mut egui::Ui, st: &mut UiState, out: &mut Vec<Action>) {
    ui.horizontal(|ui| {
        for (filter, label) in [
            (HistoryFilter::All, "All"),
            (HistoryFilter::Incoming, "In"),
            (HistoryFilter::Outgoing, "Out"),
            (HistoryFilter::Missed, "Missed"),
        ] {
            if ui
                .add(egui::Button::new(label).selected(st.history_filter == filter))
                .clicked()
            {
                out.push(Action::SetHistoryFilter(filter));
            }
        }
    });
    ui.horizontal(|ui| {
        let width = ui.available_width() - 50.0;
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
    });
    if st.history.is_empty() {
        ui.label(egui::RichText::new("No calls.").weak());
    }
    let now = state::utc_now();
    for entry in &st.history {
        let call = &entry.call;
        let direction = if call.direction == Direction::Outgoing {
            "Out"
        } else {
            "In"
        };
        let duration = call
            .duration_ms
            .map(|ms| format!("  {}", format_duration(Duration::from_millis(ms))))
            .unwrap_or_default();
        let secure = if call.secure { "  SRTP" } else { "" };
        let text = format!(
            "{direction}  {}\n{}  {}{duration}{secure}",
            call.remote,
            outcome_text(call.outcome),
            state::age_text(now - call.started_utc)
        );
        right_row(ui, 2.0 * tokens::CONTROL_HEIGHT, |ui| {
            if ui
                .small_button("Del")
                .on_hover_text("Delete this entry")
                .clicked()
            {
                out.push(Action::DeleteCall(entry.id));
            }
            if ui.small_button("Call").on_hover_text("Dial").clicked() {
                out.push(Action::DialNumber(call.remote.clone()));
            }
            let row = ui
                .add_sized(
                    [ui.available_width(), 0.0],
                    egui::Button::new(text).frame(false),
                )
                .on_hover_text("Click: copy to the number field. Double-click: dial");
            if row.double_clicked() {
                out.push(Action::DialNumber(call.remote.clone()));
            } else if row.clicked() {
                out.push(Action::FillNumber(call.remote.clone()));
            }
        });
        ui.separator();
    }
}

fn contacts(ui: &mut egui::Ui, st: &mut UiState, out: &mut Vec<Action>) {
    ui.horizontal(|ui| {
        let width = ui.available_width() - 100.0;
        if search_box(ui, &mut st.contact_search, width) {
            out.push(Action::SearchContacts);
        }
        if ui.button("Add").clicked() {
            out.push(Action::NewContact);
        }
        if ui
            .button("Import")
            .on_hover_text("Import contacts from a CSV file")
            .clicked()
        {
            out.push(Action::OpenImport);
        }
    });
    if st.contacts.is_empty() {
        ui.label(egui::RichText::new("No contacts.").weak());
    }
    for contact in &st.contacts {
        right_row(ui, tokens::CONTROL_HEIGHT, |ui| {
            if ui.small_button("Del").clicked() {
                out.push(Action::DeleteContact(contact.id));
            }
            if ui.small_button("Edit").clicked() {
                out.push(Action::EditContact(contact.id));
            }
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.add(egui::Label::new(egui::RichText::new(&contact.name).strong()).truncate());
                if contact.favorite {
                    ui.label(egui::RichText::new("Favorite").small());
                }
            });
        });
        for number in &contact.numbers {
            right_row(ui, tokens::TOGGLE_HEIGHT, |ui| {
                if ui.small_button("Call").on_hover_text("Dial").clicked() {
                    out.push(Action::DialNumber(number.number.clone()));
                }
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.add_space(tokens::SPACING * 2.0);
                    let label = number.label.as_deref().unwrap_or("");
                    if ui
                        .add(
                            egui::Button::new(format!("{}  {label}", number.number))
                                .frame(false)
                                .truncate(),
                        )
                        .on_hover_text("Copy to the number field")
                        .clicked()
                    {
                        out.push(Action::FillNumber(number.number.clone()));
                    }
                });
            });
        }
        ui.separator();
    }
}

fn contact_editor(ui: &mut egui::Ui, st: &mut UiState, out: &mut Vec<Action>) {
    let Some(form) = st.contact_form.as_mut() else {
        return;
    };
    ui.heading(if form.id.is_some() {
        "Edit contact"
    } else {
        "New contact"
    });
    contact_fields(ui, form, out);
    ui.horizontal(|ui| {
        if ui.button("Save").clicked() {
            out.push(Action::SaveContact);
        }
        if ui.button("Cancel").on_hover_text("Esc").clicked() {
            out.push(Action::CloseView);
        }
    });
}

fn contact_fields(ui: &mut egui::Ui, form: &mut ContactForm, out: &mut Vec<Action>) {
    ui.horizontal(|ui| {
        let label = ui.label("Name");
        ui.add(
            egui::TextEdit::singleline(&mut form.name)
                .char_limit(rust_microsip_storage::MAX_NAME_LEN)
                .desired_width(f32::INFINITY),
        )
        .labelled_by(label.id);
    });
    ui.checkbox(&mut form.favorite, "Favorite");
    ui.label("Numbers");
    for (index, (number, label)) in form.numbers.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            let gap = ui.spacing().item_spacing.x;
            let width = ui.available_width() - 40.0 - 2.0 * gap;
            ui.add(
                egui::TextEdit::singleline(number)
                    .hint_text("Number")
                    .char_limit(MAX_NUMBER_CHARS)
                    .desired_width(width * 0.6),
            );
            ui.add(
                egui::TextEdit::singleline(label)
                    .hint_text("Label")
                    .char_limit(rust_microsip_storage::MAX_LABEL_LEN)
                    .desired_width(width * 0.4),
            );
            if ui
                .small_button("Del")
                .on_hover_text("Remove this number")
                .clicked()
            {
                out.push(Action::RemoveNumberRow(index));
            }
        });
    }
    if ui.button("Add number").clicked() {
        out.push(Action::AddNumberRow);
    }
}

fn import(ui: &mut egui::Ui, st: &mut UiState, palette: &tokens::Palette, out: &mut Vec<Action>) {
    let Some(form) = st.import.as_mut() else {
        return;
    };
    ui.heading("Import contacts");
    ui.label(
        egui::RichText::new(
            "UTF-8 CSV with the header name,number or name,number,label; at most 10 MiB.",
        )
        .small(),
    );
    import_fields(ui, form, palette, out);
    if ui.button("Cancel").on_hover_text("Esc").clicked() {
        out.push(Action::CloseView);
    }
}

fn import_fields(
    ui: &mut egui::Ui,
    form: &mut ImportForm,
    palette: &tokens::Palette,
    out: &mut Vec<Action>,
) {
    let path = ui.add(
        egui::TextEdit::singleline(&mut form.path)
            .hint_text("/path/to/contacts.csv")
            .char_limit(4096)
            .desired_width(f32::INFINITY),
    );
    if path.changed() {
        form.preview = None;
        form.error = None;
    }
    if ui.button("Preview").clicked() {
        out.push(Action::PreviewImport);
    }
    if let Some(error) = &form.error {
        ui.label(egui::RichText::new(error).color(palette.error));
    }
    let Some(preview) = &form.preview else {
        return;
    };
    ui.label(format!(
        "{} records, {} contacts, {} duplicate rows skipped",
        preview.record_count(),
        preview.contacts().len(),
        preview.duplicate_count()
    ));
    let issues = preview.issues();
    if !issues.is_empty() {
        ui.label(
            egui::RichText::new(format!(
                "{} invalid rows; nothing will be imported until they are fixed",
                issues.len()
            ))
            .color(palette.error),
        );
        for issue in issues.iter().take(state::MAX_ISSUES_SHOWN) {
            ui.label(format!(
                "Line {}: {}",
                issue.line,
                state::csv_issue_text(issue.kind)
            ));
        }
        if issues.len() > state::MAX_ISSUES_SHOWN {
            ui.label(format!(
                "and {} more",
                issues.len() - state::MAX_ISSUES_SHOWN
            ));
        }
    }
    let ready = if preview.is_importable() {
        Ok(())
    } else {
        Err("Nothing valid to import")
    };
    if guarded(ui, egui::Button::new("Import"), ready) {
        out.push(Action::ConfirmImport);
    }
}

fn audio_settings(ui: &mut egui::Ui, st: &UiState, out: &mut Vec<Action>) {
    ui.label("Audio devices (applied on the next start)");
    let Some(devices) = &st.audio_devices else {
        ui.label(egui::RichText::new("No audio devices could be listed.").weak());
        return;
    };
    for (label, list, current, input) in [
        ("Microphone", &devices.inputs, &st.audio_input, true),
        ("Speaker", &devices.outputs, &st.audio_output, false),
    ] {
        ui.horizontal(|ui| {
            ui.label(label);
            let mut selected = current.clone();
            let shown = match selected.as_deref() {
                None => "System default".to_owned(),
                Some(key) => list
                    .iter()
                    .find(|d| state::device_key(d) == key)
                    .map_or_else(|| format!("{key} (not found)"), |d| d.name.clone()),
            };
            egui::ComboBox::from_id_salt(label)
                .selected_text(shown)
                .width(ui.available_width())
                .truncate()
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut selected, None, "System default");
                    for device in list {
                        let text = if device.is_default {
                            format!("{} (default)", device.name)
                        } else {
                            device.name.clone()
                        };
                        ui.selectable_value(&mut selected, Some(state::device_key(device)), text);
                    }
                });
            if selected != *current {
                out.push(if input {
                    Action::SetAudioInput(selected)
                } else {
                    Action::SetAudioOutput(selected)
                });
            }
        });
    }
}

fn settings(ui: &mut egui::Ui, st: &mut UiState, palette: &tokens::Palette, out: &mut Vec<Action>) {
    ui.heading("Settings");
    let form = &mut st.form;
    egui::Grid::new("rust_microsip_account")
        .num_columns(2)
        .show(ui, |ui| {
            for (label, value) in [
                ("Display name", &mut form.display_name),
                ("Username", &mut form.username),
                ("Auth user", &mut form.auth_username),
                ("Domain", &mut form.domain),
                ("Server", &mut form.server),
                ("Port", &mut form.port),
            ] {
                let label = ui.label(label);
                ui.add(
                    egui::TextEdit::singleline(value)
                        .char_limit(253)
                        .desired_width(150.0),
                )
                .labelled_by(label.id);
                ui.end_row();
            }
            let label = ui.label("Password");
            ui.add(
                egui::TextEdit::singleline(&mut form.password)
                    .id(password_id())
                    .password(true)
                    .char_limit(128)
                    .desired_width(150.0)
                    .hint_text("unchanged"),
            )
            .labelled_by(label.id);
            ui.end_row();
        });
    ui.label(egui::RichText::new(PASSWORD_NOTE).small());
    ui.horizontal(|ui| {
        ui.label("Transport");
        let mut transport = form.transport();
        for t in [Transport::Udp, Transport::Tcp, Transport::Tls] {
            ui.radio_value(&mut transport, t, t.as_str().to_uppercase());
        }
        if transport != form.transport() {
            form.set_transport(transport);
        }
    });
    ui.horizontal(|ui| {
        ui.label("Media");
        let mut media = form.media();
        ui.radio_value(&mut media, MediaSecurity::PlainRtp, "RTP");
        let tls = form.transport() == Transport::Tls;
        ui.add_enabled_ui(tls || media == MediaSecurity::Srtp, |ui| {
            ui.radio_value(&mut media, MediaSecurity::Srtp, "SRTP");
        })
        .response
        .on_disabled_hover_text("SRTP requires TLS signaling");
        form.media = Some(media);
    });
    if form.media() == MediaSecurity::Srtp && form.transport() != Transport::Tls {
        ui.label(egui::RichText::new("SRTP requires TLS signaling").color(palette.error));
    }
    if form.media() == MediaSecurity::PlainRtp {
        ui.label(egui::RichText::new("Calls will not be encrypted").color(palette.warn));
    }
    ui.separator();
    ui.horizontal(|ui| {
        ui.label("Theme");
        let mut theme = st.theme;
        for t in Theme::ALL {
            ui.radio_value(&mut theme, t, t.label());
        }
        if theme != st.theme {
            out.push(Action::SetTheme(theme));
        }
    });
    ui.separator();
    audio_settings(ui, st, out);
    ui.separator();
    ui.horizontal(|ui| {
        if ui.button("Save and register").clicked() {
            out.push(Action::SaveSettings);
        }
        if ui.button("Cancel").on_hover_text("Esc").clicked() {
            out.push(Action::CloseView);
        }
    });
}

fn about(ui: &mut egui::Ui, out: &mut Vec<Action>) {
    ui.heading(APP_NAME);
    ui.label(concat!("Version ", env!("CARGO_PKG_VERSION")));
    ui.label(PROJECT_NOTICE);
    if ui.button("Close").clicked() {
        out.push(Action::CloseView);
    }
    ui.collapsing("Font license (OFL-1.1)", |ui| {
        ui.label(FONT_NOTICE);
        ui.label(egui::RichText::new(FONT_LICENSE).small());
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_microsip_core::{CallId, CallSnapshot};
    use std::cell::RefCell;
    use std::rc::Rc;

    struct Fake {
        events: RefCell<Vec<Event>>,
        sent: Rc<RefCell<usize>>,
    }
    impl Backend for Fake {
        fn send(&self, _: Command) -> Result<(), Notice> {
            *self.sent.borrow_mut() += 1;
            Ok(())
        }
        fn poll(&self) -> Vec<Event> {
            self.events.take()
        }
    }

    #[test]
    fn frames_render_headless_in_every_view_and_theme() {
        let ctx = egui::Context::default();
        let sent = Rc::new(RefCell::new(0));
        let call = CallSnapshot {
            id: CallId(1),
            direction: Direction::Incoming,
            remote: "1002".into(),
            phase: CallPhase::Ringing,
            muted: false,
            secure: false,
            transfer: None,
            version: 1,
        };
        let backend = Fake {
            events: RefCell::new(vec![Event::Call(call)]),
            sent: Rc::clone(&sent),
        };
        let mut app = SoftphoneApp::with_context(
            &ctx,
            Box::new(backend),
            UiSettings {
                audio_input: Some("missing-device".into()),
                ..UiSettings::default()
            },
            Box::new(|_| Ok(())),
            Storage {
                store: Store::open_in_memory().ok(),
                warning: Some("Not saved to disk.".into()),
            },
        );
        app.state.audio_devices = Some(AudioDevices {
            inputs: vec![rust_microsip_audio::DeviceInfo {
                id: "alsa:default".into(),
                name: "Default Audio Device".into(),
                is_default: true,
            }],
            outputs: Vec::new(),
        });
        app.state.contact_form = Some(ContactForm {
            numbers: vec![("1002".into(), "work".into())],
            ..ContactForm::default()
        });
        app.state.import = Some(ImportForm::default());
        app.state.confirm = Some(state::Confirm::ClearHistory);
        app.poll(&ctx);
        assert!(app.state.call.is_some());
        let input = || egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(300.0, 460.0),
            )),
            ..Default::default()
        };
        for (theme, view, tab) in [
            (Theme::Light, View::Main, Tab::Phone),
            (Theme::Dark, View::Main, Tab::Calls),
            (Theme::Dark, View::Main, Tab::Contacts),
            (Theme::Light, View::Settings, Tab::Phone),
            (Theme::System, View::About, Tab::Phone),
            (Theme::Light, View::ContactEdit, Tab::Contacts),
            (Theme::Dark, View::Import, Tab::Contacts),
        ] {
            ctx.set_theme(theme_preference(theme));
            app.state.view = view;
            app.state.tab = tab;
            let output = ctx.run_ui(input(), |ui| app.draw(ui));
            assert!(!output.shapes.is_empty());
            output.drop_without_applying_deltas();
        }
        // Connected call with the transfer prompt open.
        if let Some(active) = app.state.call.as_mut() {
            active.snapshot.phase = CallPhase::Established;
            active.established_at = Some(Instant::now());
        }
        app.state.view = View::Main;
        app.state.transfer_target = Some(String::new());
        let output = ctx.run_ui(input(), |ui| app.draw(ui));
        assert!(!output.shapes.is_empty());
        output.drop_without_applying_deltas();
        assert_eq!(*sent.borrow(), 0);
    }
}
