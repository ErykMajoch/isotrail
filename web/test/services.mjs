// A local server for the browser checks. It plays the elevation, map and
// weather services from the recordings in fixtures/, serves web/ or a built
// site with the headers in its _headers file, and can be told to fail
//
//   const services = await serve({ site });
//   services.servers        the options that point a build at it
//   services.state.mode     { services: false } stops every service answering;
//                           { overpass: 'busy' } answers the next map data
//                           request with HTTP 502, and 'hold' keeps it waiting
//   services.state.seen     every request, as { method, url, body }

import { createHash } from 'node:crypto';
import { existsSync } from 'node:fs';
import { readFile } from 'node:fs/promises';
import { createServer } from 'node:http';
import { extname, join, normalize, sep } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = fileURLToPath(new URL('../..', import.meta.url));
const WEB = join(ROOT, 'web');
const FIXTURES = join(ROOT, 'fixtures');

const TYPES = {
  '.html': 'text/html',
  '.css': 'text/css',
  '.js': 'text/javascript',
  '.mjs': 'text/javascript',
  '.wasm': 'application/wasm',
};

// Paths the server answers as one of the services
export const SERVICE = /^\/(terrain|overpass|weather|busy|empty)\//;

// The services' answers, named as RecordedFetcher names them
function recorded(dir, query, body = '') {
  const hash = createHash('sha256').update(query).update('\n').update(body).digest('hex');
  return `${dir}-${hash.slice(0, 16)}`;
}

// The site's _headers rules, each a pattern and its headers. connect-src also
// allows the local servers, as some checks ask one on another port
async function readHeaders(site) {
  const rules = [];
  const text = await readFile(join(site, '_headers'), 'utf8').catch(() => '');
  for (const line of text.split(/\r?\n/)) {
    if (/^\S/.test(line)) {
      rules.push({ pattern: line.trim(), headers: {} });
    } else if (line.trim() && rules.length) {
      const colon = line.indexOf(':');
      const name = line.slice(0, colon).trim().toLowerCase();
      let value = line.slice(colon + 1).trim();
      if (name === 'content-security-policy') value = value.replace('connect-src', 'connect-src http://127.0.0.1:*');
      rules.at(-1).headers[name] = value;
    }
  }
  return rules;
}

// The headers of every rule that matches, joined by commas as Cloudflare does
function headersFor(rules, path) {
  const found = {};
  for (const { pattern, headers } of rules) {
    const source = pattern
      .replace(/[.+?^${}()|[\]\\]/g, '\\$&')
      .replace(/:[A-Za-z]\w*/g, '[^/]+')
      .replace('*', '.*');
    if (!new RegExp(`^${source}$`).test(path)) continue;
    for (const [name, value] of Object.entries(headers)) found[name] = found[name] ? `${found[name]}, ${value}` : value;
  }
  return found;
}

// Serves the services, and `site` (a built site's folder) or else web/
export async function serve({ site } = {}) {
  const rules = site ? await readHeaders(site) : [];
  const state = { seen: [], mode: { services: true, overpass: 'normal' }, open: 0, held: 0 };
  const server = createServer(async (req, res) => {
    state.open++;
    res.on('close', () => state.open--);
    const chunks = [];
    for await (const chunk of req) chunks.push(chunk);
    const body = Buffer.concat(chunks);
    state.seen.push({ method: req.method, url: req.url, body: body.toString('utf8') });
    const url = new URL(req.url, 'http://localhost');
    const path = decodeURIComponent(url.pathname);
    const query = url.search.slice(1);
    if (SERVICE.test(path)) {
      if (!state.mode.services) {
        req.socket.destroy();
        return;
      }
      // Overpass servers for the page's own list: one always busy, one that
      // answers with nothing
      if (path.startsWith('/busy/')) {
        res.writeHead(502, { 'content-type': 'text/plain' }).end('busy');
        return;
      }
      if (path.startsWith('/empty/')) {
        res.writeHead(200, { 'content-type': 'application/json' }).end('{"elements":[]}');
        return;
      }
      if (path.startsWith('/overpass/') && state.mode.overpass === 'busy') {
        state.mode.overpass = 'normal';
        res.writeHead(502, { 'content-type': 'text/plain' }).end('busy');
        return;
      }
      if (path.startsWith('/overpass/') && state.mode.overpass === 'hold') {
        state.held++;
        res.on('close', () => state.held--);
        return;
      }
    }
    let file;
    let tile;
    if (site && !SERVICE.test(path)) {
      file = normalize(join(site, path.endsWith('/') ? `${path}index.html` : path));
      if (!file.startsWith(site + sep)) file = undefined;
    } else if (path.startsWith('/web/')) {
      file = normalize(join(ROOT, path));
      if (!file.startsWith(WEB)) file = undefined;
    } else if ((tile = /^\/terrain\/(\d+)\/(\d+)\/(\d+)\.png$/.exec(path))) {
      file = join(FIXTURES, 'dem', 's3.amazonaws.com', 'elevation-tiles-prod', 'terrarium', ...tile.slice(1, 3), `${tile[3]}.png`);
    } else if (path === '/overpass/api/interpreter' && req.method === 'POST') {
      file = recorded(join(FIXTURES, 'osm', 'overpass-api.de', 'api', 'interpreter'), query, body);
    } else if (path === '/weather/v1/archive') {
      file = recorded(join(FIXTURES, 'weather', 'archive-api.open-meteo.com', 'v1', 'archive'), query);
    } else if (path === '/weather/v1/forecast') {
      file = recorded(join(FIXTURES, 'weather', 'api.open-meteo.com', 'v1', 'forecast'), query);
    }
    if (!file || !existsSync(file)) {
      res.writeHead(404).end();
      return;
    }
    const bytes = await readFile(file);
    const type = TYPES[extname(file)] ?? 'application/octet-stream';
    const headers = site && !SERVICE.test(path) ? headersFor(rules, path) : {};
    res.writeHead(200, { 'content-type': type, ...headers, 'content-length': bytes.length }).end(bytes);
  });
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  const origin = `http://127.0.0.1:${server.address().port}`;
  return {
    origin,
    state,
    servers: {
      'terrain-url': `${origin}/terrain/{z}/{x}/{y}.png`,
      'overpass-url': `${origin}/overpass/api/interpreter`,
      'weather-url': `${origin}/weather`,
    },
    close() {
      server.closeAllConnections();
      server.close();
    },
  };
}

// A port nothing listens on, from a server closed at once
export async function closedPort() {
  const unused = createServer();
  await new Promise((resolve) => unused.listen(0, '127.0.0.1', resolve));
  const port = unused.address().port;
  await new Promise((resolve) => unused.close(resolve));
  return port;
}
