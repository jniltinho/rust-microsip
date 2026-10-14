//! Native desktop window (eframe/egui, wgpu) in the compact MicroSIP layout.
//!
//! The window talks to the engine only through [`Backend`]: commands go out
//! with [`Backend::send`], events come back through [`Backend::poll`].
//! Presentation logic lives in the GPU-free `state` module; this module only
//! lays out widgets and routes clicks and keys into it.

mod calls;
mod state;
mod tokens;
mod widgets;

pub use state::{
    AUDIO_TEST_SECONDS, DESKTOP_FILE, GrantForm, GrantSpec, IssuedGrant, Theme, UiSettings,
    UiState, View, install_desktop_entry, keyring_reference, write_private,
};

use rust_microsip_audio::AudioDevices;
use rust_microsip_storage::Store;

use eframe::egui;
use rust_microsip_core::{
    CallPhase, Command, Direction, Event, MediaSecurity, Notice, RegistrationFailure,
    RegistrationState, Transport,
};
use secrecy::SecretString;
use state::{
    Action, ContactForm, ImportForm, MAX_NUMBER_CHARS, SaveFn, Tab, format_duration, phase_text,
    registration_text, transfer_text,
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
    /// Requests from outside the window (a second instance or a `sip:` link).
    fn external(&self) -> Vec<External> {
        Vec::new()
    }
    /// Start blocking work (keyring, audio devices) off the UI thread. Keyring
    /// jobs run one at a time, in order. The result arrives through
    /// [`Backend::job_results`].
    ///
    /// # Errors
    /// A user-safe reason when the job cannot be started.
    fn run_job(&self, job: Job) -> Result<(), &'static str> {
        let _ = job;
        Err("not available in this session")
    }
    /// Results of finished or progressing jobs, in order.
    fn job_results(&self) -> Vec<JobResult> {
        Vec::new()
    }
    /// Requests from CLI/MCP clients on the control socket.
    fn control_requests(&self) -> Vec<rust_microsip_platform::Incoming> {
        Vec::new()
    }
    /// Whether a tray icon is shown; the close button then hides the window
    /// to the tray instead of quitting.
    fn tray_available(&self) -> bool {
        false
    }
    /// Replace the tray menu's recent calls: `(label, number)`, newest first.
    fn set_recent_calls(&self, items: Vec<(String, String)>) {
        let _ = items;
    }
}

/// Blocking work the window hands to the backend.
#[derive(Debug)]
pub enum Job {
    /// Store the password in the Secret Service under `reference`.
    StorePassword {
        /// See [`keyring_reference`].
        reference: String,
        /// The password; `Debug` is redacted.
        password: SecretString,
    },
    /// Delete the keyring item of `reference` (succeeds when there is none).
    DeletePassword {
        /// See [`keyring_reference`].
        reference: String,
    },
    /// Read the keyring item of `reference`.
    LoadPassword {
        /// See [`keyring_reference`].
        reference: String,
    },
    /// List capture and playback devices (ALSA probing may block).
    ListAudioDevices,
    /// Play a short tone and report the microphone level for
    /// [`AUDIO_TEST_SECONDS`].
    AudioTest {
        /// Identifies this test run in its results.
        generation: u64,
        /// Capture device selection (`None`: system default).
        input: Option<String>,
        /// Playback device selection (`None`: system default).
        output: Option<String>,
    },
}

/// Outcome of a [`Job`]. Error strings are user-safe and never hold secrets.
#[derive(Debug)]
pub enum JobResult {
    /// [`Job::StorePassword`] finished.
    PasswordStored {
        /// The reference that was stored.
        reference: String,
        /// Why it failed (keyring unavailable, locked, ...).
        result: Result<(), String>,
    },
    /// [`Job::DeletePassword`] finished.
    PasswordDeleted {
        /// The reference that was deleted.
        reference: String,
        /// Why it failed.
        result: Result<(), String>,
    },
    /// [`Job::LoadPassword`] finished; `None` when no item exists.
    PasswordLoaded {
        /// The reference that was read.
        reference: String,
        /// The password, or why it could not be read.
        result: Result<Option<SecretString>, String>,
    },
    /// [`Job::ListAudioDevices`] finished; `None` when listing failed.
    AudioDevices(Option<AudioDevices>),
    /// Microphone peak (0-32767) during the audio test of `generation`.
    AudioLevel {
        /// The [`Job::AudioTest`] run this belongs to.
        generation: u64,
        /// Peak sample magnitude.
        peak: u16,
    },
    /// The audio test of `generation` ended.
    AudioTestDone {
        /// The [`Job::AudioTest`] run this belongs to.
        generation: u64,
        /// Why it failed.
        result: Result<(), String>,
    },
}

/// A request from outside the window. A dial request only fills the number:
/// the user still confirms with Call (a recent call chosen in the tray is
/// dialed by the backend before this request arrives).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum External {
    /// Bring the window to the front.
    Activate,
    /// Bring the window to the front and fill this number.
    Dial(rust_microsip_core::DialTarget),
    /// SIGTERM/SIGINT: shut down now, without asking.
    Quit,
    /// "Quit" from the tray: like the close button (asks during a call).
    QuitRequest,
}

/// Persists secret-free settings; returns a user-safe message on failure.
pub type SaveSettings = Box<SaveFn>;

/// Application name and window title.
pub const APP_NAME: &str = "rust-microsip";
const FONT_KEY: &str = "noto-sans-regular";
const FONT: &[u8] = include_bytes!("../../assets/NotoSans-Regular.ttf");
/// Window icon, rasterized from `assets/icons/rust-microsip.svg` with
/// `rsvg-convert --width 256 --height 256`.
const ICON: &[u8] = include_bytes!("../../assets/icons/rust-microsip-256.png");
const FONT_NOTICE: &str =
    "Noto Sans Regular 2.008. Copyright 2015-2021 Google LLC. All Rights Reserved.";
