// What koje's settings app shows and what its edits become (ADR 0060),
// without GTK: pure functions, tested with vitest under Node. The widgets
// only render them.

/** A context as `Contexts` reports it. */
export interface Context {
    name: string;
    label: string;
    icon: string;
    color: string;
    status: 'running' | 'paused' | 'stopped' | 'invalid';
    windows: number;
}

export function contexts(rows: [string, string, string, string, string, number][]): Context[] {
    return rows.map(([name, label, icon, color, status, windows]) => ({
        name, label, icon, color, windows,
        status: (['running', 'paused', 'stopped'].includes(status) ? status : 'invalid') as Context['status'],
    }));
}

export function statusLabel(c: Context, busy: string | undefined): string {
    if (busy)
        return BUSY[busy] ?? 'Working…';
    switch (c.status) {
    case 'running': return c.windows === 1 ? 'Running · 1 window' : `Running · ${c.windows} windows`;
    case 'paused': return 'Asleep';
    case 'stopped': return 'Stopped';
    case 'invalid': return 'context.toml cannot be read';
    }
}

const BUSY: Record<string, string> = {
    up: 'Starting…',
    down: 'Stopping…',
    sleep: 'Going to sleep…',
    recreate: 'Recreating…',
    remove: 'Removing…',
};

/** The header's actions for a context's status. */
export type Action = 'up' | 'down' | 'sleep' | 'wake';

export function actions(c: Context): Action[] {
    switch (c.status) {
    case 'running': return ['sleep', 'down'];
    case 'paused': return ['wake', 'down'];
    case 'stopped': return ['up'];
    case 'invalid': return [];
    }
}

// ----- Sizes (as koje_core::size::ByteSize) -----

const UNITS: [string, number][] = [['T', 2 ** 40], ['G', 2 ** 30], ['M', 2 ** 20], ['K', 2 ** 10]];

/** `6G`, `512m`, `1024` → bytes; `''` → 0 (none); `null` when invalid. */
export function parseSize(text: string): number | null {
    const t = text.trim();
    if (t === '')
        return 0;
    const m = t.match(/^(\d+)([a-zA-Z]?)$/);
    if (!m)
        return null;
    const factor = m[2] === '' ? 1 : UNITS.find(([u]) => u === m[2].toUpperCase())?.[1];
    if (factor === undefined)
        return null;
    const n = Number(m[1]) * factor;
    return Number.isSafeInteger(n) ? n : null;
}

/** Bytes in the largest unit that divides them (`6G`); 0 → `''`. */
export function formatSize(bytes: number): string {
    if (bytes === 0)
        return '';
    for (const [unit, factor] of UNITS) {
        if (bytes >= factor && bytes % factor === 0)
            return `${bytes / factor}${unit}`;
    }
    return String(bytes);
}

/** Bytes for people: `1.3 GiB`, `20 GiB`, `900 B`. */
export function humanSize(bytes: number): string {
    for (const [unit, factor] of UNITS) {
        if (bytes >= factor) {
            const tenths = Math.floor(bytes * 10 / factor);
            return tenths < 100
                ? `${Math.floor(tenths / 10)}.${tenths % 10} ${unit}iB`
                : `${Math.floor((bytes + factor / 2) / factor)} ${unit}iB`;
        }
    }
    return `${bytes} B`;
}

/** The memory line of a running context: used against its limits. */
export function memoryLabel(used: number, soft: number, hard: number): string {
    const limits = [soft && `soft ${humanSize(soft)}`, hard && `hard ${humanSize(hard)}`].filter(Boolean);
    return limits.length ? `${humanSize(used)} of ${limits.join(', ')}` : `${humanSize(used)} · no limit`;
}

/** How full the meter is: against the hard limit, else the soft one. */
export function memoryFraction(used: number, soft: number, hard: number): number {
    const limit = hard || soft;
    return limit ? Math.min(used / limit, 1) : 0;
}

// ----- The Context page's form -----

