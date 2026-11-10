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
    Favorites: {in: [string]; out: [boolean, string[]]};
    SetFavorites: {in: [string, boolean, string[]]; out: []};
    Images: {in: []; out: [[string, string, string, string, string, string[], boolean, string, boolean][]]};
    CustomizeImage: {in: [string]; out: []};
    ResetImage: {in: [string]; out: []};
    KeepImageChanges: {in: [string]; out: []};
    NewImage: {in: [string, string, string]; out: [string]};
    SetImage: {in: [string, Record<string, GLib.Variant>]; out: []};
    RemoveImage: {in: [string]; out: []};
    BuildImage: {in: [string]; out: []};
    NewContext: {in: [string]; out: [string]};
    Config: {in: [string]; out: [Record<string, unknown>]};
    SetConfig: {in: [string, Record<string, GLib.Variant>]; out: [string[]]};
    Up: {in: [string, string, string, boolean, boolean]; out: []};
    Down: {in: [string, string, boolean, boolean]; out: []};
    Sleep: {in: [string]; out: []};
    Wake: {in: [string]; out: []};
    Recreate: {in: [string]; out: []};
    Remove: {in: [string]; out: []};
    States: {in: [string]; out: [[string, string, boolean, boolean, number, number, number, string][]]};
    Save: {in: [string, string]; out: [string, number]};
    CopyState: {in: [string, string, string]; out: []};
    RenameState: {in: [string, string, string]; out: []};
    RemoveState: {in: [string, string]; out: []};
    Windows: {in: [string]; out: [[string, string, string, string][]]};
    Apps: {in: [string]; out: [[string, string, string, string[], string, string][]]};
    LaunchApp: {in: [string, string]; out: []};
    ContextOfPid: {in: [number]; out: [string]};
    Memory: {in: [string]; out: [string, number, number, number, number]};
    MemoryHistory: {in: [string]; out: [string, number, number[]]};
    Reload: {in: []; out: []};
}

/** Each method's D-Bus signatures: [in, out]. */
export const METHODS: {[M in keyof Methods]: [string, string]} = {
    Contexts: ['()', '(a(sssssu))'],
    ActiveContext: ['()', '(s)'],
    Focus: ['(s)', '()'],
    Favorites: ['(s)', '(bas)'],
    SetFavorites: ['(sbas)', '()'],
    Images: ['()', '(a(sssssasbsb))'],
    CustomizeImage: ['(s)', '()'],
    ResetImage: ['(s)', '()'],
    KeepImageChanges: ['(s)', '()'],
    NewImage: ['(sss)', '(s)'],
    SetImage: ['(sa{sv})', '()'],
    RemoveImage: ['(s)', '()'],
    BuildImage: ['(s)', '()'],
    NewContext: ['(s)', '(s)'],
    Config: ['(s)', '(a{sv})'],
    SetConfig: ['(sa{sv})', '(as)'],
    Up: ['(sssbb)', '()'],
    Down: ['(ssbb)', '()'],
    Sleep: ['(s)', '()'],
    Wake: ['(s)', '()'],
    Recreate: ['(s)', '()'],
    Remove: ['(s)', '()'],
    States: ['(s)', '(a(ssbbuuxs))'],
    Save: ['(ss)', '(su)'],
    CopyState: ['(sss)', '()'],
    RenameState: ['(sss)', '()'],
    RemoveState: ['(ss)', '()'],
    Windows: ['(s)', '(a(ssss))'],
    Apps: ['(s)', '(a(sssasss))'],
    LaunchApp: ['(ss)', '()'],
    ContextOfPid: ['(u)', '(s)'],
    Memory: ['(s)', '(stttt)'],
    MemoryHistory: ['(s)', '(suat)'],
    Reload: ['()', '()'],
};

/** Each signal's arguments. */
export interface Signals {
    ContextsChanged: [];
    FavoritesChanged: [string];
    ImagesChanged: [];
    ImageBuildStarted: [string];
    ImageBuildProgress: [string, string];
    ImageBuildFinished: [string];
    ImageBuildFailed: [string, string];
    OperationStarted: [string, string];
    OperationFinished: [string, string];
    OperationFailed: [string, string, string, string];
    OperationProgress: [string, string, string];
    StatesChanged: [string];
    WindowAdded: [string, string];
    WindowRemoved: [string, string];
    MemoryChanged: [];
}
