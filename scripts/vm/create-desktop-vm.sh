#!/bin/sh
# Create a clean VirtualBox desktop VM for manual and scripted checks:
#   sh scripts/vm/create-desktop-vm.sh <flavor> <ssh-port> <key-dir>
# Flavors (official cloud images + cloud-init, autologin of a fictional user):
#   kde         Ubuntu 24.04, KDE Plasma 5 (X11, SDDM)
#   gnome       Ubuntu 24.04, GNOME (Wayland, GDM)
#   ubuntu2604  Ubuntu 26.04, GNOME (Wayland, GDM)
#   fedora      Fedora 44, KDE Plasma 6 (Wayland, Plasma Login Manager)
#   arch        Arch Linux, Xfce (X11, LightDM)
# Images are checked against the distribution's checksums; Ubuntu and Fedora
# checksum files are signature-verified (Ubuntu: ubuntu-cloudimage-keyring;
# Fedora: the key at fedoraproject.org/fedora.gpg). Arch is checked against
# its SHA256 over HTTPS only.
# ponytail: Arch image signature not verified; add its signing key if needed.
# User "tester" has a locked password and an SSH key generated in <key-dir>;
# SSH is forwarded on 127.0.0.1:<ssh-port> only; clipboard and drag-and-drop
# are off; audio uses the null driver. No personal data or account is copied
# into the VM. Keep <key-dir> outside the repository: it holds the VM's
# private key. "current"/"latest" images move with distribution builds: the
# VM is repeatable, not bit-for-bit reproducible.
set -eu
flavor=${1:?flavor}; port=${2:?ssh port}; keys=${3:?key directory}
ubuntu_noble=https://cloud-images.ubuntu.com/noble/current
fedora=https://dl.fedoraproject.org/pub/fedora/linux/releases/44/Cloud/x86_64/images
arch=https://geo.mirror.pkgbuild.com/images/latest
case "$flavor" in
    kde|gnome)
        url=$ubuntu_noble/noble-server-cloudimg-amd64.img; sums=$ubuntu_noble/SHA256SUMS; check=ubuntu ;;
    ubuntu2604)
        url=https://cloud-images.ubuntu.com/releases/26.04/release/ubuntu-26.04-server-cloudimg-amd64.img
        sums=https://cloud-images.ubuntu.com/releases/26.04/release/SHA256SUMS; check=ubuntu ;;
    fedora)
        url=$fedora/Fedora-Cloud-Base-Generic-44-1.7.x86_64.qcow2
        sums=$fedora/Fedora-Cloud-44-1.7-x86_64-CHECKSUM; check=fedora ;;
    arch)
        url=$arch/Arch-Linux-x86_64-cloudimg.qcow2; sums=$url.SHA256; check=plain ;;
    *) echo "usage: $0 kde|gnome|ubuntu2604|fedora|arch <ssh-port> <key-dir>" >&2; exit 2 ;;
esac
# Desktop packages, display manager, autologin file and its content ("|" = newline).
case "$flavor" in
    kde) install="apt-get install -y kde-plasma-desktop sddm x11-utils xdotool imagemagick"; dm=sddm
         autologin_path=/etc/sddm.conf.d/autologin.conf; autologin="[Autologin]|User=tester|Session=plasma" ;;
    gnome|ubuntu2604) install="apt-get install -y ubuntu-desktop-minimal gdm3 x11-utils imagemagick"; dm=gdm3
         autologin_path=/etc/gdm3/custom.conf
         autologin="[daemon]|AutomaticLoginEnable=true|AutomaticLogin=tester|WaylandEnable=true" ;;
    # Fedora 44 KDE uses the Plasma Login Manager and a first-run wizard.
    fedora) install="dnf -y install @kde-desktop-environment xprop ImageMagick alsa-lib && dnf -y remove plasma-setup"; dm=plasmalogin
         autologin_path=/etc/plasmalogin.conf.d/autologin.conf; autologin="[Autologin]|User=tester|Session=plasma" ;;
    arch) install="pacman -Syu --noconfirm --needed xorg-server xfce4 lightdm lightdm-gtk-greeter alsa-lib xorg-xprop xdotool imagemagick && groupadd -rf autologin && gpasswd -a tester autologin"; dm=lightdm
         autologin_path=/etc/lightdm/lightdm.conf.d/50-autologin.conf
         autologin="[Seat:*]|autologin-user=tester|autologin-session=xfce" ;;
