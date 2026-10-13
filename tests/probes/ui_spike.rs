// SPDX-License-Identifier: GPL-3.0-or-later
//! rust-microsip G0 native UI spike (preparatory task 1.8).
//!
//! This file is a presentation-only spike. It has no SIP stack, audio,
//! registration, notifications, IPC, persistence, or runtime file access,
//! and it never reports that calls work. The runtime architecture is
//! deliberately unintegrated and release remains blocked. The only embedded
//! assets are the reviewed Noto Sans Regular font (native-ui builds) and its
//! OFL-1.1 license text.
//!
//! The state/action boundary (`UiState`, `UiAction`, `UiError`) uses `std`
//! only, so `rustc --test ui_spike.rs` builds without crate dependencies
//! (it still needs `ui-assets/OFL.txt` next to this file). The GUI is
//! compiled only with `feature = "native-ui"` (eframe/egui 0.36.2).
//!
//! [`preview_state`] parses optional `--preview-tab` / `--preview-theme`
//! flags into presentation state only, so external tooling can open a
//! chosen tab and theme; it injects no rows, numbers or live state.
#![forbid(unsafe_code)]

use std::fmt;
use std::process::ExitCode;

/// Application name shown in the header.
pub const APP_NAME: &str = "rust-microsip";
/// Stage label shown in the header.
pub const STAGE_LABEL: &str = "G0 preparation";
/// The only live status this spike can show.
pub const LIVE_STATUS: &str = "Backend unavailable \u{2014} G0 UI preparation";
/// Text shown for the empty Calls and Contacts lists.
pub const EMPTY_LIST: &str = "No backend connected; no history/contacts loaded";
/// Text shown next to the theme controls.
pub const SESSION_ONLY: &str = "Session only; preference persistence unavailable";
/// Maximum characters in the number entry.
pub const MAX_NUMBER_CHARS: usize = 64;
/// Maximum characters in the shared Calls/Contacts search entry.
pub const MAX_SEARCH_CHARS: usize = 128;
/// The twelve keypad keys in row-major order (3 columns x 4 rows).
pub const KEYPAD: [char; 12] = ['1', '2', '3', '4', '5', '6', '7', '8', '9', '*', '0', '#'];
/// Optional letter labels shown below each keypad digit.
pub const KEYPAD_LETTERS: [&str; 12] = [
    "", "ABC", "DEF", "GHI", "JKL", "MNO", "PQRS", "TUV", "WXYZ", "", "", "",
];
/// Original font name-table copyright notice for the embedded Noto Sans.
pub const FONT_NOTICE: &str = "Copyright 2015-2021 Google LLC. All Rights Reserved.";
/// Complete OFL-1.1 copyright/license text retained with the reviewed font.
pub const FONT_LICENSE: &str = include_str!("ui-assets/OFL.txt");

/// Top-level view selected by the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    /// Number entry and keypad.
    #[default]
    Dialpad,
    /// Call history (always empty in G0).
    Calls,
    /// Contacts (always empty in G0).
    Contacts,
}

impl Tab {
    /// All tabs in display order.
    pub const ALL: [Tab; 3] = [Tab::Dialpad, Tab::Calls, Tab::Contacts];

    /// Visible English label.
    pub fn label(self) -> &'static str {
        match self {
            Tab::Dialpad => "Phone",
            Tab::Calls => "Calls",
            Tab::Contacts => "Contacts",
        }
    }
}

/// Session-local theme preference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Theme {
    /// Light visuals.
    Light,
    /// Dark visuals.
    Dark,
    /// Follow the system; the GUI requests Light as fallback.
    #[default]
    System,
}

impl Theme {
    /// All themes in display order.
    pub const ALL: [Theme; 3] = [Theme::Light, Theme::Dark, Theme::System];

    /// Visible English label.
    pub fn label(self) -> &'static str {
        match self {
            Theme::Light => "Light",
            Theme::Dark => "Dark",
            Theme::System => "System",
        }
    }
}

/// Actions that would need a live backend. All are rejected in G0.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveAction {
    /// Place a call.
    Call,
    /// End a call.
    HangUp,
    /// Toggle do-not-disturb.
    DoNotDisturb,
    /// Toggle microphone mute.
    Mute,
    /// Hold a call.
    Hold,
    /// Transfer a call.
    Transfer,
    /// Add a contact.
    AddContact,
    /// Import contacts.
    ImportContacts,
    /// Filter contacts by favorites.
    FavoritesFilter,
    /// Open account settings.
    AccountSettings,
    /// Open audio settings.
    AudioSettings,
}

impl LiveAction {
    /// Every live action variant.
    pub const ALL: [LiveAction; 11] = [
        LiveAction::Call,
        LiveAction::HangUp,
        LiveAction::DoNotDisturb,
        LiveAction::Mute,
        LiveAction::Hold,
        LiveAction::Transfer,
        LiveAction::AddContact,
        LiveAction::ImportContacts,
        LiveAction::FavoritesFilter,
        LiveAction::AccountSettings,
        LiveAction::AudioSettings,
    ];

