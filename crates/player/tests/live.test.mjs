import { readFileSync } from 'node:fs';
import vm from 'node:vm';
import test from 'node:test';
import assert from 'node:assert/strict';

const source = (name) => readFileSync(new URL(`../web/${name}.js`, import.meta.url), 'utf8');

const makeTimeline = (round, ticks = 4) => ({
  schema: 'mechcore.player',
  producer: 'simulator',
  round,
  ticks,
  ticks_per_second: 20,
  cores: [
    { name: 'Blue', begin: 3000, maximum: 3000, damage: 0, supply: 200, officers: [], tower_cards: { energy: [], blueprints: [] } },
    { name: 'Red', begin: 3000, maximum: 3000, damage: 0, supply: 200, officers: [], tower_cards: { energy: [], blueprints: [] } },
  ],
  rosters: [[], []],
  arsenals: [{ skills: [], equipment: [] }, { skills: [], equipment: [] }],
  score_sources: [],
  field: { half_width: 24000, half_depth: 18000 },
  regions: [],
  units: [],
  buildings: [],
  projectiles: [],
  shields: [],
  terrains: [],
  deployments: [],
  cues: [],
  clips: [],
});

const makeCanvasContext = () => new Proxy({}, {
  get(target, prop) {
    if (prop in target) return target[prop];
    if (prop === 'createRadialGradient' || prop === 'createLinearGradient') {
      return () => ({ addColorStop() {} });
    }
    if (prop === 'createPattern') return () => ({});
    if (prop === 'createImageData') return (w, h) => ({ data: new Uint8ClampedArray(Math.max(1, w * h * 4)) });
    if (prop === 'measureText') return () => ({ width: 20 });
    if (prop === 'getImageData') return () => ({ data: new Uint8ClampedArray(4) });
    return () => {};
  },
});

const makeElement = (tag = 'div', id = '') => {
  const listeners = new Map();
  const children = [];
  const classSet = new Set();
  const attrs = new Map();
  const el = {
    tagName: tag.toUpperCase(),
    id,
    textContent: '',
    value: '0',
    checked: tag === 'input',
    disabled: false,
    hidden: false,
    open: false,
    width: 800,
    height: 600,
    clientWidth: 800,
    clientHeight: 600,
    offsetWidth: 160,
    offsetLeft: 0,
    scrollTop: 0,
    children,
    options: children,
    dataset: {},
    parentElement: null,
    style: {
      values: new Map(),
      setProperty(k, v) { this.values.set(k, String(v)); },
      getPropertyValue(k) { return this.values.get(k) || ''; },
    },
    classList: {
      add(...names) { names.forEach((n) => classSet.add(n)); },
      remove(...names) { names.forEach((n) => classSet.delete(n)); },
      toggle(name, force) {
        if (force === undefined) {
          if (classSet.has(name)) classSet.delete(name);
          else classSet.add(name);
        } else if (force) classSet.add(name);
        else classSet.delete(name);
      },
      contains(name) { return classSet.has(name); },
    },
    setAttribute(k, v) { attrs.set(k, String(v)); },
    getAttribute(k) { return attrs.get(k) ?? null; },
    append(...nodes) {
      for (const n of nodes) {
        if (n && typeof n === 'object') {
          n.parentElement = el;
          children.push(n);
        }
      }
    },
    insertBefore(node) {
      if (node && typeof node === 'object') {
        node.parentElement = el;
        children.push(node);
      }
      return node;
    },
    replaceChildren(...nodes) {
      children.length = 0;
      el.append(...nodes);
    },
    remove() {},
    closest() { return makeElement('div'); },
    contains() { return false; },
    matches() { return false; },
    focus() {},
    querySelector() { return makeElement('div'); },
    querySelectorAll() { return []; },
    cloneNode() { return makeElement(tag, id); },
    getBoundingClientRect() { return { left: 0, top: 0, width: 200, height: 40 }; },
    getContext() { return makeCanvasContext(); },
    addEventListener(type, fn) {
      if (!listeners.has(type)) listeners.set(type, []);
      listeners.get(type).push(fn);
    },
    dispatchEvent(event) {
      for (const fn of listeners.get(event.type) || []) fn(event);
    },
  };
  return el;
};

