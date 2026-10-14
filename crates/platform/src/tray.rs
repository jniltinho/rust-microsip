//! System tray icon over the freedesktop `StatusNotifierItem` protocol with a
//! `com.canonical.dbusmenu` menu ("Show rust-microsip", the recent calls set
//! with [`Tray::set_recent`], "Quit").
//!
//! KDE Plasma, Xfce, LXQt, Cinnamon and GNOME with the AppIndicator extension
//! host these items. The bus connection serves both objects from zbus's own
//! executor thread (never Tokio); clicks only enqueue a [`TrayAction`] into a
//! bounded queue for the UI to poll. Without a session bus or a
//! `StatusNotifierWatcher` the tray reports [`TrayStatus::Unavailable`] and the
//! window works as before.

use crate::Waker;
use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::Duration;
use zbus::fdo;
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

const WATCHER: &str = "org.kde.StatusNotifierWatcher";
const WATCHER_PATH: &str = "/StatusNotifierWatcher";
const ITEM_PATH: &str = "/StatusNotifierItem";
const MENU_PATH: &str = "/MenuBar";
const QUEUE: usize = 8;
const CALL_TIMEOUT: Duration = Duration::from_secs(5);

const SHOW: i32 = 1;
const SEPARATOR: i32 = 2;
const QUIT: i32 = 3;
/// Menu id of the first recent call; the others follow.
const RECENT: i32 = 10;
/// Most recent calls shown in the menu.
pub const MAX_RECENT: usize = 5;

/// Whether the tray icon is shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayStatus {
    /// Still connecting to the session bus.
    Connecting,
    /// Registered with the desktop's tray host.
    Available,
    /// No session bus or no tray host; the window is the only entry point.
    Unavailable,
}

/// A click on the tray icon or its menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TrayAction {
    /// Left click or "Show rust-microsip": bring the window to the front.
    Show,
    /// "Quit": quit like the window's close button (asks during a call).
    Quit,
    /// A recent call was chosen: call this number again. Not validated here.
    Dial(String),
}

/// One recent call in the tray menu.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RecentCall {
    /// Plain-text menu label, e.g. `"Missed: 1003"`.
    pub label: String,
    /// The number [`TrayAction::Dial`] carries when this item is chosen.
    pub number: String,
}

/// The dynamic part of the menu; `revision` grows on every change.
#[derive(Default)]
struct MenuState {
    revision: u32,
    recent: Vec<RecentCall>,
}

/// ARGB32 pixmap in network byte order, as `StatusNotifierItem` expects.
type Pixmap = (i32, i32, Vec<u8>);

type Shared<T> = Arc<Mutex<T>>;
fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Tray icon handle. Dropping it removes the icon.
pub struct Tray {
    actions: Receiver<TrayAction>,
    status: Shared<TrayStatus>,
    waker: Shared<Option<Waker>>,
    menu: Shared<MenuState>,
    // Nudges the worker to announce a new layout; dropping it ends the
    // worker, which closes the connection.
    changed: SyncSender<()>,
}
impl std::fmt::Debug for Tray {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tray")
            .field("status", &self.status())
            .finish_non_exhaustive()
    }
}
impl Tray {
    /// Show the icon (`rgba`: `width`×`height` RGBA8 pixels) on the user's
    /// session bus, in the background.
    pub fn session(width: u32, height: u32, rgba: &[u8]) -> Self {
        let icon = to_argb(width, height, rgba);
        let (changed, changed_rx) = mpsc::sync_channel(1);
        let (tx, actions) = mpsc::sync_channel(QUEUE);
        let status = Arc::new(Mutex::new(TrayStatus::Connecting));
        let waker: Shared<Option<Waker>> = Arc::default();
        let menu: Shared<MenuState> = Arc::default();
        let (st, wk, mn) = (Arc::clone(&status), Arc::clone(&waker), Arc::clone(&menu));
        let spawned = thread::Builder::new().name("tray".into()).spawn(move || {
            let notify = Notify { tx, waker: wk };
            match zbus::block_on(register(icon, notify, Arc::clone(&mn))) {
                Ok(conn) => {
                    *lock(&st) = TrayStatus::Available;
                    // Keep the connection (and the icon) until the handle
                    // drops, announcing each new menu layout.
                    while changed_rx.recv().is_ok() {
                        let revision = lock(&mn).revision;
                        let _ = zbus::block_on(conn.emit_signal(
                            None::<&str>,
                            MENU_PATH,
                            "com.canonical.dbusmenu",
                            "LayoutUpdated",
                            &(revision, 0i32),
                        ));
                    }
                    drop(conn);
                }
                Err(_) => *lock(&st) = TrayStatus::Unavailable,
            }
        });
        if spawned.is_err() {
            *lock(&status) = TrayStatus::Unavailable;
        }
        Self {
            actions,
            status,
            waker,
            menu,
            changed,
        }
    }