/** The access switches in the order the page shows them, with GNOME-like titles. */
export const SWITCHES: [string, string][] = [
    ['wayland', 'Windows on the desktop'],
    ['gpu', 'Graphics card'],
    ['sound', 'Sound'],
    ['network', 'Network'],
    ['nested', 'Containers inside'],
    ['notifications', 'Notifications'],
    ['file_chooser', 'File chooser'],
    ['settings', 'Desktop settings'],
    ['inhibit', 'Keep the screen on'],
    ['screencast', 'Screen sharing'],
    ['print', 'Printing'],
];

/** The four profiles, as ADR 0017 names them. */
const PROFILE_NAMES: [string, string][] = [
    ['full', 'Full'],
    ['desktop', 'Desktop'],
    ['headless', 'Headless'],
    ['isolated', 'Isolated'],
];

/**
 * The profile choices: koje's default for all contexts (`''`, set in
 * `config.toml`; `defaultProfile` is what it currently is), then the four.
 */
export function profiles(defaultProfile: string): [string, string][] {
    const name = PROFILE_NAMES.find(([p]) => p === defaultProfile)?.[1] ?? defaultProfile;
    return [['', `Default (${name})`], ...PROFILE_NAMES];
}

/** What a profile gives, in one line: `Everything but Graphics card`, `Nothing`. */
export function profileSummary(bases: Config['bases'], profile: string): string {
    const base = bases[profile] ?? {};
    const on = SWITCHES.filter(([n]) => base[n]).map(([, t]) => t);
    const off = SWITCHES.filter(([n]) => !base[n]).map(([, t]) => t);
    let text: string;
    if (off.length === 0)
        text = 'Everything';
    else if (on.length === 0)
        text = 'Nothing: no windows, no network';
    else if (off.length <= on.length)
        text = `Everything but ${lower(off).join(', ')}`;
    else
        text = `Only ${lower(on).join(', ')}`;
    return profile === '' ? `koje's default for all contexts. ${text}` : text;
}

function lower(titles: string[]): string[] {
    return titles.map(t => t.charAt(0).toLowerCase() + t.slice(1));
}

/** The page's fields; sizes as text. */
export interface Form {
    prefix: string;
    label: string;
    icon: string;
    color: string;
    /** `image = …`: an image's name, or a registry image or path as written. */
    image: string;
    profile: string;
    /** The switches the context sets itself. */
    access: Record<string, boolean>;
    mounts: string[];
    memorySoft: string;
    memoryHard: string;
}

/** `Config`, unpacked. */
export interface Config {
    name: string;
    prefix: string;
    label: string;
    icon: string;
    color: string;
    image: string;
    profile: string;
    default_profile: string;
    access: Record<string, boolean>;
    effective: Record<string, boolean>;
    bases: Record<string, Record<string, boolean>>;
    mounts: string[];
    memory_soft: number;
    memory_hard: number;
    /** Whether it has a container: Recreate needs one. */
    container: boolean;
    /** Its image folder's Containerfile; `''` for koje's and registry images. */
    image_file: string;
    /** Why its container does not run its image as it is now; `''` when it does. */
    image_outdated: string;
}

export function form(c: Config): Form {
    return {
        prefix: c.prefix,
        label: c.label,
        icon: c.icon,
        color: c.color,
        image: c.image,
        profile: c.profile,
        access: {...c.access},
        mounts: [...c.mounts],
        memorySoft: formatSize(c.memory_soft),
        memoryHard: formatSize(c.memory_hard),
    };
}

/** What each switch is with `f`'s profile and overrides. */
export function effective(f: Form, bases: Config['bases']): Record<string, boolean> {
    return {...(bases[f.profile] ?? {}), ...f.access};
}

/** `f` with another profile: the profile as it is, the context's own switches dropped. */
export function withProfile(f: Form, profile: string): Form {
    return {...f, profile, access: {}};
}

/** The subtitle of the switches' row: how many differ from the profile. */
export function adjustments(f: Form): string {
    const n = Object.keys(f.access).length;
    if (n === 0)
        return 'As the profile gives';
    return n === 1 ? '1 changed from the profile' : `${n} changed from the profile`;
}

