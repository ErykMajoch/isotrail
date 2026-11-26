// The page's controls, made from the description of every option that
// `isotrail render` takes: site/src/options.json, written by the build from
// the tool's own definitions. This file only says where each option goes,
// what it is called on the page, and how a change reaches the diorama

import description from '../options.json';

export const OPTIONS = new Map(description.map((option) => [option.name, option]));

// The groups shown, in the usage guide's order
export const GROUPS = ['Terrain and look', 'Scene, light and route', 'Camera', 'Data'];
// Shown above the groups
export const FIRST = ['aspect', 'palette', 'season', 'weather', 'time'];
// Groups that come with stills, animations and their files
export const LATER_GROUPS = ['Image', 'Text and overlays', 'Animation', 'Encoding'];
// Change a finished scene, with no download
export const ADJUST = ['exaggeration', 'season', 'no-weather'];
// Need the scene built again from the GPX file
export const REBUILD = ['detail', 'no-map-data', 'padding', 'footprint', 'times', 'trim-start', 'trim-end'];
// Set by turning and zooming the diorama
export const CAMERA = ['turn', 'pitch', 'zoom'];
// Left out of the page, and why
export const LEFT_OUT = {
  offline: 'a page has no cache folder to build from',
  'cache-dir': 'a page has no cache folder',
  'overpass-url': 'the map data servers are chosen with their boxes',
  'terrain-url': 'the page asks the built-in elevation service',
  'weather-url': 'the page asks the built-in weather service',
  heading: 'the turn gives the same view',
  ffmpeg: 'a page runs no programs',
};
// Where the page starts when it differs from the tool's default
export const PAGE_DEFAULTS = { aspect: 'wide' };

export const SHAPES = {
  story: '9 / 16',
  video: '9 / 16',
  portrait: '3 / 4',
  feed: '4 / 5',
  square: '1 / 1',
  wide: '16 / 9',
};

const LABELS = {
  aspect: 'Shape',
  palette: 'Palette',
  season: 'Season',
  weather: 'Weather',
  time: 'Time of day',
  mesher: 'Terrain',
  'block-height': 'Block height',
  occlusion: 'Shade in hollows',
  'soft-shadows': 'Soft shadows',
  'god-rays': 'Sunbeams',
  exaggeration: 'Exaggeration',
  theme: 'Theme file',
  'prop-density': 'Trees and boulders',
  'base-blocks': 'Base depth',
  'night-sky': 'Night sky',
  'weather-effects': 'Weather effects',
  'route-colour': 'Route colour',
  'route-width': 'Route width',
  'route-hidden': 'Route behind ridges',
  'no-map-data': 'Leave out map data',
  'no-weather': 'Leave out the weather',
  'trim-start': 'Trim the start',
  'trim-end': 'Trim the end',
};
// Hints that say more than the help line does on a page
const HINTS = {
  aspect: "The diorama's shape, for a story, a post or a wide screen.",
};
// What a choice with no default shows
const UNSET = { weather: 'As recorded' };
// Examples in fields that start empty
const EXAMPLES = { sun: '150,40', 'trim-start': '300m', 'trim-end': '1.5km' };
// Number fields that take whole numbers
const WHOLE = ['N', 'PX'];

export const SERVERS = [
  { name: 'dresel', title: 'dresel.systems', runs: 'Run by Christian Dresel. It keeps no IP addresses or queries.' },
  { name: 'vk-maps', title: 'VK Maps', runs: 'Run by VK, in Russia.' },
  { name: 'overpass-api', title: 'overpass-api.de', runs: 'Run by FOSSGIS. It is often busy.' },
];

// Where an option goes on the page: `first`, `group`, `later` or `left-out`
export function place(option) {
  if (Object.hasOwn(LEFT_OUT, option.name)) return 'left-out';
  if (FIRST.includes(option.name)) return 'first';
  if (LATER_GROUPS.includes(option.group)) return 'later';
  return 'group';
}

// How a change reaches the diorama: `camera`, `adjust`, `rebuild`, `file`
// or `live` (an attribute of <isotrail-hike>)
export function route(name) {
  if (CAMERA.includes(name)) return 'camera';
  if (ADJUST.includes(name)) return 'adjust';
  if (REBUILD.includes(name)) return 'rebuild';
  if (name === 'theme') return 'file';
  return 'live';
}

export function label(name) {
  return LABELS[name] ?? `${name.charAt(0).toUpperCase()}${name.slice(1).replaceAll('-', ' ')}`;
}

function sentence(text) {
  const start = text.charAt(0).toUpperCase() + text.slice(1);
  return /[.?!]$/.test(start) ? start : `${start}.`;
}

function element(tag, properties = {}, children = []) {
  const made = Object.assign(document.createElement(tag), properties);
  made.append(...children);
  return made;
}

// The default as a value: a number for number fields, the first colour of a
// written default such as "#d64834, or the theme's"
function defaultOf(option) {
  if (Object.hasOwn(PAGE_DEFAULTS, option.name)) return PAGE_DEFAULTS[option.name];
  const given = option.default;
  if (given === null || given === undefined) return undefined;
  if (option.kind === 'number') {
    const number = Number.parseFloat(given);
    return Number.isFinite(number) ? number : undefined;
  }
  if (option.name === 'route-colour') return /#[0-9a-f]{6}/i.exec(given)?.[0];
  return given;
}

