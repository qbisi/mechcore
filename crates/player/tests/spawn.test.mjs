import {readFileSync} from 'node:fs';
import vm from 'node:vm';
import test from 'node:test';
import assert from 'node:assert/strict';

const source=name=>readFileSync(new URL(`../web/${name}.js`,import.meta.url),'utf8');
const {SpawnEffects,SnapshotContext}=vm.runInNewContext(
  source('snapshot')+'\n'+source('spawn')+'\n({SpawnEffects,SnapshotContext})',{Math});

test('creation waits for the first visible snapshot and never animates deployed units',()=>{
  assert.equal(SpawnEffects.state(null,1,1,20),null);
  assert.equal(SpawnEffects.state(28,48,47,20),null);
  const first=SpawnEffects.state(28,48,48,20);
  assert.equal(first.age,0);
  assert.ok(first.opacity>0 && first.opacity<1);
  assert.ok(first.scale<1);
  const later=SpawnEffects.state(28,48,55,20);
  assert.ok(later.opacity>first.opacity && later.scale>first.scale);
  assert.equal(SpawnEffects.state(28,48,63,20),null);
  assert.equal(SpawnEffects.state(1,1,1,20).age,0);
});

test('ground and airborne creation effects seek and export deterministic vector shapes',()=>{
  const pictures=new Set();
  for(const air of [false,true]){
    const draw=tick=>{
      const c=new SnapshotContext(),s=SpawnEffects.state(28,48,tick,20);
      SpawnEffects.draw(c,s,5,air,{rgb:'61,141,255'},2,17);
      assert.equal(c.stack.length,0);
      return c.snapshot('unit creation',300,300);
    };
    const first=draw(48);
    assert.doesNotMatch(first,/NaN|Infinity|<image|data:image/);
    assert.notEqual(draw(55),first);
    draw(80);assert.equal(draw(48),first);
    pictures.add(first);
    const c=new SnapshotContext();
    SpawnEffects.draw(c,SpawnEffects.state(28,48,63,20),5,air,{rgb:'61,141,255'});
    assert.equal(c.body.length,0);
  }
  assert.equal(pictures.size,2);
});
