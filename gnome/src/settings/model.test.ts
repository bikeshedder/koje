import {describe, expect, it} from 'vitest';

import {
    type Config, actions, ago, changes, contexts, effective, form, formatSize, humanSize,
    memoryFraction, memoryLabel, mountPath, parseSize, problems, stateActions, stateSubtitle,
    LOG_LINES, SWITCHES, adjustments, imageChoice, imageChoices, imageNameOf, imageOrigin, imageRemoval, reference, validVersion, imageStatus, images,
    logEnded, logLine, logStarted, logTitle, profileSummary, profiles, recreateRow, states, statusLabel,
    suggestImageName, usedBy, validName, withProfile, withSwitch,
} from './model.js';

const config: Config = {
    name: 'kp', prefix: 'KP', label: '', icon: '', color: '#2E7D5B', image: 'koje/base',
    profile: '', default_profile: 'full',
    access: {sound: false},
    effective: {gpu: true, sound: false},
    bases: {'': {gpu: true, sound: true}, desktop: {gpu: false, sound: true}},
    mounts: ['~/src'], memory_soft: 6 * 2 ** 30, memory_hard: 0,
    container: true, image_file: '', image_outdated: '',
};

describe('contexts', () => {
    const [kp, bad] = contexts([
        ['kp', 'Kundenportal', 'folder-symbolic', '#2E7D5B', 'running', 2],
        ['zz', 'zz', '', '', 'nonsense', 0],
    ]);
    it('read the rows', () => {
        expect(kp.label).toBe('Kundenportal');
        expect(bad.status).toBe('invalid');
    });
    it('say their status, or what is under way', () => {
        expect(statusLabel(kp, undefined)).toBe('Running · 2 windows');
        expect(statusLabel(kp, 'down')).toBe('Stopping…');
        expect(statusLabel({...kp, status: 'paused'}, undefined)).toBe('Asleep');
    });
    it('offer what their status allows', () => {
        expect(actions(kp)).toEqual(['sleep', 'down']);
        expect(actions({...kp, status: 'stopped'})).toEqual(['up']);
        expect(actions({...kp, status: 'paused'})).toEqual(['wake', 'down']);
        expect(actions(bad)).toEqual([]);
    });
});

describe('sizes as koje writes them', () => {
    it('parse', () => {
        expect(parseSize('6G')).toBe(6 * 2 ** 30);
        expect(parseSize('512m')).toBe(512 * 2 ** 20);
        expect(parseSize('1024')).toBe(1024);
        expect(parseSize(' ')).toBe(0);
        expect(parseSize('6 GB')).toBeNull();
        expect(parseSize('lots')).toBeNull();
    });
    it('format', () => {
        expect(formatSize(6 * 2 ** 30)).toBe('6G');
        expect(formatSize(1536 * 2 ** 20)).toBe('1536M');
        expect(formatSize(0)).toBe('');
        expect(humanSize(1_503_238_553)).toBe('1.3 GiB');
        expect(humanSize(20 * 2 ** 30)).toBe('20 GiB');
        expect(humanSize(900)).toBe('900 B');
    });
    it('describe memory against the limits', () => {
        expect(memoryLabel(2 ** 30, 2 * 2 ** 30, 0)).toBe('1.0 GiB of soft 2.0 GiB');
        expect(memoryLabel(2 ** 30, 0, 0)).toBe('1.0 GiB · no limit');
        expect(memoryFraction(2 ** 30, 2 ** 30, 4 * 2 ** 30)).toBe(0.25);
        expect(memoryFraction(3, 2, 0)).toBe(1);
        expect(memoryFraction(3, 0, 0)).toBe(0);
    });
});

describe('the recreate row', () => {
    it('needs a container', () => {
        expect(recreateRow(config).sensitive).toBe(true);
        const none = recreateRow({...config, container: false});
        expect(none).toEqual({
            sensitive: false,
            subtitle: 'No container yet: its first start creates one from the current settings.',
        });
    });
});