    /// Replace the recent calls in the menu (most recent first; at most
    /// [`MAX_RECENT`] are shown). Never blocks.
    pub fn set_recent(&self, mut items: Vec<RecentCall>) {
        items.truncate(MAX_RECENT);
        {
            let mut menu = lock(&self.menu);
            if menu.recent == items {
                return;
            }
            menu.recent = items;
            menu.revision = menu.revision.wrapping_add(1);
        }
        let _ = self.changed.try_send(());
    }

    /// Current availability.
    pub fn status(&self) -> TrayStatus {
        *lock(&self.status)
    }

    /// Run `waker` after each queued [`TrayAction`] (replaces any previous one).
    pub fn set_waker(&self, waker: Waker) {
        *lock(&self.waker) = Some(waker);
    }

    /// Next click, without blocking.
    pub fn try_recv(&self) -> Option<TrayAction> {
        self.actions.try_recv().ok()
    }
}

/// Convert RGBA8 to ARGB32 big-endian; an inconsistent size yields no pixmap.
fn to_argb(width: u32, height: u32, rgba: &[u8]) -> Vec<Pixmap> {
    let (Ok(w), Ok(h)) = (i32::try_from(width), i32::try_from(height)) else {
        return Vec::new();
    };
    if u64::from(width) * u64::from(height) * 4 != rgba.len() as u64 {
        return Vec::new();
    }
    let argb = rgba
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|&[r, g, b, a]| [a, r, g, b])
        .collect();
    vec![(w, h, argb)]
}

#[derive(Clone)]
struct Notify {
    tx: SyncSender<TrayAction>,
    waker: Shared<Option<Waker>>,
}
impl Notify {
    fn send(&self, action: TrayAction) {
        if self.tx.try_send(action).is_ok()
            && let Some(wake) = lock(&self.waker).as_ref()
        {
            wake();
        }
    }
}

// ponytail: registers once; if the tray host restarts (e.g. plasmashell), the
// icon stays gone until the app restarts. Watch the watcher's NameOwnerChanged
// and re-register if users hit that.
async fn register(
    icon: Vec<Pixmap>,
    notify: Notify,
    state: Shared<MenuState>,
) -> zbus::Result<zbus::Connection> {
    let name = format!("org.kde.StatusNotifierItem-{}-1", std::process::id());
    let conn = zbus::connection::Builder::session()?
        .method_timeout(CALL_TIMEOUT)
        .serve_at(
            ITEM_PATH,
            Item {
                icon,
                notify: notify.clone(),
            },
        )?
        .serve_at(MENU_PATH, Menu { notify, state })?
        .name(name.as_str())?
        .build()
        .await?;
    conn.call_method(
        Some(WATCHER),
        WATCHER_PATH,
        Some(WATCHER),
        "RegisterStatusNotifierItem",
        &(name.as_str(),),
    )
    .await?;
    Ok(conn)
}

struct Item {
    icon: Vec<Pixmap>,
    notify: Notify,
}

