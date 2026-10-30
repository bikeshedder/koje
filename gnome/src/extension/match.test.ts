import {describe, expect, it} from 'vitest';

import {favoriteFor, favorites, programFor, type Program} from './match.js';

const program = (id: string, name: string, wmClass = '', prog = ''): Program =>
    ({id, name, wmClass, program: prog});

const PROGRAMS = [
    program('org.mozilla.firefox.desktop', 'Firefox', '', 'firefox'),
    program('org.gnome.Ptyxis.desktop', 'Terminal', '', 'ptyxis'),
    program('code.desktop', 'Visual Studio Code', 'Code', 'code'),
    program('foot.desktop', 'Foot', '', 'foot'),
    program('htop.desktop', 'Htop', '', 'htop'),
];

describe('a window to its program', () => {
    it('by StartupWMClass first', () => {
        expect(programFor('Code', PROGRAMS)?.id).toBe('code.desktop');
    });

    it('by the id or its last part', () => {
        expect(programFor('org.gnome.Ptyxis', PROGRAMS)?.id).toBe('org.gnome.Ptyxis.desktop');
        expect(programFor('firefox', PROGRAMS)?.id).toBe('org.mozilla.firefox.desktop');
        expect(programFor('foot', PROGRAMS)?.id).toBe('foot.desktop');
    });

    it('by its name or program', () => {
        expect(programFor('terminal', PROGRAMS)?.id).toBe('org.gnome.Ptyxis.desktop');
        expect(programFor('ptyxis', PROGRAMS)?.id).toBe('org.gnome.Ptyxis.desktop');
    });

    it('none for an unknown or empty app id', () => {
        expect(programFor('gimp', PROGRAMS)).toBeNull();
        expect(programFor('', PROGRAMS)).toBeNull();
    });

    it('an earlier rule wins over a later one', () => {
        const both = [program('a.desktop', 'foot'), program('foot.desktop', 'Foot')];
        expect(programFor('foot', both)?.id).toBe('foot.desktop');
    });
});

describe("GNOME's favorites in a context", () => {
    it('map to the context\'s programs', () => {
        expect(favoriteFor('firefox.desktop', 'Firefox', PROGRAMS)?.id).toBe('org.mozilla.firefox.desktop');
        expect(favoriteFor('org.gnome.Terminal.desktop', 'Terminal', PROGRAMS)?.id)
            .toBe('org.gnome.Ptyxis.desktop');
        expect(favoriteFor('org.gnome.Nautilus.desktop', 'Files', PROGRAMS)).toBeNull();
    });

    it('unset: GNOME\'s that the context has, in order, once each', () => {
        const gnome = [
            {id: 'firefox.desktop', name: 'Firefox'},
            {id: 'org.gnome.Nautilus.desktop', name: 'Files'},
            {id: 'org.gnome.Terminal.desktop', name: 'Terminal'},
            {id: 'org.gnome.Console.desktop', name: 'Terminal'},
        ];
        expect(favorites(null, gnome, PROGRAMS))
            .toEqual(['org.mozilla.firefox.desktop', 'org.gnome.Ptyxis.desktop']);
    });

    it('its own list wins; ids it lacks are left out', () => {
        expect(favorites(['foot.desktop', 'gone.desktop', 'htop.desktop'], [{id: 'firefox.desktop', name: 'Firefox'}], PROGRAMS))
            .toEqual(['foot.desktop', 'htop.desktop']);
        expect(favorites([], [{id: 'firefox.desktop', name: 'Firefox'}], PROGRAMS)).toEqual([]);
    });
});
