#!/bin/sh
# Runs inside a test VM (see create-desktop-vm.sh) over SSH:
#   sh vm-run-app.sh <binary> <label>
# Starts the app in the logged-in desktop session with scratch XDG directories
# and reports the session type, tray watcher/item and (X11) window icon. The
# app keeps running until vm-quit-app.sh.
bin=$1; label=$2
# Environment of a client in the graphical session (the compositor itself lacks WAYLAND_DISPLAY).
shell=$(for p in $(pgrep -u tester); do tr '\0' '\n' 2>/dev/null < /proc/$p/environ | grep -qE '^(WAYLAND_)?DISPLAY=' && { echo $p; break; }; done)
eval "$(tr '\0' '\n' < /proc/$shell/environ | grep -E '^(DISPLAY|WAYLAND_DISPLAY|XAUTHORITY|XDG_RUNTIME_DIR|DBUS_SESSION_BUS_ADDRESS|XDG_CURRENT_DESKTOP|XDG_SESSION_TYPE)=' | sed 's/^/export /')"
scratch=$(mktemp -d /tmp/rms-XXXX); mkdir -p $scratch/c $scratch/d
XDG_CONFIG_HOME=$scratch/c XDG_DATA_HOME=$scratch/d setsid "$bin" > $scratch/app.log 2>&1 < /dev/null &
pid=$!
sleep 8
kill -0 $pid 2>/dev/null || pid=$(pgrep -f "^$bin" -n)
echo "label=$label desktop=$XDG_CURRENT_DESKTOP session=$XDG_SESSION_TYPE pid=${pid:-none}"
echo "watcher=$(busctl --user list 2>/dev/null | grep -c '^org.kde.StatusNotifierWatcher ')"
echo "item=$(busctl --user list 2>/dev/null | grep -c "^org.kde.StatusNotifierItem-$pid-1 ")"
[ -n "$DISPLAY" ] && [ "$XDG_SESSION_TYPE" = x11 ] && for w in $(xprop -root _NET_CLIENT_LIST | tr -d ',' | grep -o '0x[0-9a-f]*'); do [ "$(xprop -id $w _NET_WM_PID | grep -o '[0-9]*$')" = "$pid" ] && echo "icon=$(xprop -id $w -len 8 _NET_WM_ICON | grep -o 'Icon ([0-9]* x [0-9]*)')"; done
echo "log:"; cat $scratch/app.log
echo "$pid $scratch" > /tmp/rms-last