esac
name="rust-microsip-$flavor"
base=$(VBoxManage list systemproperties | sed -n 's/^Default machine folder: *//p')
dir="$base/$name"
command -v git >/dev/null || { echo "git is needed to check <key-dir>" >&2; exit 2; }
mkdir -p "$keys"
if git -C "$keys" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    echo "keep <key-dir> outside any Git work tree: it holds a private key" >&2; exit 2
fi
mkdir -p "$dir"
[ -f "$keys/id_vm" ] || ssh-keygen -q -t ed25519 -N '' -C rust-microsip-vm-test -f "$keys/id_vm"
file=$(basename "$url")
curl -fsSL -o "$dir/$file" "$url"
curl -fsSL -o "$dir/SUMS" "$sums"
case "$check" in
    ubuntu)
        curl -fsSL -o "$dir/SUMS.gpg" "$sums.gpg"
        gpgv --keyring /usr/share/keyrings/ubuntu-cloudimage-keyring.gpg "$dir/SUMS.gpg" "$dir/SUMS" ;;
    fedora)
        curl -fsSL -o "$dir/fedora.gpg" https://fedoraproject.org/fedora.gpg
        gpg --dearmor < "$dir/fedora.gpg" > "$dir/fedora.kbx"
        # Clearsigned: parse only the signed text, never what surrounds it.
        gpgv --keyring "$dir/fedora.kbx" --output "$dir/SUMS.signed" "$dir/SUMS"
        mv "$dir/SUMS.signed" "$dir/SUMS"
        rm "$dir/fedora.gpg" "$dir/fedora.kbx" ;;
esac
# Ubuntu "<sum> *file", Arch "<sum>  file", Fedora "SHA256 (file) = <sum>".
sum=$(sed -nE "s/^([0-9a-f]{64}) [ *]?$file\$/\\1/p; s/^SHA256 \\($file\\) = ([0-9a-f]{64})\$/\\1/p" "$dir/SUMS" | head -1)
[ -n "$sum" ] || { echo "no checksum for $file" >&2; exit 1; }
printf '%s  %s\n' "$sum" "$file" | (cd "$dir" && sha256sum --check --strict -)
qemu-img convert -O vdi "$dir/$file" "$dir/disk.vdi"
rm -f "$dir/$file" "$dir/SUMS" "$dir/SUMS.gpg"
VBoxManage modifymedium disk "$dir/disk.vdi" --resize 40960 >/dev/null
seed=$(mktemp -d)
trap 'rm -rf "$seed"' EXIT
{
    echo "#cloud-config"
    echo "hostname: rms-$flavor"
    echo "users:"
    echo "  - name: tester"
    echo "    gecos: Fictional Tester"
    echo "    shell: /bin/bash"
    case "$flavor" in fedora|arch) admin=wheel ;; *) admin=sudo ;; esac
    echo "    groups: [$admin, audio, video]"
    echo "    sudo: ALL=(ALL) NOPASSWD:ALL"
    echo "    lock_passwd: true"
    echo "    ssh_authorized_keys: [\"$(cat "$keys/id_vm.pub")\"]"
    echo "ssh_pwauth: false"
    echo "write_files:"
    echo "  - path: $autologin_path"
    echo "    content: |"
    printf '%s\n' "$autologin" | tr '|' '\n' | sed 's/^/      /'
    echo "runcmd:"
    # The marker exists only when every step succeeded.
    # Keep the autologin file written above when the display manager ships its own.
    case "$flavor" in kde|gnome|ubuntu2604) install="apt-get update && DEBIAN_FRONTEND=noninteractive $install -o Dpkg::Options::=--force-confold" ;; esac
    echo "  - [sh, -c, '$install && systemctl set-default graphical.target && systemctl enable $dm && touch /var/tmp/provisioned']"
    echo "power_state: {mode: reboot, condition: true}"
} > "$seed/user-data"
printf 'instance-id: %s-1\nlocal-hostname: rms-%s\n' "$name" "$flavor" > "$seed/meta-data"
genisoimage -quiet -output "$dir/seed.iso" -volid cidata -joliet -rock "$seed/user-data" "$seed/meta-data"
VBoxManage createvm --name "$name" --ostype Linux_64 --register >/dev/null
VBoxManage modifyvm "$name" --memory 4096 --cpus 2 --graphicscontroller vmsvga --vram 128 \
    --audio-driver null --audio-controller hda --audio-enabled on \
    --nic1 nat --natpf1 "ssh,tcp,127.0.0.1,$port,,22" --boot1 disk --rtc-use-utc on \
    --clipboard-mode disabled --drag-and-drop disabled
VBoxManage storagectl "$name" --name SATA --add sata --portcount 2
VBoxManage storageattach "$name" --storagectl SATA --port 0 --type hdd --medium "$dir/disk.vdi"
VBoxManage storageattach "$name" --storagectl SATA --port 1 --type dvddrive --medium "$dir/seed.iso"
VBoxManage startvm "$name" --type headless
echo "provisioning; ready when: ssh -i $keys/id_vm -p $port tester@127.0.0.1 test -f /var/tmp/provisioned"
