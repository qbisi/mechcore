import {readFileSync} from 'node:fs';
import vm from 'node:vm';
import test from 'node:test';
import assert from 'node:assert/strict';
const code=name=>readFileSync(new URL(`../web/${name}.js`,import.meta.url),'utf8');
const {ScoreSettlement,SnapshotContext}=vm.runInNewContext(
  `${code('snapshot')}\n${code('settlement')}\n({ScoreSettlement,SnapshotContext})`,{Math});
const source=(unit,team,score)=>({unit,team,score,x:unit*100,y:0,z:100});
const data={cores:[{damage:25},{damage:104}],score_sources:[source(1,0,100),source(2,0,4),source(3,1,25)]};
const close=(a,b)=>assert.ok(Math.abs(a-b)<1e-9,`${a} != ${b}`);

test('survivors attack the opposing core in ascending value and settle the exact total',()=>{
  const plan=ScoreSettlement.prepare(data);
  assert.deepEqual(Array.from(plan.packets,p=>p.unit),[2,1,3]);
  assert.deepEqual(Array.from(ScoreSettlement.sample(plan,0).damage),[0,0]);
  const first=plan.packets[0];
  assert.deepEqual(Array.from(ScoreSettlement.sample(plan,first.arrive-1e-6).damage),[0,0]);
  assert.deepEqual(Array.from(ScoreSettlement.sample(plan,first.arrive).damage),[0,4]);
  const hit=ScoreSettlement.sample(plan,first.arrive+.18);
  assert.ok(hit.damage[1]>=4 && hit.damage[1]<104);
  assert.deepEqual(Array.from(ScoreSettlement.sample(plan,plan.duration).damage),[25,104]);
});

test('the schedule is the native score attack: lead-in, spacing, flight and tail',()=>{
  const plan=ScoreSettlement.prepare(data);
  const [small,large,red]=plan.packets;
  // Blue's total is 104: each launch waits min(0.9, score * min(0.01, 3/104)).
  close(small.launch,.3+.04);
  close(large.launch,.3+.04+.9);
  close(red.launch,.3+.25);
  for(const p of plan.packets)close(p.arrive-p.launch,.8+.4*p.score/400);
  close(plan.duration,large.arrive+1.1);
});

test('unverified attribution falls back to an explicit team total without changing damage',()=>{
  const plan=ScoreSettlement.prepare({...data,score_sources:[source(1,0,99)]});
  assert.equal(plan.packets.length,2);
  assert.ok(plan.packets.every(p=>p.unit===null));
  assert.deepEqual(Array.from(ScoreSettlement.sample(plan,plan.duration).damage),[25,104]);
  const empty=ScoreSettlement.prepare({cores:[{damage:0},{damage:0}],score_sources:[]});
  assert.equal(empty.duration,0);
  assert.equal(empty.packets.length,0);
});

test('score flights and hits seek and export deterministically, without advancing combat',()=>{
  const plan=ScoreSettlement.prepare(data);
  const render=time=>{
    const c=new SnapshotContext();
    ScoreSettlement.draw(c,plan,time,p=>({x:p.x/100+200,y:300-p.z/100}),
      [{x:40,y:30,w:160,h:20},{x:440,y:30,w:160,h:20}],[{rgb:'61,141,255'},{rgb:'255,79,60'}],640,480);
    assert.equal(c.stack.length,0);
    return c.snapshot('settlement',640,480);
  };
  const flight=render(.8);
  for(const time of [.1,.6,1.2,plan.packets[0].arrive+.1,plan.duration])
    assert.doesNotMatch(render(time),/NaN|Infinity|<image/);
  assert.equal(render(.8),flight);
  assert.notEqual(flight,render(0));
  assert.notEqual(flight,render(plan.packets[0].arrive+.1));
  assert.match(render(plan.packets[0].arrive+.3),/−4</);
  assert.equal(render(plan.duration),render(0),'the tail ends after every effect and hit label has faded');
  assert.deepEqual(Array.from(ScoreSettlement.sample(plan,0).damage),[0,0]);
});
