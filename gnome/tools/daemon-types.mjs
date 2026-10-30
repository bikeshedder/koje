// Generates src/api/daemon1.ts from kojed's interface file (ADR 0060):
// the TypeScript signatures of io.koje.Daemon1's methods and signals.
//
//   node tools/daemon-types.mjs           write src/api/daemon1.ts
//   node tools/daemon-types.mjs --check   fail when it is not current
import {readFileSync, writeFileSync} from 'node:fs';
import {dirname, join} from 'node:path';
import {fileURLToPath} from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const XML = join(here, '../../crates/kojed/dbus/io.koje.Daemon1.xml');
const OUT = join(here, '../src/api/daemon1.ts');

/**
 * One complete type at `sig[i]`: [TypeScript type, next index]. A variant
 * the client sends is a `GLib.Variant`; one it receives comes unpacked.
 */
function tsType(sig, i, input = false) {
    const c = sig[i];
    switch (c) {
    case 's': case 'o': case 'g': return ['string', i + 1];
    case 'b': return ['boolean', i + 1];
    case 'y': case 'n': case 'q': case 'i': case 'u': case 'x': case 't': case 'd': case 'h':
        return ['number', i + 1];
    case 'v': return [input ? 'GLib.Variant' : 'unknown', i + 1];
    case 'a':
        if (sig[i + 1] === '{') {
            const [, afterKey] = tsType(sig, i + 2, input);
            const [value, afterValue] = tsType(sig, afterKey, input);
            return [`Record<string, ${value}>`, afterValue + 1];
        } else {
            const [item, next] = tsType(sig, i + 1, input);
            return [`${item}[]`, next];
        }
    case '(': {
        const items = [];
        let j = i + 1;
        while (sig[j] !== ')') {
            const [t, next] = tsType(sig, j, input);
            items.push(t);
            j = next;
        }
        return [`[${items.join(', ')}]`, j + 1];
    }
    default:
        throw new Error(`unknown D-Bus type ${c} in ${sig}`);
    }
}

/** The types of a signature's complete types. */
function tsTypes(sig, input) {
    const out = [];
    for (let i = 0; i < sig.length;) {
        const [t, next] = tsType(sig, i, input);
        out.push(t);
        i = next;
    }
    return out;
}

function members(xml) {
    const iface = xml.match(/<interface name="io\.koje\.Daemon1">([\s\S]*?)<\/interface>/);
    if (!iface)
        throw new Error('no io.koje.Daemon1 in the interface file');
    const out = [];
    const re = /<(method|signal) name="(\w+)"(?:\/>|>([\s\S]*?)<\/\1>)/g;
    for (const [, kind, name, body = ''] of iface[1].matchAll(re)) {
        const args = [...body.matchAll(/<arg\b([^>]*)\/?>/g)].map(([, attrs]) => ({
            name: attrs.match(/name="(\w+)"/)?.[1],
            type: attrs.match(/type="([^"]+)"/)[1],
            out: /direction="out"/.test(attrs),
        }));
        out.push({kind, name, ins: args.filter(a => !a.out), outs: args.filter(a => a.out)});
    }
    return out;
}

function generate(xml) {
    const all = members(xml);
    const methods = all.filter(m => m.kind === 'method');
    const signals = all.filter(m => m.kind === 'signal');
    const sig = args => `(${args.map(a => a.type).join('')})`;
    const tuple = (args, input) => `[${args.map(a => tsTypes(a.type, input)[0]).join(', ')}]`;
    const lines = [
        '// Generated from crates/kojed/dbus/io.koje.Daemon1.xml by',
        '// tools/daemon-types.mjs – do not edit; `just ui` regenerates it.',
        '',
        'import type GLib from \'gi://GLib\';',
        '',
        'export const BUS_NAME = \'io.koje.Daemon\';',
        'export const OBJECT_PATH = \'/io/koje/Daemon\';',
        'export const INTERFACE = \'io.koje.Daemon1\';',
        '',
        '/** Each method\'s in and out arguments. */',
        'export interface Methods {',
        ...methods.map(m => `    ${m.name}: {in: ${tuple(m.ins, true)}; out: ${tuple(m.outs, false)}};`),
        '}',
        '',
        '/** Each method\'s D-Bus signatures: [in, out]. */',
        'export const METHODS: {[M in keyof Methods]: [string, string]} = {',
        ...methods.map(m => `    ${m.name}: ['${sig(m.ins)}', '${sig(m.outs)}'],`),
        '};',
        '',
        '/** Each signal\'s arguments. */',
        'export interface Signals {',
        ...signals.map(s => `    ${s.name}: ${tuple(s.ins, false)};`),
        '}',
        '',
    ];
    return lines.join('\n');
}

const text = generate(readFileSync(XML, 'utf8'));
if (process.argv.includes('--check')) {
    let current = '';
    try {
        current = readFileSync(OUT, 'utf8');
    } catch {
        // missing: not current
    }
    if (current !== text) {
        console.error('src/api/daemon1.ts is not current: run `just ui`');
        process.exit(1);
    }
} else {
    writeFileSync(OUT, text);
}