const FONT_LICENSE: &str = include_str!("../../assets/OFL.txt");
const PROJECT_NOTICE: &str = "rust-microsip is free software: you can redistribute it and/or modify it \
under the terms of the GNU General Public License as published by the Free Software Foundation, \
either version 3 of the License, or (at your option) any later version (GPL-3.0-or-later). It is \
distributed WITHOUT ANY WARRANTY. The embedded font is licensed separately under the SIL Open \
Font License 1.1.";
const PASSWORD_NOTE: &str = "Without the keyring option the password is kept in memory for \
this session only. It is never written to the settings database or to files.";
const OVERRIDE_NOTE: &str = "Settings are saved in the local database. Values from the \
command line, environment, .env or the TOML file take precedence and cannot be edited here; \
change them at their source.";
const LINK_NOTE: &str = "Installs a desktop entry so sip: and tel: links can open this \
window (the number is only filled in, never dialed). To make it the default handler, run: \
xdg-mime default rust-microsip.desktop x-scheme-handler/sip (and the same for \
x-scheme-handler/tel). The entry starts \"rust-microsip\" from your PATH; with an AppImage, \
put a link named rust-microsip to it in a PATH directory such as ~/.local/bin.";
const KEYPAD: [char; 12] = ['1', '2', '3', '4', '5', '6', '7', '8', '9', '*', '0', '#'];
const KEYPAD_LETTERS: [&str; 12] = [
    "", "ABC", "DEF", "GHI", "JKL", "MNO", "PQRS", "TUV", "WXYZ", "", "", "",
];

/// Supply or override a widget's accessible name: placeholder-only fields,
/// sliders, combo boxes, and one-glyph buttons named by their purpose.
fn named(response: egui::Response, name: &str) -> egui::Response {
    response
        .ctx
        .accesskit_node_builder(response.id, |node| node.set_label(name));
    response
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

/// The embedded app icon; an empty icon (the platform default) if it cannot be
/// decoded, which the unit test rules out.
pub fn window_icon() -> Arc<egui::IconData> {
    Arc::new(eframe::icon_data::from_png_bytes(ICON).unwrap_or_default())
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
            .with_resizable(true)
            .with_icon(window_icon()),
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
    /// Close calls an earlier process left open as interrupted. Only safe
    /// when this process is known to be the single instance; otherwise
    /// another window may own those open calls.
    pub recover_interrupted: bool,
    /// Control-socket session when this process serves CLI/MCP clients.
    pub control: Option<crate::owner::ControlOwner>,
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
    /// The recent calls last handed to [`Backend::set_recent_calls`].
    tray_recent: Vec<(String, String)>,
    /// winit runs on Wayland, which cannot hide a window: minimize instead.
    wayland: bool,
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
        tokens::install(ctx);
        ctx.options_mut(|o| o.fallback_theme = egui::Theme::Light);
        ctx.set_theme(theme_preference(settings.theme));
        backend.attach_context(ctx);
        let applied_theme = settings.theme;
        let mut state = UiState::new(settings);
        state.attach_store(
            storage.store,
            storage.warning,
            storage.recover_interrupted,
            Instant::now(),
        );
        if let Some(owner) = storage.control {
            state.attach_control(owner);
        }
        Self {
            backend,
            save,
            applied_theme,
            state,
            focus_number: true,
            tray_recent: Vec::new(),
            wayland: cfg!(target_os = "linux") && wayland_session(),
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
        for result in self.backend.job_results() {
            self.state.apply_job(result, &*self.backend, now);
        }
        if self.state.audio_test.is_some() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        for incoming in self.backend.control_requests() {
            self.state.control_request(incoming, now);
        }
        self.state.control_tick(&*self.backend, now);
        if !self.state.pending_approvals().is_empty() || self.state.control_busy() {
            ctx.request_repaint_after(Duration::from_millis(200));
        }
        for request in self.backend.external() {
            if request == External::Quit {
                self.dispatch(ctx, Action::QuitNow);
                continue;
            }
            if request == External::QuitRequest {
                self.dispatch(ctx, Action::Quit);
            }
            if let External::Dial(target) = request
                && self.state.call.is_none()
            {
                self.state.number = target.as_str().to_owned();
                self.dispatch(ctx, Action::SelectTab(Tab::Phone));
                self.focus_number = true;
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }
        if ctx.input(|i| i.viewport().close_requested()) && !self.state.quitting {
            if self.backend.tray_available() {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.hide(ctx);
            } else {
                // During a call this asks "Hang up and quit?" instead of closing.
                self.dispatch(ctx, Action::Quit);
                if !self.state.quitting {
                    ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                }
            }
        }
        let recent = calls::tray_recent(&self.state);
        if recent != self.tray_recent {
            self.tray_recent = recent.clone();
            self.backend.set_recent_calls(recent);
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

    /// Close button with a tray: keep running in the tray. X11 and Windows
    /// unmap the window, and eframe keeps running [`eframe::App::logic`] for
    /// it, so tray clicks raise it again. winit cannot hide a Wayland window
    /// and eframe runs no frames for a minimized one there, so it is
    /// minimized: the dock or taskbar restores it, tray clicks only take
    /// effect once it is drawn again (a chosen recent call is still dialed
    /// at once, by the backend).
    fn hide(&self, ctx: &egui::Context) {
        if self.wayland {
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
        } else {
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
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
        egui::Panel::top("rust_microsip_tabs")
            .show_separator_line(false)
            .frame(
                egui::Frame::side_top_panel(ui.style()).inner_margin(egui::Margin {
                    left: 4,
                    right: 4,
                    top: 4,
                    bottom: 0,
                }),
            )
            .show(ui, |ui| tab_strip(ui, st, &mut out));
        egui::Panel::bottom("rust_microsip_status")
            .frame(
                egui::Frame::side_top_panel(ui.style()).inner_margin(egui::Margin::symmetric(8, 2)),
            )
            .show(ui, |ui| status_bar(ui, st, &palette));
        if st.view == View::Settings {
            // Save stays visible while the form scrolls.
            egui::Panel::bottom("rust_microsip_settings_footer")
                .show(ui, |ui| settings_footer(ui, &palette, &mut out));
        }
        if st.view == View::Main && matches!(st.tab, Tab::Calls | Tab::Contacts) {
            // MicroSIP keeps the search box under the list.
            egui::Panel::bottom("rust_microsip_list_footer")
                .show(ui, |ui| calls::footer(ui, st, &mut out));
        }
        approval_modal(&ctx, st, &palette, now, &mut out);
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
                            if ui.button(confirm.button()).clicked() {
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
                        Tab::Calls => calls::calls(ui, st, &palette, &mut out),
                        Tab::Contacts => calls::contacts(ui, st, &palette, &mut out),
                    },
                }
            });
        });
        for action in out {
            self.dispatch(&ctx, action);
        }
        // "Add to contacts" from the Calls tab: fill the new editor's number.
        if let Some(number) = ctx.data_mut(|d| d.remove_temp::<String>(calls::prefill_id()))
            && let Some(row) = self
                .state
                .contact_form
                .as_mut()
                .and_then(|form| form.numbers.first_mut())
        {
            row.0 = number;
        }
    }
}