    /// Visible label for the disabled control, stating it is unavailable.
    pub fn label(self) -> &'static str {
        match self {
            LiveAction::Call => "Call (unavailable)",
            LiveAction::HangUp => "Hang up (unavailable)",
            LiveAction::DoNotDisturb => "Do not disturb (unavailable)",
            LiveAction::Mute => "Mute (unavailable)",
            LiveAction::Hold => "Hold (unavailable)",
            LiveAction::Transfer => "Transfer (unavailable)",
            LiveAction::AddContact => "Add contact (unavailable)",
            LiveAction::ImportContacts => "Import contacts (unavailable)",
            LiveAction::FavoritesFilter => "Favorites only (unavailable)",
            LiveAction::AccountSettings => "Account settings (unavailable)",
            LiveAction::AudioSettings => "Audio settings (unavailable)",
        }
    }
}

/// A user intent routed through [`UiState::dispatch`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiAction {
    /// Switch the visible tab.
    SelectTab(Tab),
    /// Change the session-local theme.
    SetTheme(Theme),
    /// Show or hide the settings area.
    ToggleSettings,
    /// Replace the number with a full candidate string (typed or pasted).
    SetNumber(String),
    /// Replace the number with raw pasted bytes that must be valid UTF-8.
    SetNumberFromBytes(Vec<u8>),
    /// Append one keypad key.
    PressKey(char),
    /// Remove the last number character.
    Backspace,
    /// Clear the number.
    ClearNumber,
    /// Replace the shared Calls/Contacts search text (no I/O is performed).
    SetSearch(String),
    /// Any action that needs a live backend.
    Live(LiveAction),
}

/// Typed, non-secret UI errors. Messages never echo user input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiError {
    /// Number exceeds [`MAX_NUMBER_CHARS`].
    NumberTooLong,
    /// Number contains a character other than 0-9, `*`, `#`, or leading `+`.
    NumberInvalidCharacter,
    /// `+` appears somewhere other than the first position.
    NumberPlusNotFirst,
    /// Pasted bytes were not valid UTF-8.
    NumberNotUtf8,
    /// Key is not one of the twelve keypad keys.
    KeyNotOnKeypad,
    /// Search exceeds [`MAX_SEARCH_CHARS`].
    SearchTooLong,
    /// Search contains a control character.
    SearchInvalidCharacter,
    /// The action needs a backend that does not exist in G0.
    LiveUnavailable,
    /// Preview arguments were unknown, missing, invalid or duplicated.
    PreviewArguments,
}

impl fmt::Display for UiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            UiError::NumberTooLong => "Number rejected: longer than 64 characters",
            UiError::NumberInvalidCharacter => "Number rejected: only 0-9, * and # are allowed",
            UiError::NumberPlusNotFirst => "Number rejected: + is only allowed first",
            UiError::NumberNotUtf8 => "Number rejected: pasted text is not valid UTF-8",
            UiError::KeyNotOnKeypad => "Key rejected: not a keypad key",
            UiError::SearchTooLong => "Search rejected: longer than 128 characters",
            UiError::SearchInvalidCharacter => {
                "Search rejected: control characters are not allowed"
            }
            UiError::LiveUnavailable => "Unavailable: no backend connected in G0",
            UiError::PreviewArguments => {
                "Preview arguments rejected: use --preview-tab dialpad|calls|contacts \
                 and/or --preview-theme light|dark|system, each once"
            }
        })
    }
}

impl std::error::Error for UiError {}

/// Typed launch failures. Messages carry no paths or host details.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchError {
    /// Injected font bytes were empty.
    EmptyFontBytes,
    /// The native window runtime failed to start or exited with an error.
    NativeRuntime,
}

impl fmt::Display for LaunchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            LaunchError::EmptyFontBytes => "launch refused: font bytes are empty",
            LaunchError::NativeRuntime => "native UI runtime failed",
        })
    }
}

impl std::error::Error for LaunchError {}

/// Reject empty font bytes. The caller must still verify the exact asset
/// hash and licensing; this check does not replace that review.
pub fn validate_font_bytes(bytes: &[u8]) -> Result<(), LaunchError> {
    if bytes.is_empty() {
        Err(LaunchError::EmptyFontBytes)
    } else {
        Ok(())
    }
}

/// Print the embedded font notice and complete OFL-1.1 license when the only
/// argument is exactly `--font-license`. Returns whether it was requested.
/// Only the public license text is printed; nothing else is read.
pub fn print_font_license_if_requested() -> bool {
    let mut args = std::env::args_os().skip(1);
    match (args.next(), args.next()) {
        (Some(arg), None) if arg == "--font-license" => {
            println!("{FONT_NOTICE}\n\n{FONT_LICENSE}");
            true
        }
        _ => false,
    }
}

