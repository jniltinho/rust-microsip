#!/bin/sh
# Quit the app started by vm-run-app.sh: tray menu "Quit" when a tray item
# exists, otherwise SIGTERM; reports whether it exited and anything is left.
read pid scratch < /tmp/rms-last
# Environment of a client in the graphical session (the compositor itself lacks WAYLAND_DISPLAY).
shell=$(for p in $(pgrep -u tester); do tr '\0' '\n' < /proc/$p/environ 2>/dev/null | grep -qE '^(WAYLAND_)?DISPLAY=' && { echo $p; break; }; done)
eval "$(tr '\0' '\n' < /proc/$shell/environ | grep -E '^DBUS_SESSION_BUS_ADDRESS=' | sed 's/^/export /')"
if busctl --user call org.kde.StatusNotifierItem-$pid-1 /MenuBar com.canonical.dbusmenu Event isvu 3 clicked i 0 0 2>/dev/null; then how=tray; else kill -TERM $pid; how=sigterm; fi
sleep 3; kill -0 $pid 2>/dev/null && echo "quit_via=$how still_running" || echo "quit_via=$how exited"
pgrep -u tester "^rust-microsip" >/dev/null && echo leftover=yes || echo leftover=no
rm -rf "$scratch"
