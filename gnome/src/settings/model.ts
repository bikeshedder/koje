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

/** The profile choices: `config.toml`'s (`''`), then the four. */
export const PROFILES: [string, string][] = [
    ['', 'From config.toml'],
    ['full', 'Full'],
    ['desktop', 'Desktop'],
    ['headless', 'Headless'],
    ['isolated', 'Isolated'],
];

/** The page's fields; sizes as text. */
export interface Form {
    prefix: string;
    label: string;
    icon: string;
    color: string;
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
}

export function form(c: Config): Form {
    return {
        prefix: c.prefix,
        label: c.label,
        icon: c.icon,
        color: c.color,
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
    for (const key of ['prefix', 'label', 'icon', 'color', 'profile'] as const) {
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