#[zbus::interface(name = "org.kde.StatusNotifierItem")]
impl Item {
    fn activate(&self, _x: i32, _y: i32) {
        self.notify.send(TrayAction::Show);
    }
    fn secondary_activate(&self, _x: i32, _y: i32) {
        self.notify.send(TrayAction::Show);
    }
    // The host shows the dbusmenu itself; nothing to do here.
    fn context_menu(&self, _x: i32, _y: i32) {}
    fn scroll(&self, _delta: i32, _orientation: String) {}

    #[zbus(property)]
    fn category(&self) -> &str {
        "Communications"
    }
    #[zbus(property)]
    fn id(&self) -> &str {
        "rust-microsip"
    }
    #[zbus(property)]
    fn title(&self) -> &str {
        "rust-microsip"
    }
    #[zbus(property)]
    fn status(&self) -> &str {
        "Active"
    }
    #[zbus(property)]
    fn window_id(&self) -> i32 {
        0
    }
    // Empty: hosts use the embedded pixmap, so no installed theme icon is needed.
    #[zbus(property)]
    fn icon_name(&self) -> &str {
        ""
    }
    #[zbus(property)]
    fn icon_pixmap(&self) -> Vec<Pixmap> {
        self.icon.clone()
    }
    #[zbus(property)]
    fn tool_tip(&self) -> (String, Vec<Pixmap>, String, String) {
        (
            String::new(),
            Vec::new(),
            "rust-microsip".into(),
            "SIP softphone".into(),
        )
    }
    #[zbus(property)]
    fn item_is_menu(&self) -> bool {
        false
    }
    #[zbus(property)]
    fn menu(&self) -> OwnedObjectPath {
        OwnedObjectPath::try_from(MENU_PATH).unwrap_or_default()
    }
}

/// `(id, properties, children)` of a dbusmenu node.
type Layout = (i32, HashMap<String, OwnedValue>, Vec<OwnedValue>);

// ponytail: recent-call ids are positions, so a click on a menu the host
// drew before a layout change dials what is at that position now. Encode the
// revision in the ids if hosts are seen to lag.
struct Menu {
    notify: Notify,
    state: Shared<MenuState>,
}

/// Index into the recent calls of a menu id.
fn recent_index(id: i32) -> Option<usize> {
    usize::try_from(id.checked_sub(RECENT)?).ok()
}

/// dbusmenu labels treat `_` as a mnemonic marker; show it literally.
fn escape(label: &str) -> String {
    label.replace('_', "__")
}

impl MenuState {
    fn known(&self, id: i32) -> bool {
        matches!(id, 0 | SHOW | SEPARATOR | QUIT)
            || recent_index(id).is_some_and(|i| i < self.recent.len())
    }

    /// Top-level item ids in menu order.
    fn ids(&self) -> Vec<i32> {
        let recent = (RECENT..).take(self.recent.len());
        std::iter::once(SHOW)
            .chain(recent)
            .chain([SEPARATOR, QUIT])
            .collect()
    }

    fn properties(&self, id: i32) -> HashMap<String, OwnedValue> {
        let mut out = HashMap::new();
        let mut put = |key: &str, value: Value<'_>| {
            if let Ok(value) = OwnedValue::try_from(value) {
                out.insert(key.to_owned(), value);
            }
        };
        match id {
            0 => put("children-display", Value::from("submenu")),
            SHOW => put("label", Value::from("Show rust-microsip")),
            SEPARATOR => put("type", Value::from("separator")),
            QUIT => put("label", Value::from("Quit")),
            _ => {
                if let Some(call) = recent_index(id).and_then(|i| self.recent.get(i)) {
                    put("label", Value::from(escape(&call.label)));
                }
            }
        }
        out
    }

    fn layout(&self, id: i32) -> Layout {
        let children = if id == 0 {
            self.ids()
                .into_iter()
                .filter_map(|child| {
                    let props = self.properties(child);
                    OwnedValue::try_from(Value::from((child, props, Vec::<OwnedValue>::new()))).ok()
                })
                .collect()
        } else {
            Vec::new()
        };
        (id, self.properties(id), children)
    }

    fn action(&self, id: i32) -> Option<TrayAction> {
        match id {
            SHOW => Some(TrayAction::Show),
            QUIT => Some(TrayAction::Quit),
            _ => recent_index(id)
                .and_then(|i| self.recent.get(i))
                .map(|call| TrayAction::Dial(call.number.clone())),
        }
    }
}