impl eframe::App for SoftphoneApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        ui.ctx()
            .accesskit_node_builder(egui::accesskit_root_id(), |node| {
                node.set_label(APP_NAME);
            });
        self.draw(ui);
    }
}

/// Whether winit picks its Wayland backend: it does whenever
/// `WAYLAND_DISPLAY` (or else `WAYLAND_SOCKET`) is set and not empty.
fn wayland_session() -> bool {
    std::env::var("WAYLAND_DISPLAY")
        .ok()
        .filter(|v| !v.is_empty())
        .or_else(|| std::env::var("WAYLAND_SOCKET").ok())
        .is_some_and(|v| !v.is_empty())
}

/// The palette of the theme `ui` is drawn in.
fn palette_of(ui: &egui::Ui) -> tokens::Palette {
    tokens::palette(ui.visuals().dark_mode)
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

/// MicroSIP's notebook row: Phone | Calls | Contacts as tabs on the left,
/// the ▼ menu button (accessible name "Menu") at the far right, and the
/// panel border running under every tab except the selected one.
fn tab_strip(ui: &mut egui::Ui, st: &UiState, out: &mut Vec<Action>) {
    let palette = tokens::palette(ui.visuals().dark_mode);
    let row = ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        let mut selected_rect = None;
        for tab in Tab::ALL {
            let selected =
                st.tab == tab && matches!(st.view, View::Main | View::ContactEdit | View::Import);
            let response = widgets::tab(ui, tab.label(), selected);
            if selected {
                selected_rect = Some(response.rect);
            }
            if response.clicked() {
                out.push(Action::SelectTab(tab));
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
            let button = widgets::menu_button(ui, "Menu").on_hover_text("Menu");
            egui::Popup::menu(&button).show(|ui| {
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
        selected_rect
    });
    let y = row.response.rect.bottom() - 0.5;
    let stroke = egui::Stroke::new(1.0, palette.control_border);
    let (left, right) = (ui.max_rect().left(), ui.max_rect().right());
    match row.inner {
        Some(tab) => {
            ui.painter().hline(left..=tab.left(), y, stroke);
            ui.painter().hline(tab.right()..=right, y, stroke);
        }
        None => {
            ui.painter().hline(left..=right, y, stroke);
        }
    }
}

/// Registration presence as MicroSIP shows it: a round icon and one word.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Presence {
    /// Registered.
    Online,
    /// Registered with do-not-disturb on.
    DoNotDisturb,
    /// Registration in progress.
    Connecting,
    /// No account, or unregistered.
    Offline,
    /// Registration failed or the engine stopped.
    Error,
}

/// Presence and its short status text for the status bar.
fn presence(st: &UiState) -> (Presence, String) {
    if st.engine_failed {
        return (Presence::Error, "Engine stopped; restart".to_owned());
    }
    let (presence, text) = match st.registration {
        RegistrationState::Registered if st.dnd => {
            (Presence::DoNotDisturb, "Do not disturb".to_owned())
        }
        RegistrationState::Registered => (Presence::Online, "Online".to_owned()),
        RegistrationState::Registering => (Presence::Connecting, "Connecting\u{2026}".to_owned()),
        RegistrationState::Disabled => (Presence::Offline, "Offline".to_owned()),
        RegistrationState::Failed(reason) => (
            Presence::Error,
            match reason {
                RegistrationFailure::Unauthorized => "Incorrect password".to_owned(),
                RegistrationFailure::Timeout | RegistrationFailure::Network => {
                    "Connection failed".to_owned()
                }
                RegistrationFailure::Certificate => "Certificate rejected".to_owned(),
                RegistrationFailure::Rejected(code) => format!("Server error {code}"),
            },
        ),
    };
    if st.dnd && presence != Presence::DoNotDisturb {
        return (presence, format!("{text} \u{00b7} DND"));
    }
    (presence, text)
}

/// The round presence icon. Besides the color, DND carries a bar and an
/// error a cross, so the state never rests on color alone.
fn presence_icon(
    painter: &egui::Painter,
    center: egui::Pos2,
    presence: Presence,
    palette: &tokens::Palette,
) {
    let r = tokens::STATUS_DOT;
    let color = match presence {
        Presence::Online => palette.ok,
        Presence::DoNotDisturb | Presence::Error => palette.error,
        Presence::Connecting => palette.warn,
        Presence::Offline => palette.muted,
    };
    painter.circle_filled(center, r, color);
    let mark = egui::Stroke::new(1.5, palette.window);
    match presence {
        Presence::DoNotDisturb => {
            painter.hline(
                (center.x - r * 0.55)..=(center.x + r * 0.55),
                center.y,
                mark,
            );
        }
        Presence::Error => {
            let d = r * 0.4;
            painter.line_segment(
                [center + egui::vec2(-d, -d), center + egui::vec2(d, d)],
                mark,
            );
            painter.line_segment(
                [center + egui::vec2(-d, d), center + egui::vec2(d, -d)],
                mark,
            );
        }
        Presence::Connecting => {
            painter.circle_filled(center, r * 0.35, palette.window);
        }
        Presence::Online | Presence::Offline => {}
    }
}

/// MicroSIP-style status bar: presence icon, status word, account at the right.
/// Its accessible name keeps the registration text ("Registered",
/// "Failed: wrong credentials", "DND on", ...).
fn status_bar(ui: &mut egui::Ui, st: &UiState, palette: &tokens::Palette) {
    let (presence, text) = presence(st);
    let detail = if st.engine_failed {
        text.clone()
    } else {
        registration_text(st.registration)
    };
    let account = st.account_text();
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(width, tokens::STATUS_HEIGHT),
        egui::Sense::hover(),
    );
    let painter = ui.painter_at(rect);
    let dot = egui::pos2(rect.left() + tokens::STATUS_DOT + 1.0, rect.center().y);
    presence_icon(&painter, dot, presence, palette);
    let font = egui::TextStyle::Body.resolve(ui.style());
    let status = painter.layout_no_wrap(text.clone(), font, palette.text);
    let text_left = dot.x + tokens::STATUS_DOT + 6.0;
    let status_width = status.size().x;
    painter.galley(
        egui::pos2(text_left, rect.center().y - status.size().y / 2.0),
        status,
        palette.text,
    );
    // Leave the bottom-right corner to the size grip, as MicroSIP does.
    widgets::grip(
        ui.painter(),
        rect.right_bottom() + egui::vec2(6.0, 1.0),
        palette,
    );
    let right = rect.right() - 12.0;
    let room = right - (text_left + status_width + 12.0);
    if room > 24.0 {
        let small = egui::TextStyle::Small.resolve(ui.style());
        let mut job =
            egui::text::LayoutJob::simple_singleline(account.clone(), small, palette.muted);
        job.wrap = egui::text::TextWrapping::truncate_at_width(room);
        let galley = painter.layout_job(job);
        let pos = egui::pos2(
            right - galley.size().x,
            rect.center().y - galley.size().y / 2.0,
        );
        painter.galley(pos, galley, palette.muted);
    }
    let dnd = if st.dnd { ", DND on" } else { "" };
    let name = format!("{text}: {detail}{dnd}, {account}");
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &name));
    response.on_hover_text(format!("{detail}{dnd}\n{account}"));
}