fn validate_number(s: &str) -> Result<(), UiError> {
    if s.chars().count() > MAX_NUMBER_CHARS {
        return Err(UiError::NumberTooLong);
    }
    for (i, c) in s.chars().enumerate() {
        match c {
            '0'..='9' | '*' | '#' => {}
            '+' if i == 0 => {}
            '+' => return Err(UiError::NumberPlusNotFirst),
            _ => return Err(UiError::NumberInvalidCharacter),
        }
    }
    Ok(())
}

fn validate_search(s: &str) -> Result<(), UiError> {
    if s.chars().count() > MAX_SEARCH_CHARS {
        return Err(UiError::SearchTooLong);
    }
    if s.chars().any(char::is_control) {
        return Err(UiError::SearchInvalidCharacter);
    }
    Ok(())
}

/// Presentation state only; not domain or account truth.
///
/// Fields are private; every change goes through [`UiState::dispatch`],
/// which either applies a whole valid change or leaves state untouched.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UiState {
    tab: Tab,
    theme: Theme,
    settings_open: bool,
    number: String,
    search: String,
}

impl UiState {
    /// Honest defaults: Dialpad, System theme, empty entries, no live state.
    pub fn new() -> Self {
        Self::default()
    }

    /// Selected tab.
    pub fn tab(&self) -> Tab {
        self.tab
    }

    /// Session-local theme.
    pub fn theme(&self) -> Theme {
        self.theme
    }

    /// Whether the settings area is visible.
    pub fn settings_open(&self) -> bool {
        self.settings_open
    }

    /// Last valid number entry.
    pub fn number(&self) -> &str {
        &self.number
    }

    /// Last valid search text.
    pub fn search(&self) -> &str {
        &self.search
    }

    /// Live status; always the fixed unavailable text in G0.
    pub fn status_text(&self) -> &'static str {
        LIVE_STATUS
    }

    /// Apply one action atomically. On error, state is unchanged.
    pub fn dispatch(&mut self, action: UiAction) -> Result<(), UiError> {
        match action {
            UiAction::SelectTab(tab) => self.tab = tab,
            UiAction::SetTheme(theme) => self.theme = theme,
            UiAction::ToggleSettings => self.settings_open = !self.settings_open,
            UiAction::SetNumber(candidate) => {
                validate_number(&candidate)?;
                self.number = candidate;
            }
            UiAction::SetNumberFromBytes(bytes) => {
                let candidate = String::from_utf8(bytes).map_err(|_| UiError::NumberNotUtf8)?;
                validate_number(&candidate)?;
                self.number = candidate;
            }
            UiAction::PressKey(key) => {
                if !KEYPAD.contains(&key) {
                    return Err(UiError::KeyNotOnKeypad);
                }
                let mut next = self.number.clone();
                next.push(key);
                validate_number(&next)?;
                self.number = next;
            }
            UiAction::Backspace => {
                self.number.pop();
            }
            UiAction::ClearNumber => self.number.clear(),
            UiAction::SetSearch(candidate) => {
                validate_search(&candidate)?;
                self.search = candidate;
            }
            UiAction::Live(_) => return Err(UiError::LiveUnavailable),
        }
        Ok(())
    }
}

/// Build presentation state from optional preview flags.
///
/// Accepts only the paired flags `--preview-tab dialpad|calls|contacts` and
/// `--preview-theme light|dark|system`, each at most once and in any order;
/// empty `args` returns [`UiState::new`]. Unknown flags (including
/// `--font-license`), missing or invalid values and duplicates are rejected
/// with the static [`UiError::PreviewArguments`], which never echoes input.
/// State is built only through [`UiState::dispatch`]: number, search and
/// lists stay empty and no backend is contacted.
pub fn preview_state(args: &[&str]) -> Result<UiState, UiError> {
    let mut tab = None;
    let mut theme = None;
    let mut rest = args.iter().copied();
    while let Some(flag) = rest.next() {
        let value = rest.next();
        match flag {
            "--preview-tab" if tab.is_none() => {
                tab = Some(match value {
                    Some("dialpad") => Tab::Dialpad,
                    Some("calls") => Tab::Calls,
                    Some("contacts") => Tab::Contacts,
                    _ => return Err(UiError::PreviewArguments),
                });
            }
            "--preview-theme" if theme.is_none() => {
                theme = Some(match value {
                    Some("light") => Theme::Light,
                    Some("dark") => Theme::Dark,
                    Some("system") => Theme::System,
                    _ => return Err(UiError::PreviewArguments),
                });
            }
            _ => return Err(UiError::PreviewArguments),
        }
    }
    let mut state = UiState::new();
    if let Some(tab) = tab {
        state.dispatch(UiAction::SelectTab(tab))?;
    }
    if let Some(theme) = theme {
        state.dispatch(UiAction::SetTheme(theme))?;
    }
    Ok(state)
}

