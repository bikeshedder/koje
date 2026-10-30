// Generated from crates/kojed/dbus/io.koje.Daemon1.xml by
// tools/daemon-types.mjs – do not edit; `just ui` regenerates it.

import type GLib from 'gi://GLib';

export const BUS_NAME = 'io.koje.Daemon';
export const OBJECT_PATH = '/io/koje/Daemon';
export const INTERFACE = 'io.koje.Daemon1';

/** Each method's in and out arguments. */
export interface Methods {
    Contexts: {in: []; out: [[string, string, string, string, string, number][]]};
    ActiveContext: {in: []; out: [string]};
    Focus: {in: [string]; out: []};
    NewContext: {in: [string]; out: [string]};
    Config: {in: [string]; out: [Record<string, unknown>]};
    SetConfig: {in: [string, Record<string, GLib.Variant>]; out: [string[]]};
    Up: {in: [string, string, string]; out: []};
    Down: {in: [string, string, boolean]; out: []};
    Sleep: {in: [string]; out: []};
    Wake: {in: [string]; out: []};
    Recreate: {in: [string]; out: []};
    States: {in: [string]; out: [[string, string, boolean, boolean, number, number, number, string][]]};
    Save: {in: [string, string]; out: []};
    CopyState: {in: [string, string, string]; out: []};
    RenameState: {in: [string, string, string]; out: []};
    RemoveState: {in: [string, string]; out: []};
    Windows: {in: [string]; out: [[string, string, string, string][]]};
    Apps: {in: [string]; out: [[string, string, string, string[]][]]};
    LaunchApp: {in: [string, string]; out: []};
    ContextOfPid: {in: [number]; out: [string]};
    Memory: {in: [string]; out: [string, number, number, number, number]};
    MemoryHistory: {in: [string]; out: [string, number, number[]]};
    WaylandSocket: {in: [string]; out: [string]};
    SoundSockets: {in: [string]; out: [string]};
    ReleaseWaylandSocket: {in: [string]; out: []};
    State: {in: [string]; out: [string]};
    Restore: {in: [string, string, string]; out: [number]};
    CloseWindows: {in: [string, number]; out: [string[]]};
    Started: {in: [string]; out: []};
    Stopped: {in: [string]; out: []};
    Hide: {in: [string]; out: []};
    Show: {in: [string]; out: []};
    CurrentState: {in: [string]; out: [string]};
    WindowsLost: {in: [string]; out: [boolean]};
    SwitchState: {in: [string, string, string]; out: [number, number, number]};
    Reload: {in: []; out: []};
}

/** Each method's D-Bus signatures: [in, out]. */
export const METHODS: {[M in keyof Methods]: [string, string]} = {
    Contexts: ['()', '(a(sssssu))'],
    ActiveContext: ['()', '(s)'],
    Focus: ['(s)', '()'],
    NewContext: ['(s)', '(s)'],
    Config: ['(s)', '(a{sv})'],
    SetConfig: ['(sa{sv})', '(as)'],
    Up: ['(sss)', '()'],
    Down: ['(ssb)', '()'],
    Sleep: ['(s)', '()'],
    Wake: ['(s)', '()'],
    Recreate: ['(s)', '()'],
    States: ['(s)', '(a(ssbbuuxs))'],
    Save: ['(ss)', '()'],
    CopyState: ['(sss)', '()'],
    RenameState: ['(sss)', '()'],
    RemoveState: ['(ss)', '()'],
    Windows: ['(s)', '(a(ssss))'],
    Apps: ['(s)', '(a(sssas))'],
    LaunchApp: ['(ss)', '()'],
    ContextOfPid: ['(u)', '(s)'],
    Memory: ['(s)', '(stttt)'],
    MemoryHistory: ['(s)', '(suat)'],
    WaylandSocket: ['(s)', '(s)'],
    SoundSockets: ['(s)', '(s)'],
    ReleaseWaylandSocket: ['(s)', '()'],
    State: ['(s)', '(s)'],
    Restore: ['(sss)', '(u)'],
    CloseWindows: ['(su)', '(as)'],
    Started: ['(s)', '()'],
    Stopped: ['(s)', '()'],
    Hide: ['(s)', '()'],
    Show: ['(s)', '()'],
    CurrentState: ['(s)', '(s)'],
    WindowsLost: ['(s)', '(b)'],
    SwitchState: ['(sss)', '(uuu)'],
    Reload: ['()', '()'],
};

/** Each signal's arguments. */
export interface Signals {
    ContextsChanged: [];
    OperationStarted: [string, string];
    OperationFinished: [string, string];
    OperationFailed: [string, string, string];
    StatesChanged: [string];
    WindowAdded: [string, string];
    WindowRemoved: [string, string];
    MemoryChanged: [];
}