/// Square segmented buttons: `(label, selected, disabled reason)` each.
/// Returns the index clicked. The selected one has an accent underline.
fn segmented(
    ui: &mut egui::Ui,
    palette: &tokens::Palette,
    options: &[(&str, bool, Option<&str>)],
) -> Option<usize> {
    let mut clicked = None;
    let count = options.len().max(1) as f32;
    let width = ui.available_width() / count;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        for (index, (label, selected, disabled)) in options.iter().enumerate() {
            let text = if *selected {
                egui::RichText::new(*label).color(palette.text).strong()
            } else {
                egui::RichText::new(*label).color(palette.muted)
            };
            let button = egui::Button::new(text)
                .selected(*selected)
                .min_size(egui::vec2(width, tokens::SEGMENT_HEIGHT));
            let response = ui.add_enabled(disabled.is_none(), button);
            if *selected {
                let rect = response.rect;
                ui.painter().hline(
                    rect.x_range(),
                    rect.bottom() - tokens::UNDERLINE / 2.0,
                    egui::Stroke::new(tokens::UNDERLINE, palette.accent),
                );
            }
            let response = match disabled {
                Some(reason) => response.on_disabled_hover_text(*reason),
                None => response,
            };
            if response.clicked() {
                clicked = Some(index);
            }
        }
    });
    clicked
}

/// Delay before Allow/Deny accept a click, so a click aimed at something else
/// when the prompt appears can never approve it.
const APPROVAL_ARM: Duration = Duration::from_secs(1);

