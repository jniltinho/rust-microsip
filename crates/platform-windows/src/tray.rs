//! Notification-area icon with Show/Quit (design W5).
//!
//! A dedicated thread owns a hidden window and its message loop; the icon is
//! added with `Shell_NotifyIconW` and re-added when Explorer restarts
//! (`TaskbarCreated`). Clicks only enqueue a [`TrayAction`] into a bounded
//! queue and run the waker. The window is a hidden top-level window rather
//! than a message-only one because message-only windows never receive the
//! broadcast `TaskbarCreated` message. Dropping the [`Tray`] posts `WM_CLOSE`,
//! which removes the icon and ends the thread.

use crate::Waker;
use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use windows_sys::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIN_SELECT, NOTIFY_ICON_MESSAGE,
    NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreateIcon, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyIcon,
    DestroyMenu, DestroyWindow, DispatchMessageW, GetCursorPos, GetMessageW, HICON, HMENU,
    MF_SEPARATOR, MF_STRING, MSG, PostMessageW, PostQuitMessage, RegisterClassW,
    RegisterWindowMessageW, SetForegroundWindow, TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTBUTTON,
    TrackPopupMenu, WM_APP, WM_CLOSE, WM_CONTEXTMENU, WM_DESTROY, WM_LBUTTONUP, WM_NULL,
    WM_RBUTTONUP, WNDCLASSW, WS_EX_TOOLWINDOW, WS_OVERLAPPED,
};

const QUEUE: usize = 8;
const CALLBACK: u32 = WM_APP + 1;
const SHOW: usize = 1;
const QUIT: usize = 2;
/// `NIN_SELECT | NINF_KEY` (not exported by windows-sys).
const NIN_KEYSELECT: u32 = NIN_SELECT + 1;

/// Whether the tray icon is shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayStatus {
    /// Still creating the icon.
    Connecting,
    /// Shown in the notification area.
    Available,
    /// Not shown; the window is the only entry point.
    Unavailable,
}

/// A click on the tray icon or its menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayAction {
    /// Left click or "Show rust-microsip": bring the window to the front.
    Show,
    /// "Quit": quit like the window's close button (asks during a call).
    Quit,
}

type Shared<T> = Arc<Mutex<T>>;
fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Tray icon handle. Dropping it removes the icon.
pub struct Tray {
    actions: Receiver<TrayAction>,
    status: Shared<TrayStatus>,
    waker: Shared<Option<Waker>>,
    stop: Arc<Stop>,
}

/// Hand-off between [`Tray::drop`] and the thread that creates the window.
#[derive(Default)]
struct Stop {
    stopped: AtomicBool,
    /// The window handle as an address; 0 until created.
    hwnd: AtomicUsize,
}

impl std::fmt::Debug for Tray {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tray")
            .field("status", &self.status())
            .finish_non_exhaustive()
    }
}