function bootLivePlayer({ rounds, fetchImpl }) {
  const elements = new Map();
  const getEl = (id) => {
    if (!elements.has(id)) elements.set(id, makeElement('div', id));
    return elements.get(id);
  };
  getEl('timeline').textContent = JSON.stringify({
    live: true,
    rounds: rounds.map((round, index) => ({ index, round: round.round })),
    initial: rounds[0],
  });
  getEl('roster-assets').textContent = JSON.stringify({ units: {}, techs: {}, images: {} });
  getEl('arsenal-assets').textContent = JSON.stringify({ skills: {}, equipment: {}, images: {} });
  getEl('tower-assets').textContent = readFileSync(new URL('../web/tower-cards.json', import.meta.url), 'utf8');
  const optAutoplay = makeElement('input', 'opt-autoplay');
  optAutoplay.checked = true;
  elements.set('opt-autoplay', optAutoplay);
  const speedEl = makeElement('select', 'speed');
  speedEl.append(...['0.5', '1', '2', '4'].map((v) => Object.assign(makeElement('option'), { value: v })));
  speedEl.value = '1';
  elements.set('speed', speedEl);

  const document = {
    activeElement: null,
    documentElement: makeElement('html'),
    body: makeElement('body'),
    getElementById: getEl,
    createElement: (tag) => makeElement(tag),
    addEventListener() {},
  };
  const windowObj = {
    devicePixelRatio: 1,
    innerWidth: 1280,
    innerHeight: 720,
    addEventListener() {},
  };
  let now = 0;
  let animationCallback;
  const context = {
    document,
    window: windowObj,
    fetch: fetchImpl,
    performance: { now: () => (now += 16) },
    requestAnimationFrame: (fn) => { animationCallback = fn; return 0; },
    cancelAnimationFrame: () => {},
    Math,
    Number,
    String,
    Boolean,
    Array,
    Object,
    Map,
    Set,
    Promise,
    RangeError,
    Error,
    Intl,
    Float64Array,
    Uint8ClampedArray,
    JSON,
    structuredClone,
    console: { ...console, error() {} },
    setTimeout,
    clearTimeout,
  };
  windowObj.window = windowObj;

  const scripts = [
    'sprites', 'snapshot', 'terrain', 'skill-icons', 'deployment',
    'lead-in', 'spawn', 'settlement', 'hud', 'roster', 'arsenal', 'player',
  ];
  const bundle = scripts.map((name) => source(name)).join('\n');
  vm.runInNewContext(bundle, context);
  return { player: windowObj.player, elements, getEl, advance: (time) => animationCallback(time) };
}

const deferred = () => {
  let resolve;
  let reject;
  const promise = new Promise((res, rej) => { resolve = res; reject = rej; });
  return { promise, resolve, reject };
};

const flushMicrotasks = async () => {
  for (let i = 0; i < 8; i++) await Promise.resolve();
};