/// The oldest control request waiting for a person, in a modal that blocks the
/// rest of the window: who asks and what would happen. **Allow** arms only
/// after this request has been on screen in the focused window for one second,
/// so a click aimed elsewhere (or the click that focuses the window) cannot
/// approve it. **Deny** and **Deny all** always work; Deny all also pauses
/// control requests for a while, so a client cannot keep the window blocked.
fn approval_modal(
    ctx: &egui::Context,
    st: &mut UiState,
    palette: &tokens::Palette,
    now: Instant,
    out: &mut Vec<Action>,
) {
    let Some(pending) = st.pending_approvals().first().cloned() else {
        st.approval_shown = None;
        st.approval_frame = None;
        return;
    };
    let focused = ctx.input(|i| i.viewport().focused.unwrap_or(false));
    let fresh = !matches!(&st.approval_shown, Some((id, _)) if *id == pending.operation_id);
    // The arming clock starts when this request is visible in a focused window
    // and restarts whenever focus is lost or frames stopped for a while (a
    // minimized window may not draw unfocused frames).
    let gap = st
        .approval_frame
        .is_some_and(|last| now.saturating_duration_since(last) > Duration::from_millis(500));
    st.approval_frame = Some(now);
    if fresh || !focused || gap {
        st.approval_shown = Some((pending.operation_id.clone(), now));
    }
    let shown = st.approval_shown.as_ref().map_or(now, |(_, at)| *at);
    let armed = focused && now.saturating_duration_since(shown) >= APPROVAL_ARM;
    if !armed {
        ctx.request_repaint_after(Duration::from_millis(200));
    }
    let waiting = st.pending_approvals().len() - 1;
    egui::Modal::new(egui::Id::new("rust_microsip_approval")).show(ctx, |ui| {
        ui.set_max_width(260.0);
        ui.label(egui::RichText::new(format!("{} asks to:", pending.requester)).strong());
        ui.label(egui::RichText::new(&pending.description).color(palette.warn));
        let left = pending.deadline.saturating_duration_since(now).as_secs();
        ui.label(egui::RichText::new(format!("Expires in {left} s")).small());
        if waiting > 0 {
            ui.label(egui::RichText::new(format!("{waiting} more waiting")).small());
        }
        ui.horizontal(|ui| {
            let deny = named(ui.button("Deny"), "Deny request");
            if fresh {
                // Keyboard focus starts on the safe choice.
                deny.request_focus();
            }
            if deny.clicked() {
                out.push(Action::DenyControl(pending.operation_id.clone()));
            }
            let allow = ui.add_enabled(armed, egui::Button::new("Allow"));
            if named(allow, "Allow request").clicked() {
                out.push(Action::ApproveControl(pending.operation_id.clone()));
            }
        });
        let pause = named(
            ui.small_button("Deny all and pause"),
            "Deny all control requests",
        );
        if pause
            .on_hover_text("Deny every waiting request and refuse new ones for 5 minutes")
            .clicked()
        {
            out.push(Action::DenyAllControl);
        }
    });
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
        let response =
            named(ui.add(edit), "Number to call").on_hover_text("Number to call (Ctrl+L)");
        if focus_number {
            response.request_focus();
            response.scroll_to_me(None);
        }
    }
    // MicroSIP's grid: 3x4 bordered keys, digit left-aligned, letters after it.
    let gap = 2.0;
    let width = (ui.available_width() - 2.0 * gap) / 3.0;
    let key_size = egui::vec2(width, tokens::KEY_HEIGHT);
    ui.spacing_mut().item_spacing.y = gap;
    for row in KEYPAD.chunks(3).zip(KEYPAD_LETTERS.chunks(3)) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for (key, letters) in row.0.iter().zip(row.1) {
                let digit = key.to_string();
                let name = if letters.is_empty() {
                    digit.clone()
                } else {
                    format!("{digit} {letters}")
                };
                let button = widgets::key(ui, key_size, &digit, letters, &name, false, true);
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
    // The R / + / C row; R (redial) is not offered, Back takes its place.
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        for (text, hover, action) in [
            ("Back", "Delete the last character", Action::Backspace),
            ("+", "Add +", Action::Plus),
            ("C", "Clear the number", Action::ClearNumber),
        ] {
            // The hover text doubles as the screen-reader name.
            let button = widgets::key(ui, key_size, text, "", hover, true, !in_call);
            if button.on_hover_text(hover).clicked() && !in_call {
                out.push(action);
            }
        }
    });
    ui.spacing_mut().item_spacing.y = tokens::SPACING;
    ui.add_space(tokens::SPACING);
    // A wide Call button and a small square End beside it, as MicroSIP's
    // Call and message buttons.
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = gap + 4.0;
        let end_width = tokens::END_WIDTH;
        let call_width = ui.available_width() - end_width - gap - 4.0;
        let call = egui::Button::new(st.call_label())
            .min_size(egui::vec2(call_width, tokens::CALL_HEIGHT));
        if guarded(ui, call, st.can_call()) {
            out.push(Action::Call);
        }
        let end =
            egui::Button::new(st.end_label()).min_size(egui::vec2(end_width, tokens::CALL_HEIGHT));
        if guarded(ui, end, st.can_end().map(|_| ())) {
            out.push(Action::End);
        }
    });
    ui.add_space(tokens::SPACING);
    // Speaker and microphone: icon box, "−" slider "+".
    for (glyph, name, hover) in [
        (
            widgets::Glyph::Speaker,
            "Speaker volume",
            "Speaker volume arrives in a later release",
        ),
        (
            widgets::Glyph::Microphone,
            "Microphone volume",
            "Microphone volume arrives in a later release",
        ),
    ] {
        ui.horizontal(|ui| {
            widgets::icon_box(ui, tokens::TOGGLE_HEIGHT, glyph);
            let muted = egui::RichText::new("-").color(palette_of(ui).muted);
            ui.label(muted);
            let plus_width = 12.0;
            ui.spacing_mut().slider_width =
                (ui.available_width() - plus_width - ui.spacing().item_spacing.x).max(40.0);
            let mut level = 1.0_f32;
            named(
                ui.add_enabled(
                    false,
                    egui::Slider::new(&mut level, 0.0..=1.0).show_value(false),
                ),
                name,
            )
            .on_disabled_hover_text(hover);
            ui.label(egui::RichText::new("+").color(palette_of(ui).muted));
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
            let label = ui.label("Transfer to");
            let response = ui
                .add(
                    egui::TextEdit::singleline(target)
                        .id(transfer_id())
                        .char_limit(MAX_NUMBER_CHARS)
                        .desired_width(f32::INFINITY)
                        .hint_text("Number"),
                )
                .labelled_by(label.id);
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
    named(
        ui.add(
            egui::TextEdit::singleline(text)
                .hint_text("Search")
                .char_limit(MAX_NUMBER_CHARS)
                .desired_width(width),
        ),
        "Search",
    )
    .on_hover_text("Searches local data only")
    .changed()
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
            named(
                ui.add(
                    egui::TextEdit::singleline(number)
                        .hint_text("Number")
                        .char_limit(MAX_NUMBER_CHARS)
                        .desired_width(width * 0.6),
                ),
                "Contact number",
            );
            named(
                ui.add(
                    egui::TextEdit::singleline(label)
                        .hint_text("Label")
                        .char_limit(rust_microsip_storage::MAX_LABEL_LEN)
                        .desired_width(width * 0.4),
                ),
                "Number label",
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
    let path = named(
        ui.add(
            egui::TextEdit::singleline(&mut form.path)
                .hint_text("/path/to/contacts.csv")
                .char_limit(4096)
                .desired_width(f32::INFINITY),
        ),
        "CSV file path",
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

/// A titled settings group: section title, then the fields in a flat
/// surface panel with a hairline border.
fn section(
    ui: &mut egui::Ui,
    palette: &tokens::Palette,
    title: &str,
    hint: Option<&str>,
    add: impl FnOnce(&mut egui::Ui),
) {
    ui.add_space(tokens::SPACING * 2.0);
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(title)
                .size(tokens::SECTION_SIZE)
                .strong()
                .color(palette.text),
        );
        if let Some(hint) = hint {
            ui.label(egui::RichText::new(hint).small().color(palette.muted));
        }
    });
    egui::Frame::new()
        .fill(palette.surface)
        .stroke(egui::Stroke::new(1.0, palette.hairline))
        .inner_margin(egui::Margin::same(8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui);
        });
}

/// One form line as in MicroSIP's dialogs: a fixed-width column with the
/// label right-aligned, then the field filling the rest. `add` receives the
/// label's id for `labelled_by`.
fn field_row<R>(
    ui: &mut egui::Ui,
    label: &str,
    add: impl FnOnce(&mut egui::Ui, egui::Id) -> R,
) -> R {
    ui.horizontal(|ui| {
        let height = ui.spacing().interact_size.y;
        let label = ui
            .allocate_ui_with_layout(
                egui::vec2(tokens::LABEL_WIDTH, height),
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| {
                    ui.set_min_width(tokens::LABEL_WIDTH);
                    ui.add_space(2.0);
                    ui.add(egui::Label::new(label).truncate())
                },
            )
            .inner;
        add(ui, label.id)
    })
    .inner
}

/// A single-line text field filling the rest of a [`field_row`].
fn text_field(ui: &mut egui::Ui, label: egui::Id, edit: egui::TextEdit<'_>) -> egui::Response {
    let width = ui.available_width();
    ui.add(edit.desired_width(width)).labelled_by(label)
}

/// Muted explanatory text inside a section.
fn note(ui: &mut egui::Ui, palette: &tokens::Palette, text: &str) {
    ui.label(egui::RichText::new(text).small().color(palette.muted));
}

fn audio_settings(
    ui: &mut egui::Ui,
    st: &UiState,
    palette: &tokens::Palette,
    out: &mut Vec<Action>,
) {
    if st.audio_devices_loading {
        note(ui, palette, "Loading devices\u{2026}");
    } else if let Some(devices) = &st.audio_devices {
        for (label, list, current, input) in [
            ("Microphone", &devices.inputs, &st.audio_input, true),
            ("Speaker", &devices.outputs, &st.audio_output, false),
        ] {
            field_row(ui, label, |ui, _| {
                let mut selected = current.clone();
                let shown = match selected.as_deref() {
                    None => "System default".to_owned(),
                    Some(key) => list
                        .iter()
                        .find(|d| state::device_key(d) == key)
                        .map_or_else(|| format!("{key} (not found)"), |d| d.name.clone()),
                };
                let combo = egui::ComboBox::from_id_salt(label)
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
                            ui.selectable_value(
                                &mut selected,
                                Some(state::device_key(device)),
                                text,
                            );
                        }
                    });
                named(combo.response, &format!("{label} device"));
                if selected != *current {
                    out.push(if input {
                        Action::SetAudioInput(selected)
                    } else {
                        Action::SetAudioOutput(selected)
                    });
                }
            });
        }
    } else {
        note(ui, palette, "No audio devices could be listed.");
    }
    field_row(ui, "Audio test", |ui, _| {
        if guarded(ui, egui::Button::new("Test"), st.can_test_audio()) {
            out.push(Action::TestAudio);
        }
        match st.audio_test {
            Some(peak) => {
                ui.add(
                    egui::ProgressBar::new(f32::from(peak) / f32::from(i16::MAX))
                        .desired_height(tokens::TOGGLE_HEIGHT - 6.0)
                        .text("Microphone level"),
                );
            }
            None => note(ui, palette, "Plays a tone, shows the mic level"),
        }
    });
}

