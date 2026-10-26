import { readFileSync } from 'node:fs';
import vm from 'node:vm';
import test from 'node:test';
import assert from 'node:assert/strict';

const source = (name) => readFileSync(new URL(`../web/${name}.js`, import.meta.url), 'utf8');
const { SkillPreview, SnapshotContext } = vm.runInNewContext(
  `${source('snapshot')}\n${source('deployment')}\n({ SkillPreview, SnapshotContext });`, { Math });
const team = { rgb: '61,141,255' };
const release = (kind = 'sticky_oil_bomb', extra = {}) => SkillPreview.prepare({
  kind, team: 0, ordinal: 0, x0: -6000, z0: -4000, x1: 6000, z1: -4000,
  radius: 3000, start_ms: 3000, interval_ms: 200, count: 7, ...extra,
  ...(kind === 'ion_blast' ? { radius: 2000, interval_ms: 300, count: 60, ...extra } : {}),
});

test('the footprint follows world coordinates and its configured radius', () => {
  const short = release();
  const long = release('sticky_oil_bomb', { x0: -12000, x1: 12000 });
  assert.equal(short.x, 0); assert.equal(short.y, 40);
  assert.equal(short.length, 180); assert.equal(long.length, 300);
  assert.equal(short.r, long.r);
  const upright = release('acid_blast', { x0: 0, z0: 0, x1: 0, z1: 12000 });
  assert.equal(upright.y, -60);
  assert.equal(upright.angle, -Math.PI / 2);
});

test('countdown and whole-view cleanup follow configured times', () => {
  const d = release();
  assert.equal(SkillPreview.state(d, 1, 20).progress, 0);
  assert.equal(SkillPreview.state(d, 31, 20).progress, 0.5);
  assert.equal(SkillPreview.state(d, 61, 20).countdown, false);
  assert.ok(SkillPreview.state(d, 89, 20));
  assert.equal(SkillPreview.state(d, 90, 20), null);
  assert.equal(SkillPreview.state(d, 0, 20), null);
});

test('five glyphs draw distinct, repeatable SVG without bitmap assets', () => {
  const pictures = new Set();
  for (const kind of ['sticky_oil_bomb', 'incendiary_bomb', 'acid_blast', 'smoke_bomb', 'ion_blast']) {
    const d = release(kind);
    const draw = (tick) => {
      const c = new SnapshotContext();
      SkillPreview.draw(c, d, tick, 20, team);
      assert.equal(c.stack.length, 0);
      return c.snapshot('deployment', 400, 300);
    };
    const initial = draw(1);
    draw(50);
    assert.equal(draw(1), initial);
    assert.notEqual(draw(31), initial);
    assert.doesNotMatch(initial, /NaN|Infinity|<image|data:image/);
    pictures.add(initial);
    const ended = new SnapshotContext();
    SkillPreview.draw(ended, d, Math.ceil((d.start+d.interval*d.count)*20)+2, 20, team);
    assert.equal(ended.body.length, 0);
  }
  assert.equal(pictures.size, 5);
});

test('zero-length control segments stay finite and team tint is supplied by the page', () => {
  const d = release('smoke_bomb', { x0: 0, x1: 0 });
  const c = new SnapshotContext();
  SkillPreview.draw(c, d, 1, 20, { rgb: '255,79,60' });
  const svg = c.snapshot('red', 300, 300);
  assert.match(svg, /rgb\(255,79,60\)/);
  assert.doesNotMatch(svg, /NaN|Infinity/);
});

test('the rounded band spans the impact segment plus one terrain radius at either end', () => {
  const d = release();
  assert.equal(d.points.length, 7);
  assert.deepEqual(Array.from(d.points, p => p.x), [-60, -40, -20, 0, 20, 40, 60]);
  assert.equal(d.x-d.length/2, -90);
  assert.equal(d.x+d.length/2, 90);
  assert.equal(d.y-d.r, 10);
  assert.equal(d.y+d.r, 70);
  const reverse = release('sticky_oil_bomb', { x0: 6000, x1: -6000 });
  assert.equal(reverse.length, d.length);
});