test('live mode starts with round 0, prefetches sequentially, prioritizes on-demand rounds, and switches cached rounds synchronously', async () => {
  const timelines = [makeTimeline(1, 4), makeTimeline(2, 6), makeTimeline(3, 8)];
  const gates = new Map();
  const requests = [];

  const { player, getEl } = bootLivePlayer({
    rounds: timelines,
    fetchImpl: (url) => {
      requests.push(url);
      const gate = deferred();
      if (!gates.has(url)) gates.set(url, []);
      gates.get(url).push(gate);
      return gate.promise;
    },
  });

  await flushMicrotasks();
  assert.equal(player.roundIndex, 0);
  assert.equal(player.data.round, 1);
  assert.equal(getEl('battle-round').textContent, 'Round 1');
  assert.deepEqual(requests, ['/api/round/1']);
  assert.equal(player.rounds[0].round, 1);
  assert.equal(player.rounds[1], null);
  assert.equal(player.rounds[2], null);

  // User jumps directly to round 3 while background prefetch is still blocked on round 2 (/api/round/1).
  const jumpPromise = player.selectRound(2);
  assert.equal(getEl('battle-round').textContent, 'Round 3…');
  assert.deepEqual(requests, ['/api/round/1', '/api/round/2']);

  // On-demand round 3 (/api/round/2) completes first.
  gates.get('/api/round/2').shift().resolve({
    ok: true,
    status: 200,
    json: async () => timelines[2],
  });
  await jumpPromise;
  assert.equal(player.roundIndex, 2);
  assert.equal(player.data.round, 3);
  assert.equal(getEl('battle-round').textContent, 'Round 3');
  assert.equal(getEl('round').value, '2');

  // Releasing background prefetch for /api/round/1 caches round 2 without hijacking the active round or re-fetching /api/round/2.
  gates.get('/api/round/1').shift().resolve({
    ok: true,
    status: 200,
    json: async () => timelines[1],
  });
  await flushMicrotasks();
  assert.equal(player.roundIndex, 2);
  assert.equal(player.rounds[1].round, 2);
  assert.equal(player.rounds[2].round, 3);
  assert.deepEqual(requests, ['/api/round/1', '/api/round/2']);

  // Already-cached rounds switch synchronously with zero additional network calls.
  const syncResult = player.selectRound(1);
  assert.equal(syncResult, undefined);
  assert.equal(player.roundIndex, 1);
  assert.equal(player.data.round, 2);
  assert.equal(getEl('battle-round').textContent, 'Round 2');
  player.selectRound(0);
  assert.equal(player.roundIndex, 0);
  assert.equal(player.data.round, 1);
  assert.deepEqual(requests, ['/api/round/1', '/api/round/2']);
});

test('live mode handles failed round fetch cleanly without breaking current playback', async () => {
  const timelines = [makeTimeline(1, 4), makeTimeline(2, 6)];
  const gates = [];

  const { player, getEl } = bootLivePlayer({
    rounds: timelines,
    fetchImpl: () => {
      const gate = deferred();
      gates.push(gate);
      return gate.promise;
    },
  });

  await flushMicrotasks();
  assert.equal(gates.length, 1);

  const failedSelect = player.selectRound(1);
  assert.equal(getEl('battle-round').textContent, 'Round 2…');
  // Dedupes against the in-flight prefetch request for /api/round/1.
  assert.equal(gates.length, 1);

  gates.shift().resolve({ ok: false, status: 500, json: async () => ({}) });
  await failedSelect;
  assert.equal(player.roundIndex, 0);
  assert.equal(getEl('round').value, '0');
  assert.equal(getEl('battle-round').textContent, 'Round 2 unavailable');
});

test('next-round navigation retries the failed round instead of skipping it', async () => {
  const timelines = [makeTimeline(1), makeTimeline(2)];
  const gates = [];
  const requests = [];
  const { player, getEl } = bootLivePlayer({
    rounds: timelines,
    fetchImpl: (url) => {
      requests.push(url);
      const gate = deferred();
      gates.push(gate);
      return gate.promise;
    },
  });
  const failed = player.selectRound(1);
  gates.shift().resolve({ ok: false, status: 500 });
  await failed;
  assert.equal(player.roundIndex, 0);
  assert.equal(getEl('round').value, '0');
  getEl('next-round').dispatchEvent({ type: 'click' });
  assert.deepEqual(requests, ['/api/round/1', '/api/round/1']);
  gates.shift().resolve({ ok: true, json: async () => timelines[1] });
  await flushMicrotasks();
  assert.equal(player.roundIndex, 1);
});

for (const paused of [false, true]) {
  test(`cancelling a round load preserves ${paused ? 'paused' : 'playing'} state`, async () => {
    const timelines = [makeTimeline(1, 100), makeTimeline(2, 100)];
    const gate = deferred();
    const { player, advance } = bootLivePlayer({
      rounds: timelines,
      fetchImpl: () => gate.promise,
    });
    if (paused) player.pause();
    const pending = player.selectRound(1);
    player.selectRound(0);
    const before = player.playhead;
    advance(1000);
    if (paused) assert.equal(player.playhead, before);
    else assert.ok(player.playhead > before);
    gate.resolve({ ok: true, json: async () => timelines[1] });
    await pending;
    assert.equal(player.roundIndex, 0, 'the cancelled request must not switch rounds later');
    assert.equal(player.rounds[1].round, 2, 'the cancelled request can still populate the cache');
  });
}
