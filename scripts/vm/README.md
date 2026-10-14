# Test VMs

Disposable VirtualBox VMs for checks on real desktops. They hold no personal data and are deleted
after use; recreate them when needed. Results are recorded in receipts, for example
[the desktop VM receipt](../../docs/receipts/desktop-vms-2026-10-13.md).

Requirements on the host: VirtualBox 7, `curl`, `gpgv`, `qemu-img`, `genisoimage`, `ssh` and
`git` (used to refuse a key directory inside a repository), plus the `ubuntu-cloudimage-keyring`
package for Ubuntu images. Each VM uses 4 GB of RAM, 2 CPUs and a 40 GB dynamic disk (3 to 8 GB
used after provisioning).

## Linux desktops

| Flavor | System | Desktop | Session |
| --- | --- | --- | --- |
| `kde` | Ubuntu 24.04 | KDE Plasma 5 | X11 |
| `gnome` | Ubuntu 24.04 | GNOME | Wayland |
| `ubuntu2604` | Ubuntu 26.04 | GNOME | Wayland |
| `fedora` | Fedora 44 | KDE Plasma 6 | Wayland |
| `arch` | Arch Linux | Xfce | X11 |

### Create

Keep the key directory **outside** any repository (the script refuses otherwise):

```sh
KEYS=$HOME/.cache/rust-microsip-vm-keys
sh scripts/vm/create-desktop-vm.sh kde 2251 "$KEYS"      # one SSH port per VM
until ssh -i "$KEYS/id_vm" -p 2251 -o StrictHostKeyChecking=no tester@127.0.0.1 \
    test -f /var/tmp/provisioned; do sleep 30; done      # 10-20 min of package installs
```

The script downloads the official cloud image, verifies its signed checksums (Arch: SHA-256 over
HTTPS only), and creates the VM `rust-microsip-<flavor>`. The VM gets a fictional user `tester`
with a locked password and autologin. SSH is forwarded on `127.0.0.1` only, and clipboard and
drag-and-drop are disabled.

### Use

```sh
O="-i $KEYS/id_vm -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null"
scp $O -P 2251 dist/rust-microsip dist/rust-microsip-*-x86_64.AppImage dist/SHA256SUMS \
    scripts/vm/vm-run-app.sh scripts/vm/vm-quit-app.sh tester@127.0.0.1:
ssh $O -p 2251 tester@127.0.0.1 'sha256sum -c --ignore-missing SHA256SUMS'
ssh $O -p 2251 tester@127.0.0.1 'sh vm-run-app.sh ./rust-microsip elf'   # session, tray, icon
VBoxManage controlvm rust-microsip-kde screenshotpng shot.png              # see the desktop
ssh $O -p 2251 tester@127.0.0.1 'sh vm-quit-app.sh'                       # tray Quit or SIGTERM
```

Known setup details:
- GNOME shows a welcome wizard on first login. Stop it with `pkill -x gnome-initial-s`; never use
  `pkill -f`, which also matches your own SSH command line.
- Turn off screen locking: `gsettings set org.gnome.desktop.screensaver lock-enabled false`.
- The lab PBX runs on an internal Docker network and these VMs cannot reach it, so call tests need
  the host-only profile (Linux release task 1.5).

### Delete

```sh
VBoxManage controlvm rust-microsip-kde poweroff
VBoxManage unregistervm rust-microsip-kde --delete
rm -rf "$HOME/VirtualBox VMs/rust-microsip-kde"     # leftover seed.iso
```

## Windows

There is no committed script yet. Two routes have worked:
- **Windows Server 2022 evaluation box with Vagrant**, the setup used by the sibling rustXterm
  project. Use `gusztavvargadr/windows-server-2022-standard`, account vagrant/vagrant, and drive
  it with `VBoxManage guestcontrol`.
- **Windows 11 evaluation image** for desktop-only checks: toasts, Focus Assist, Default apps
  (add-windows-port task 6.2).

Run Windows checks this way:
1. Build `make windows` and copy `dist/windows/*.exe` into a scratch folder with
   `VBoxManage guestcontrol <vm> copyto`.
2. Start GUI programs in the logged-on session through a scheduled task that uses the
   interactive logon type.
3. Take screenshots inside the guest with `System.Drawing` `CopyFromScreen`. The VirtualBox
   screenshot fails once wgpu renders.
4. GPU-less VMs render through DX12 WARP. This needs the vendored wgpu-hal patch
   ([vendor/README.md](../../vendor/README.md)).
5. MSI install and uninstall tests run in the same VM.
6. When done, delete the scratch folder, the scheduled tasks and
   `%LOCALAPPDATA%\rust-microsip`, then power the VM off or delete it.
