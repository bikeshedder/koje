#!/usr/bin/env python3
"""A fake Hyprland for koje-shell smoke tests: answers `j/…` requests from a
fixed desktop (two contexts, a stash, host workspaces) and logs every other
request – dispatches – to LOG. ACTIVE (env) names the active workspace.

Usage: fake_hypr.py <instance dir> <log>"""
import json, os, socket, sys, threading, time

inst, log = sys.argv[1], sys.argv[2]
os.makedirs(inst, exist_ok=True)
active = os.environ.get("ACTIVE", "kundenportal")
ids = {"1": 1, "2": 2, "kundenportal": -1337, "privat": -1338, "special:stash-kundenportal": -98}


def win(address, workspace, cls, title, at, size, focus):
    return {"address": address, "mapped": True, "hidden": False, "at": at, "size": size,
            "workspace": {"id": ids[workspace], "name": workspace}, "floating": False,
            "class": cls, "title": title, "focusHistoryID": focus, "pid": 100}


desktop = {
    "j/monitors": [{"id": 0, "name": "HEADLESS-1", "activeWorkspace": {"id": ids[active], "name": active},
                    "focused": True}],
    "j/workspaces": [{"id": i, "name": n, "monitor": "HEADLESS-1", "windows": 1} for n, i in ids.items()],
    "j/clients": [
        win("0x1", "kundenportal", "org.gnome.Nautilus", "Dokumente", [0, 32], [800, 1048], 1),
        win("0x2", "kundenportal", "firefox", "Kundenportal – Staging – Mozilla Firefox", [800, 32], [1120, 520], 0),
        win("0x3", "kundenportal", "foot", "~/src/kundenportal", [800, 552], [1120, 528], 2),
        win("0x4", "special:stash-kundenportal", "org.gnome.TextEditor", "notes.md", [0, 32], [900, 700], 3),
        win("0x5", "special:stash-kundenportal", "foot", "htop", [0, 32], [900, 700], 4),
        win("0x6", "1", "foot", "host terminal", [0, 32], [1920, 1048], 5),
        win("0x7", "privat", "firefox", "Mail", [0, 32], [1920, 1048], 6),
    ],
    "j/activewindow": {"address": "0x2"},
}


def log_line(text):
    with open(log, "a") as f:
        f.write(text + "\n")


def serve(name, handle):
    path = os.path.join(inst, name)
    if os.path.exists(path):
        os.unlink(path)
    s = socket.socket(socket.AF_UNIX)
    s.bind(path)
    s.listen()
    while True:
        c, _ = s.accept()
        threading.Thread(target=handle, args=(c,), daemon=True).start()


def command(c):
    req = c.recv(65536).decode()
    if req in desktop:
        c.sendall(json.dumps(desktop[req]).encode())
    else:
        log_line(req)
        n = req.count(";") + 1 if req.startswith("[[BATCH]]") else 1
        c.sendall(("ok\n" * n).encode())
    c.close()


def events(c):
    # Silent: the shell reads the desktop when it connects.
    try:
        while True:
            time.sleep(60)
    except OSError:
        pass


threading.Thread(target=serve, args=(".socket2.sock", events), daemon=True).start()
serve(".socket.sock", command)