#[cfg(feature = "native-ui")]
mod native {
    use super::*;
    use eframe::egui;

    const FONT_KEY: &str = "reviewed-ui-font";
    // Compact MicroSIP-like density; scaling/target-size gates are pending.
    const KEY_HEIGHT: f32 = 36.0;
    const CONTROL_HEIGHT: f32 = 28.0;
    const CALL_HEIGHT: f32 = 34.0;

    fn number_id() -> egui::Id {
        egui::Id::new("rust_microsip_number_entry")
    }

    fn theme_preference(theme: Theme) -> egui::ThemePreference {
        match theme {
            Theme::Light => egui::ThemePreference::Light,
            Theme::Dark => egui::ThemePreference::Dark,
            Theme::System => egui::ThemePreference::System,
        }
    }

    fn install_font(ctx: &egui::Context, bytes: &'static [u8]) {
        let mut fonts = egui::FontDefinitions::empty();
        fonts.font_data.insert(
            FONT_KEY.to_owned(),
            std::sync::Arc::new(egui::FontData::from_static(bytes)),
        );
        fonts
            .families
            .insert(egui::FontFamily::Proportional, vec![FONT_KEY.to_owned()]);
        fonts
            .families
            .insert(egui::FontFamily::Monospace, vec![FONT_KEY.to_owned()]);
        ctx.set_fonts(fonts);
    }

    /// The spike window. State is injected; there are no globals or services.
    pub struct SpikeApp {
        state: UiState,
        number_buf: String,
        search_buf: String,
        notice: Option<UiError>,
        initial_focus_done: bool,
        pending_focus: bool,
    }

    impl SpikeApp {
        /// Build the app with an injected font and presentation state.
        pub fn new(cc: &eframe::CreationContext<'_>, font: &'static [u8], state: UiState) -> Self {
            install_font(&cc.egui_ctx, font);
            // Requested System fallback is Light; actual runtime behaviour
            // has not been verified on target desktops.
            cc.egui_ctx
                .options_mut(|o| o.fallback_theme = egui::Theme::Light);
            cc.egui_ctx.set_theme(theme_preference(state.theme()));
            Self {
                number_buf: state.number().to_owned(),
                search_buf: state.search().to_owned(),
                state,
                notice: None,
                initial_focus_done: false,
                pending_focus: false,
            }
        }

        fn apply(&mut self, ctx: &egui::Context, action: UiAction) {
            let before = self.state.theme();
            self.notice = self.state.dispatch(action).err();
            if self.state.theme() != before {
                ctx.set_theme(theme_preference(self.state.theme()));
            }
            // Edit buffers always fall back to the last valid state.
            self.number_buf = self.state.number().to_owned();
            self.search_buf = self.state.search().to_owned();
        }

        /// Disabled compact control: short visible text, full honest label
        /// on hover. The status bar always states that no backend exists.
        fn disabled(ui: &mut egui::Ui, text: &str, action: LiveAction, size: egui::Vec2) {
            ui.add_enabled(false, egui::Button::new(text).min_size(size))
                .on_disabled_hover_text(action.label());
        }

        /// MicroSIP-style tab strip with the menu button on the right.
        fn tab_strip(&mut self, ui: &mut egui::Ui) {
            ui.horizontal(|ui| {
                for tab in Tab::ALL {
                    let selected = self.state.tab() == tab;
                    if ui
                        .add(egui::Button::new(tab.label()).selected(selected))
                        .clicked()
                    {
                        self.apply(ui.ctx(), UiAction::SelectTab(tab));
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let menu = ui
                        .add(egui::Button::new("Menu").selected(self.state.settings_open()))
                        .on_hover_text("Show or hide settings");
                    if menu.clicked() {
                        self.apply(ui.ctx(), UiAction::ToggleSettings);
                    }
                });
            });
        }

