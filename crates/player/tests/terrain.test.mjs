import { readFileSync } from 'node:fs';
import vm from 'node:vm';
import test from 'node:test';
import assert from 'node:assert/strict';

const source = (name) => readFileSync(new URL(`../web/${name}.js`, import.meta.url), 'utf8');
const { TerrainEffects, SnapshotContext } = vm.runInNewContext(
  `${source('snapshot')}\n${source('terrain')}\n({ TerrainEffects, SnapshotContext });`, { Math });

test('a cut mask preserves negative coordinates, row direction and bit 31', () => {
  const f = TerrainEffects.prepare({ x: -1000, z: -2000, radius: 8000,
    grid: { x: -1250, z: -2250, width: 32, height: 2, rows: [0x80000001, 2] } });
  assert.equal(f.cells.length, 3);
  for (const [x, y] of [[-12.5, 22.5], [142.5, 22.5], [-7.5, 17.5]]) {
    assert.ok(TerrainEffects.contains(f, x, y));
  }
  assert.ok(!TerrainEffects.contains(f, -7.5, 22.5));
  assert.ok(!TerrainEffects.contains(f, -12.5, 17.5));
  const c = new SnapshotContext();
  TerrainEffects.path(c, f);
  assert.equal(c.path.filter((s) => s.startsWith('M')).length, 3);
});

test('whole circles use the same five-metre lattice and cell paths as cut masks', () => {
  const f = TerrainEffects.prepare({ x: 0, z: 0, radius: 3000, grid: null });
  assert.equal(f.originX, -27.5);
  assert.equal(f.originZ, -27.5);
  assert.equal(f.cells.length, 112);
  // The native radius-30 template's first row is 0b000011110000.
  assert.equal(f.occupied.has('3,0'), false);
  assert.equal(f.occupied.has('4,0'), true);
  assert.equal(f.occupied.has('7,0'), true);
  assert.equal(f.occupied.has('8,0'), false);
  // Grid corners can extend past the mathematical circle; texture sampling
  // must use that same footprint instead of clipping back to the circle.
  assert.ok(TerrainEffects.contains(f, -9.9, 29.9));
  const c = new SnapshotContext();
  TerrainEffects.path(c, f);
  assert.equal(c.path.filter((s) => s.startsWith('M')).length, 112);
  assert.ok(c.path.every((s) => !s.startsWith('A')));
});

test('circle placement rounds half cells to even on both sides of zero', () => {
  for (const x of [-250, 250]) {
    const f = TerrainEffects.prepare({ x, z: x, radius: 3000, grid: null });
    assert.equal(f.originX, -27.5);
    assert.equal(f.originZ, -27.5);
  }
});

test('an empty recorded mask stays empty, and large circles do not wrap bits', () => {
  const empty = TerrainEffects.prepare({ x: 0, z: 0, radius: 3000,
    grid: { x: -250, z: -250, width: 2, height: 2, rows: [0, 0] } });
  assert.equal(empty.cells.length, 0);
  const large = TerrainEffects.prepare({ x: 0, z: 0, radius: 12000, grid: null });
  assert.ok(TerrainEffects.contains(large, 117.5, -2.5));
  assert.ok(!TerrainEffects.contains(large, 122.5, -2.5));
});

test('all terrain materials export repeatable vector paths with recorded clipping', () => {
  for (const kind of ['oil', 'fire', 'acid', 'fog', 'fog_sand']) {
    const f = TerrainEffects.prepare({ kind, x: 0, z: 0, radius: 3000,
      grid: { x: -250, z: -250, width: 2, height: 2, rows: [3, 1] } });
    const draw = (tick) => {
      const c = new SnapshotContext();
      TerrainEffects.draw(c, f, tick, 20);
      assert.equal(c.stack.length, 0);
      return c.snapshot(kind, 100, 100);
    };
    const first = draw(70);
    draw(100); // A later frame leaves no particles behind when seeking back.
    assert.equal(draw(70), first);
    assert.match(first, /<clipPath/);
    assert.doesNotMatch(first, /<image|NaN|Infinity/);
  }
});

test('fire animates within its footprint using vector flames', () => {
  const f = TerrainEffects.prepare({ kind: 'fire', x: 0, z: 0, radius: 3000, grid: null });
  const draw = (tick) => {
    const c = new SnapshotContext();
    TerrainEffects.draw(c, f, tick, 20);
    return c.snapshot('fire', 100, 100);
  };
  assert.match(draw(70), /rgb\(246,206,132\)/);
  assert.notEqual(draw(70), draw(71));
  assert.equal(draw(70), draw(70));
});

