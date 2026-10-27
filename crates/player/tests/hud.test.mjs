import {readFileSync} from 'node:fs';
import vm from 'node:vm';
import test from 'node:test';
import assert from 'node:assert/strict';

const {HudFrame}=vm.runInNewContext(
  readFileSync(new URL('../web/hud.js',import.meta.url),'utf8')+'\n({HudFrame})',{Math});

test('names show 30 graphemes without splitting combining marks or emoji',()=>{
  for(const character of ['a','\u754c','e\u0301','👨‍👩‍👧‍👦']) {
    const name=character.repeat(30);
    assert.equal(HudFrame.displayName(name),name);
    assert.equal(HudFrame.displayName(name+character),character.repeat(15)+'*'+character.repeat(14));
  }
});

test('resizing the HUD extends its middle without scaling its end caps or strokes',()=>{
  for(const width of [80,120,240,462,800]){
    const shape=HudFrame.outline(width);
    const end=Number(shape.outer.match(/V18L([\d.]+) 30/)[1]);
    assert.equal(width-1-end,12);
    assert.ok(shape.plate+16<=end);
    assert.match(shape.outer,/ 53H1V8Z$/);
    const svg=HudFrame.border(width,'test');
    assert.match(svg,/stroke-width="1.4"/);
    assert.doesNotMatch(svg,/scale\(|<image|NaN|Infinity/);
    assert.equal(svg,HudFrame.border(width,'test'));
  }
  assert.notEqual(HudFrame.border(240,'test'),HudFrame.border(800,'test'));
});

test('the centre panel retains fixed-height bevels across responsive widths',()=>{
  for(const width of [118,170,360,620]){
    const svg=HudFrame.center(width);
    assert.ok(svg.includes(`L${width-12} 24`));
    assert.match(svg,/ 28H/);
    assert.doesNotMatch(svg,/NaN|Infinity|<image/);
  }
});

test('the entire HUD keeps its size relative to the naturally fitted map',()=>{
  const reference=HudFrame.uiScale(1.25);
  for(const fit of [.3,.625,1.25,2.5]){
    const scale=HudFrame.uiScale(fit);
    assert.equal(scale/reference,fit/1.25);
    assert.equal(56*scale/fit,56/1.25);
    assert.equal(24*scale/fit,24/1.25);
  }
});