fn settings(ui: &mut egui::Ui, st: &mut UiState, palette: &tokens::Palette, out: &mut Vec<Action>) {
    ui.label(
        egui::RichText::new("Settings")
            .size(tokens::NUMBER_SIZE)
            .color(palette.text),
    );
    if !st.overridden.is_empty() {
        ui.label(
            egui::RichText::new(format!("Fixed elsewhere: {}", st.overridden.join(", ")))
                .small()
                .color(palette.warn),
        );
    }
    let form = &mut st.form;
    section(ui, palette, "Account", None, |ui| {
        // MicroSIP's Account dialog order: server first, then identity.
        for (label, value, hint) in [
            ("SIP Server", &mut form.server, "same as domain"),
            ("Port", &mut form.port, "5060"),
            ("Username", &mut form.username, "required"),
            ("Domain", &mut form.domain, "required"),
            ("Login", &mut form.auth_username, "same as username"),
        ] {
            field_row(ui, label, |ui, id| {
                text_field(
                    ui,
                    id,
                    egui::TextEdit::singleline(value)
                        .char_limit(253)
                        .hint_text(hint),
                );
            });
        }
        let show_id = egui::Id::new("rust_microsip_show_password");
        let mut show = ui.data(|d| d.get_temp::<bool>(show_id).unwrap_or(false));
        field_row(ui, "Password", |ui, id| {
            text_field(
                ui,
                id,
                egui::TextEdit::singleline(&mut form.password)
                    .id(password_id())
                    .password(!show)
                    .char_limit(128)
                    .hint_text("unchanged"),
            );
        });
        field_row(ui, "", |ui, _| {
            if ui.checkbox(&mut show, "Display password").changed() {
                ui.data_mut(|d| d.insert_temp(show_id, show));
            }
        });
        field_row(ui, "Display Name", |ui, id| {
            text_field(
                ui,
                id,
                egui::TextEdit::singleline(&mut form.display_name).char_limit(253),
            );
        });
        ui.checkbox(
            &mut form.remember,
            "Remember password in the system keyring",
        )
        .on_hover_text("Stored in the desktop Secret Service; never in the settings database");
        note(ui, palette, PASSWORD_NOTE);
        ui.add_space(tokens::SPACING);
        field_row(ui, "Transport", |ui, _| {
            let current = form.transport();
            let all = [Transport::Udp, Transport::Tcp, Transport::Tls];
            let labels = all.map(|t| t.as_str().to_uppercase());
            let options: Vec<_> = all
                .iter()
                .zip(&labels)
                .map(|(t, label)| (label.as_str(), *t == current, None))
                .collect();
            if let Some(index) = segmented(ui, palette, &options) {
                form.set_transport(all[index]);
            }
        });
        field_row(ui, "Encryption", |ui, _| {
            let media = form.media();
            let srtp_allowed = form.transport() == Transport::Tls || media == MediaSecurity::Srtp;
            let options = [
                ("RTP", media == MediaSecurity::PlainRtp, None),
                (
                    "SRTP",
                    media == MediaSecurity::Srtp,
                    (!srtp_allowed).then_some("SRTP requires TLS signaling"),
                ),
            ];
            form.media = Some(match segmented(ui, palette, &options) {
                Some(0) => MediaSecurity::PlainRtp,
                Some(_) => MediaSecurity::Srtp,
                None => media,
            });
        });
        if form.media() == MediaSecurity::Srtp && form.transport() != Transport::Tls {
            ui.label(egui::RichText::new("SRTP requires TLS signaling").color(palette.error));
        }
        if form.media() == MediaSecurity::PlainRtp {
            ui.label(egui::RichText::new("Calls will not be encrypted").color(palette.warn));
        }
    });
    section(ui, palette, "Settings", None, |ui| {
        audio_settings(ui, st, palette, out);
        field_row(ui, "", |ui, _| {
            note(ui, palette, "Devices apply on the next start");
        });
        ui.add_space(tokens::SPACING);
        field_row(ui, "Theme", |ui, _| {
            let options: Vec<_> = Theme::ALL
                .iter()
                .map(|t| (t.label(), *t == st.theme, None))
                .collect();
            if let Some(index) = segmented(ui, palette, &options) {
                out.push(Action::SetTheme(Theme::ALL[index]));
            }
        });
    });
    section(ui, palette, "Links", None, |ui| {
        ui.horizontal(|ui| {
            if ui.button("Handle sip:/tel: links").clicked() {
                out.push(Action::InstallLinkHandler);
            }
            if ui
                .button("Remove")
                .on_hover_text("Delete the link handler entry")
                .clicked()
            {
                out.push(Action::RemoveLinkHandler);
            }
        });
        note(ui, palette, LINK_NOTE);
    });
    section(ui, palette, "Agent access", Some("CLI/MCP"), |ui| {
        agent_access(ui, st, palette, out);
    });
    section(ui, palette, "Configuration sources", None, |ui| {
        note(ui, palette, OVERRIDE_NOTE);
    });
    ui.add_space(tokens::SPACING * 2.0);
}

/// Pinned under the settings form: the primary save button and Cancel.
fn settings_footer(ui: &mut egui::Ui, palette: &tokens::Palette, out: &mut Vec<Action>) {
    ui.add_space(tokens::SPACING);
    right_row(ui, tokens::CONTROL_HEIGHT, |ui| {
        let save =
            egui::Button::new(egui::RichText::new("Save and register").color(palette.on_accent))
                .fill(palette.accent)
                .min_size(egui::vec2(0.0, tokens::CONTROL_HEIGHT));
        if ui
            .add(save)
            .on_hover_text("Save, then register again")
            .clicked()
        {
            out.push(Action::SaveSettings);
        }
        let cancel = egui::Button::new("Cancel").min_size(egui::vec2(0.0, tokens::CONTROL_HEIGHT));
        if ui
            .add(cancel)
            .on_hover_text("Esc: discard changes")
            .clicked()
        {
            out.push(Action::CloseView);
        }
    });
    ui.add_space(tokens::SPACING);
}