// One control: its element, and its value as the options JSON takes it,
// `undefined` at the default
export class Control {
  constructor(option) {
    this.option = option;
    this.name = option.name;
    this.default = defaultOf(option);
    const id = `option-${option.name}`;
    this.problem = element('p', { className: 'problem', id: `${id}-problem`, hidden: true });
    this.problem.setAttribute('role', 'alert');
    const hint = element('p', { className: 'hint', id: `${id}-hint`, textContent: sentence(HINTS[this.name] ?? option.help) });
    this.element = element('div', { className: 'control' });
    this.element.dataset.option = option.name;
    this.element.dataset.kind = this.#widget();
    const flag = element('code', { className: 'flag', textContent: `--${option.name}` });
    const title = element('span', { className: 'label-text', textContent: label(option.name) });
    if (this.element.dataset.kind === 'choices') {
      const legend = element('legend', {}, [title, ' ', flag]);
      this.inputs = (option.choices ?? []).filter((choice) => choice !== 'none').map((choice) => {
        const box = element('input', { type: 'checkbox', value: choice, name: id });
        return { box, label: element('label', { className: 'choice' }, [box, ` ${choice}`]) };
      });
      const set = element('fieldset', {}, [legend, ...this.inputs.map((input) => input.label)]);
      set.setAttribute('aria-describedby', hint.id);
      this.element.append(set, hint, this.problem);
    } else {
      this.input = this.#input(id);
      this.input.setAttribute('aria-describedby', `${hint.id} ${this.problem.id}`);
      const name = element('label', { htmlFor: id }, [title, ' ', flag]);
      if (this.input.type === 'checkbox') {
        this.element.append(element('label', { className: 'switch' }, [this.input, ' ', title, ' ', flag]), hint, this.problem);
      } else {
        this.element.append(name, this.input, hint, this.problem);
      }
    }
    this.value = undefined;
  }

  // switch, on-off, select, number, choices, colour, time, text or file
  #widget() {
    const { kind, choices = [], open } = this.option;
    if (this.name === 'theme') return 'file';
    if (this.name === 'route-colour') return 'colour';
    if (this.name === 'time') return 'time';
    if (kind === 'switch') return 'switch';
    if (kind === 'choice' && !open && choices.length === 2 && choices.includes('on') && choices.includes('off')) return 'on-off';
    if (kind === 'choice' || this.name === 'aspect') return 'select';
    if (kind === 'list') return 'choices';
    if (kind === 'number') return 'number';
    return 'text';
  }

  #input(id) {
    const option = this.option;
    switch (this.element.dataset.kind) {
      case 'switch':
      case 'on-off':
        return element('input', { type: 'checkbox', id });
      case 'select': {
        const choices = (option.choices ?? []).map((choice) => element('option', { value: choice, textContent: choice }));
        if (this.default === undefined) choices.unshift(element('option', { value: '', textContent: UNSET[this.name] ?? 'None' }));
        return element('select', { id }, choices);
      }
      case 'number': {
        const [min, max] = option.range ?? [];
        const input = element('input', { type: 'number', id, step: WHOLE.includes(option.value_name) ? '1' : 'any' });
        if (min !== undefined) Object.assign(input, { min, max });
        if (this.default === undefined && option.default) input.placeholder = option.default;
        return input;
      }
      case 'colour':
        return element('input', { type: 'color', id });
      case 'time':
        return element('input', { type: 'time', id });
      case 'file':
        return element('input', { type: 'file', id, accept: '.toml,text/plain' });
      default: {
        const input = element('input', { type: 'text', id, spellcheck: false, autocomplete: 'off' });
        input.placeholder = option.default ?? EXAMPLES[this.name] ?? '';
        return input;
      }
    }
  }

  get value() {
    const kind = this.element.dataset.kind;
    let value;
    if (kind === 'switch') value = this.input.checked ? true : undefined;
    else if (kind === 'on-off') value = this.input.checked ? 'on' : 'off';
    else if (kind === 'choices') {
      const picked = this.inputs.filter(({ box }) => box.checked).map(({ box }) => box.value);
      value = picked.length ? picked.join(',') : 'none';
    } else if (kind === 'number') value = this.input.value === '' ? undefined : Number(this.input.value);
    else if (kind === 'file') return undefined;
    else value = this.input.value.trim() || undefined;
    return value === this.default ? undefined : value;
  }

  set value(value) {
    const kind = this.element.dataset.kind;
    const shown = value ?? this.default;
    if (kind === 'switch') this.input.checked = shown === true;
    else if (kind === 'on-off') this.input.checked = (shown ?? 'on') === 'on';
    else if (kind === 'choices') {
      const picked = String(shown ?? '').split(',');
      for (const { box } of this.inputs) box.checked = picked.includes(box.value);
    } else if (kind === 'file') this.input.value = '';
    else this.input.value = shown ?? '';
  }

  // The tool's message under the control, or none
  say(message) {
    this.problem.textContent = message ? sentence(message) : '';
    this.problem.hidden = !message;
  }
}

// The boxes that leave out built-in Overpass servers
export function serverBoxes() {
  const boxes = SERVERS.map((server) => {
    const box = element('input', { type: 'checkbox', checked: true, value: server.name });
    box.dataset.server = server.name;
    const title = element('span', { className: 'label-text', textContent: server.title });
    const runs = element('span', { className: 'hint', textContent: ` ${server.runs}` });
    return { box, label: element('label', { className: 'server' }, [box, ' ', title, runs]) };
  });
  const problem = element('p', { className: 'problem', id: 'servers-problem', hidden: true });
  problem.setAttribute('role', 'alert');
  const set = element('fieldset', { className: 'servers' }, [
    element('legend', { textContent: 'Map data servers' }),
    element('p', { className: 'hint', textContent: 'The page asks these in turn for the map data. Each one left ticked may learn the area around the route.' }),
    ...boxes.map(({ label }) => label),
    problem,
  ]);
  return { set, boxes, problem };
}
