#!/bin/sh
# Quit the app started by vm-run-app.sh: tray menu "Quit" when a tray item
# exists, otherwise SIGTERM; reports whether it exited and anything is left.
read -r pid scratch < "${XDG_RUNTIME_DIR:?}/rms-last"
bus=unix:path=$XDG_RUNTIME_DIR/bus
export DBUS_SESSION_BUS_ADDRESS="${DBUS_SESSION_BUS_ADDRESS:-$bus}"
if busctl --user call org.kde.StatusNotifierItem-$pid-1 /MenuBar com.canonical.dbusmenu Event isvu 3 clicked i 0 0 2>/dev/null; then how=tray; else kill -TERM $pid; how=sigterm; fi
sleep 3; kill -0 $pid 2>/dev/null && echo "quit_via=$how still_running" || echo "quit_via=$how exited"
pgrep -u "$(id -un)" "^rust-microsip" >/dev/null && echo leftover=yes || echo leftover=no
case "$scratch" in *..*) ;; /tmp/tmp.*) rm -rf "$scratch" ;; esac
rm -f "$XDG_RUNTIME_DIR/rms-last"