impl Tray {
    /// Show the icon (`rgba`: `width`×`height` RGBA8 pixels) in the
    /// notification area, from a background thread.
    pub fn session(width: u32, height: u32, rgba: &[u8]) -> Self {
        let icon = to_bgra(width, height, rgba);
        let (tx, actions) = mpsc::sync_channel(QUEUE);
        let status = Arc::new(Mutex::new(TrayStatus::Connecting));
        let waker: Shared<Option<Waker>> = Arc::default();
        let stop = Arc::new(Stop::default());
        let notify = Notify {
            tx,
            waker: Arc::clone(&waker),
        };
        let (st, sp) = (Arc::clone(&status), Arc::clone(&stop));
        let spawned = thread::Builder::new()
            .name("tray".into())
            .spawn(move || run(icon, notify, &st, &sp));
        if spawned.is_err() {
            *lock(&status) = TrayStatus::Unavailable;
        }
        Self {
            actions,
            status,
            waker,
            stop,
        }
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

impl Drop for Tray {
    fn drop(&mut self) {
        // SeqCst on both sides: either this sees the handle or the thread sees
        // `stopped` right after publishing it.
        self.stop.stopped.store(true, Ordering::SeqCst);
        let hwnd = self.stop.hwnd.load(Ordering::SeqCst);
        if hwnd != 0 {
            post(hwnd as HWND, WM_CLOSE);
        }
    }
}

/// `width`×`height` RGBA8 to the BGRA8 rows `CreateIcon` expects (top-down);
/// an inconsistent size yields `None`.
fn to_bgra(width: u32, height: u32, rgba: &[u8]) -> Option<(i32, i32, Vec<u8>)> {
    let (w, h) = (i32::try_from(width).ok()?, i32::try_from(height).ok()?);
    if w == 0 || h == 0 || u64::from(width) * u64::from(height) * 4 != rgba.len() as u64 {
        return None;
    }
    let bgra = rgba
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|&[r, g, b, a]| [b, g, r, a])
        .collect();
    Some((w, h, bgra))
}

/// Size of the all-zero 1-bpp AND mask: rows padded to 16 bits.
fn mask_len(width: i32, height: i32) -> usize {
    let row = (width.unsigned_abs() as usize).div_ceil(16) * 2;
    row * height.unsigned_abs() as usize
}

/// UTF-16, NUL-terminated.
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
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

/// Per-thread state read by the window procedure.
struct State {
    notify: Notify,
    data: NOTIFYICONDATAW,
    taskbar_created: u32,
    status: Shared<TrayStatus>,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

fn run(
    icon: Option<(i32, i32, Vec<u8>)>,
    notify: Notify,
    status: &Shared<TrayStatus>,
    stop: &Stop,
) {
    let Some(hicon) = icon.and_then(|(w, h, bgra)| create_icon(w, h, &bgra)) else {
        *lock(status) = TrayStatus::Unavailable;
        return;
    };
    let class = wide("rust-microsip-tray");
    let instance = module_handle();
    // Fails harmlessly when a previous Tray already registered the class.
    register_class(&WNDCLASSW {
        lpfnWndProc: Some(window_proc),
        hInstance: instance,
        lpszClassName: class.as_ptr(),
        ..WNDCLASSW::default()
    });
    let hwnd = create_window(&class, instance);
    if hwnd.is_null() {
        *lock(status) = TrayStatus::Unavailable;
        destroy_icon(hicon);
        return;
    }
    let mut data = NOTIFYICONDATAW {
        cbSize: size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: 1,
        uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
        uCallbackMessage: CALLBACK,
        hIcon: hicon,
        ..NOTIFYICONDATAW::default()
    };
    for (dst, src) in data.szTip.iter_mut().zip(wide("rust-microsip")) {
        *dst = src;
    }
    let added = notify_icon(NIM_ADD, &data);
    *lock(status) = if added {
        TrayStatus::Available
    } else {
        TrayStatus::Unavailable
    };
    STATE.with_borrow_mut(|s| {
        *s = Some(State {
            notify,
            data,
            taskbar_created: register_message(&wide("TaskbarCreated")),
            status: Arc::clone(status),
        });
    });
    stop.hwnd.store(hwnd as usize, Ordering::SeqCst);
    if stop.stopped.load(Ordering::SeqCst) {
        post(hwnd, WM_CLOSE);
    }
    let mut msg = MSG::default();
    // 0 on WM_QUIT, -1 on error: both end the loop.
    while get_message(&mut msg) > 0 {
        dispatch(&msg);
    }
    STATE.with_borrow_mut(Option::take);
    *lock(status) = TrayStatus::Unavailable;
    destroy_icon(hicon);
}

extern "system" fn window_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // Copy what is needed and release the borrow: the popup menu below runs a
    // nested message loop that re-enters this procedure.
    let state = STATE.with_borrow(|s| {
        s.as_ref().map(|s| {
            (
                s.notify.clone(),
                s.data,
                s.taskbar_created,
                Arc::clone(&s.status),
            )
        })
    });
    let Some((notify, data, taskbar_created, status)) = state else {
        return def_window_proc(hwnd, msg, wparam, lparam);
    };
    match msg {
        CALLBACK => {
            // Low word: the mouse/keyboard event (any NOTIFYICON_VERSION).
            match (lparam & 0xffff) as u32 {
                WM_LBUTTONUP | NIN_SELECT | NIN_KEYSELECT => notify.send(TrayAction::Show),
                WM_RBUTTONUP | WM_CONTEXTMENU => {
                    if let Some(action) = show_menu(hwnd) {
                        notify.send(action);
                    }
                }
                _ => {}
            }
            0
        }
        WM_CLOSE => {
            notify_icon(NIM_DELETE, &data);
            destroy_window(hwnd);
            0
        }
        WM_DESTROY => {
            post_quit();
            0
        }
        _ if msg == taskbar_created && taskbar_created != 0 => {
            // Explorer restarted: the old icon is gone.
            let added = notify_icon(NIM_ADD, &data);
            *lock(&status) = if added {
                TrayStatus::Available
            } else {
                TrayStatus::Unavailable
            };
            0
        }
        _ => def_window_proc(hwnd, msg, wparam, lparam),
    }
}

/// "Show rust-microsip", separator, "Quit" at the cursor; the chosen action.
fn show_menu(hwnd: HWND) -> Option<TrayAction> {
    let menu = create_popup_menu();
    if menu.is_null() {
        return None;
    }
    append_menu(menu, MF_STRING, SHOW, &wide("Show rust-microsip"));
    append_menu(menu, MF_SEPARATOR, 0, &[0]);
    append_menu(menu, MF_STRING, QUIT, &wide("Quit"));
    let point = cursor_pos();
    // Without this the menu does not close when the user clicks elsewhere.
    set_foreground(hwnd);
    let chosen = track_menu(menu, hwnd, point);
    // KB135788: make the next click on the icon work.
    post(hwnd, WM_NULL);
    destroy_menu(menu);
    match usize::try_from(chosen).ok()? {
        SHOW => Some(TrayAction::Show),
        QUIT => Some(TrayAction::Quit),
        _ => None,
    }
}

// --- Win32 wrappers: one unsafe call each ----------------------------------

fn module_handle() -> HINSTANCE {
    // SAFETY: a null name asks for the current exe's handle; nothing is freed.
    unsafe { GetModuleHandleW(std::ptr::null()) }
}

fn register_class(class: &WNDCLASSW) -> u16 {
    // SAFETY: `class` and the class name it points to outlive the call.
    unsafe { RegisterClassW(class) }
}

fn create_window(class: &[u16], instance: HINSTANCE) -> HWND {
    // SAFETY: `class` is NUL-terminated and alive; the window is never shown
    // (no WS_VISIBLE) and has no parent, so it receives broadcasts.
    unsafe {
        CreateWindowExW(
            WS_EX_TOOLWINDOW,
            class.as_ptr(),
            class.as_ptr(),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            instance,
            std::ptr::null(),
        )
    }
}

fn register_message(name: &[u16]) -> u32 {
    // SAFETY: `name` is NUL-terminated and alive.
    unsafe { RegisterWindowMessageW(name.as_ptr()) }
}

fn notify_icon(message: NOTIFY_ICON_MESSAGE, data: &NOTIFYICONDATAW) -> bool {
    // SAFETY: `data` is a fully initialized NOTIFYICONDATAW with its cbSize.
    unsafe { Shell_NotifyIconW(message, data) != 0 }
}

/// A 32-bit icon from top-down BGRA rows; alpha gives the transparency, so
/// the AND mask is all zero. `None` if Windows refuses it.
fn create_icon(width: i32, height: i32, bgra: &[u8]) -> Option<HICON> {
    let mask = vec![0u8; mask_len(width, height)];
    // SAFETY: `bgra` holds width*height*4 bytes (checked by `to_bgra`) and
    // `mask` one padded 1-bpp plane; both outlive the call, which copies them.
    let icon = unsafe {
        CreateIcon(
            std::ptr::null_mut(),
            width,
            height,
            1,
            32,
            mask.as_ptr(),
            bgra.as_ptr(),
        )
    };
    (!icon.is_null()).then_some(icon)
}

fn destroy_icon(icon: HICON) {
    // SAFETY: `icon` came from CreateIcon and is no longer used by the shell
    // (deleted or never added).
    unsafe { DestroyIcon(icon) };
}

fn get_message(msg: &mut MSG) -> i32 {
    // SAFETY: `msg` is a valid, writable MSG; a null window takes all of this
    // thread's messages.
    unsafe { GetMessageW(msg, std::ptr::null_mut(), 0, 0) }
}

fn dispatch(msg: &MSG) {
    // SAFETY: `msg` was filled by GetMessageW on this thread.
    unsafe { DispatchMessageW(msg) };
}

fn def_window_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY: forwards the arguments the system passed to the window procedure.
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

fn post(hwnd: HWND, msg: u32) -> bool {
    // SAFETY: posting takes no pointers; a stale handle only makes it fail.
    unsafe { PostMessageW(hwnd, msg, 0, 0) != 0 }
}

fn destroy_window(hwnd: HWND) {
    // SAFETY: called on the thread that created `hwnd`.
    unsafe { DestroyWindow(hwnd) };
}

fn post_quit() {
    // SAFETY: no arguments besides the exit code.
    unsafe { PostQuitMessage(0) };
}

fn create_popup_menu() -> HMENU {
    // SAFETY: no arguments; the menu is destroyed by `destroy_menu`.
    unsafe { CreatePopupMenu() }
}

fn append_menu(menu: HMENU, flags: u32, id: usize, text: &[u16]) {
    // SAFETY: `menu` is live and `text` NUL-terminated (ignored for separators).
    unsafe { AppendMenuW(menu, flags, id, text.as_ptr()) };
}

fn cursor_pos() -> POINT {
    let mut point = POINT::default();
    // SAFETY: `point` is a valid, writable POINT.
    unsafe { GetCursorPos(&mut point) };
    point
}

fn set_foreground(hwnd: HWND) {
    // SAFETY: takes only a window handle.
    unsafe { SetForegroundWindow(hwnd) };
}

/// The chosen command id, or 0 when the menu was dismissed.
fn track_menu(menu: HMENU, hwnd: HWND, point: POINT) -> i32 {
    // SAFETY: `menu` and `hwnd` are live and owned by this thread; the
    // rectangle pointer may be null.
    unsafe {
        TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON,
            point.x,
            point.y,
            0,
            hwnd,
            std::ptr::null(),
        )
    }
}

