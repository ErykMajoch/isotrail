// Prints the built site's files as a Markdown table, with their sizes as
// served and gzipped, largest first
//
//   node scripts/size.mjs

import { readdir, readFile } from 'node:fs/promises';
import { join, relative } from 'node:path';
import { fileURLToPath } from 'node:url';
import { gzipSync } from 'node:zlib';

const DIST = fileURLToPath(new URL('../dist', import.meta.url));

const rows = [];
for (const entry of await readdir(DIST, { recursive: true, withFileTypes: true })) {
  if (!entry.isFile()) continue;
  const path = join(entry.parentPath, entry.name);
  const bytes = await readFile(path);
  rows.push({
    file: relative(DIST, path).replaceAll('\\', '/'),
    bytes: bytes.length,
    gzipped: gzipSync(bytes, { level: 9 }).length,
  });
}
rows.sort((a, b) => b.bytes - a.bytes || a.file.localeCompare(b.file));

const total = (key) => rows.reduce((sum, row) => sum + row[key], 0);
console.log('| Site file | Bytes | Gzipped bytes |');
console.log('|---|---|---|');
for (const { file, bytes, gzipped } of rows) console.log(`| \`${file}\` | ${bytes} | ${gzipped} |`);
console.log(`| **Total** | **${total('bytes')}** | **${total('gzipped')}** |`);
