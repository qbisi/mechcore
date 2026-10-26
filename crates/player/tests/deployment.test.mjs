import { readFileSync } from 'node:fs';
import vm from 'node:vm';
import test from 'node:test';
import assert from 'node:assert/strict';

const source = (name) => readFileSync(new URL(`../web/${name}.js`, import.meta.url), 'utf8');
const { SkillPreview, SnapshotContext } = vm.runInNewContext(
  `${source('snapshot')}\n${source('skill-icons')}\n${source('deployment')}\n({ SkillPreview, SnapshotContext });`, { Math });
const team = { rgb: '61,141,255' };
const release = (kind = 'sticky_oil_bomb', extra = {}) => SkillPreview.prepare({
  kind, geometry: 'line', team: 0, ordinal: 0, x0: -6000, z0: -4000, x1: 6000, z1: -4000,
  radius: 3000, start_ms: 3000, interval_ms: 200, count: 7, ...extra,
  ...(kind === 'ion_blast' ? { radius: 2000, interval_ms: 300, count: 60, ...extra } : {}),
});
const circle = (kind, extra = {}) => release(kind, {
  geometry:'circle', x0:2000, z0:3000, x1:2000, z1:3000,
  radius:kind==='nuke'?10000:kind==='orbital_javelin'?3000:4000,
  start_ms:kind==='nuke'?15000:3000, interval_ms:0, count:1, ...extra,
});
const scattered = (kind, extra = {}) => circle(kind, {
  radius:13000, interval_ms:kind==='lightning_storm'?400:1000,
  count:kind==='lightning_storm'?30:15, ...extra,
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

test('single impacts use native vector icons inside circles at their configured world radius', () => {
  const icons = new Set();
  for (const kind of ['missile_strike','heavy_missile_strike','nuke','orbital_javelin']) {
    const d = circle(kind);
    assert.equal(d.x,20); assert.equal(d.y,-30);
    const draw = tick => {
      const c = new SnapshotContext();
      SkillPreview.draw(c,d,tick,20,team);
      assert.equal(c.stack.length,0);
      return c.snapshot('single-impact',400,300);
    };
    const initial = draw(1);
    assert.match(initial,new RegExp(`A${d.r} ${d.r}`));
    assert.match(initial,/C/);
    assert.doesNotMatch(initial,/NaN|Infinity|<image|data:image/);
    draw(21); assert.equal(draw(1),initial);
    icons.add(initial);
    assert.equal(SkillPreview.strike(d,100,20),null);
  }
  assert.equal(icons.size,4);
});

test('Nuke retains its longer countdown while the three-second impact previews clear', () => {
  const missile=circle('missile_strike'), nuke=circle('nuke');
  assert.equal(SkillPreview.state(missile,62,20),null);
  assert.ok(SkillPreview.state(nuke,62,20).countdown);
  assert.ok(Math.abs(SkillPreview.state(nuke,151,20).progress-0.5)<1e-9);
  assert.equal(SkillPreview.state(nuke,302,20),null);
  const c=new SnapshotContext();
  SkillPreview.draw(c,nuke,302,20,team);
  assert.equal(c.body.length,0);
});

test('summon symbols preview the native appearance delay and fade as units arrive',()=>{
  const icons=new Set();
  for(const kind of ['underground_threat','rhino_assault','wasp_swarm','mobilize_battleship','vulcans_descent']){
    const d=circle(kind,{radius:2500,start_ms:2300});
    assert.equal(SkillPreview.state(d,21,20).countdown,true);
    assert.equal(SkillPreview.state(d,48,20).countdown,false);
    assert.ok(SkillPreview.state(d,48,20).opacity<1);
    assert.equal(SkillPreview.state(d,52,20),null);
    const c=new SnapshotContext();
    SkillPreview.draw(c,d,21,20,team);
    const svg=c.snapshot('summon',400,300);
    assert.match(svg,/C/);
    assert.doesNotMatch(svg,/NaN|Infinity|<image|data:image/);
    icons.add(svg);
  }
  assert.equal(icons.size,5);
});

test('single-impact contour highlights stay aligned with their countdown arcs', () => {
  for (const kind of ['missile_strike','heavy_missile_strike','nuke','orbital_javelin']) {
    const d=circle(kind);
    for (const progress of [0,0.25,0.5,0.9]) {
      const c=new SnapshotContext(), arcs=[], arc=c.arc.bind(c);
      c.arc=(...args)=>{ arcs.push(args); return arc(...args); };
      SkillPreview.draw(c,d,1+progress*d.start*20,20,team);
      const reflection=arcs.find(a=>a[2]===d.r && Math.abs(a[4]-a[3]-0.3)<1e-9);
      const countdown=arcs.find(a=>a[2]<d.r && a[4]===Math.PI*1.5);
      assert.ok(reflection && countdown);
      assert.ok(Math.abs(reflection[4]-countdown[3])<1e-9);
    }
  }
});

test('large circles add an outer rail without moving their actual range contour', () => {
  for (const radius of [9900,10000,10100]) {
    const d=circle('nuke',{radius}), c=new SnapshotContext(), arcs=[], arc=c.arc.bind(c);
    c.arc=(...args)=>{ arcs.push(args); return arc(...args); };
    SkillPreview.draw(c,d,21,20,team);
    assert.ok(arcs.some(a=>a[2]===d.r && a[3]===0 && a[4]===Math.PI*2));
    assert.equal(arcs.filter(a=>a[2]>d.r).length,radius>=10000?4:0);
  }
});

test('all deployment glyphs and their rings scale in proportion to battlefield zoom', () => {
  const previews=[
    ...['sticky_oil_bomb','incendiary_bomb','acid_blast','smoke_bomb','ion_blast'].map(k=>release(k)),
    ...['missile_strike','heavy_missile_strike','nuke','orbital_javelin'].map(k=>circle(k)),
    ...['orbital_bombardment','lightning_storm'].map(k=>scattered(k)),
  ];
  for (const d of previews) {
    const measure=zoom=>{
      const c=new SnapshotContext(), boxes=[];
      let ringWeight;
      const fill=c.fill.bind(c), stroke=c.stroke.bind(c);
      c.fill=(...args)=>{
        if (c.fillStyle===`rgba(${team.rgb},0.92)`) boxes.push(c.pathBox.slice());
        return fill(...args);
      };
      c.stroke=(...args)=>{
        if (c.strokeStyle===`rgba(${team.rgb},0.35)`) ringWeight=c.lineWidth*c.scaleOf();
        return stroke(...args);
      };
      c.scale(zoom,zoom);
      SkillPreview.draw(c,d,21,20,team,zoom);
      assert.ok(boxes.length>0);
      return [Math.max(...boxes.map(b=>b[2]))-Math.min(...boxes.map(b=>b[0])),
        Math.max(...boxes.map(b=>b[3]))-Math.min(...boxes.map(b=>b[1])),ringWeight];
    };
    const a=measure(1), b=measure(2);
    for (let i=0;i<a.length;i++) assert.ok(Math.abs(b[i]-2*a[i])<1e-9,d.kind);
  }
});

test('random strike previews fade with their first effect and never expose fabricated impact points', () => {
  const icons=new Set();
  for (const kind of ['orbital_bombardment','lightning_storm']) {
    const d=scattered(kind);
    assert.equal(d.r,130);
    assert.equal(d.points.length,1);
    assert.equal(d.points[0].x,d.x);
    assert.equal(d.points[0].y,d.y);
    assert.equal(SkillPreview.state(d,31,20).progress,0.5);
    assert.equal(SkillPreview.state(d,61,20).opacity,1);
    assert.ok(Math.abs(SkillPreview.state(d,63,20).opacity-0.6)<1e-9);
    assert.equal(SkillPreview.state(d,66,20),null);
    assert.equal(SkillPreview.state(d,100,20),null);
    const draw=tick=>{
      const c=new SnapshotContext();
      SkillPreview.draw(c,d,tick,20,team);
      assert.equal(c.stack.length,0);
      return c.snapshot('random-strike',500,400);
    };
    const before=draw(31);
    assert.match(before,/A130 130/);
    assert.doesNotMatch(before,/NaN|Infinity|<image|data:image/);
    assert.notEqual(draw(63),before);
    draw(100);
    assert.equal(draw(31),before);
    icons.add(before);
    const ended=new SnapshotContext();
    SkillPreview.draw(ended,d,66,20,team);
    assert.equal(ended.body.length,0);
    assert.equal(SkillPreview.strike(d,100,20),null);
  }
  assert.equal(icons.size,2);
});

test('seed-reconstructed impacts use their exact ticks and positions with deterministic effects', () => {
  const images=new Set();
  for (const kind of ['orbital_bombardment','lightning_storm']) {
    const d=scattered(kind,{forecast:{radius:3000,points:[
      {tick:63,x:0,z:5000},{tick:83,x:7000,z:-4000},
    ]}});
    assert.equal(SkillPreview.scatteredStrike(d,62,20),null);
    assert.equal(SkillPreview.state(d,62,20).countdown,true);
    assert.equal(SkillPreview.state(d,63,20).countdown,false);
    const first=SkillPreview.scatteredStrike(d,63,20);
    assert.equal(first.radius,30);
    assert.equal(first.flashes.length,1);
    assert.equal(first.flashes[0].x,0);
    assert.equal(first.flashes[0].y,-50);
    assert.equal(first.flashes[0].age,0);
    assert.equal(SkillPreview.scatteredStrike(d,82,20),null);
    const next=SkillPreview.scatteredStrike(d,83,20);
    assert.equal(next.flashes[0].x,70);
    assert.equal(next.flashes[0].y,40);
    const draw=tick=>{
      const c=new SnapshotContext();
      SkillPreview.drawStrike(c,d,tick,20,team);
      assert.equal(c.stack.length,0);
      return c.snapshot('planned strike',500,400);
    };
    const initial=draw(63);
    assert.match(initial,/A30 30/);
    assert.doesNotMatch(initial,/NaN|Infinity|<image|data:image/);
    assert.notEqual(draw(83),initial);
    draw(100);
    assert.equal(draw(63),initial);
    images.add(initial);
    const ended=new SnapshotContext();
    SkillPreview.drawStrike(ended,d,100,20,team);
    assert.equal(ended.body.length,0);
  }
  assert.equal(images.size,2);
});
