#!/bin/sh
# Create a clean VirtualBox desktop VM for manual and scripted checks:
#   sh scripts/vm/create-desktop-vm.sh kde|gnome <ssh-port> <key-dir>
# kde: Ubuntu 24.04 + Plasma (X11, SDDM autologin); gnome: Ubuntu 24.04 +
# GNOME (Wayland, GDM autologin). The official cloud image is checked against
# Ubuntu's SHA256SUMS; cloud-init creates the fictional user "tester" with a
# locked password and an SSH key generated in <key-dir>. SSH is forwarded on
# 127.0.0.1:<ssh-port> only; clipboard and drag-and-drop are off; audio uses
# the null driver. No personal data or account is copied into the VM.
set -eu
flavor=${1:?kde|gnome}; port=${2:?ssh port}; keys=${3:?key directory}
case "$flavor" in
    kde) packages="kde-plasma-desktop, sddm"; dm=sddm
         autologin_path=/etc/sddm.conf.d/autologin.conf
         autologin="[Autologin]|User=tester|Session=plasma" ;;
    gnome) packages="ubuntu-desktop-minimal, gdm3"; dm=gdm3
         autologin_path=/etc/gdm3/custom.conf
         autologin="[daemon]|AutomaticLoginEnable=true|AutomaticLogin=tester|WaylandEnable=true" ;;
    *) echo "usage: $0 kde|gnome <ssh-port> <key-dir>" >&2; exit 2 ;;
esac
name="rust-microsip-$flavor"
base=$(VBoxManage list systemproperties | sed -n 's/^Default machine folder: *//p')
dir="$base/$name"
mkdir -p "$dir" "$keys"
[ -f "$keys/id_vm" ] || ssh-keygen -q -t ed25519 -N '' -C rust-microsip-vm-test -f "$keys/id_vm"
image=https://cloud-images.ubuntu.com/noble/current
curl -fsSL -o "$dir/noble.img" "$image/noble-server-cloudimg-amd64.img"
curl -fsSL "$image/SHA256SUMS" | awk '$2=="*noble-server-cloudimg-amd64.img"{print $1"  noble.img"}' \
    | (cd "$dir" && sha256sum --check --strict -)
qemu-img convert -O vdi "$dir/noble.img" "$dir/disk.vdi"
rm "$dir/noble.img"
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
    echo "    groups: [sudo, audio, video]"
    echo "    sudo: ALL=(ALL) NOPASSWD:ALL"
    echo "    lock_passwd: true"
    echo "    ssh_authorized_keys: [\"$(cat "$keys/id_vm.pub")\"]"
    echo "ssh_pwauth: false"
    echo "package_update: true"
    echo "packages: [$packages, x11-utils, xdotool, libasound2t64, dbus-x11, imagemagick, alsa-utils]"
    echo "write_files:"
    echo "  - path: $autologin_path"
    echo "    content: |"
    printf '%s\n' "$autologin" | tr '|' '\n' | sed 's/^/      /'
    echo "runcmd:"
    echo "  - systemctl set-default graphical.target"
    echo "  - systemctl enable $dm"
    echo "  - touch /var/tmp/provisioned"
    echo "power_state: {mode: reboot, condition: true}"
} > "$seed/user-data"
printf 'instance-id: %s-1\nlocal-hostname: rms-%s\n' "$name" "$flavor" > "$seed/meta-data"
genisoimage -quiet -output "$dir/seed.iso" -volid cidata -joliet -rock "$seed/user-data" "$seed/meta-data"
VBoxManage createvm --name "$name" --ostype Ubuntu_64 --register >/dev/null
VBoxManage modifyvm "$name" --memory 4096 --cpus 2 --graphicscontroller vmsvga --vram 128 \
    --audio-driver null --audio-controller hda --audio-enabled on \
    --nic1 nat --natpf1 "ssh,tcp,127.0.0.1,$port,,22" --boot1 disk --rtc-use-utc on \
    --clipboard-mode disabled --drag-and-drop disabled
VBoxManage storagectl "$name" --name SATA --add sata --portcount 2
VBoxManage storageattach "$name" --storagectl SATA --port 0 --type hdd --medium "$dir/disk.vdi"
VBoxManage storageattach "$name" --storagectl SATA --port 1 --type dvddrive --medium "$dir/seed.iso"
VBoxManage startvm "$name" --type headless
echo "provisioning; ready when: ssh -i $keys/id_vm -p $port tester@127.0.0.1 test -f /var/tmp/provisioned"