/**
 * `f` with switch `name` turned `on`: an override, dropped when the profile
 * gives the same.
 */
export function withSwitch(f: Form, bases: Config['bases'], name: string, on: boolean): Form {
    const access = {...f.access};
    if ((bases[f.profile] ?? {})[name] === on)
        delete access[name];
    else
        access[name] = on;
    return {...f, access};
}

/** Why `f` cannot be saved, by field; empty when it can. */
export function problems(f: Form): Partial<Record<keyof Form, string>> {
    const out: Partial<Record<keyof Form, string>> = {};
    if (f.prefix.trim() === '')
        out.prefix = 'The short label cannot be empty';
    if (!/^#[0-9a-fA-F]{6}$/.test(f.color))
        out.color = 'A color is #RRGGBB';
    if (f.icon !== '' && !/^[A-Za-z0-9_.-]+$/.test(f.icon))
        out.icon = 'An icon name from the icon theme, like folder-symbolic';
    const soft = parseSize(f.memorySoft);
    const hard = parseSize(f.memoryHard);
    if (soft === null)
        out.memorySoft = 'A size like 6G';
    if (hard === null)
        out.memoryHard = 'A size like 8G';
    if (soft && hard && soft > hard)
        out.memorySoft = 'The soft limit is above the hard one';
    return out;
}

/** `SetConfig`'s keys for what changed from `before` to `after` (plain values). */
export function changes(before: Form, after: Form): Record<string, string | number | string[] | Record<string, boolean>> {
    const out: Record<string, string | number | string[] | Record<string, boolean>> = {};
    for (const key of ['prefix', 'label', 'icon', 'color', 'image', 'profile'] as const) {
        if (before[key] !== after[key])
            out[key] = after[key];
    }
    if (!sameRecord(before.access, after.access))
        out.access = after.access;
    if (before.mounts.join('\n') !== after.mounts.join('\n'))
        out.mounts = after.mounts;
    if (before.memorySoft !== after.memorySoft)
        out.memory_soft = parseSize(after.memorySoft) ?? 0;
    if (before.memoryHard !== after.memoryHard)
        out.memory_hard = parseSize(after.memoryHard) ?? 0;
    return out;
}

function sameRecord(a: Record<string, boolean>, b: Record<string, boolean>): boolean {
    const keys = new Set([...Object.keys(a), ...Object.keys(b)]);
    return [...keys].every(k => a[k] === b[k]);
}

// ----- Images (ADR 0064) -----

/** Sidebar and output keys of images: context names have no `:`. */
export const IMAGE_KEY = 'image:';

/** What a new image builds on unless chosen otherwise. */
export const IMAGE_DEFAULT_FROM = 'fedora';

/** An image as `Images` reports it. */
export interface Image {
    name: string;
    label: string;
    description: string;
    /** `customized`: a folder in koje's own image's place (ADR 0065). */
    kind: 'builtin' | 'folder' | 'customized';
    /** Its Containerfile; `''` for koje's own. */
    file: string;
    usedBy: string[];
    built: boolean;
    /** Why it is not usable; `''` when it is. */
    error: string;
    /** A customized one: koje ships another version than it was made from. */
    update: boolean;
}

export function images(rows: [string, string, string, string, string, string[], boolean, string, boolean][]): Image[] {
    return rows.map(([name, label, description, kind, file, usedBy, built, error, update]) => ({
        name, label, description, file, usedBy, built, error, update,
        kind: kind === 'builtin' || kind === 'customized' ? kind : 'folder',
    }));
}

/** What can be done with koje's own image, or a customized one (ADR 0065). */
export type OriginAction = 'customize' | 'reset' | 'keep';

/** The "koje's version" row; `null` for the user's own images. */
export function imageOrigin(img: Image): {subtitle: string; actions: OriginAction[]} | null {
    switch (img.kind) {
    case 'folder': return null;
    case 'builtin': return {
        subtitle: 'koje\'s own image, updated with koje. Customize it to change it for every context that uses it.',
        actions: ['customize'],
    };
    case 'customized': return img.update
        ? {subtitle: 'koje ships a newer version than the one you customized. Keep yours, or go back to koje\'s (your changes are lost).', actions: ['keep', 'reset']}
        : {subtitle: 'Your version of koje\'s image: it takes its place for every context. Reset brings koje\'s back.', actions: ['reset']};
    }
}

/** An image's name in lists: its label, and its name when that differs. */
export function imageTitle(img: Image): string {
    return img.label.toLowerCase() === img.name ? img.label : `${img.label} (${img.name})`;
}

/**
 * The context's image dropdown: every image, and `current` as written when
 * it is none of them (a registry image or a path).
 */
export function imageChoices(list: Image[], current: string): [string, string][] {
    const out: [string, string][] = list.map(i => [i.name, imageTitle(i)]);
    const legacy = ['koje/base:latest', 'koje/base', 'localhost/koje/base:latest'];
    if (current !== '' && !legacy.includes(current) && !list.some(i => i.name === current))
        out.push([current, current]);
    return out;
}

/** Which choice `image = …` is (the old base image is `fedora`). */
export function imageChoice(choices: [string, string][], current: string): number {
    const value = ['koje/base:latest', 'koje/base', 'localhost/koje/base:latest'].includes(current) ? 'fedora' : current;
    return Math.max(0, choices.findIndex(([v]) => v === value));
}

/** An image's line in the sidebar and its build row. */
export function imageStatus(img: Image, building: boolean): string {
    if (building)
        return 'Building…';
    if (img.error)
        return img.error;
    return img.built ? 'Built' : 'Not built yet: the first start of a context using it builds it';
}

/** Who uses an image, in a line. */
export function usedBy(img: Image): string {
    if (img.update)
        return 'Newer version from koje';
    return img.usedBy.length ? `Used by ${img.usedBy.join(', ')}` : 'No context uses it';
}

/** The Remove row: koje's own and images in use stay. */
export function imageRemoval(img: Image): {sensitive: boolean; subtitle: string} {
    if (img.kind === 'builtin')
        return {sensitive: false, subtitle: 'koje\'s own image cannot be removed.'};
    if (img.kind === 'customized')
        return {sensitive: false, subtitle: 'It takes the place of koje\'s own image: reset it instead.'};
    if (img.usedBy.length)
        return {sensitive: false, subtitle: `${usedBy(img)}: give them another image first.`};
    return {sensitive: true, subtitle: 'Deletes its folder with the Containerfile and what it copies.'};
}

/** A name for a new image made for `context`: its name, else with a number. */
export function suggestImageName(context: string, list: Image[]): string {
    const taken = (n: string) => list.some(i => i.name === n) || n === 'fedora';
    if (!taken(context))
        return context;
    for (let i = 2; ; i++) {
        const name = `${context}-${i}`;
        if (!taken(name))
            return name;
    }
}

/** The Recreate row: only a context with a container can be recreated. */
export function recreateRow(c: Config): {sensitive: boolean; subtitle: string} {
    if (c.container && c.image_outdated)
        return {sensitive: true, subtitle: `Its image changed (${c.image_outdated}): Recreate builds and applies it.`};
    return c.container
        ? {
            sensitive: true,
            subtitle: 'A new container from the current settings. Everything installed in it outside your home is lost; the windows come back.',
        }
        : {
            sensitive: false,
            subtitle: 'No container yet: its first start creates one from the current settings.',
        };
}

/** A folder as `context.toml` writes it: `~/…` below the home; `null` outside it (ADR 0034). */
export function mountPath(path: string, home: string): string | null {
    const h = home.replace(/\/+$/, '');
    if (path === h || !path.startsWith(`${h}/`))
        return null;
    return `~${path.slice(h.length)}`;
}

// ----- States -----

export interface State {
    name: string;
    role: string;
    current: boolean;
    stored: boolean;
    windows: number;
    layouts: number;
    /** Seconds since the epoch, 0 when unknown. */
    modified: number;
    error: string;
}

export function states(rows: [string, string, boolean, boolean, number, number, number, string][]): State[] {
    return rows.map(([name, role, current, stored, windows, layouts, modified, error]) =>
        ({name, role, current, stored, windows, layouts, modified, error}));
}

/** `5 min ago`, `2 days ago`, as `koje state list`. */
export function ago(seconds: number, now: number): string {
    const s = Math.max(0, now - seconds);
    if (s < 60)
        return 'just now';
    if (s < 3600)
        return `${Math.floor(s / 60)} min ago`;
    if (s < 86400)
        return `${Math.floor(s / 3600)} h ago`;
    const days = Math.floor(s / 86400);
    return days === 1 ? '1 day ago' : `${days} days ago`;
}

/** A state's subtitle in the list. */
export function stateSubtitle(s: State, now: number): string {
    if (s.error)
        return `Cannot be read: ${s.error}`;
    if (s.name === 'empty')
        return 'No windows (built in)';
    if (!s.stored)
        return 'Not saved yet';
    const parts = [s.windows === 1 ? '1 window' : `${s.windows} windows`];
    if (s.modified)
        parts.push(`saved ${ago(s.modified, now)}`);
    return parts.join(' · ');
}

/** What a state's menu offers. */
export function stateActions(s: State, running: boolean): ('load' | 'save' | 'copy' | 'rename' | 'remove')[] {
    const out: ('load' | 'save' | 'copy' | 'rename' | 'remove')[] = ['load'];
    if (running && s.name !== 'empty')
        out.push('save');
    if (s.stored)
        out.push('copy', 'rename', 'remove');
    return out;
}

/** A context or state name koje accepts (koje_core::name): 1–48 of a-z, 0-9, `-`, starting with a letter. */
export function validName(name: string): boolean {
    return name.length <= 48 && /^[a-z][a-z0-9-]*$/.test(name);
}

// ----- What an operation printed (kojed's OperationProgress) -----

/** The output of a context's last operation. */
export interface Log {
    operation: string;
    lines: string[];
    state: 'running' | 'finished' | 'failed';
    /** Why it failed. */
    message: string;
    /** Whether it built or pulled an image (the output is then shown at once). */
    building: boolean;
}

/** Lines kept per operation: a build can print many thousands. */
export const LOG_LINES = 5000;

export function logStarted(operation: string): Log {
    return {operation, lines: [], state: 'running', message: '', building: false};
}

/**
 * Add `line` to `log` (in place: builds print thousands of lines); the
 * oldest go beyond {@link LOG_LINES}.
 */
export function logLine(log: Log, line: string): void {
    log.lines.push(line);
    if (log.lines.length > LOG_LINES)
        log.lines.splice(0, log.lines.length - LOG_LINES);
    if (/^(building|pulling|adding your user to) /.test(line))
        log.building = true;
}

export function logEnded(log: Log, failed: string | null): Log {
    return failed === null
        ? {...log, state: 'finished'}
        : {...log, state: 'failed', message: failed, lines: [...log.lines, `failed: ${failed}`]};
}

/** The output dialog's title: `Starting kp…`, `Started kp`, `Recreating kp failed`. */
export function logTitle(context: string, log: Log): string {
    const [doing, done] = OPERATION_TITLES[log.operation] ?? [log.operation, log.operation];
    switch (log.state) {
    case 'running': return `${doing} ${context}…`;
    case 'finished': return `${done} ${context}`;
    case 'failed': return `${doing} ${context} failed`;
    }
}

const OPERATION_TITLES: Record<string, [string, string]> = {
    build: ['Building', 'Built'],
    up: ['Starting', 'Started'],
    down: ['Stopping', 'Stopped'],
    sleep: ['Putting to sleep', 'Put to sleep'],
    recreate: ['Recreating', 'Recreated'],
    remove: ['Removing', 'Removed'],
};