describe('images', () => {
    const list = images([
        ['fedora', '45', 'Fedora 45', 'koje\'s image', 'builtin', '', ['kp'], true, '', false],
        ['web', '1', 'Web development', '', 'folder', '/c/koje/images/web/Containerfile', [], false, '', false],
        ['kp', '3', 'kp', '', 'folder', '/c/koje/images/kp/Containerfile', ['kp', 'shop'], true, '', false],
    ]);
    it('read the rows', () => {
        expect(list[0].kind).toBe('builtin');
        expect(list[1].usedBy).toEqual([]);
    });
    it('are the context\'s choices, with a registry image as written', () => {
        const choices = imageChoices(list, 'localhost/koje/web:1');
        expect(choices).toEqual([
            ['localhost/koje/fedora:45', 'Fedora 45 (fedora)'],
            ['localhost/koje/web:1', 'Web development (web)'],
            ['localhost/koje/kp:3', 'kp'],
        ]);
        expect(imageChoice(choices, 'localhost/koje/web:1')).toBe(1);
        const reg = imageChoices(list, 'docker.io/library/debian:13');
        expect(reg.at(-1)).toEqual(['docker.io/library/debian:13', 'docker.io/library/debian:13']);
        expect(imageChoice(reg, 'docker.io/library/debian:13')).toBe(3);
        // Older spellings are the image they name.
        for (const old of ['koje/base:latest', 'fedora', 'localhost/koje/fedora:44']) {
            expect(imageChoices(list, old), old).toHaveLength(3);
            expect(imageChoice(imageChoices(list, old), old), old).toBe(0);
        }
        expect(reference(list[2])).toBe('localhost/koje/kp:3');
        expect(imageNameOf('localhost/koje/user/fedora:45')).toBeNull();
        expect(validVersion('26.04') && validVersion('1') && !validVersion('latest') && !validVersion('a b')).toBe(true);
    });
    it('say whether they are built and who uses them', () => {
        expect(imageStatus(list[0], false)).toBe('Built');
        expect(imageStatus(list[1], false)).toMatch(/^Not built yet/);
        expect(imageStatus(list[1], true)).toBe('Building…');
        expect(imageStatus({...list[1], error: 'it has no Containerfile'}, false)).toBe('it has no Containerfile');
    });
    it('are removed only when koje\'s and unused', () => {
        expect(imageRemoval(list[0]).sensitive).toBe(false);
        expect(imageRemoval(list[2])).toEqual({
            sensitive: false, subtitle: 'Used by kp, shop: give them another image first.',
        });
        expect(imageRemoval(list[1]).sensitive).toBe(true);
    });
    it('of koje are customized, kept or reset', () => {
        expect(imageOrigin(list[1])).toBeNull();
        expect(imageOrigin(list[0])?.actions).toEqual(['customize']);
        const mine = {...list[0], kind: 'customized' as const, file: '/c/koje/images/fedora/Containerfile'};
        expect(imageOrigin(mine)?.actions).toEqual(['reset']);
        expect(imageRemoval(mine).sensitive).toBe(false);
        const newer = {...mine, update: true};
        expect(imageOrigin(newer)?.actions).toEqual(['keep', 'reset']);
        expect(usedBy(newer)).toBe('Newer version from koje');
    });
    it('get a free name for a context', () => {
        expect(suggestImageName('shop', list)).toBe('shop');
        expect(suggestImageName('kp', list)).toBe('kp-2');
        expect(suggestImageName('fedora', list)).toBe('fedora-2');
    });
    it('make Recreate say when the image changed', () => {
        expect(recreateRow({...config, image_outdated: 'web was rebuilt'}).subtitle)
            .toBe('Its image changed (web was rebuilt): Recreate builds and applies it.');
    });
});

describe('profiles', () => {
    const all = Object.fromEntries(SWITCHES.map(([n]) => [n, true]));
    const none = Object.fromEntries(SWITCHES.map(([n]) => [n, false]));
    const bases = {
        '': all, full: all, isolated: none,
        desktop: {...all, gpu: false},
        headless: {...none, network: true, nested: true},
    };
    it('name koje\'s default by what it is', () => {
        expect(profiles('desktop')[0]).toEqual(['', 'Default (Desktop)']);
        expect(profiles('full').map(([p]) => p)).toEqual(['', 'full', 'desktop', 'headless', 'isolated']);
    });
    it('say in a line what they give', () => {
        expect(profileSummary(bases, 'full')).toBe('Everything');
        expect(profileSummary(bases, 'desktop')).toBe('Everything but graphics card');
        expect(profileSummary(bases, 'headless')).toBe('Only network, containers inside');
        expect(profileSummary(bases, 'isolated')).toBe('Nothing: no windows, no network');
        expect(profileSummary(bases, '')).toBe("koje's default for all contexts. Everything");
    });
});

