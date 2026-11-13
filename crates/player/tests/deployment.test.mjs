import { readFileSync } from 'node:fs';
import vm from 'node:vm';
import test from 'node:test';
import assert from 'node:assert/strict';

const source = (name) => readFileSync(new URL(`../web/${name}.js`, import.meta.url), 'utf8');
const { SkillPreview, LeadIn, SnapshotContext } = vm.runInNewContext(
  `${source('snapshot')}\n${source('skill-icons')}\n${source('deployment')}\n${source('lead-in')}\n({ SkillPreview, LeadIn, SnapshotContext });`, { Math });
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

const beacon = (path, extra={}) => release('mobile_beacon', {
  geometry:'path', path, radius:4000, start_ms:0, interval_ms:0, count:1, ...extra,
});

test('beacon flow follows both ordered segments and persists without an icon or countdown', () => {
  const d=beacon([[0,0],[10000,0],[10000,-10000]]);
  assert.equal(d.routeLength,200);
  const marker = (frame,index) => frame[index].markers.find(p=>p.along>=0 && p.along<46);
  const a=marker(SkillPreview.flow(d,21,20),0), b=marker(SkillPreview.flow(d,25,20),0);
  assert.ok(b.x>a.x);
  const c=marker(SkillPreview.flow(d,21,20),1), e=marker(SkillPreview.flow(d,25,20),1);
  assert.ok(e.y>c.y);
  assert.equal(SkillPreview.state(d,2000,20).countdown,false);
  const draw=t=>{
    const c=new SnapshotContext();
    SkillPreview.draw(c,d,t,20,team);
    assert.equal(c.stack.length,0);
    return c.snapshot('beacon',400,300);
  };
  const initial=draw(21);
  assert.notEqual(initial,draw(31));
  assert.equal(initial,draw(21));
  assert.match(draw(2000),/<path/);
  assert.doesNotMatch(initial,/<image|NaN|Infinity|<ellipse|<circle/);
  // The path has no glyph in NativeSkillIcons; successful rendering must
  // take the path branch without relying on the icon-based release preview.
});

test('beacon legs have full endpoint circles and clip the paired chevron texture', () => {
  const d=beacon([[0,0],[10000,0],[10000,-10000]]);
  const c=new SnapshotContext(), arcs=[];
  const arc=c.arc.bind(c);
  c.arc=(x,y,r,a,b,...rest)=>{
    arcs.push({centre:Array.from(c.point(x,y)),r,span:b-a});
    arc(x,y,r,a,b,...rest);
  };
  SkillPreview.draw(c,d,21,20,team);
  assert.equal(arcs.length,6);
  assert.deepEqual(arcs.map(a=>a.centre),[[100,0],[0,0],[0,0],[100,100],[100,0],[100,100]]);
  for (const a of arcs) {
    assert.equal(a.r,40);
    assert.ok(Math.abs(a.span-Math.PI)<1e-9);
  }
  for (const [x,y] of [[0,0],[100,100]]) {
    const total=arcs.filter(a=>a.centre[0]===x && a.centre[1]===y).reduce((sum,a)=>sum+a.span,0);
    assert.ok(Math.abs(total-Math.PI*2)<1e-9);
  }
  for (const leg of SkillPreview.flow(d,21,20)) {
    for (let i=1;i<leg.markers.length;i++) {
      assert.ok(Math.abs(leg.markers[i].along-leg.markers[i-1].along-46)<1e-9);
    }
  }
  const svg=c.snapshot('beacon',400,300);
  assert.equal((svg.match(/<clipPath /g)||[]).length,2);
  assert.equal(c.stack.length,0);
  assert.doesNotMatch(svg,/NaN|Infinity|<image/);
});

