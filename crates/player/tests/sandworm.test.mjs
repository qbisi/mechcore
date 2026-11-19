import { readFileSync } from 'node:fs';
import vm from 'node:vm';
import test from 'node:test';
import assert from 'node:assert/strict';

const source = (name) => readFileSync(new URL(`../web/${name}.js`, import.meta.url), 'utf8');

function loadSprites() {
  return vm.runInNewContext(
    `${source('sandworm-motion')}\n${source('snapshot')}\n${source('sprites')}\n({ Sprites, SnapshotContext });`,
    { Math },
  );
}

function makeCanvasContext() {
  return new Proxy({}, {
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
}

function makeElement(tag = 'div', id = '') {
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
}

function bootPlayer(timeline) {
  const elements = new Map();
  const getEl = (id) => {
    if (!elements.has(id)) elements.set(id, makeElement('div', id));
    return elements.get(id);
  };
  getEl('timeline').textContent = JSON.stringify(timeline);
  getEl('roster-assets').textContent = JSON.stringify({ units: {}, techs: {}, images: {} });
  getEl('arsenal-assets').textContent = JSON.stringify({ skills: {}, equipment: {}, images: {} });
  getEl('tower-assets').textContent = readFileSync(new URL('../web/tower-cards.json', import.meta.url), 'utf8');
  const optAutoplay = makeElement('input', 'opt-autoplay');
  optAutoplay.checked = false;
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
  const context = {
    document,
    window: windowObj,
    fetch: () => Promise.reject(new Error('unexpected fetch')),
    performance: { now: () => (now += 16) },
    requestAnimationFrame: () => 0,
    cancelAnimationFrame: () => {},
    Math, Number, String, Boolean, Array, Object, Map, Set, Promise,
    RangeError, Error, Intl, Float64Array, Uint8ClampedArray, JSON, structuredClone,
    console: { ...console, error() {} },
    setTimeout, clearTimeout,
  };
  windowObj.window = windowObj;
  const scripts = [
    'sandworm-motion', 'sprites', 'snapshot', 'terrain', 'skill-icons', 'deployment',
    'lead-in', 'spawn', 'settlement', 'hud', 'roster', 'arsenal', 'player',
  ];
  vm.runInNewContext(scripts.map((name) => source(name)).join('\n'), context);
  return windowObj.player;
}

function encodeConst(value, count) {
  return count > 1 ? [value, String(count - 1)] : [value];
}

function encodeSeq(values) {
  const out = [];
  let prev = 0;
  let run = 0;
  for (const v of values) {
    if (v === prev) {
      run++;
    } else {
      if (run > 0) { out.push(String(run)); run = 0; }
      out.push(v - prev);
      prev = v;
    }
  }
  if (run > 0) out.push(String(run));
  return out;
}

test('sandworm and larva sprites animate burrow, underground mound, emerge, and blended strikes cleanly', () => {
  const { Sprites, SnapshotContext } = loadSprites();
  Sprites.setScale(0.1);

  const renderPose = (kind, setup) => {
    const c = new SnapshotContext();
    const p = Sprites.pose();
    setup(p);
    Sprites.drawUnit(c, kind, p, kind === 'larva' ? 6 : 12, 1);
    assert.equal(c.stack.length, 0);
    const svg = c.snapshot(`${kind}`, 300, 300);
    assert.doesNotMatch(svg, /NaN|Infinity|<image|data:image/);
    return { svg, box: c.box.slice() };
  };

  for (const kind of ['sandworm', 'larva']) {
    const s = kind === 'larva' ? 0.5 : 1;
    const rest = renderPose(kind, () => {});
    const b0 = renderPose(kind, (p) => { p.burrow = 0; p.crater = 1; });
    const bMid = renderPose(kind, (p) => { p.burrow = 0.35; p.crater = 1; });
    const bDeep = renderPose(kind, (p) => { p.burrow = 0.7; p.crater = 1; });
    const bEnd = renderPose(kind, (p) => { p.burrow = 0.95; p.crater = 1; });
    const ug = renderPose(kind, (p) => { p.underground = 1; p.speed = 12; p.walk = 8; });
    const emLow = renderPose(kind, (p) => { p.emerge = 0.1; p.crater = 1; });
    const emMid = renderPose(kind, (p) => { p.emerge = 0.5; p.crater = 1; });
    const emEnd = renderPose(kind, (p) => { p.emerge = 0.85; p.crater = 1; });
    const gNear = renderPose(kind, (p) => { p.strike = 0.49; p.strikeReach = 0; p.crater = 1; });
    const gFar = renderPose(kind, (p) => { p.strike = 0.49; p.strikeReach = 1; p.crater = 1; });
    const aNear = renderPose(kind, (p) => { p.strike = 0.49; p.strikeReach = 0; p.strikeAir = true; p.crater = 1; });
    const aFar = renderPose(kind, (p) => { p.strike = 0.49; p.strikeReach = 1; p.strikeAir = true; p.crater = 1; });

    assert.equal(new Set([bMid.svg, bDeep.svg, bEnd.svg, ug.svg, emLow.svg, emMid.svg, emEnd.svg]).size, 7);
    assert.notEqual(rest.svg, b0.svg);

    // Near vs far strike forward extent
    assert.ok(gFar.box[1] < -45 * s, `${kind} ground_far minY ${gFar.box[1]}`);
    assert.ok(gNear.box[1] > -30 * s, `${kind} ground_near minY ${gNear.box[1]}`);
    assert.ok(aFar.box[1] < -50 * s, `${kind} air_far minY ${aFar.box[1]}`);
    assert.notEqual(gFar.svg, aFar.svg);
    assert.notEqual(gNear.svg, aNear.svg);

    // Underground representation collapses into a compact soil mound rather than the long surface hull
    const restH = rest.box[3] - rest.box[1];
    const ugH = ug.box[3] - ug.box[1];
    const b0H = b0.box[3] - b0.box[1];
    const bEndH = bEnd.box[3] - bEnd.box[1];
    assert.ok(ugH < restH * 0.45, `${kind} ug height ${ugH} vs rest ${restH}`);
    assert.ok(b0H > ugH * 1.8, `${kind} b0 height ${b0H} vs ug ${ugH}`);
    assert.ok(bEndH < b0H * 0.5, `${kind} bEnd height ${bEndH} vs b0 ${b0H}`);
  }

  const worm = renderPose('sandworm', () => {});
  const larva = renderPose('larva', () => {});
  const ratioW = (larva.box[2] - larva.box[0]) / (worm.box[2] - worm.box[0]);
  const ratioH = (larva.box[3] - larva.box[1]) / (worm.box[3] - worm.box[1]);
  assert.ok(Math.abs(ratioW - 0.5) < 0.03, `larva width ratio ${ratioW}`);
  assert.ok(Math.abs(ratioH - 0.5) < 0.03, `larva height ratio ${ratioH}`);
});

function svgBody(svg) {
  return svg.slice(svg.indexOf('</defs>'));
}

function shapeElements(svg) {
  return svgBody(svg).match(/<(?:path|ellipse|rect|circle)\b[^>]*>/g) || [];
}

test('player drives timeline underground transitions, 50m splash reach, wakes, and layer order', () => {
  const N = 110;
  const wormMo = Array(N).fill(0);
  const wormV = Array(N).fill(0);
  const wormZ = Array(N).fill(0);
  // Ticks 1..20 (idx 0..19): 20-tick burrow (Normal -> Hide at tick 11 / idx 10)
  for (let i = 0; i < 20; i++) { wormMo[i] = 4; wormV[i] = i < 10 ? 0 : 3; }
  // Ticks 21..40 (idx 20..39): moving underground (+z)
  for (let i = 20; i < 40; i++) {
    wormMo[i] = 1;
    wormV[i] = 3;
    wormZ[i] = (i - 19) * 200; // +2 m/tick -> +40 m
  }
  for (let i = 40; i < N; i++) wormZ[i] = 4000;
  // Ticks 41..70 (idx 40..69): 30-tick emerge
  for (let i = 40; i < 70; i++) { wormMo[i] = 4; wormV[i] = i < 60 ? 3 : 0; }
  for (let i = 72; i < N; i++) wormMo[i] = 2;

  const wormAim = Array(N).fill(0);
  wormAim[74] = 3; // primary target at t=75 is unit #3
  wormAim[84] = 4; // primary target at t=85 is air unit #4

  // Larva #5: 15-tick Strike emerge on ticks 1..15, attacks at 17, truncated burrow on ticks 19..24
  const L = 24;
  const larvaMo = Array(L).fill(0);
  const larvaV = Array(L).fill(0);
  for (let i = 0; i < 15; i++) { larvaMo[i] = 4; larvaV[i] = i < 10 ? 3 : 0; }
  for (let i = 15; i < 18; i++) larvaMo[i] = 2;
  for (let i = 18; i < L; i++) { larvaMo[i] = 4; larvaV[i] = i >= 22 ? 3 : 0; }

  const makeTimeline = (movingZ, extraUnits = []) => ({
    schema: 'mechcore.player',
    producer: 'simulator',
    round: 1,
    ticks: N,
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
    units: [
      {
        id: 1, team: 0, kind: 'sandworm', air: false, radius: 1200, max_life: 5000, from: 1, to: N,
        x: encodeConst(0, N), y: encodeConst(0, N), z: encodeSeq(movingZ), body: encodeConst(0, N),
        life: encodeConst(5000, N), motion: encodeSeq(wormMo), aim: encodeSeq(wormAim), visibility: encodeSeq(wormV),
        underground: {
          transitions: [
            { kind: 'burrow', from: 1, to: 21, duration: 20 },
            { kind: 'emerge', from: 41, to: 71, duration: 30 },
          ],
        },
      },
      ...extraUnits,
    ],
    buildings: [],
    projectiles: [],
    shields: [],
    terrains: [],
    deployments: [],
    cues: [
      { t: 75, k: 'hit', by: 'u1', at: 'u2', slot: 0, n: 500, splash: 1200 },
      { t: 75, k: 'hit', by: 'u1', at: 'u3', slot: 0, n: 500, splash: 1200 },
      { t: 85, k: 'hit', by: 'u1', at: 'u4', slot: 0, n: 500, splash: 1200 },
      { t: 17, k: 'hit', by: 'u5', at: 'u7', slot: 0, n: 200, splash: 600 },
    ],
    clips: [],
  });

  const extraUnits = [
    {
      id: 2, team: 1, kind: 'crawler', air: false, radius: 150, max_life: 300, from: 1, to: N,
      x: encodeConst(500, N), y: encodeConst(0, N), z: encodeConst(4000, N), body: encodeConst(1800, N),
      life: encodeConst(300, N), motion: encodeConst(2, N), aim: encodeConst(1, N),
    },
    {
      id: 3, team: 1, kind: 'sledgehammer', air: false, radius: 400, max_life: 2000, from: 1, to: N,
      x: encodeConst(0, N), y: encodeConst(0, N), z: encodeConst(8000, N), body: encodeConst(1800, N),
      life: encodeConst(2000, N), motion: encodeConst(2, N), aim: encodeConst(1, N),
    },
    {
      id: 4, team: 1, kind: 'phoenix', air: true, radius: 500, max_life: 1500, from: 1, to: N,
      x: encodeConst(0, N), y: encodeConst(2500, N), z: encodeConst(6000, N), body: encodeConst(1800, N),
      life: encodeConst(1500, N), motion: encodeConst(2, N), aim: encodeConst(1, N),
    },
    {
      id: 5, team: 0, kind: 'larva', air: false, radius: 600, max_life: 2000, from: 1, to: L,
      x: encodeConst(-3000, L), y: encodeConst(0, L), z: encodeConst(0, L), body: encodeConst(0, L),
      life: encodeConst(2000, L), motion: encodeSeq(larvaMo), aim: encodeConst(0, L), visibility: encodeSeq(larvaV),
      underground: {
        transitions: [
          { kind: 'emerge', from: 1, to: 16, duration: 15 },
          { kind: 'burrow', from: 19, to: 25, duration: 20 },
        ],
      },
    },
    {
      id: 6, team: 1, kind: 'rhino', air: false, radius: 700, max_life: 4000, from: 1, to: N,
      x: encodeConst(0, N), y: encodeConst(0, N), z: encodeConst(3000, N), body: encodeConst(1800, N),
      life: encodeConst(4000, N), motion: encodeConst(0, N), aim: encodeConst(0, N),
    },
    {
      id: 7, team: 1, kind: 'fang', air: false, radius: 200, max_life: 400, from: 1, to: N,
      x: encodeConst(-3000, N), y: encodeConst(0, N), z: encodeConst(2000, N), body: encodeConst(1800, N),
      life: encodeConst(400, N), motion: encodeConst(0, N), aim: encodeConst(0, N),
    },
  ];

  const player = bootPlayer(makeTimeline(wormZ, extraUnits));
  const worm = player.units[0];
  const larva = player.units[4];

  assert.equal(worm.underground.length, 2);
  assert.equal(larva.underground.length, 2);

  // Both Sandworm and Larva normalize lunge reach against 50 m attackMaxDistance
  assert.equal(worm.strikeMeta.length, 2);
  assert.ok(Math.abs(worm.strikeMeta[0].reach - 0.8) < 1e-6);
  assert.equal(worm.strikeMeta[0].air, false);
  assert.ok(Math.abs(worm.strikeMeta[1].reach - 0.4) < 1e-6);
  assert.equal(worm.strikeMeta[1].air, true);
  assert.equal(larva.strikeMeta.length, 1);
  assert.ok(Math.abs(larva.strikeMeta[0].reach - 0.4) < 1e-6, `larva reach ${larva.strikeMeta[0].reach}`);

  const stageSvgs = [1, 12, 35, 50, 71, 74, 76, 85].map((tick) => {
    const svg = player.snapshot(tick);
    assert.match(svg, /^<svg /);
    assert.doesNotMatch(svg, /NaN|Infinity|<image|data:image/);
    return svg;
  });
  assert.equal(new Set(stageSvgs).size, stageSvgs.length);

  // Moving underground emits wake primitives that fade out after 2.5 s of travel
  const movingSolo = bootPlayer(makeTimeline(wormZ));
  const stationarySolo = bootPlayer(makeTimeline(Array(N).fill(0)));
  const movingWakeShapes = shapeElements(movingSolo.snapshot(35));
  const stationaryShapes = shapeElements(stationarySolo.snapshot(35));
  assert.ok(
    movingWakeShapes.length > stationaryShapes.length,
    `expected moving wakes (${movingWakeShapes.length} > ${stationaryShapes.length})`,
  );
  // By tick 92 (52 ticks = 2.6 s after moving stopped at tick 40), wakes have fully faded
  assert.equal(
    shapeElements(movingSolo.snapshot(92)).length,
    shapeElements(stationarySolo.snapshot(92)).length,
  );

  // Underground mound & wake render before overlapping surface units in SVG document order
  const rhinoSolo = bootPlayer({
    ...makeTimeline(wormZ),
    units: [extraUnits[4]],
    cues: [],
  });
  const combinedShapes = shapeElements(player.snapshot(35));
  const rhinoShapes = new Set(shapeElements(rhinoSolo.snapshot(35)));
  const wormSet = new Set(movingWakeShapes);
  const lastWormOnly = combinedShapes.reduce((last, s, idx) => (!rhinoShapes.has(s) && wormSet.has(s) ? idx : last), -1);
  const firstRhinoOnly = combinedShapes.findIndex((s) => rhinoShapes.has(s) && !wormSet.has(s));
  assert.ok(lastWormOnly >= 0 && firstRhinoOnly > lastWormOnly, `layering order: worm=${lastWormOnly}, rhino=${firstRhinoOnly}`);
});