describe('the context form', () => {
    const f = form(config);
    it('comes from the config', () => {
        expect(f.memorySoft).toBe('6G');
        expect(f.memoryHard).toBe('');
        expect(effective(f, config.bases)).toEqual({gpu: true, sound: false});
    });
    it('shows the switches of a profile chosen before saving', () => {
        expect(effective({...f, profile: 'desktop'}, config.bases).gpu).toBe(false);
    });
    it('keeps only switches that differ from the profile', () => {
        const back = withSwitch(f, config.bases, 'sound', true);
        expect(back.access).toEqual({});
        const off = withSwitch(f, config.bases, 'gpu', false);
        expect(off.access).toEqual({sound: false, gpu: false});
    });
    it('takes a chosen profile as it is', () => {
        const desktop = withProfile(f, 'desktop');
        expect(desktop.access).toEqual({});
        expect(changes(f, desktop)).toEqual({profile: 'desktop', access: {}});
        expect(adjustments(desktop)).toBe('As the profile gives');
        expect(adjustments(f)).toBe('1 changed from the profile');
    });
    it('sends only what changed', () => {
        expect(changes(f, f)).toEqual({});
        const edited = {...f, label: 'Kundenportal', image: 'localhost/koje/web:1', memoryHard: '8G', mounts: ['~/src', '~/docs']};
        expect(changes(f, edited)).toEqual({
            label: 'Kundenportal', image: 'localhost/koje/web:1', memory_hard: 8 * 2 ** 30, mounts: ['~/src', '~/docs'],
        });
        expect(changes(f, {...f, access: {}})).toEqual({access: {}});
    });
    it('says what keeps it from being saved', () => {
        expect(problems(f)).toEqual({});
        expect(Object.keys(problems({...f, prefix: ' ', color: 'green', memoryHard: '4G'})).sort())
            .toEqual(['color', 'memorySoft', 'prefix']);
        expect(problems({...f, icon: '../x'}).icon).toBeTruthy();
    });
    it('takes folders below the home only', () => {
        expect(mountPath('/home/mj/src/koje', '/home/mj')).toBe('~/src/koje');
        expect(mountPath('/home/mj', '/home/mj')).toBeNull();
        expect(mountPath('/home/mjx/a', '/home/mj')).toBeNull();
        expect(mountPath('/srv', '/home/mj/')).toBeNull();
    });
});

describe('states', () => {
    const now = 1_000_000;
    const [def, fresh, empty, broken] = states([
        ['default', 'default', true, true, 3, 1, now - 300, ''],
        ['fresh', 'fresh', false, false, 0, 0, 0, ''],
        ['empty', '', false, false, 0, 0, 0, ''],
        ['old', '', false, true, 0, 0, 0, 'bad toml'],
    ]);
    it('say what they hold', () => {
        expect(stateSubtitle(def, now)).toBe('3 windows · saved 5 min ago');
        expect(stateSubtitle(fresh, now)).toBe('Not saved yet');
        expect(stateSubtitle(empty, now)).toBe('No windows (built in)');
        expect(stateSubtitle(broken, now)).toBe('Cannot be read: bad toml');
    });
    it('offer what can be done with them', () => {
        expect(stateActions(def, true)).toEqual(['load', 'save', 'copy', 'rename', 'remove']);
        expect(stateActions(empty, true)).toEqual(['load']);
        expect(stateActions(fresh, false)).toEqual(['load']);
    });
    it('age as koje state list', () => {
        expect(ago(now - 30, now)).toBe('just now');
        expect(ago(now - 7200, now)).toBe('2 h ago');
        expect(ago(now - 86400, now)).toBe('1 day ago');
        expect(ago(now - 3 * 86400, now)).toBe('3 days ago');
    });
});

it('accepts names as koje does', () => {
    expect(validName('kundenportal')).toBe(true);
    expect(validName('kp-2')).toBe(true);
    expect(validName('Kp')).toBe(false);
    expect(validName('2kp')).toBe(false);
    expect(validName('kp_2')).toBe(false);
    expect(validName('a'.repeat(49))).toBe(false);
});

describe('an operation\'s output', () => {
    it('collects lines and notices a build', () => {
        const log = logStarted('up');
        logLine(log, 'started kp');
        expect(log.building).toBe(false);
        logLine(log, 'building koje\'s image fedora (not built yet) – this can take a few minutes');
        logLine(log, '  STEP 1/6: FROM registry.fedoraproject.org/fedora:44');
        expect(log.building).toBe(true);
        expect(log.lines).toHaveLength(3);
        expect(logTitle('kp', log)).toBe('Starting kp…');
        expect(logTitle('kp', logEnded(log, null))).toBe('Started kp');
        const failed = logEnded(log, 'podman build failed');
        expect(failed.state).toBe('failed');
        expect(failed.lines.at(-1)).toBe('failed: podman build failed');
        expect(logTitle('kp', {...failed, operation: 'recreate'})).toBe('Recreating kp failed');
    });
    it('keeps the last lines of a long build', () => {
        const log = logStarted('recreate');
        for (let i = 0; i < LOG_LINES + 10; i++)
            logLine(log, String(i));
        expect(log.lines).toHaveLength(LOG_LINES);
        expect(log.lines[0]).toBe('10');
        expect(log.lines.at(-1)).toBe(String(LOG_LINES + 9));
    });
});