test('beacon paths handle reverse, folded and coincident control points', () => {
  for (const path of [
    [[10000,0],[0,0],[0,10000]],
    [[0,0],[10000,0],[0,0]],
    [[0,0],[0,0],[0,10000]],
    [[0,0],[10000,0],[10000,0]],
    [[0,0],[0,0],[0,0]],
  ]) {
    const d=beacon(path);
    const c=new SnapshotContext();
    SkillPreview.draw(c,d,101,20,{rgb:'255,79,60'});
    const svg=c.snapshot('beacon',400,300);
    assert.doesNotMatch(svg,/NaN|Infinity/);
    if (d.routeLength) assert.match(svg,/rgb\(255,79,60\)/);
    else assert.equal(c.body.length,0);
    assert.equal(c.stack.length,0);
  }
  const backwards=beacon([[10000,0],[0,0],[0,10000]]);
  const marker=(tick,index)=>SkillPreview.flow(backwards,tick,20)[index].markers.find(p=>p.along>=0 && p.along<46);
  assert.ok(marker(25,0).x<marker(21,0).x);
  assert.ok(marker(25,1).y<marker(21,1).y);
});

test('beacon hides after every initially selected unit finishes or dies, including seeking', () => {
  const d=beacon([[0,0],[10000,0],[10000,-10000]]);
  const unit=(id,X,Z,extra={})=>({id,team:0,from:1,to:X.length,n:X.length,
    radius:2,X,Z,L:X.map(()=>100),...extra});
  // Preserve a 10m lateral offset: the final destination is (110,100).
  const finishes=unit(1,[10,110,110,110],[0,0,-60,-100]);
  // This unit dies before reaching either waypoint.
  const dies=unit(2,[0,0,0,0,0],[0,0,0,0,0],{death:{t:5}});
  const later=unit(3,[0,0,0],[0,0,0],{from:2,to:4});
  const enemy=unit(4,[0,0,0],[0,0,0],{team:1});
  const outside=unit(5,[100,100,100],[0,0,0]);
  SkillPreview.bindPaths([d],[finishes,dies,later,enemy,outside]);
  assert.equal(d.endTick,5);
  const draw=t=>{
    const c=new SnapshotContext();
    SkillPreview.draw(c,d,t,20,team);
    return {body:c.body.length,svg:c.snapshot('beacon',400,300)};
  };
  assert.ok(draw(4).body>0);
  assert.equal(draw(5).body,0);
  assert.equal(draw(500).body,0);
  assert.equal(SkillPreview.state(d,5,20),null);
  assert.equal(draw(4).svg,draw(4).svg);
  assert.doesNotMatch(draw(4).svg,/223,244,255/);

  const completion=beacon([[0,0],[10000,0],[10000,-10000]]);
  SkillPreview.bindPaths([completion],[finishes]);
  assert.equal(completion.endTick,4);
  const empty=beacon([[0,0],[10000,0],[10000,-10000]]);
  SkillPreview.bindPaths([empty],[later,enemy,outside]);
  assert.equal(SkillPreview.state(empty,1,20),null);
});

test('beacon keeps unfinished units and assigns overlapping releases only once', () => {
  const path=[[0,0],[10000,0],[0,0]];
  const a=beacon(path), b=beacon(path);
  // Being at the final point initially does not mean the round trip is done.
  const u={id:1,team:0,from:1,to:3,n:3,radius:2,
    X:[0,0,0],Z:[0,0,0],L:[100,100,100]};
  SkillPreview.bindPaths([a,b],[u]);
  assert.equal(a.endTick,Infinity);
  assert.equal(b.endTick,1);
  assert.ok(SkillPreview.state(a,200,20));
  const dead=beacon(path);
  SkillPreview.bindPaths([dead],[{...u,L:[100,0,100]}]);
  assert.equal(dead.endTick,2); // Rebirth does not restore an old instruction.
});

