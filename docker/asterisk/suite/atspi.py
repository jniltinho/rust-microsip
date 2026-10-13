"""Drive and inspect the rust-microsip window over AT-SPI (AccessKit).

Usage: atspi.py press NAME... | sweep LABEL | screens | focused
Runs inside the suite container's private D-Bus session; fictional data only.
"""

import sys
import time

import gi

gi.require_version("Atspi", "2.0")
from gi.repository import Atspi  # noqa: E402


def app():
    desktop = Atspi.get_desktop(0)
    for index in range(desktop.get_child_count()):
        child = desktop.get_child_at_index(index)
        if child is not None and "microsip" in (child.get_name() or "").lower():
            return child
    return None


def nodes(node, out):
    try:
        out.append(node)
        for index in range(min(node.get_child_count(), 300)):
            child = node.get_child_at_index(index)
            if child is not None:
                nodes(child, out)
    except Exception:  # nodes can vanish between frames
        pass
    return out


def wait_for_window():
    for _ in range(60):
        window = app()
        if window is not None and len(nodes(window, [])) > 5:
            return window
        time.sleep(0.5)
    print("NO_WINDOW")
    sys.exit(2)


def press(name):
    for node in nodes(wait_for_window(), []):
        if node.get_name() == name:
            action = node.get_action_iface()
            if action is not None and action.get_n_actions() > 0:
                action.do_action(0)
                time.sleep(1.5)
                return True
    print(f"NOT_FOUND {name!r}")
    return False


def sweep(label):
    """Print focusable/editable nodes; return how many have no name."""
    found = []
    for node in nodes(wait_for_window(), []):
        states = node.get_state_set()
        if states.contains(Atspi.StateType.FOCUSABLE) or states.contains(
            Atspi.StateType.EDITABLE
        ):
            found.append(f"{node.get_role_name()}: {node.get_name()!r}")
    unnamed = sum(1 for line in found if line.endswith("''"))
    print(f"== {label}: {len(found)} focusable, {unnamed} unnamed")
    for line in found:
        print("   ", line)
    return unnamed


def focused():
    for node in nodes(wait_for_window(), []):
        try:
            if node.get_state_set().contains(Atspi.StateType.FOCUSED):
                return f"{node.get_role_name()}: {node.get_name()!r}"
        except Exception:
            pass
    return "none"


def screens():
    bad = sweep("Phone")
    for steps, label in [
        (["Calls"], "Calls"),
        (["Contacts"], "Contacts"),
        (["Add"], "Contact editor"),
        (["Cancel", "Import"], "Import"),
        (["Cancel", "Phone", "Menu", "Settings…"], "Settings"),
        (["Cancel", "Menu", "About and licenses…"], "About"),
    ]:
        if not all(press(step) for step in steps):
            bad += 1
        bad += sweep(label)
    return bad


def main(argv):
    command, args = argv[0], argv[1:]
    if command == "press":
        return 0 if all(press(name) for name in args) else 1
    if command == "sweep":
        return 1 if sweep(args[0] if args else "window") else 0
    if command == "screens":
        bad = screens()
        print("UNNAMED_TOTAL", bad)
        return 1 if bad else 0
    if command == "focused":
        print(focused())
        return 0
    print(__doc__)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