        /// Bottom status bar: honest backend/account state and last error.
        fn status_bar(&self, ui: &mut egui::Ui) {
            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new("\u{2022}").size(18.0).weak());
                ui.label(self.state.status_text());
            });
            ui.label(egui::RichText::new("Account: none configured").small());
            if let Some(err) = self.notice {
                ui.label(egui::RichText::new(format!("Error: {err}")).small());
            }
        }

        fn settings(&mut self, ui: &mut egui::Ui) {
            ui.group(|ui| {
                ui.label("Theme");
                let mut choice = self.state.theme();
                ui.horizontal_wrapped(|ui| {
                    for theme in Theme::ALL {
                        ui.radio_value(&mut choice, theme, theme.label());
                    }
                });
                if choice != self.state.theme() {
                    self.apply(ui.ctx(), UiAction::SetTheme(choice));
                }
                ui.label(egui::RichText::new(SESSION_ONLY).small());
                let size = egui::vec2(0.0, CONTROL_HEIGHT);
                ui.horizontal_wrapped(|ui| {
                    Self::disabled(ui, "Account...", LiveAction::AccountSettings, size);
                    Self::disabled(ui, "Audio...", LiveAction::AudioSettings, size);
                });
                ui.collapsing("Font license (OFL-1.1)", |ui| {
                    ui.label(FONT_NOTICE);
                    ui.label(FONT_LICENSE);
                });
            });
        }

        fn dialpad(&mut self, ui: &mut egui::Ui) {
            // One char over the limit so an oversized paste is rejected by
            // dispatch instead of being silently truncated by the widget.
            let edit = egui::TextEdit::singleline(&mut self.number_buf)
                .id(number_id())
                .char_limit(MAX_NUMBER_CHARS + 1)
                .font(egui::FontId::proportional(18.0))
                .desired_width(f32::INFINITY)
                .hint_text("Number");
            let response = ui.add(edit).on_hover_text("Digits, * or #; + only first");
            if !self.initial_focus_done {
                response.request_focus();
                self.initial_focus_done = true;
            }
            // Ctrl+L: focus and scroll the number entry into view, once.
            if self.pending_focus {
                response.request_focus();
                response.scroll_to_me(None);
                self.pending_focus = false;
            }
            if response.changed() {
                let candidate = self.number_buf.clone();
                self.apply(ui.ctx(), UiAction::SetNumber(candidate));
            }

            let gap = ui.spacing().item_spacing.x;
            let width = (ui.available_width() - 2.0 * gap) / 3.0;
            let key = egui::vec2(width, KEY_HEIGHT);
            let mut pressed = None;
            for row in 0..4 {
                ui.horizontal(|ui| {
                    for col in 0..3 {
                        let i = row * 3 + col;
                        let digit = egui::RichText::new(KEYPAD[i].to_string()).size(20.0);
                        let letters = egui::RichText::new(KEYPAD_LETTERS[i]).small().weak();
                        if ui
                            .add_sized(key, egui::Button::new((digit, letters)))
                            .clicked()
                        {
                            pressed = Some(KEYPAD[i]);
                        }
                    }
                });
            }
            if let Some(key) = pressed {
                self.apply(ui.ctx(), UiAction::PressKey(key));
            }

            // Edit row in MicroSIP's R / + / C position. "+" is routed through
            // full-number validation, so it is accepted only as first char.
            let mut edit_action = None;
            ui.horizontal(|ui| {
                let size = egui::vec2(width, CONTROL_HEIGHT);
                if ui
                    .add_sized(
                        size,
                        egui::Button::new(egui::RichText::new("Back").size(16.0)),
                    )
                    .on_hover_text("Backspace")
                    .clicked()
                {
                    edit_action = Some(UiAction::Backspace);
                }
                if ui
                    .add_sized(size, egui::Button::new(egui::RichText::new("+").size(16.0)))
                    .clicked()
                {
                    edit_action = Some(UiAction::SetNumber(format!("{}+", self.state.number())));
                }
                if ui
                    .add_sized(size, egui::Button::new(egui::RichText::new("C").size(16.0)))
                    .on_hover_text("Clear")
                    .clicked()
                {
                    edit_action = Some(UiAction::ClearNumber);
                }
            });
            if let Some(action) = edit_action {
                self.apply(ui.ctx(), action);
            }

            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let side = width * 0.6;
                let call = ui.available_width() - side - gap;
                Self::disabled(ui, "Call", LiveAction::Call, egui::vec2(call, CALL_HEIGHT));
                Self::disabled(ui, "End", LiveAction::HangUp, egui::vec2(side, CALL_HEIGHT));
            });

            ui.add_space(4.0);
            for (text, hover) in [
                ("Spk", "Speaker volume (unavailable)"),
                ("Mic", "Microphone volume (unavailable)"),
            ] {
                ui.horizontal(|ui| {
                    ui.label(text);
                    ui.spacing_mut().slider_width = ui.available_width();
                    // Fixed presentation value; no audio device is opened.
                    let mut level = 0.9_f32;
                    ui.add_enabled(
                        false,
                        egui::Slider::new(&mut level, 0.0..=1.0).show_value(false),
                    )
                    .on_disabled_hover_text(hover);
                });
            }

            ui.add_space(2.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let size = egui::vec2(0.0, 22.0);
                // Right-to-left: listed in reverse visual order.
                Self::disabled(ui, "XFER", LiveAction::Transfer, size);
                Self::disabled(ui, "HOLD", LiveAction::Hold, size);
                Self::disabled(ui, "MUTE", LiveAction::Mute, size);
                Self::disabled(ui, "DND", LiveAction::DoNotDisturb, size);
            });
        }

        fn search_entry(&mut self, ui: &mut egui::Ui, id: &'static str, hint: &str) {
            // Calls and Contacts share one bounded search string; each tab
            // keeps its own stable widget id.
            let edit = egui::TextEdit::singleline(&mut self.search_buf)
                .id(egui::Id::new(id))
                .char_limit(MAX_SEARCH_CHARS + 1)
                .desired_width(f32::INFINITY)
                .hint_text(hint);
            if ui.add(edit).changed() {
                let candidate = self.search_buf.clone();
                self.apply(ui.ctx(), UiAction::SetSearch(candidate));
            }
        }

        fn calls(&mut self, ui: &mut egui::Ui) {
            self.search_entry(ui, "rust_microsip_calls_search", "Search calls");
            ui.separator();
            ui.label(egui::RichText::new(EMPTY_LIST).weak());
        }

        fn contacts(&mut self, ui: &mut egui::Ui) {
            self.search_entry(ui, "rust_microsip_contacts_search", "Search contacts");
            let size = egui::vec2(0.0, CONTROL_HEIGHT);
            ui.horizontal_wrapped(|ui| {
                Self::disabled(ui, "Add", LiveAction::AddContact, size);
                Self::disabled(ui, "Import", LiveAction::ImportContacts, size);
                let mut favorites_only = false;
                ui.add_enabled(false, egui::Checkbox::new(&mut favorites_only, "Favorites"))
                    .on_disabled_hover_text(LiveAction::FavoritesFilter.label());
            });
            ui.separator();
            ui.label(egui::RichText::new(EMPTY_LIST).weak());
        }
    }

    impl eframe::App for SpikeApp {
        fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
            // Local Ctrl+L: switch to the Dialpad, then focus and scroll to
            // the number entry when it is next laid out.
            if ui.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::L)) {
                if self.state.tab() != Tab::Dialpad {
                    self.apply(ui.ctx(), UiAction::SelectTab(Tab::Dialpad));
                }
                self.pending_focus = true;
            }
            ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
            egui::Panel::top("rust_microsip_tabs").show(ui, |ui| self.tab_strip(ui));
            egui::Panel::bottom("rust_microsip_status").show(ui, |ui| self.status_bar(ui));
            egui::CentralPanel::default().show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    if self.state.settings_open() {
                        self.settings(ui);
                    }
                    match self.state.tab() {
                        Tab::Dialpad => self.dialpad(ui),
                        Tab::Calls => self.calls(ui),
                        Tab::Contacts => self.contacts(ui),
                    }
                });
            });
        }
    }

    /// Launch the spike with injected font bytes (for a controlled harness).
    pub fn launch(font_bytes: &'static [u8]) -> Result<(), LaunchError> {
        launch_with_state(font_bytes, UiState::new())
    }

    /// Launch the spike with injected font bytes and presentation state, for
    /// example from [`preview_state`]. The state is moved into
    /// [`SpikeApp::new`]; everything else matches [`launch`].
    pub fn launch_with_state(font_bytes: &'static [u8], state: UiState) -> Result<(), LaunchError> {
        validate_font_bytes(font_bytes)?;
        let options = eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([300.0, 460.0])
                .with_min_inner_size([260.0, 360.0])
                .with_resizable(true)
                .with_icon(egui::IconData::default()),
            renderer: eframe::Renderer::Wgpu,
            persist_window: false,
            ..Default::default()
        };
        eframe::run_native(
            &format!("{APP_NAME} ({STAGE_LABEL})"),
            options,
            Box::new(move |cc| Ok(Box::new(SpikeApp::new(cc, font_bytes, state)))),
        )
        .map_err(|_| LaunchError::NativeRuntime)
    }

    /// Reviewed, unmodified Noto Sans Regular 2.008 (notofonts/noto-fonts
    /// ffebf8c1ee449e544955a7e813c54f9b73848eac, SHA-256
    /// b85c38ecea8a7cfb39c24e395a4007474fa5a4fc864f6ee33309eb4948d232d5),
    /// authorized only for this G0 spike under OFL-1.1 (`ui-assets/OFL.txt`).
    pub fn reviewed_font() -> Result<&'static [u8], LaunchError> {
        static REVIEWED_FONT: &[u8] = include_bytes!("ui-assets/NotoSans-Regular.ttf");
        validate_font_bytes(REVIEWED_FONT)?;
        Ok(REVIEWED_FONT)
    }
}

