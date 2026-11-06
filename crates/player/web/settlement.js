// A presentation tail after the last recorded tick. No combat is advanced.
//
// The schedule follows the native ScoreAttackController: both sides attack at
// once, each side's survivors in ascending score after a 0.3-second lead-in,
// each launch spaced by min(0.9, score * min(0.01, 3 / total)) seconds, each
// orb flying 0.8 + 0.4 * score / 400 seconds on an InBack ease in a straight
// line into its target core, the core changing on the hit, and the tail ending
// 1.1 seconds after the last hit. The native random launch delay is zero here
// and the random landing point a fixed hash of the unit, so seeking and SVG
// export agree. The overload wash, lightning sweep, sparks and hit numbers are
// drawn after the native effect as seen on screen, not from its assets.
'use strict';
const ScoreSettlement = (() => {
  const clamp=x=>Math.max(0,Math.min(1,x));
  const ease=x=>{x=clamp(x);return x*x*(3-2*x);};
  // DOTween's Ease.InBack, overshoot 1.70158: it backs off, then rushes in.
  const inBack=x=>{x=clamp(x);return x*x*(2.70158*x-1.70158);};
  // A fixed pseudo-random number in [0, 1) for each key, never Math.random.
  const hash=(...keys)=>{
    let h=2166136261;
    for(const k of keys){h=Math.imul(h^(k|0),16777619);h^=h>>>13;h=Math.imul(h,0x5bd1e995);h^=h>>>15;}
    return (h>>>0)/4294967296;
  };
  const LEAD=.3, TAIL=.1+1, CHARGE=.3, NUMBER_DELAY=.05, NUMBER_LIFE=1;
  const flight=score=>.8+.4*score/400;
  function prepare(data) {
    const packets=[];
    let duration=0;
    for (const team of [0,1]) {
      const total=data.cores[1-team].damage;
      if (!total) continue;
      let sources=(data.score_sources||[]).filter(s=>s.team===team && s.score>0)
        .sort((a,b)=>a.score-b.score || a.unit-b.unit);
      // A total without a verified breakdown travels from a neutral score
      // position, never from a unit that did not demonstrably earn it.
      if (sources.reduce((n,s)=>n+s.score,0)!==total)
        sources=[{team,score:total,unit:null}];
      const step=Math.min(.01,3/total);
      let launch=LEAD;
      for (const s of sources) {
        launch+=Math.min(.9,step*s.score);
        const arrive=launch+flight(s.score);
        packets.push({...s,target:1-team,launch,arrive,
          landing:hash(s.unit===null?-1-team:s.unit,team,7)});
        duration=Math.max(duration,arrive+TAIL);
      }
    }
    return {packets,duration};
  }
  function sample(plan,time) {
    const damage=[0,0],flash=[0,0];
    for (const p of plan.packets) {
      if(time>=p.arrive)damage[p.target]+=p.score;
      if(time>=p.arrive)flash[p.target]=Math.max(flash[p.target],1-clamp((time-p.arrive)/.35));
    }
    return {damage:damage.map(Math.round),flash};
  }
  // `targets` are the two cores' bars on screen, as rectangles; an orb lands
  // at its fixed point along the bar, inset from either end as the native
  // landing point is inset from its target's corners.
  function landing(p,targets,scale) {
    const r=targets[p.target], inset=Math.min(12*scale,r.w/4);
    return {x:r.x+inset+p.landing*(r.w-2*inset),y:r.y+r.h/2};
  }
  function streak(c,x,y,angle,length,width,style) {
    c.strokeStyle=style;c.lineWidth=width;
    c.beginPath();c.moveTo(x,y);c.lineTo(x-Math.cos(angle)*length,y-Math.sin(angle)*length);c.stroke();
  }
  function glow(c,x,y,r,rgb,alpha) {
    if(r<=0||alpha<=0)return;
    const g=c.createRadialGradient(x,y,0,x,y,r);
    g.addColorStop(0,`rgba(255,255,255,${alpha})`);
    g.addColorStop(.35,`rgba(${rgb},${alpha*.55})`);
    g.addColorStop(1,`rgba(${rgb},0)`);
    c.fillStyle=g;c.beginPath();c.arc(x,y,r,0,Math.PI*2);c.fill();
  }
  // The fight ends in an overload: a violet wash over the field and sparks
  // streaking across it, fading as the first orbs charge.
  function drawOverload(c,time,width,height,scale) {
    const a=Math.min(clamp(time/.12),1-clamp((time-.25)/.75));
    if(a<=0)return;
    const g=c.createRadialGradient(width/2,height*.45,0,width/2,height*.45,Math.hypot(width,height)*.6);
    g.addColorStop(0,`rgba(255,170,215,${.10*a})`);
    g.addColorStop(.55,`rgba(150,110,255,${.16*a})`);
    g.addColorStop(1,`rgba(70,40,150,${.24*a})`);
    c.fillStyle=g;c.fillRect(0,0,width,height);
    for(let i=0;i<64;i++){
      const life=.55+.3*hash(i,2), age=time-.4*hash(i,3);
      if(age<0||age>life)continue;
      const f=age/life, angle=-.9+.5*hash(i,4)+(hash(i,5)<.5?0:Math.PI);
      const x=hash(i,0)*width+Math.cos(angle)*f*90*scale, y=hash(i,1)*height+Math.sin(angle)*f*90*scale;
      streak(c,x,y,angle,(14+30*hash(i,6))*scale,1.4*scale,`rgba(${hash(i,7)<.7?'205,170,255':'255,255,255'},${(1-f)*.85})`);
    }
  }
  // Lightning sweeps out from each core under attack across the field, a few
  // ragged arcs widening and fading before the orbs arrive.
  function drawLightning(c,plan,time,targets,teams,width,height,scale) {
    const reach=Math.hypot(width,height);
    for (const target of [0,1]) {
      const first=plan.packets.find(p=>p.target===target);
      if(!first)continue;
      const r=targets[target], cx=r.x+r.w/2, cy=r.y+r.h/2, rgb=teams[first.team].rgb;
      for(let k=0;k<3;k++){
        const age=time-.45-.28*k, life=1.1;
        if(age<0||age>life)continue;
        const f=age/life, radius=reach*(.18+1.05*ease(f)), alpha=Math.sin(Math.PI*f)*.9;
        const flicker=Math.floor(time/.05), segments=56;
        const from=.05*Math.PI+hash(k,target,9)*.2, to=Math.PI*.95-hash(k,target,10)*.2;
        c.beginPath();
        for(let i=0;i<=segments;i++){
          const t=from+(to-from)*i/segments;
          const jag=(hash(k,i,flicker)-.5)*radius*.06+(hash(k,i>>2,flicker,1)-.5)*radius*.05;
          const x=cx+Math.cos(t)*(radius+jag), y=cy+Math.sin(t)*(radius+jag)*.62;
          if(i)c.lineTo(x,y);else c.moveTo(x,y);
        }
        c.lineJoin='round';
        c.strokeStyle=`rgba(${rgb},${alpha*.22})`;c.lineWidth=10*scale;c.stroke();
        c.strokeStyle=`rgba(190,150,255,${alpha*.55})`;c.lineWidth=3.5*scale;c.stroke();
        c.strokeStyle=`rgba(250,245,255,${alpha})`;c.lineWidth=1.2*scale;c.stroke();
      }
    }
  }
  function draw(c,plan,time,project,targets,teams,width,height,scale=1) {
    if(time<=0 || !plan.packets.length)return;
    c.save();c.lineCap='round';c.textAlign='center';c.textBaseline='middle';
    c.globalCompositeOperation='lighter';
    drawOverload(c,time,width,height,scale);
    drawLightning(c,plan,time,targets,teams,width,height,scale);
    const origin=p=>p.unit===null?{x:width/2,y:height*.42}:project(p);
    for (const p of plan.packets) {
      const rgb=teams[p.team].rgb, weight=Math.min(1,p.score/400);
      const from=origin(p), to=landing(p,targets,scale);
      const at=f=>{const e=inBack(f);return {x:from.x+(to.x-from.x)*e,y:from.y+(to.y-from.y)*e};};
      const size=(5+3*weight)*scale;
      if(time>=p.launch-CHARGE && time<p.launch) {
        // The orb gathers on the unit before it leaves.
        const a=ease((time-p.launch+CHARGE)/CHARGE);
        glow(c,from.x,from.y,size*3.2*a,rgb,.9*a);
        c.strokeStyle=`rgba(225,240,255,${.7*a})`;c.lineWidth=1.2*scale;
        c.beginPath();c.arc(from.x,from.y,size*(2.6-1.4*a),0,Math.PI*2);c.stroke();
      } else if(time>=p.launch && time<p.arrive) {
        const f=(time-p.launch)/(p.arrive-p.launch), head=at(f);
        c.beginPath();
        for(let i=0;i<=10;i++){
          const q=at(Math.max(0,f-.12+i*.012));
          if(i)c.lineTo(q.x,q.y);else c.moveTo(q.x,q.y);
        }
        c.strokeStyle=`rgba(${rgb},.55)`;c.lineWidth=size*1.1;c.stroke();
        c.strokeStyle='rgba(235,246,255,.75)';c.lineWidth=size*.35;c.stroke();
        glow(c,head.x,head.y,size*3,rgb,.95);
        c.fillStyle='rgba(255,255,255,.95)';c.beginPath();c.arc(head.x,head.y,size*.7,0,Math.PI*2);c.fill();
        c.strokeStyle='rgba(215,235,255,.8)';c.lineWidth=scale;
        c.beginPath();c.arc(head.x,head.y,size*1.3,0,Math.PI*2);c.stroke();
      } else if(time>=p.arrive && time<p.arrive+.5) {
        // The hit bursts on the core's bar.
        const f=(time-p.arrive)/.5;
        glow(c,to.x,to.y,(18+20*weight)*(.4+.6*ease(f*2))*scale,rgb,1-f);
        c.strokeStyle=`rgba(235,246,255,${(1-f)*.9})`;c.lineWidth=1.5*scale;
        for(let i=0;i<8;i++){
          const angle=i*Math.PI/4+hash(p.unit??-1,i)*.4, r0=(6+30*ease(f))*scale, r1=r0+10*(1-f)*scale;
          c.beginPath();c.moveTo(to.x+Math.cos(angle)*r0,to.y+Math.sin(angle)*r0);
          c.lineTo(to.x+Math.cos(angle)*r1,to.y+Math.sin(angle)*r1);c.stroke();
        }
      }
    }
    c.globalCompositeOperation='source-over';
    for (const p of plan.packets) {
      // Each hit's value pops beside the core it struck and drifts away.
      const age=time-p.arrive-NUMBER_DELAY;
      if(age<0||age>=NUMBER_LIFE)continue;
      const pop=1+.6*(1-ease(age/.2)), a=1-clamp((age-.6)/.4);
      const bar=targets[p.target];
      // The inner end has no name plate or officer/supply buttons below it.
      // Keep labels there instead of hiding them behind those DOM panels.
      const labelX=bar.x+bar.w*(p.target===0?.72+.24*p.landing:.04+.24*p.landing);
      c.save();c.translate(labelX,bar.y+bar.h+(42-14*ease(age/NUMBER_LIFE))*scale);c.scale(pop*scale,pop*scale);
      c.font='22px "Mechcore HUD", system-ui, sans-serif';
      c.fillStyle=`rgba(10,14,24,${.6*a})`;c.fillText(`−${p.score}`,1.5,1.5);
      c.fillStyle=`rgba(255,246,240,${a})`;c.fillText(`−${p.score}`,0,0);
      c.restore();
    }
    c.restore();
  }
  return {prepare,sample,draw,flight};
})();
