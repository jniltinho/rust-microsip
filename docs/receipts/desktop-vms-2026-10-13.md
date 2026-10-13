# Desktop VM receipt: window icon, tray and AppImage on clean desktops (2026-10-13)

**Result: on clean Ubuntu 24.04 VMs with KDE Plasma (X11) and GNOME (Wayland), the ELF and the AppImage start, show the app icon, register the tray icon where a tray host exists, run normally without one, and quit from the tray menu or on SIGTERM with nothing left running. The AppImage runs on a system without `libfuse2`. Running these checks found one defect, fixed in this commit: the link handler entry named a program that was not on `PATH`, so GLib ignored it, which left GNOME without the dock icon and the link handler without a target. No calls were made; the lab PBX is not reachable from the VMs (task 1.5). This is not the task 9.6 checklist.**

## Setup

- VirtualBox 7.2.10 on the owner's machine. The VMs come from the official Ubuntu 24.04 cloud image (checked against Ubuntu's `SHA256SUMS`) and cloud-init; [`scripts/vm/create-desktop-vm.sh`](../../scripts/vm/create-desktop-vm.sh) reproduces them.
  - `rust-microsip-kde`: Plasma 5.27, SDDM autologin, X11 session.
  - `rust-microsip-gnome`: Ubuntu GNOME 46, GDM autologin, Wayland session.
  - The VMs for this run (`rust-microsip-kde-x11`, `rust-microsip-gnome-wayland`) were made with an earlier version of the script, which also set a fictional password for the test user. The committed script locks the password, because autologin and the SSH key do not need one.
- Fictional user `tester`; SSH forwarded on 127.0.0.1 only; null audio driver; no clipboard or drag-and-drop; no personal settings or accounts.
- Artifacts: `make dist` at commit `1940f2e`, checked with `make dist-check` and `sha256sum -c` inside each VM.
  - ELF `aa6603a8231306d7091fcab06aed4cdfacfff84fefb1d52459fdd45d079d8ed0`.
  - AppImage `27b5f3c507e5dc02ac04a9369bbc88edab329ce52faff4c0f9669bfa887b3fa9`.
- [`scripts/vm/vm-run-app.sh`](../../scripts/vm/vm-run-app.sh) starts the app in the logged-in session with scratch XDG directories. [`vm-quit-app.sh`](../../scripts/vm/vm-quit-app.sh) quits it through the tray menu, or with SIGTERM when there is no tray item. Screens were captured with `VBoxManage controlvm … screenshotpng`.

## Results

| Desktop | Artifact | Tray host | Window | Tray item | Window icon | Quit | Left running |
| --- | --- | --- | --- | --- | --- | --- | --- |
| KDE X11 | ELF | yes | opens | registered, shown in the panel | `_NET_WM_ICON` 256×256, title bar and task bar | tray menu | nothing |
| KDE X11 | AppImage | yes | opens | registered | 256×256 | tray menu | nothing |
| GNOME Wayland, AppIndicator on | ELF | yes | opens (software GL) | registered, shown in the top bar | dock shows the icon once the entry is installed (see below) | tray menu | nothing |
| GNOME Wayland, AppIndicator off | ELF | **no** | opens; no error logged | none (expected) | — | SIGTERM | nothing |
| GNOME Wayland, no `libfuse2` | AppImage | yes | opens | registered | — | tray menu | nothing |

## Defect found and fixed

On Wayland, a dock takes the icon from the `.desktop` entry whose name matches the app id, not from the window. The opt-in link handler entry used `Exec=rust-microsip %u`. That program is not on `PATH` for a downloaded ELF or an AppImage, so GLib refused the entry (`Gio.DesktopAppInfo.new` returned nothing). As a result, GNOME showed a generic icon, and `sip:`/`tel:` links had no valid handler.

The fix:
- **Install**: the entry now names the running executable (`$APPIMAGE` inside an AppImage) as a quoted absolute path. Paths that would need escaping fall back to the bare name. The icon is installed under `$XDG_DATA_HOME/icons/hicolor/256x256/apps/`.
- **Remove**: the entry is deleted only if it is exactly one that rust-microsip can write; the icon goes only if it is still exactly ours.
- **Check in the VM**: with an entry in the new form, GLib loaded it with icon `rust-microsip` and the GNOME dock showed the app icon.

Unit tests cover entry rendering, recognition, odd paths, and the install and remove round trip.

## Not established

- Incoming-call notifications and their Answer/Reject actions on these desktops: no PBX is reachable from the VMs (task 8.1 stays open).
- On Wayland, the fallback window frame drawn by winit shows no title text. This is cosmetic; the title is still set for the compositor.
- Calls, audio and the full manual checklist on the exact candidate (tasks 1.5, 9.6).