/// Grants for the CLI/MCP: what an agent may do without asking each time.
fn agent_access(
    ui: &mut egui::Ui,
    st: &mut UiState,
    palette: &tokens::Palette,
    out: &mut Vec<Action>,
) {
    note(
        ui,
        palette,
        "Without a grant every change waits for Allow here. A grant lets a local program \
         do only what you tick, for the numbers you list, for at most an hour.",
    );
    let form = &mut st.grant_form;
    egui::Grid::new("rust_microsip_grant_scopes")
        .num_columns(2)
        .show(ui, |ui| {
            for (index, scope) in crate::owner::SCOPES.iter().enumerate() {
                ui.checkbox(&mut form.scopes[index], *scope);
                if index % 2 == 1 {
                    ui.end_row();
                }
            }
        });
    field_row(ui, "May call", |ui, id| {
        text_field(
            ui,
            id,
            egui::TextEdit::singleline(&mut form.destinations).hint_text("1002, 1003"),
        );
    });
    field_row(ui, "May answer", |ui, id| {
        text_field(
            ui,
            id,
            egui::TextEdit::singleline(&mut form.incoming).hint_text("1002"),
        )
        .on_hover_text("Trusts the caller ID the PBX passes on, which callers can fake");
    });
    field_row(ui, "Minutes", |ui, id| {
        named(
            ui.add(egui::DragValue::new(&mut form.minutes).range(1..=60)),
            "Grant minutes",
        )
        .labelled_by(id);
        let label = ui.label("Calls");
        named(
            ui.add(egui::DragValue::new(&mut form.quota).range(0..=100)),
            "Grant calls",
        )
        .labelled_by(label.id);
    });
    ui.horizontal_wrapped(|ui| {
        if ui.button("Create grant file").clicked() {
            out.push(Action::CreateGrant { bootstrap: false });
        }
        if ui
            .button("Export for headless service")
            .on_hover_text(
                "Single use: rust-microsip service --grant-file FILE (quit the window first)",
            )
            .clicked()
        {
            out.push(Action::CreateGrant { bootstrap: true });
        }
        if ui
            .button("Clear audit log")
            .on_hover_text("Delete the record of agent requests and decisions (asks first)")
            .clicked()
        {
            out.push(Action::ClearAudit);
        }
    });
    if let Some(path) = &st.grant_form.written {
        note(ui, palette, &format!("Written to {path}"));
    }
    for grant in st.listed_grants() {
        ui.horizontal(|ui| {
            let kind = if grant.bootstrap_id.is_some() {
                "service"
            } else {
                "session"
            };
            if named(ui.small_button("Revoke"), "Revoke grant").clicked() {
                out.push(Action::RevokeGrant(grant.id.clone()));
            }
            ui.add(
                egui::Label::new(
                    egui::RichText::new(format!(
                        "{} ({kind}): {} \u{00b7} calls {}/{}",
                        grant.id,
                        grant.scopes.join(", "),
                        grant.used,
                        grant.quota
                    ))
                    .small(),
                )
                .truncate(),
            );
        });
    }
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

    #[test]
    fn window_icon_decodes() {
        let icon = window_icon();
        assert_eq!((icon.width, icon.height), (256, 256));
        assert_eq!(icon.rgba.len(), 256 * 256 * 4);
    }

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
            Box::new(|_, _| Ok(())),
            Storage {
                store: Store::open_in_memory().ok().map(|mut store| {
                    seed(&mut store, state::utc_now());
                    store
                }),
                warning: Some("Not saved to disk.".into()),
                recover_interrupted: true,
                control: None,
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
        // Calls list with rows, one selected (action strip), and every
        // status bar state.
        app.state.history_filter = rust_microsip_storage::HistoryFilter::All;
        app.dispatch(&ctx, Action::SearchHistory);
        app.dispatch(&ctx, Action::SearchContacts);
        assert!(app.state.history.len() >= 4);
        let first = app.state.history.first().map(|e| e.id);
        ctx.data_mut(|d| d.insert_temp(egui::Id::new("rust_microsip_history_selection"), first));
        app.state.view = View::Main;
        app.state.tab = Tab::Calls;
        for (theme, registration, dnd) in [
            (Theme::Dark, RegistrationState::Registered, false),
            (Theme::Light, RegistrationState::Registered, true),
            (Theme::Dark, RegistrationState::Registering, false),
            (
                Theme::Light,
                RegistrationState::Failed(RegistrationFailure::Unauthorized),
                false,
            ),
            (Theme::Dark, RegistrationState::Disabled, true),
        ] {
            ctx.set_theme(theme_preference(theme));
            app.state.registration = registration;
            app.state.dnd = dnd;
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

    /// Every recent-call list the window handed to the backend.
    type Pushed = Rc<RefCell<Vec<Vec<(String, String)>>>>;

    struct TrayFake {
        tray: bool,
        recent: Pushed,
    }
    impl Backend for TrayFake {
        fn send(&self, _: Command) -> Result<(), Notice> {
            Ok(())
        }
        fn poll(&self) -> Vec<Event> {
            Vec::new()
        }
        fn tray_available(&self) -> bool {
            self.tray
        }
        fn set_recent_calls(&self, items: Vec<(String, String)>) {
            self.recent.borrow_mut().push(items);
        }
    }

    fn tray_app(ctx: &egui::Context, tray: bool, store: Option<Store>) -> (SoftphoneApp, Pushed) {
        let recent = Rc::default();
        let backend = TrayFake {
            tray,
            recent: Rc::clone(&recent),
        };
        let app = SoftphoneApp::with_context(
            ctx,
            Box::new(backend),
            UiSettings::default(),
            Box::new(|_, _| Ok(())),
            Storage {
                store,
                warning: None,
                recover_interrupted: false,
                control: None,
            },
        );
        (app, recent)
    }

    /// Commands the window sent after a close request.
    fn close_window(ctx: &egui::Context, app: &mut SoftphoneApp) -> Vec<egui::ViewportCommand> {
        let mut input = egui::RawInput::default();
        if let Some(root) = input.viewports.get_mut(&egui::ViewportId::ROOT) {
            root.events.push(egui::ViewportEvent::Close);
        }
        let output = ctx.run_ui(input, |ui| app.poll(ui.ctx()));
        let commands = output
            .viewport_output
            .get(&egui::ViewportId::ROOT)
            .map(|v| v.commands.clone())
            .unwrap_or_default();
        output.drop_without_applying_deltas();
        commands
    }

    #[test]
    fn close_hides_to_the_tray_only_when_one_is_shown() {
        use egui::ViewportCommand as Cmd;
        let ctx = egui::Context::default();
        let (mut app, _) = tray_app(&ctx, true, None);
        app.wayland = false;
        let commands = close_window(&ctx, &mut app);
        assert!(commands.contains(&Cmd::CancelClose));
        assert!(commands.contains(&Cmd::Visible(false)));
        assert!(!app.state.quitting);
        app.wayland = true;
        let commands = close_window(&ctx, &mut app);
        assert!(commands.contains(&Cmd::Minimized(true)));
        assert!(!commands.contains(&Cmd::Visible(false)));
        assert!(!app.state.quitting);

        let ctx = egui::Context::default();
        let (mut app, _) = tray_app(&ctx, false, None);
        let commands = close_window(&ctx, &mut app);
        assert!(app.state.quitting);
        assert!(commands.contains(&Cmd::Close));
        assert!(!commands.contains(&Cmd::Visible(false)));
    }

    #[test]
    fn tray_lists_the_newest_five_calls_with_contact_names() {
        let ctx = egui::Context::default();
        let mut store = Store::open_in_memory().ok();
        if let Some(store) = store.as_mut() {
            seed(store, state::utc_now());
        }
        let (mut app, recent) = tray_app(&ctx, true, store);
        // A filtered, searched Calls list must not change the tray menu.
        app.state.history_filter = rust_microsip_storage::HistoryFilter::Missed;
        app.dispatch(&ctx, Action::SearchHistory);
        app.poll(&ctx);
        app.poll(&ctx);
        let pushed = recent.borrow();
        assert_eq!(pushed.len(), 1, "pushed once, then only on change");
        let labels: Vec<_> = pushed[0]
            .iter()
            .map(|(l, n)| (l.as_str(), n.as_str()))
            .collect();
        assert_eq!(
            labels,
            vec![
                ("\u{2197} Ana Lima", "1002"),
                ("Missed: 1003", "1003"),
                ("\u{2199} Support desk", "600"),
                ("\u{2197} 1003", "1003"),
                ("\u{2197} Support desk", "600"),
            ]
        );
    }

    /// Fictional history and contacts (lab extensions, invented names).
    fn seed(store: &mut Store, now: i64) {
        use rust_microsip_storage::{CallOutcome, CallRecord, ContactNumber, NewContact};
        for (name, number) in [("Ana Lima", "1002"), ("Support desk", "600")] {
            let contact = NewContact {
                name: name.into(),
                favorite: false,
                numbers: vec![ContactNumber {
                    number: number.into(),
                    label: Some("work".into()),
                }],
            };
            assert!(store.add_contact(&contact).is_ok());
        }
        for (index, (ago, direction, remote, outcome, seconds, secure)) in [
            (
                40 * 86_400,
                Direction::Incoming,
                "1002",
                CallOutcome::Answered,
                Some(48),
                false,
            ),
            (
                2 * 86_400,
                Direction::Outgoing,
                "600",
                CallOutcome::Answered,
                Some(128),
                true,
            ),
            (
                5 * 3600,
                Direction::Outgoing,
                "1003",
                CallOutcome::Busy,
                None,
                false,
            ),
            (
                3 * 3600,
                Direction::Incoming,
                "600",
                CallOutcome::Answered,
                Some(192),
                false,
            ),
            (
                40 * 60,
                Direction::Incoming,
                "1003",
                CallOutcome::Missed,
                None,
                false,
            ),
            (
                14 * 60,
                Direction::Outgoing,
                "1002",
                CallOutcome::Answered,
                Some(84),
                true,
            ),
        ]
        .into_iter()
        .enumerate()
        {
            let started = now - ago;
            let record = CallRecord {
                session_key: format!("fixture:{index}"),
                started_utc: started,
                answered_utc: seconds.map(|_| started + 5),
                ended_utc: Some(started + 5 + seconds.unwrap_or(0)),
                direction,
                remote: remote.into(),
                outcome,
                duration_ms: seconds.map(|s| u64::try_from(s).unwrap_or(0) * 1000),
                secure,
            };
            assert!(store.record_call(&record).is_ok());
        }
    }

    /// Writes the fictional fixture to `$RMS_SEED_DB` for screenshots of a
    /// scratch profile: `RMS_SEED_DB=<absolute path>/rust-microsip.sqlite3 cargo
    /// test -p rust-microsip seed_fixture -- --ignored` (tests run in the package directory).
    #[test]
    #[ignore = "writes a fixture database for captures"]
    fn seed_fixture() {
        let Some(path) = std::env::var_os("RMS_SEED_DB") else {
            return;
        };
        let path = std::path::PathBuf::from(path);
        if let Some(parent) = path.parent() {
            assert!(std::fs::create_dir_all(parent).is_ok());
        }
        let store = Store::open(&path);
        assert!(store.is_ok());
        if let Ok(mut store) = store {
            seed(&mut store, state::utc_now());
        }
    }

    #[test]
    fn presence_mirrors_microsip_status_words() {
        let mut st = UiState::new(UiSettings::default());
        for (registration, dnd, expected, text) in [
            (
                RegistrationState::Registered,
                false,
                Presence::Online,
                "Online",
            ),
            (
                RegistrationState::Registered,
                true,
                Presence::DoNotDisturb,
                "Do not disturb",
            ),
            (
                RegistrationState::Registering,
                false,
                Presence::Connecting,
                "Connecting\u{2026}",
            ),
            (
                RegistrationState::Disabled,
                false,
                Presence::Offline,
                "Offline",
            ),
            (
                RegistrationState::Disabled,
                true,
                Presence::Offline,
                "Offline \u{00b7} DND",
            ),
            (
                RegistrationState::Failed(RegistrationFailure::Unauthorized),
                false,
                Presence::Error,
                "Incorrect password",
            ),
            (
                RegistrationState::Failed(RegistrationFailure::Network),
                false,
                Presence::Error,
                "Connection failed",
            ),
        ] {
            st.registration = registration;
            st.dnd = dnd;
            assert_eq!(
                presence(&st),
                (expected, text.to_owned()),
                "{registration:?}"
            );
        }
        st.engine_failed = true;
        assert_eq!(presence(&st).0, Presence::Error);
    }
}
