"""Drive and inspect the rust-microsip window over AT-SPI (AccessKit).

Usage: atspi.py press NAME... | sweep LABEL [MARKER] | screens | focused
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


ERRORS = []


def nodes(node, out):
    try:
        out.append(node)
        for index in range(min(node.get_child_count(), 300)):
            child = node.get_child_at_index(index)
            if child is not None:
                nodes(child, out)
    except Exception as error:  # nodes can vanish between frames; counted
        ERRORS.append(type(error).__name__)
    return out


def named(node):
    return bool((node.get_name() or "").strip())


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
            if action is not None and action.get_n_actions() > 0 and action.do_action(0):
                time.sleep(1.5)
                return True
    print(f"NOT_FOUND {name!r}")
    return False


def sweep(label, marker=None, minimum=5):
    """Print focusable/editable nodes; return the number of problems: nodes
    without a name, a missing screen marker, too few nodes, or read errors."""
    found, unnamed, names = [], 0, set()
    del ERRORS[:]
    for node in nodes(wait_for_window(), []):
        names.add(node.get_name())
        states = node.get_state_set()
        if states.contains(Atspi.StateType.FOCUSABLE) or states.contains(
            Atspi.StateType.EDITABLE
        ):
            found.append(f"{node.get_role_name()}: {node.get_name()!r}")
            unnamed += not named(node)
    missing = marker is not None and marker not in names
    problems = unnamed + len(ERRORS) + (len(found) < minimum) + missing
    print(f"== {label}: {len(found)} focusable, {unnamed} unnamed,"
          f" marker {marker!r} {'missing' if missing else 'present'}, {len(ERRORS)} read errors")
    for line in found:
        print("   ", line)
    return problems


def focused():
    for node in nodes(wait_for_window(), []):
        try:
            if node.get_state_set().contains(Atspi.StateType.FOCUSED):
                return f"{node.get_role_name()}: {node.get_name()!r}"
        except Exception:
            pass
    return "none"


# Each screen is identified by a control only it shows.
SCREENS = [
    ([], "Phone", "Number to call"),
    (["Calls"], "Calls", "Missed"),
    (["Contacts"], "Contacts", "Import"),
    (["Add"], "Contact editor", "Add number"),
    (["Cancel", "Import"], "Import", "Preview"),
    (["Cancel", "Phone", "Menu", "Settings…"], "Settings", "Save and register"),
    (["Cancel", "Menu", "About and licenses…"], "About", "Close"),
]


def screens():
    bad = 0
    for steps, label, marker in SCREENS:
        if not all(press(step) for step in steps):
            bad += 1
        bad += sweep(label, marker)
    return bad


def main(argv):
    command, args = argv[0], argv[1:]
    if command == "press":
        return 0 if all(press(name) for name in args) else 1
    if command == "sweep":
        label = args[0] if args else "window"
        return 1 if sweep(label, args[1] if len(args) > 1 else None) else 0
    if command == "screens":
        bad = screens()
        print("SCREENS", len(SCREENS), "PROBLEMS", bad)
        return 1 if bad else 0
    if command == "focused":
        print(focused())
        return 0
    print(__doc__)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
