import {readFileSync} from 'node:fs';
import vm from 'node:vm';
import test from 'node:test';
import assert from 'node:assert/strict';
const code=name=>readFileSync(new URL(`../web/${name}.js`,import.meta.url),'utf8');
const {ScoreSettlement,SnapshotContext}=vm.runInNewContext(
  `${code('snapshot')}\n${code('settlement')}\n({ScoreSettlement,SnapshotContext})`,{Math});
const source=(unit,team,score)=>({unit,team,score,x:unit*100,y:0,z:100});
const data={cores:[{damage:25},{damage:104}],score_sources:[source(1,0,100),source(2,0,4),source(3,1,25)]};

test('survivors attack the opposing core in ascending value and settle the exact total',()=>{
  const plan=ScoreSettlement.prepare(data);
  assert.deepEqual(Array.from(plan.packets,p=>p.unit),[2,1,3]);
  assert.deepEqual(Array.from(ScoreSettlement.sample(plan,0).damage),[0,0]);
  const first=plan.packets[0];
  const hit=ScoreSettlement.sample(plan,first.arrive+.18);
  assert.ok(hit.damage[1]>=4 && hit.damage[1]<104);
  assert.deepEqual(Array.from(ScoreSettlement.sample(plan,plan.duration).damage),[25,104]);
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
      [{x:80,y:40},{x:560,y:40}],[{rgb:'61,141,255'},{rgb:'255,79,60'}],640,480);
    assert.equal(c.stack.length,0);
    return c.snapshot('settlement',640,480);
  };
  const flight=render(.8);
  render(plan.duration);
  assert.equal(render(.8),flight);
  assert.notEqual(flight,render(0));
  assert.notEqual(flight,render(plan.packets[0].arrive+.1));
  assert.doesNotMatch(flight,/NaN|Infinity|<image/);
  assert.deepEqual(Array.from(ScoreSettlement.sample(plan,0).damage),[0,0]);
});
