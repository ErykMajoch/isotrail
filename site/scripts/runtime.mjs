// Copies the web runtime and the scene worker into public/runtime/<hash>/,
// named by a hash of their contents so browsers can keep them for a year,
// and writes that address to src/runtime.json for the pages
//
//   node scripts/runtime.mjs
//
// Build both first: sh site/build.sh does it all

import { createHash } from 'node:crypto';
import { existsSync } from 'node:fs';
import { copyFile, mkdir, readFile, rm, writeFile } from 'node:fs/promises';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const SITE = fileURLToPath(new URL('..', import.meta.url));
const WEB = join(SITE, '..', 'web');
const OUT = join(SITE, 'public', 'runtime');

// Paths inside web/, kept as they are so the files find each other
const FILES = [
  ['isotrail-embed.js', 'sh web/build.sh'],
  ['pkg/isotrail_web.js', 'sh web/build.sh'],
  ['pkg/isotrail_web_bg.wasm', 'sh web/build.sh'],
  ['scene/scene.js', 'sh web/build-scene.sh'],
  ['scene/worker.js', 'sh web/build-scene.sh'],
  ['scene/pkg/isotrail_scene.js', 'sh web/build-scene.sh'],
  ['scene/pkg/isotrail_scene_bg.wasm', 'sh web/build-scene.sh'],
  ['scene/pkg/lite.tzb', 'sh web/build-scene.sh'],
];

const missing = FILES.filter(([file]) => !existsSync(join(WEB, file)));
if (missing.length) {
  for (const [file, build] of missing) console.error(`web/${file} is missing: build it first with ${build}`);
  process.exit(1);
}

const hash = createHash('sha256');
for (const [file] of FILES) {
  hash.update(file).update('\0').update(await readFile(join(WEB, file))).update('\0');
}
const name = hash.digest('hex').slice(0, 12);

await rm(OUT, { recursive: true, force: true });
for (const [file] of FILES) {
  const to = join(OUT, name, file);
  await mkdir(dirname(to), { recursive: true });
  await copyFile(join(WEB, file), to);
}
await writeFile(join(SITE, 'src', 'runtime.json'), `${JSON.stringify({ path: `/runtime/${name}/` }, null, 2)}\n`);
console.log(`runtime copied to public/runtime/${name}/`);
