// Canvas semantics needed when the normal battle frame is drawn into SVG.
import { readFileSync } from 'node:fs';
import vm from 'node:vm';
import test from 'node:test';
import assert from 'node:assert/strict';

const SnapshotContext = vm.runInNewContext(`${readFileSync(new URL('../web/snapshot.js', import.meta.url), 'utf8')}\nSnapshotContext;`, { Math });

test('rectangle drawing does not replace the current path', () => {
  const c = new SnapshotContext();
  c.moveTo(0, 0); c.lineTo(10, 0);
  c.fillRect(100, 100, 20, 20);
  c.lineTo(10, 10); c.stroke();
  assert.match(c.body.at(-1), /d="M0 0L10 0L10 10"/);
});

test('save and restore preserve paint, text, blend and dashed stroke state', () => {
  const c = new SnapshotContext();
  c.setLineDash([2, 3]); c.lineCap = 'round'; c.font = '12px sans-serif';
  c.save();
  c.setLineDash([]); c.lineCap = 'butt'; c.font = '30px serif'; c.globalCompositeOperation = 'lighter';
  c.restore();
  c.moveTo(0, 0); c.lineTo(10, 10); c.stroke();
  assert.match(c.body.at(-1), /stroke-linecap="round"/);
  assert.match(c.body.at(-1), /stroke-dasharray="2 3"/);
  assert.doesNotMatch(c.body.at(-1), /mix-blend-mode/);
  assert.equal(c.font, '12px sans-serif');
});

test('ground images stay vector and clip to their canvas bounds', () => {
  const ground = new SnapshotContext();
  ground.width = 100; ground.height = 50; ground.prefix = 'ground-';
  const g = ground.createLinearGradient(0, 0, 0, 50);
  g.addColorStop(0, '#000'); g.addColorStop(1, '#fff');
  ground.fillStyle = g; ground.fillRect(0, 0, 100, 50);
  const c = new SnapshotContext();
  c.drawImage(ground, 0, 0);
  const svg = c.snapshot('frame', 200, 100);
  assert.match(svg, /<linearGradient id="ground-g1"/);
  assert.match(svg, /viewBox="0 0 100 50" overflow="hidden"/);
  assert.doesNotMatch(svg, /<image|data:image/);
});

test('labels and titles are escaped without duplicate style attributes', () => {
  const c = new SnapshotContext();
  c.globalCompositeOperation = 'lighter';
  c.fillText('<unit & "name">', 1, 2);
  assert.equal((c.body[0].match(/style=/g) || []).length, 1);
  assert.match(c.body[0], /&lt;unit &amp; &quot;name&quot;&gt;/);
  assert.match(c.snapshot('<fight>', 100, 100), /<title>&lt;fight&gt;<\/title>/);
});