const patch = (kind, x, z, rows, width) => TerrainEffects.prepare({ kind,
  x: 0, z: 0, radius: 3000,
  grid: { x, z, width, height: rows.length, rows } });

test('overlapping patches have one surface and no internal rim', () => {
  const left = patch('oil', 250, 250, [3, 3], 2);
  const right = patch('oil', 750, 250, [3, 3], 2);
  const [union] = TerrainEffects.merge([left, right]);
  assert.equal(union.cells.length, 6); // Two overlapping cells are counted once.
  assert.equal(union.rim.length, 10); // The perimeter of a 3 by 2 rectangle.
  assert.ok(union.rim.every(([x1, y1, x2, y2]) =>
    (x1 === x2 && (x1 === 0 || x1 === 15)) ||
    (y1 === y2 && (y1 === 0 || y1 === -10))));
});

test('the same covered cells draw identically regardless of source decomposition', () => {
  for (const kind of ['oil', 'fire', 'acid', 'fog', 'fog_sand']) {
    const whole = patch(kind, 250, 250, Array(6).fill(255), 8);
    const left = patch(kind, 250, 250, Array(6).fill(31), 5);
    const right = patch(kind, 1750, 250, Array(6).fill(31), 5);
    const draw = (frames) => {
      const c = new SnapshotContext();
      for (const f of TerrainEffects.merge(frames)) TerrainEffects.draw(c, f, 70, 20);
      return c.snapshot(kind, 100, 100);
    };
    assert.equal(draw([left, right]), draw([whole]));
    assert.equal(draw([right, left, right]), draw([whole]));
    draw([]); // Disappearance and seeking back rebuild the same union.
    assert.equal(draw([left, right]), draw([whole]));
  }
});

test('union preserves holes and keeps fire and oil as distinct materials', () => {
  const ring = patch('oil', -750, -750, [7, 5, 7], 3);
  const fire = patch('fire', -750, -750, [7, 7, 7], 3);
  const [oil, flame] = TerrainEffects.merge([fire, ring]);
  assert.equal(oil.kind, 'oil');
  assert.equal(flame.kind, 'fire');
  assert.equal(oil.rim.length, 16); // 12 outside and 4 around the hole.
  assert.ok(!TerrainEffects.contains(oil, -2.5, 2.5));
  assert.ok(TerrainEffects.contains(flame, -2.5, 2.5));
});

test('growing the region leaves existing flame texture in place', () => {
  const first = patch('fire', 250, 250, Array(8).fill(255), 8);
  const second = patch('fire', 4250, 250, Array(8).fill(255), 8);
  const paths = (frames) => {
    const c = new SnapshotContext();
    for (const f of TerrainEffects.merge(frames)) TerrainEffects.draw(c, f, 70, 20);
    return new Set([...c.snapshot('fire', 100, 100).matchAll(
      /<path d="([^"]+)" fill="rgb\(246,206,132\)"/g)].map(m => m[1]));
  };
  const before = paths([first]);
  const after = paths([first, second]);
  assert.ok(before.size > 0);
  assert.ok(after.size > before.size);
  for (const path of before) assert.ok(after.has(path));
});

test('acid, smoke and sand have distinct animated surfaces', () => {
  const surfaces = new Set();
  for (const kind of ['acid', 'fog', 'fog_sand']) {
    const [f] = TerrainEffects.merge([TerrainEffects.prepare({ kind,
      x: 0, z: 0, radius: 5000, grid: null })]);
    const draw = (tick) => {
      const c = new SnapshotContext();
      TerrainEffects.draw(c, f, tick, 20);
      return c.snapshot('terrain', 200, 200);
    };
    assert.notEqual(draw(70), draw(90), kind);
    assert.equal(draw(70), draw(70), kind);
    surfaces.add(draw(70));
  }
  assert.equal(surfaces.size, 3);
});

test('large overlapping sandstorms merge without losing distant cells', () => {
  const first = TerrainEffects.prepare({ kind: 'fog_sand', x: 0, z: 0, radius: 12000, grid: null });
  const second = TerrainEffects.prepare({ kind: 'fog_sand', x: 10000, z: 0, radius: 12000, grid: null });
  const [f] = TerrainEffects.merge([first, second]);
  assert.ok(TerrainEffects.contains(f, -117.5, -2.5));
  assert.ok(TerrainEffects.contains(f, 217.5, -2.5));
  assert.ok(f.cells.length > first.cells.length);
  assert.ok(f.cells.length < first.cells.length + second.cells.length);
  const c = new SnapshotContext();
  TerrainEffects.draw(c, f, 150, 20);
  assert.doesNotMatch(c.snapshot('sand', 1000, 1000), /NaN|Infinity|<image/);
});
