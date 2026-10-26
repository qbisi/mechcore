// Recorded unit creation, animated only once its first snapshot is visible.
'use strict';

const SpawnEffects = (() => {
  function state(created, first, tick, tps) {
    if (created === null || created === undefined) return null;
    const age=(tick-Math.max(created,first))/tps, duration=0.75;
    if (age<0 || age>=duration) return null;
    const progress=age/duration, ease=1-(1-progress)**3;
    return {age,progress,opacity:0.2+0.8*ease,scale:0.72+0.28*ease,fade:(1-progress)**2};
  }

  function draw(c,s,radius,air,team,pixelsPerMetre=1,seed=0) {
    if (!s) return;
    const r=Math.max(2,radius), pixel=1/Math.max(0.3,pixelsPerMetre), rgb=team.rgb;
    c.save(); c.lineCap='round';
    const ring=r*(0.7+s.progress*1.2);
    c.strokeStyle=`rgba(${rgb},${0.65*s.fade})`; c.lineWidth=1.3*pixel;
    c.beginPath(); c.ellipse(0,0,ring,ring*(air?0.75:0.6),0,0,Math.PI*2); c.stroke();
    const glow=c.createRadialGradient(0,0,0,0,0,r*1.5);
    glow.addColorStop(0,`rgba(${air?'163,224,255':'205,182,138'},${0.34*s.fade})`);
    glow.addColorStop(1,`rgba(${rgb},0)`);
    c.fillStyle=glow; c.beginPath(); c.arc(0,0,r*1.5,0,Math.PI*2); c.fill();
    c.beginPath();
    for (let i=0;i<6;i++) {
      const a=seed*0.71+i*Math.PI/3, x=Math.cos(a), y=Math.sin(a);
      if (air) {
        const length=r*(0.5+0.7*s.fade);
        c.moveTo(x*r,y*r*0.65-length); c.lineTo(x*r,y*r*0.65);
      } else {
        const spread=r*(0.65+1.1*s.progress);
        c.moveTo(x*spread,y*spread*0.6);
        c.lineTo(x*(spread+r*0.28),y*(spread+r*0.28)*0.6-r*0.12);
      }
    }
    c.strokeStyle=`rgba(${air?rgb:'188,165,124'},${0.62*s.fade})`;
    c.lineWidth=(air?1:1.8)*pixel; c.stroke();
    c.restore();
  }
  return {state,draw};
})();