#[zbus::interface(name = "com.canonical.dbusmenu")]
impl Menu {
    fn get_layout(
        &self,
        parent_id: i32,
        _recursion_depth: i32,
        _property_names: Vec<String>,
    ) -> fdo::Result<(u32, Layout)> {
        let state = lock(&self.state);
        if !state.known(parent_id) {
            return Err(fdo::Error::InvalidArgs("unknown menu item".into()));
        }
        Ok((state.revision, state.layout(parent_id)))
    }
    fn get_group_properties(
        &self,
        ids: Vec<i32>,
        _property_names: Vec<String>,
    ) -> Vec<(i32, HashMap<String, OwnedValue>)> {
        let state = lock(&self.state);
        let ids = if ids.is_empty() {
            std::iter::once(0).chain(state.ids()).collect()
        } else {
            ids
        };
        ids.into_iter()
            .filter(|id| state.known(*id))
            .map(|id| (id, state.properties(id)))
            .collect()
    }
    fn get_property(&self, id: i32, name: String) -> fdo::Result<OwnedValue> {
        lock(&self.state)
            .properties(id)
            .remove(&name)
            .ok_or_else(|| fdo::Error::InvalidArgs("unknown menu property".into()))
    }
    fn event(&self, id: i32, event_id: String, _data: OwnedValue, _timestamp: u32) {
        if event_id != "clicked" {
            return;
        }
        // Release the lock before notifying (the waker runs other code).
        let action = lock(&self.state).action(id);
        if let Some(action) = action {
            self.notify.send(action);
        }
    }
    fn event_group(&self, events: Vec<(i32, String, OwnedValue, u32)>) -> Vec<i32> {
        let mut errors = Vec::new();
        for (id, event_id, data, timestamp) in events {
            let known = lock(&self.state).known(id);
            if known {
                self.event(id, event_id, data, timestamp);
            } else {
                errors.push(id);
            }
        }
        errors
    }
    fn about_to_show(&self, _id: i32) -> bool {
        false
    }
    fn about_to_show_group(&self, _ids: Vec<i32>) -> (Vec<i32>, Vec<i32>) {
        (Vec::new(), Vec::new())
    }

    #[zbus(property)]
    fn version(&self) -> u32 {
        3
    }
    #[zbus(property)]
    fn text_direction(&self) -> &str {
        "ltr"
    }
    #[zbus(property)]
    fn status(&self) -> &str {
        "normal"
    }
    #[zbus(property)]
    fn icon_theme_path(&self) -> Vec<String> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notify() -> (Notify, Receiver<TrayAction>) {
        let (tx, rx) = mpsc::sync_channel(QUEUE);
        (
            Notify {
                tx,
                waker: Arc::default(),
            },
            rx,
        )
    }

    #[test]
    fn pixels_become_argb_big_endian() {
        let icon = to_argb(1, 1, &[0x10, 0x20, 0x30, 0x40]);
        assert_eq!(icon, vec![(1, 1, vec![0x40, 0x10, 0x20, 0x30])]);
        assert!(to_argb(2, 2, &[0; 4]).is_empty());
    }

    fn label(state: &MenuState, id: i32) -> Option<String> {
        state
            .properties(id)
            .remove("label")
            .and_then(|v| String::try_from(v).ok())
    }

    fn menu(notify: Notify, recent: Vec<RecentCall>) -> Menu {
        Menu {
            notify,
            state: Arc::new(Mutex::new(MenuState {
                revision: 1,
                recent,
            })),
        }
    }

    fn call(label: &str, number: &str) -> RecentCall {
        RecentCall {
            label: label.into(),
            number: number.into(),
        }
    }

