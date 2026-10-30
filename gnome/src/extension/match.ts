// Which of a context's programs a window or one of GNOME's favorites is
// (ADR 0062, from ADR 0053): its `StartupWMClass`, its desktop file id or the
// id's last part, its name, or its program. Pure (vitest).

/** A context's program as kojed's `Apps` gives it. */
export interface Program {
    /** Desktop file id, `firefox.desktop`. */
    id: string;
    name: string;
    /** `StartupWMClass`, `""` for none. */
    wmClass: string;
    /** The command's file name, `""` for none. */
    program: string;
}

const stem = (id: string) => id.replace(/\.desktop$/, '');
const last = (s: string) => s.split('.').pop() ?? s;
const same = (a: string, b: string) => a !== '' && a.toLowerCase() === b.toLowerCase();

/** The rules in order: the first that any program meets decides. */
const RULES: ((p: Program, appId: string) => boolean)[] = [
    (p, a) => same(p.wmClass, a),
    (p, a) => same(stem(p.id), a),
    (p, a) => same(last(stem(p.id)), last(a)),
    (p, a) => same(p.name, a) || same(p.name, last(a)),
    (p, a) => same(p.program, a) || same(p.program, last(a)),
];

/** The program a window with app id `appId` (Wayland's app id, X11's class) belongs to. */
export function programFor<P extends Program>(appId: string, programs: P[]): P | null {
    if (appId === '')
        return null;
    for (const rule of RULES) {
        const p = programs.find(q => rule(q, appId));
        if (p)
            return p;
    }
    return null;
}

/** The program one of GNOME's favorites (desktop file id, name) is in a context. */
export function favoriteFor<P extends Program>(id: string, name: string, programs: P[]): P | null {
    return programFor(stem(id), programs) ??
        programs.find(p => same(p.name, name)) ?? null;
}

/**
 * A context's favorites as desktop file ids of its programs: its own list
 * (`own`; ids it no longer has left out), else GNOME's favorites it has.
 */
export function favorites(own: string[] | null, gnome: {id: string; name: string}[], programs: Program[]): string[] {
    const ids = own !== null
        ? own.filter(id => programs.some(p => p.id === id))
        : gnome.map(f => favoriteFor(f.id, f.name, programs)?.id)
            .filter((id): id is string => id !== undefined);
    return [...new Set(ids)];
}