test('contour reflection follows forward, reversed and diagonal release order', () => {
  for (const [extra, dx, dy] of [
    [{}, 1, 0],
    [{ x0: 6000, x1: -6000 }, -1, 0],
    [{ x0: 0, z0: 0, x1: 6000, z1: 6000 }, 1, -1],
  ]) {
    const c = new SnapshotContext();
    const gradients = [];
    const linear = c.createLinearGradient.bind(c);
    c.createLinearGradient = (...v) => {
      gradients.push([...c.point(v[0],v[1]),...c.point(v[2],v[3])]);
      return linear(...v);
    };
    const d = release('sticky_oil_bomb', extra);
    SkillPreview.draw(c, d, 10, 20, team);
    SkillPreview.draw(c, d, 20, 20, team);
    const centre = g => [(g[0]+g[2])/2, (g[1]+g[3])/2];
    const a = centre(gradients[0]), b = centre(gradients[1]);
    const sign = v => Math.abs(v) < 1e-9 ? 0 : Math.sign(v);
    assert.equal(sign(b[0]-a[0]), dx);
    assert.equal(sign(b[1]-a[1]), dy);
  }
});

test('the Ion forecast follows its configured cadence and clears its final residual', () => {
  const d = release('ion_blast');
  assert.equal(SkillPreview.strike(d,60,20), null);
  const first = SkillPreview.strike(d,61,20);
  assert.equal(first.cursor.x,-60);
  assert.equal(first.flashes[0].age,0);
  const next = SkillPreview.strike(d,67,20);
  assert.ok(Math.abs(next.flashes[0].x-(-60+120/59))<1e-9);
  assert.ok(next.flashes[0].age<1e-9);
  const between = SkillPreview.strike(d,70,20);
  assert.ok(between.cursor.x>next.cursor.x);
  assert.ok(between.cursor.x<d.points[2].x);
  const last = SkillPreview.strike(d,415,20);
  assert.ok(Math.abs(last.cursor.x-60)<1e-9);
  assert.ok(SkillPreview.strike(d,420,20).fade<1);
  assert.equal(SkillPreview.strike(d,427,20),null);
  assert.equal(SkillPreview.strike(release(),70,20),null);
});

test('Ion cursor and flashes preserve reverse direction and seek deterministically', () => {
  const d = release('ion_blast',{ x0:6000,x1:-6000 });
  const first = SkillPreview.strike(d,61,20);
  const next = SkillPreview.strike(d,67,20);
  assert.ok(next.cursor.x<first.cursor.x);
  const draw = tick => {
    const c = new SnapshotContext();
    SkillPreview.draw(c,d,tick,20,team);
    SkillPreview.drawStrike(c,d,tick,20,team);
    assert.equal(c.stack.length,0);
    return c.snapshot('Ion forecast',400,300);
  };
  const early = draw(67);
  assert.notEqual(draw(70),early);
  draw(350);
  assert.equal(draw(67),early);
  assert.doesNotMatch(early,/NaN|Infinity|<image|data:image/);
  const ended = new SnapshotContext();
  SkillPreview.drawStrike(ended,d,427,20,team);
  assert.equal(ended.body.length,0);
});

test('coincident Ion endpoints retain a finite circular forecast', () => {
  const d = release('ion_blast',{ x0:0,z0:0,x1:0,z1:0 });
  const s = SkillPreview.strike(d,100,20);
  assert.equal(s.cursor.x,0); assert.equal(s.cursor.y,0);
  const c = new SnapshotContext();
  SkillPreview.drawStrike(c,d,100,20,{rgb:'255,79,60'},2);
  const svg = c.snapshot('stationary Ion',300,300);
  assert.match(svg,/rgb\(255,79,60\)/);
  assert.doesNotMatch(svg,/NaN|Infinity/);
});
