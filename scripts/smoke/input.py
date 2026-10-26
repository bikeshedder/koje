#!/usr/bin/env python3
"""A virtual pointer and keyboard that stay for the whole smoke test
(scripts/smoke/run.sh): `wlrctl` makes a new device per call, which is gone
before GTK binds it, so its clicks and keys are lost. Reads lines from a
FIFO:

  move X Y      pointer to X,Y (logical pixels on the WIDTH × HEIGHT output)
  click         left button
  rclick        right button
  key NAME      Escape, Return, BackSpace, Tab, Up, Down, Left, Right
  type TEXT     letters, digits, space and a little punctuation (US layout)
  down MOD      hold Alt or Super (until `up MOD`)
  up MOD

Usage: input.py <fifo> <width> <height>"""
import os, string, subprocess, sys, tempfile, time

fifo, width, height = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])
here = os.path.dirname(os.path.abspath(__file__))
gen = tempfile.mkdtemp()
# The generated modules import each other relatively: one package for all.
package = os.path.join(gen, "smokeproto")
subprocess.run([sys.executable, "-m", "pywayland.scanner", "-o", package, "-i",
                "/usr/share/wayland/wayland.xml",
                os.path.join(here, "wlr-virtual-pointer-unstable-v1.xml"),
                os.path.join(here, "virtual-keyboard-unstable-v1.xml")], check=True)
open(os.path.join(package, "__init__.py"), "w").close()
sys.path.insert(0, gen)
from pywayland.client import Display  # noqa: E402
from smokeproto.wayland import WlSeat  # noqa: E402
from smokeproto.wlr_virtual_pointer_unstable_v1 import ZwlrVirtualPointerManagerV1  # noqa: E402
from smokeproto.virtual_keyboard_unstable_v1 import ZwpVirtualKeyboardManagerV1  # noqa: E402

BTN_LEFT, BTN_RIGHT = 0x110, 0x111
SHIFT = 1  # the Shift modifier's bit in the keymap
# Held modifiers: evdev code and bit (Mod1, Mod4).
MODS = {"Alt": (56, 8), "Super": (125, 64)}
held = 0
# evdev key codes (US layout).
KEYS = {"Escape": 1, "BackSpace": 14, "Tab": 15, "Return": 28, "space": 57, "Shift": 42,
        "Up": 103, "Down": 108, "Left": 105, "Right": 106}
for row, first in (("1234567890-=", 2), ("qwertyuiop", 16), ("asdfghjkl;'", 30), ("zxcvbnm,./", 44)):
    for i, c in enumerate(row):
        KEYS[c] = first + i
KEYS[" "] = KEYS["space"]

display = Display()
display.connect()
found = {}


def global_added(registry, name, interface, version):
    if interface == "wl_seat":
        found["seat"] = registry.bind(name, WlSeat, 1)
    elif interface == "zwlr_virtual_pointer_manager_v1":
        found["pointers"] = registry.bind(name, ZwlrVirtualPointerManagerV1, 1)
    elif interface == "zwp_virtual_keyboard_manager_v1":
        found["keyboards"] = registry.bind(name, ZwpVirtualKeyboardManagerV1, 1)


registry = display.get_registry()
registry.dispatcher["global"] = global_added
display.roundtrip()
pointer = found["pointers"].create_virtual_pointer(found["seat"])
keyboard = found["keyboards"].create_virtual_keyboard(found["seat"])
keymap = subprocess.run(["xkbcli", "compile-keymap", "--layout", "us"], check=True,
                        capture_output=True).stdout + b"\0"
with tempfile.TemporaryFile() as f:
    f.write(keymap)
    f.flush()
    keyboard.keymap(1, f.fileno(), len(keymap))  # 1: XKB_V1
    display.roundtrip()


def now():
    return int(time.monotonic() * 1000) & 0xFFFFFFFF


def press(code, shift=False):
    if shift:
        keyboard.key(now(), KEYS["Shift"], 1)
        keyboard.modifiers(SHIFT | held, 0, 0, 0)
    keyboard.key(now(), code, 1)
    display.flush()
    time.sleep(0.02)
    keyboard.key(now(), code, 0)
    if shift:
        keyboard.key(now(), KEYS["Shift"], 0)
        keyboard.modifiers(held, 0, 0, 0)
    display.flush()
    time.sleep(0.02)


os.mkfifo(fifo)
while True:
    with open(fifo) as f:
        for line in f:
            cmd, _, rest = line.rstrip("\n").partition(" ")
            if cmd == "move":
                x, y = rest.split()
                pointer.motion_absolute(now(), int(x), int(y), width, height)
                pointer.frame()
            elif cmd in ("click", "rclick"):
                button = BTN_LEFT if cmd == "click" else BTN_RIGHT
                pointer.button(now(), button, 1)
                pointer.frame()
                display.flush()
                time.sleep(0.05)
                pointer.button(now(), button, 0)
                pointer.frame()
            elif cmd == "key":
                press(KEYS[rest])
            elif cmd in ("down", "up"):
                code, bit = MODS[rest]
                held = held | bit if cmd == "down" else held & ~bit
                keyboard.key(now(), code, 1 if cmd == "down" else 0)
                keyboard.modifiers(held, 0, 0, 0)
            elif cmd == "type":
                for c in rest:
                    upper = c in string.ascii_uppercase
                    press(KEYS[c.lower()], shift=upper)
            display.flush()