fn destroy_menu(menu: HMENU) {
    // SAFETY: `menu` came from CreatePopupMenu and is not shown any more.
    unsafe { DestroyMenu(menu) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pixels_become_bgra() {
        let icon = to_bgra(1, 1, &[0x10, 0x20, 0x30, 0x40]);
        assert_eq!(icon, Some((1, 1, vec![0x30, 0x20, 0x10, 0x40])));
        assert_eq!(to_bgra(2, 2, &[0; 4]), None);
        assert_eq!(to_bgra(0, 0, &[]), None);
    }

    #[test]
    fn mask_rows_are_word_aligned() {
        assert_eq!(mask_len(16, 16), 32);
        assert_eq!(mask_len(17, 1), 4);
        assert_eq!(mask_len(1, 3), 6);
    }

    #[test]
    fn wide_strings_are_terminated() {
        assert_eq!(wide("ab"), vec![u16::from(b'a'), u16::from(b'b'), 0]);
    }

    #[test]
    fn session_settles_and_drops() {
        let tray = Tray::session(16, 16, &[0x80; 16 * 16 * 4]);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while tray.status() == TrayStatus::Connecting && std::time::Instant::now() < deadline {
            thread::sleep(std::time::Duration::from_millis(20));
        }
        // Available with a shell, Unavailable on a headless runner.
        eprintln!("tray status: {:?}", tray.status());
        assert_ne!(tray.status(), TrayStatus::Connecting);
        drop(tray);
    }

    #[test]
    fn dropping_before_start_is_safe() {
        let tray = Tray::session(1, 1, &[0, 0, 0, 0]);
        drop(tray);
    }
}