test('buffs and shield airdrop clear their release preview without predicting targets or shields', () => {
  const pictures = new Set();
  for (const [kind, radius, start_ms] of [
    ['electromagnetic_impact',6000,2700], ['electromagnetic_blast',13000,2700],
    ['photon_emission',11000,600], ['shield_airdrop',7000,1000],
  ]) {
    const d = circle(kind, {radius,start_ms});
    const initial = () => {
      const c = new SnapshotContext();
      SkillPreview.draw(c,d,1,20,team);
      assert.equal(c.stack.length,0);
      return c.snapshot(kind,400,300);
    };
    const svg = initial();
    assert.match(svg, /<path/);
    assert.doesNotMatch(svg, /NaN|Infinity|<image|data:image/);
    assert.equal(SkillPreview.state(d,1+start_ms/100,20).progress,0.5);
    const ended = new SnapshotContext();
    SkillPreview.draw(ended,d,start_ms/50+2,20,team);
    SkillPreview.drawStrike(ended,d,start_ms/50+2,20,team);
    assert.equal(ended.body.length,0);
    assert.equal(SkillPreview.strike(d,60,20),null);
    assert.equal(SkillPreview.scatteredStrike(d,60,20),null);
    assert.equal(initial(),svg);
    // Compare only drawing, not the distinct document title.
    pictures.add(svg.replaceAll(kind,'skill'));
  }
  assert.equal(pictures.size,4);
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

test('deployment lead-in runs independent tracks, animates v1 actions cleanly through SnapshotContext, and hands off seamlessly to tick 1', () => {
  const lead = {
    ticks: 90,
    supply: [400, 300],
    formations: [
      { team: 0, index: 0, kind: 'marksman', formation: 10 },
      { team: 0, index: 1, kind: 'crawler', formation: 11, opening: { x: -4000, z: -2000, width: 2000, depth: 800, facing: 0, level: 1 } },
      { team: 1, index: 0, kind: 'fang', formation: 20 },
    ],
    tracks: [
      [
        { start: 8, ticks: 20, type: 'buy_unit', name: 'marksman', spent: 100, supply: 300, changes: [{ formation: 0, to: { x: 0, z: -3000, width: 1000, depth: 400, facing: 0, level: 1 } }] },
        { start: 28, ticks: 16, type: 'move_unit', unit: 'crawler', spent: 0, supply: 300, changes: [{ formation: 1, to: { x: -1000, z: -1500, width: 2000, depth: 800, facing: 0, level: 1 } }] },
        { start: 44, ticks: 16, type: 'upgrade_unit', unit: 'marksman', spent: 100, supply: 200, changes: [{ formation: 0, to: { x: 0, z: -3000, width: 1000, depth: 400, facing: 0, level: 2 } }] },
        { start: 60, ticks: 18, type: 'upgrade_technology', unit: 'marksman', tech: 5, spent: 200, supply: 0 },
      ],
      [
        { start: 8, ticks: 22, type: 'buy_unit', name: 'fang', spent: 100, supply: 200, changes: [{ formation: 2, to: { x: 18000, z: 2000, width: 2000, depth: 800, facing: -900, level: 1 } }] },
        { start: 30, ticks: 10, type: 'choose_reinforce_item', name: 'supply_specialist', spent: -200, supply: 400 },
      ],
    ],
  };
  const units = [
    { id: 1, team: 0, from: 1, formation: 10, kind: 'marksman', radius: 1.5, air: false, X: [0], Y: [0], Z: [-30], B: [0], R: [0] },
    { id: 2, team: 0, from: 1, formation: 11, kind: 'crawler', radius: 1.0, air: false, X: [-12, -8], Y: [0, 0], Z: [-16, -14], B: [0, 0], R: [0, 0] },
    { id: 3, team: 1, from: 1, formation: 20, kind: 'fang', radius: 1.0, air: false, X: [180], Y: [0], Z: [20], B: [-900], R: [-900] },
  ];
  const model = LeadIn.prepare({ lead_in: lead }, units);
  assert.equal(model.active, true);
  assert.equal(model.start, -89);
  assert.equal(LeadIn.sample(model, 1), null);

  // At frame -59 (start of choose_reinforce_item on red), supply updates at midpoint (-54).
  const mid = LeadIn.sample(model, -59);
  assert.equal(mid.playing[0].entry.action, 'move_unit');
  assert.equal(mid.playing[1].entry.action, 'choose_reinforce_item');
  assert.equal(LeadIn.supplyAt(model, mid, 0), 300);
  assert.equal(LeadIn.supplyAt(model, mid, 1), 200);
  assert.equal(LeadIn.supplyAt(model, LeadIn.sample(model, -54), 1), 400);

  assert.equal(LeadIn.describe(model.tracks[1][0], { fang: 'Fang' }, {}).tag, 'FLANK');
  assert.equal(LeadIn.describe(model.tracks[0][2], { marksman: 'Marksman' }, {}).label, 'Marksman Lv.2');
  assert.equal(LeadIn.describe(model.tracks[0][3], { marksman: 'Marksman' }, { 5: { name: 'Range Enhancement' } }).label, 'Marksman · Range Enhancement');
  assert.equal(LeadIn.describe(model.tracks[1][1], {}, {}).placeholder, true);

  // Tick-1 handoff: final stance places tick-1 members identically to their recorded coordinates.
  for (const f of model.formations) {
    const members = LeadIn.membersAt(f, f.last);
    const matching = units.filter((u) => u.formation === (f.team === 0 ? (f.index === 0 ? 10 : 11) : 20));
    members.forEach((m, i) => {
      assert.ok(Math.abs(m.x - matching[i].X[0]) < 1e-6);
      assert.ok(Math.abs(m.y - matching[i].Y[0]) < 1e-6);
      assert.ok(Math.abs(m.z - matching[i].Z[0]) < 1e-6);
      assert.equal(m.body, matching[i].B[0]);
      assert.equal(m.turret, matching[i].R[0]);
    });
  }

  // Draw key moments across both draw and drawOverlay with SnapshotContext.
  for (const frame of [-75, -60, -37, -21, -5]) {
    const c = new SnapshotContext();
    const drawn = [];
    const h = {
      place: () => c.setTransform(1, 0, 0, 1, 200, 200),
      sx: (x) => 200 + x,
      sy: (z) => 200 - z,
      scale: 1,
      ui: 1,
      teams: [{ rgb: '61,141,255', accent: '#60b8ff' }, { rgb: '255,79,60', accent: '#ff7060' }],
      footprints: [{ x0: -500, z0: -3200, x1: 500, z1: -2800 }],
      unit: (f, p, level, alpha, opts) => drawn.push({ f: f.i, level, alpha, opts }),
    };
    const st = LeadIn.sample(model, frame);
    LeadIn.draw(c, st, h, false);
    LeadIn.draw(c, st, h, true);
    LeadIn.drawOverlay(c, st, h);
    const svg = c.snapshot(`deploy-${frame}`, 400, 400);
    assert.equal(c.stack.length, 0);
    assert.ok(drawn.length > 0);
    assert.doesNotMatch(svg, /NaN|Infinity|<image/);
  }
});

test('deployment lead-in tracks per-formation active techs from opening and mid-round research cleanly through SnapshotContext', () => {
  const lead = {
    ticks: 90,
    supply: [300, 300],
    techs: [{ marksman: [101], crawler: [209] }, { fang: [301] }],
    formations: [
      { team: 0, index: 0, kind: 'marksman', formation: 10, opening: { x: 0, z: -3000, width: 1000, depth: 400, facing: 0, level: 1 } },
      { team: 0, index: 1, kind: 'crawler', formation: 11, opening: { x: -4000, z: -2000, width: 2000, depth: 800, facing: 0, level: 1 } },
      { team: 1, index: 0, kind: 'fang', formation: 20 },
    ],
    tracks: [
      [
        { start: 8, ticks: 20, type: 'move_unit', unit: 'crawler', spent: 0, supply: 300, changes: [{ formation: 1, to: { x: 0, z: -1000, width: 2000, depth: 800, facing: 0, level: 1 } }] },
        { start: 28, ticks: 20, type: 'upgrade_unit', unit: 'marksman', spent: 100, supply: 200, changes: [{ formation: 0, to: { x: 0, z: -3000, width: 1000, depth: 400, facing: 0, level: 2 } }] },
        { start: 48, ticks: 20, type: 'upgrade_technology', unit: 'marksman', tech: 102, spent: 200, supply: 0 },
      ],
      [
        { start: 8, ticks: 20, type: 'buy_unit', name: 'fang', spent: 100, supply: 200, changes: [{ formation: 2, to: { x: 6000, z: 2000, width: 2000, depth: 800, facing: 1800, level: 1 } }] },
      ],
    ],
  };
  const units = [
    { id: 1, team: 0, from: 1, formation: 10, kind: 'marksman', radius: 1.5, air: false, X: [0], Y: [0], Z: [-30], B: [0], R: [0] },
    { id: 2, team: 0, from: 1, formation: 11, kind: 'crawler', radius: 1.0, air: false, X: [-2, 2], Y: [0, 0], Z: [-11, -9], B: [0, 0], R: [0, 0] },
    { id: 3, team: 1, from: 1, formation: 20, kind: 'fang', radius: 1.0, air: false, X: [60], Y: [0], Z: [20], B: [1800], R: [1800] },
  ];
  const model = LeadIn.prepare({ lead_in: lead }, units);
  const byIndex = (state, team, index) => state.formations.find((x) => x.f.team === team && x.f.index === index);

  // Initial state: Marksman and Crawler have opening techs positioned near the upper-middle of each formation; Red Fang waits until k >= 0.55.
  const early = LeadIn.sample(model, -78);
  const marksmanEarly = LeadIn.formationTechs(model, early, byIndex(early, 0, 0));
  assert.deepEqual(Array.from(marksmanEarly.items, (x) => [x.id, x.state, x.isNew, x.k]), [[101, 'active', false, 1]]);
  assert.ok(marksmanEarly.anchor.z > -30 && marksmanEarly.anchor.z < -28);
  assert.equal(LeadIn.formationTechs(model, early, byIndex(early, 1, 0)), null);

  // Mid-move: Crawler's opening tech tracks smoothly with the moving formation, and Red Fang reveals its opening tech after k >= 0.55.
  const moving = LeadIn.sample(model, -68);
  const crawlerMoving = LeadIn.formationTechs(model, moving, byIndex(moving, 0, 1));
  assert.ok(crawlerMoving.anchor.x > -40 && crawlerMoving.anchor.x < 0);
  assert.deepEqual(Array.from(crawlerMoving.items, (x) => [x.id, x.state, x.isNew, x.k]), [[209, 'active', false, 1]]);
  const fangReveal = LeadIn.formationTechs(model, moving, byIndex(moving, 1, 0));
  assert.ok(fangReveal && fangReveal.alpha > 0 && fangReveal.alpha < 1);
  assert.deepEqual(Array.from(fangReveal.items, (x) => [x.id, x.state, x.isNew, x.k]), [[301, 'active', false, 1]]);

  // Charge phase (k = 0.25 of upgrade_technology).
  const charging = LeadIn.sample(model, -36);
  const marksmanCharge = LeadIn.formationTechs(model, charging, byIndex(charging, 0, 0));
  assert.equal(marksmanCharge.items[1].id, 102);
  assert.equal(marksmanCharge.items[1].state, 'charging');

  // Ignition phase (k = 0.55 of upgrade_technology): flips into activating and emits vector shockwave paths in SnapshotContext.
  const ignite = LeadIn.sample(model, -30);
  const marksmanIgnite = LeadIn.formationTechs(model, ignite, byIndex(ignite, 0, 0));
  assert.equal(marksmanIgnite.items[1].state, 'activating');
  const c = new SnapshotContext();
  const h = {
    place: () => c.setTransform(1, 0, 0, 1, 200, 200),
    sx: (x) => 200 + x,
    sy: (z) => 200 - z,
    scale: 1,
    ui: 1,
    teams: [{ rgb: '61,141,255', accent: '#60b8ff' }, { rgb: '255,79,60', accent: '#ff7060' }],
    footprints: [{ x0: -500, z0: -3200, x1: 500, z1: -2800 }],
    unit: () => {},
  };
  LeadIn.draw(c, ignite, h, false);
  LeadIn.draw(c, ignite, h, true);
  LeadIn.drawOverlay(c, ignite, h);
  const svg = c.snapshot('tech-ignite', 400, 400);
  assert.equal(c.stack.length, 0);
  assert.match(svg, /<clipPath /);
  assert.doesNotMatch(svg, /NaN|Infinity|<image|data:image/);

  // Settled & outro fade: newly unlocked tech stays active as new, and outro fades down smoothly.
  const afterTech = LeadIn.sample(model, -10);
  const marksmanAfter = LeadIn.formationTechs(model, afterTech, byIndex(afterTech, 0, 0));
  assert.deepEqual(Array.from(marksmanAfter.items, (x) => [x.id, x.state, x.isNew]), [[101, 'active', false], [102, 'active', true]]);
  const endOutro = LeadIn.sample(model, 0);
  assert.equal(LeadIn.formationTechs(model, endOutro, byIndex(endOutro, 0, 0)), null);
});