#[cfg(feature = "native-ui")]
pub use native::{SpikeApp, launch, launch_with_state};

#[cfg(feature = "native-ui")]
fn main() -> ExitCode {
    if print_font_license_if_requested() {
        return ExitCode::SUCCESS;
    }
    // Non-UTF-8 arguments are rejected instead of panicking like env::args.
    let raw: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    let args: Option<Vec<&str>> = raw.iter().map(|arg| arg.to_str()).collect();
    let parsed = args
        .ok_or(UiError::PreviewArguments)
        .and_then(|args| preview_state(&args));
    let state = match parsed {
        Ok(state) => state,
        Err(err) => {
            eprintln!("rust-microsip G0 UI spike: {err}");
            return ExitCode::FAILURE;
        }
    };
    match native::reviewed_font().and_then(|font| native::launch_with_state(font, state)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("rust-microsip G0 UI spike: {err}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(not(feature = "native-ui"))]
fn main() -> ExitCode {
    if print_font_license_if_requested() {
        return ExitCode::SUCCESS;
    }
    eprintln!(
        "rust-microsip G0 UI spike: native UI unavailable (built without the native-ui feature)."
    );
    ExitCode::FAILURE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_honest_and_not_live() {
        let s = UiState::new();
        assert_eq!(s.tab(), Tab::Dialpad);
        assert_eq!(s.theme(), Theme::System);
        assert!(!s.settings_open());
        assert_eq!(s.number(), "");
        assert_eq!(s.search(), "");
        assert_eq!(s.status_text(), LIVE_STATUS);
        assert!(EMPTY_LIST.starts_with("No backend connected"));
    }

    #[test]
    fn all_keypad_keys_append() {
        let mut s = UiState::new();
        for key in KEYPAD {
            assert_eq!(s.dispatch(UiAction::PressKey(key)), Ok(()));
        }
        assert_eq!(s.number(), "123456789*0#");
        assert_eq!(
            s.dispatch(UiAction::PressKey('+')),
            Err(UiError::KeyNotOnKeypad)
        );
        assert_eq!(
            s.dispatch(UiAction::PressKey('a')),
            Err(UiError::KeyNotOnKeypad)
        );
    }

    #[test]
    fn plus_only_first() {
        let mut s = UiState::new();
        assert_eq!(s.dispatch(UiAction::SetNumber("+4412".into())), Ok(()));
        let before = s.clone();
        assert_eq!(
            s.dispatch(UiAction::SetNumber("1+2".into())),
            Err(UiError::NumberPlusNotFirst)
        );
        assert_eq!(
            s.dispatch(UiAction::SetNumber("++1".into())),
            Err(UiError::NumberPlusNotFirst)
        );
        assert_eq!(s, before);
    }

    #[test]
    fn invalid_pastes_rejected_atomically() {
        let mut s = UiState::new();
        assert_eq!(s.dispatch(UiAction::SetNumber("555".into())), Ok(()));
        let before = s.clone();
        let bad = ["12\n3", "12\u{0}", "12 34", "12-34", "abc", "\u{0661}2"];
        for text in bad {
            assert_eq!(
                s.dispatch(UiAction::SetNumber(text.into())),
                Err(UiError::NumberInvalidCharacter)
            );
            assert_eq!(s, before);
        }
        assert_eq!(
            s.dispatch(UiAction::SetNumberFromBytes(vec![b'1', 0xFF])),
            Err(UiError::NumberNotUtf8)
        );
        assert_eq!(s, before);
        assert_eq!(
            s.dispatch(UiAction::SetNumberFromBytes(b"*99#".to_vec())),
            Ok(())
        );
        assert_eq!(s.number(), "*99#");
    }

    #[test]
    fn max_number_length() {
        let mut s = UiState::new();
        let full = "1".repeat(MAX_NUMBER_CHARS);
        assert_eq!(s.dispatch(UiAction::SetNumber(full.clone())), Ok(()));
        assert_eq!(
            s.dispatch(UiAction::PressKey('2')),
            Err(UiError::NumberTooLong)
        );
        assert_eq!(
            s.dispatch(UiAction::SetNumber("1".repeat(MAX_NUMBER_CHARS + 1))),
            Err(UiError::NumberTooLong)
        );
        assert_eq!(s.number(), full);
    }

    #[test]
    fn backspace_and_clear() {
        let mut s = UiState::new();
        assert_eq!(s.dispatch(UiAction::Backspace), Ok(()));
        assert_eq!(s.dispatch(UiAction::SetNumber("+123".into())), Ok(()));
        assert_eq!(s.dispatch(UiAction::Backspace), Ok(()));
        assert_eq!(s.number(), "+12");
        assert_eq!(s.dispatch(UiAction::ClearNumber), Ok(()));
        assert_eq!(s.number(), "");
    }

    #[test]
    fn theme_tab_settings_are_local() {
        let mut s = UiState::new();
        for theme in Theme::ALL {
            assert_eq!(s.dispatch(UiAction::SetTheme(theme)), Ok(()));
            assert_eq!(s.theme(), theme);
        }
        for tab in Tab::ALL {
            assert_eq!(s.dispatch(UiAction::SelectTab(tab)), Ok(()));
            assert_eq!(s.tab(), tab);
        }
        assert_eq!(s.dispatch(UiAction::ToggleSettings), Ok(()));
        assert!(s.settings_open());
        assert_eq!(s.status_text(), LIVE_STATUS);
    }

    #[test]
    fn search_limits() {
        let mut s = UiState::new();
        let max = "a".repeat(MAX_SEARCH_CHARS);
        assert_eq!(s.dispatch(UiAction::SetSearch(max.clone())), Ok(()));
        assert_eq!(
            s.dispatch(UiAction::SetSearch("a".repeat(MAX_SEARCH_CHARS + 1))),
            Err(UiError::SearchTooLong)
        );
        assert_eq!(
            s.dispatch(UiAction::SetSearch("a\tb".into())),
            Err(UiError::SearchInvalidCharacter)
        );
        assert_eq!(s.search(), max);
    }

    #[test]
    fn every_live_action_rejected_without_change() {
        let mut s = UiState::new();
        assert_eq!(s.dispatch(UiAction::SetNumber("+100".into())), Ok(()));
        let before = s.clone();
        for action in LiveAction::ALL {
            assert_eq!(
                s.dispatch(UiAction::Live(action)),
                Err(UiError::LiveUnavailable)
            );
            assert_eq!(s, before);
            assert!(action.label().ends_with("(unavailable)"));
        }
    }

    #[test]
    fn error_messages_are_static_and_nonempty() {
        let err = UiState::new().dispatch(UiAction::SetNumber("secret-abc".into()));
        let text = err.map_err(|e| e.to_string()).err().unwrap_or_default();
        assert!(!text.is_empty());
        assert!(!text.contains("secret"));
    }

    #[test]
    fn empty_font_bytes_rejected() {
        assert_eq!(validate_font_bytes(&[]), Err(LaunchError::EmptyFontBytes));
        assert_eq!(validate_font_bytes(&[0u8]), Ok(()));
    }

    #[test]
    fn font_license_is_embedded() {
        assert_eq!(
            FONT_NOTICE,
            "Copyright 2015-2021 Google LLC. All Rights Reserved."
        );
        assert!(!FONT_LICENSE.trim().is_empty());
    }

    fn preview_summary(args: &[&str]) -> Result<(Tab, Theme), UiError> {
        let s = preview_state(args)?;
        assert!(!s.settings_open());
        assert_eq!(s.number(), "");
        assert_eq!(s.search(), "");
        assert_eq!(s.status_text(), LIVE_STATUS);
        Ok((s.tab(), s.theme()))
    }

    #[test]
    fn preview_state_empty_is_default() {
        assert_eq!(preview_state(&[]), Ok(UiState::new()));
    }

    #[test]
    fn preview_state_all_tab_theme_combinations() {
        let tabs = [
            ("dialpad", Tab::Dialpad),
            ("calls", Tab::Calls),
            ("contacts", Tab::Contacts),
        ];
        let themes = [
            ("light", Theme::Light),
            ("dark", Theme::Dark),
            ("system", Theme::System),
        ];
        for (tab_arg, tab) in tabs {
            assert_eq!(
                preview_summary(&["--preview-tab", tab_arg]),
                Ok((tab, Theme::System))
            );
            for (theme_arg, theme) in themes {
                assert_eq!(
                    preview_summary(&["--preview-tab", tab_arg, "--preview-theme", theme_arg]),
                    Ok((tab, theme))
                );
                assert_eq!(
                    preview_summary(&["--preview-theme", theme_arg, "--preview-tab", tab_arg]),
                    Ok((tab, theme))
                );
            }
        }
        for (theme_arg, theme) in themes {
            assert_eq!(
                preview_summary(&["--preview-theme", theme_arg]),
                Ok((Tab::Dialpad, theme))
            );
        }
    }

    #[test]
    fn preview_state_rejects_bad_arguments() {
        let bad: &[&[&str]] = &[
            &["--preview-tab"],
            &["--preview-theme"],
            &["--preview-tab", ""],
            &["--preview-tab", "Calls"],
            &["--preview-tab", "history"],
            &["--preview-tab", "secret-value"],
            &["--preview-theme", "blue"],
            &["--preview-tab", "--preview-theme", "dark"],
            &["--preview-tab", "calls", "--preview-tab", "contacts"],
            &["--preview-theme", "dark", "--preview-theme", "dark"],
            &["--preview-tab", "calls", "extra"],
            &["--preview-tab=calls"],
            &["--unknown"],
            &["calls"],
            &["--font-license", "--preview-tab", "calls"],
            &["--preview-theme", "dark", "--font-license"],
        ];
        for args in bad {
            assert_eq!(preview_state(args), Err(UiError::PreviewArguments));
        }
        let text = UiError::PreviewArguments.to_string();
        assert!(!text.is_empty());
        assert!(!text.contains("secret"));
    }

    #[cfg(feature = "native-ui")]
    #[test]
    fn reviewed_font_embedded_and_launch_rejects_empty() {
        let font = native::reviewed_font();
        assert!(matches!(font, Ok(bytes) if !bytes.is_empty() && bytes.starts_with(&[0, 1, 0, 0])));
        assert_eq!(launch(&[]), Err(LaunchError::EmptyFontBytes));
        assert_eq!(
            launch_with_state(&[], UiState::new()),
            Err(LaunchError::EmptyFontBytes)
        );
    }
}