    #[test]
    fn menu_lists_show_and_quit() {
        let state = MenuState::default();
        let (_, props, children) = state.layout(0);
        assert!(props.contains_key("children-display"));
        assert_eq!(children.len(), 3);
        assert_eq!(label(&state, SHOW).as_deref(), Some("Show rust-microsip"));
        assert_eq!(label(&state, QUIT).as_deref(), Some("Quit"));
    }

    #[test]
    fn recent_calls_sit_between_show_and_quit() {
        let (notify, _rx) = notify();
        let menu = menu(notify, Vec::new());
        let before = menu.get_layout(0, -1, Vec::new()).map(|(rev, _)| rev);
        let tray_menu = &menu.state;
        lock(tray_menu).recent = vec![
            call("\u{2197} Ana_Lima", "1002"),
            call("Missed: 1003", "1003"),
        ];
        lock(tray_menu).revision += 1;
        let state = lock(tray_menu);
        assert_eq!(state.ids(), vec![SHOW, RECENT, RECENT + 1, SEPARATOR, QUIT]);
        assert_eq!(state.layout(0).2.len(), 5);
        assert_eq!(label(&state, RECENT).as_deref(), Some("\u{2197} Ana__Lima"));
        assert_eq!(label(&state, RECENT + 1).as_deref(), Some("Missed: 1003"));
        assert!(!state.known(RECENT + 2));
        drop(state);
        let after = menu.get_layout(0, -1, Vec::new()).map(|(rev, _)| rev);
        assert_eq!((before.ok(), after.ok()), (Some(1), Some(2)));
        assert!(menu.get_layout(RECENT + 2, -1, Vec::new()).is_err());
        assert_eq!(menu.get_group_properties(Vec::new(), Vec::new()).len(), 6);
    }

    #[test]
    fn set_recent_bumps_the_revision_only_on_change() {
        let tray = Tray::session(1, 1, &[0; 4]);
        let revision = || lock(&tray.menu).revision;
        let items: Vec<_> = (0..7)
            .map(|i| call(&format!("{i}"), &format!("10{i}")))
            .collect();
        tray.set_recent(items.clone());
        assert_eq!(revision(), 1);
        assert_eq!(lock(&tray.menu).recent.len(), MAX_RECENT);
        tray.set_recent(items);
        assert_eq!(revision(), 1);
        tray.set_recent(Vec::new());
        assert_eq!(revision(), 2);
    }

    #[test]
    fn clicks_become_actions() {
        let (notify, rx) = notify();
        let menu = menu(notify.clone(), vec![call("\u{2199} 1002", "1002")]);
        let data = || OwnedValue::from(0i32);
        menu.event(QUIT, "hovered".into(), data(), 0);
        menu.event(SEPARATOR, "clicked".into(), data(), 0);
        menu.event(RECENT + 1, "clicked".into(), data(), 0);
        assert!(rx.try_recv().is_err());
        menu.event(QUIT, "clicked".into(), data(), 0);
        assert_eq!(rx.try_recv(), Ok(TrayAction::Quit));
        menu.event(RECENT, "clicked".into(), data(), 0);
        assert_eq!(rx.try_recv(), Ok(TrayAction::Dial("1002".into())));
        assert_eq!(
            menu.event_group(vec![
                (SHOW, "clicked".into(), data(), 0),
                (99, "clicked".into(), data(), 0)
            ]),
            vec![99]
        );
        assert_eq!(rx.try_recv(), Ok(TrayAction::Show));
        Item {
            icon: Vec::new(),
            notify,
        }
        .activate(0, 0);
        assert_eq!(rx.try_recv(), Ok(TrayAction::Show));
    }

    #[test]
    fn missing_bus_degrades_to_unavailable() {
        // Not a test of the real session: without DBUS_SESSION_BUS_ADDRESS the
        // probe must end Unavailable, never hang.
        let tray = Tray::session(1, 1, &[0; 4]);
        for _ in 0..100 {
            if tray.status() != TrayStatus::Connecting {
                break;
            }
            thread::sleep(Duration::from_millis(50));
        }
        assert_ne!(tray.status(), TrayStatus::Connecting);
    }
}
