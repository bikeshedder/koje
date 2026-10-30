// Bundle koje's GNOME Shell extension (src/extension, with the kojed client
// from src/api) into build/extension/koje@koje.io: extension.js, metadata.json
// and the D-Bus interface it serves. GNOME Shell loads the folder as is.

import {cpSync, mkdirSync, rmSync} from 'node:fs';

import {build} from 'esbuild';

const src = 'src/extension';
const out = 'build/extension/koje@koje.io';

rmSync(out, {recursive: true, force: true});
mkdirSync(out, {recursive: true});
await build({
    entryPoints: [`${src}/extension.ts`],
    outfile: `${out}/extension.js`,
    bundle: true,
    format: 'esm',
    target: 'es2022',
    // GJS' and GNOME Shell's modules are there at run time.
    external: ['gi://*', 'resource://*', 'system', 'cairo'],
    logLevel: 'warning',
});
cpSync(`${src}/metadata.json`, `${out}/metadata.json`);
cpSync(`${src}/dbus`, `${out}/dbus`, {recursive: true});
