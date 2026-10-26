// A presentation tail after the last recorded tick. No combat is advanced.
'use strict';
const ScoreSettlement = (() => {
  const clamp=x=>Math.max(0,Math.min(1,x));
  const ease=x=>{x=clamp(x);return x*x*(3-2*x);};
  function prepare(data) {
    const packets=[];
    for (const team of [0,1]) {
      const total=data.cores[1-team].damage;
      if (!total) continue;
      let sources=(data.score_sources||[]).filter(s=>s.team===team && s.score>0)
        .sort((a,b)=>a.score-b.score || a.unit-b.unit);
      // A total without a verified breakdown travels from a neutral score
      // position, never from a unit that did not demonstrably earn it.
      if (sources.reduce((n,s)=>n+s.score,0)!==total)
        sources=[{team,score:total,unit:null}];
      const gap=Math.min(.1,1.2/sources.length);
      sources.forEach((s,i)=>{
        const launch=.4+i*gap;
        const arrive=launch+.7+Math.min(.4,s.score/1000);
        packets.push({...s,target:1-team,launch,arrive});
      });
    }
    return {packets,duration:packets.length?Math.max(...packets.map(p=>p.arrive))+.8:0};
  }
  function sample(plan,time) {
    const damage=[0,0],flash=[0,0];
    for (const p of plan.packets) {
      damage[p.target]+=p.score*ease((time-p.arrive)/.18);
      if(time>=p.arrive)flash[p.target]=Math.max(flash[p.target],1-clamp((time-p.arrive)/.28));
    }
    return {damage:damage.map(Math.round),flash};
  }
  function draw(c,plan,time,project,targets,teams,width,height) {
    if(time<=0)return;
    const point=(p,f)=>{
      const from=p.unit===null?{x:width/2,y:height*.42}:project(p);
      const to=targets[p.target], bend={x:from.x+(to.x-from.x)*.25,y:Math.min(from.y,to.y)-65};
      return {x:(1-f)**2*from.x+2*(1-f)*f*bend.x+f*f*to.x,
        y:(1-f)**2*from.y+2*(1-f)*f*bend.y+f*f*to.y};
    };
    const disc=(x,y,r)=>{c.beginPath();c.arc(x,y,r,0,Math.PI*2);c.fill();};
    c.save();c.lineCap='round';c.textAlign='center';c.textBaseline='middle';
    for (const p of plan.packets) {
      if(time<p.launch-.3 || time>p.arrive+.3)continue;
      const rgb=teams[p.team].rgb, weight=Math.min(1,p.score/350);
      const from=point(p,0);
      if(time<p.launch) {
        const a=ease((time-p.launch+.3)/.3);
        c.strokeStyle=`rgba(${rgb},${a*.75})`;c.lineWidth=1.5;
        c.beginPath();c.arc(from.x,from.y,12+5*a,0,Math.PI*2);c.stroke();
        c.fillStyle=`rgba(235,246,255,${a})`;c.font='600 13px system-ui';
        c.fillText(String(p.score),from.x,from.y-24);
      } else if(time<p.arrive) {
        const f=clamp((time-p.launch)/(p.arrive-p.launch)),head=point(p,f);
        c.beginPath();
        for(let i=0;i<=12;i++){
          const at=point(p,Math.max(0,f-.15+i*.15/12));
          if(i)c.lineTo(at.x,at.y);else c.moveTo(at.x,at.y);
        }
        c.strokeStyle=`rgba(${rgb},.65)`;c.lineWidth=2+weight*2;c.stroke();
        c.fillStyle=`rgba(${rgb},.17)`;disc(head.x,head.y,9+weight*4);
        c.fillStyle='#effaff';disc(head.x,head.y,2+weight*2);
        c.fillStyle=`rgba(235,246,255,.95)`;c.font='600 12px system-ui';
        c.fillText(String(p.score),head.x,head.y-15);
      } else {
        const a=clamp((time-p.arrive)/.3),to=targets[p.target];
        c.strokeStyle=`rgba(${rgb},${(1-a)*.9})`;c.lineWidth=2*(1-a)+.5;
        c.beginPath();c.arc(to.x,to.y,6+a*24,0,Math.PI*2);c.stroke();
      }
    }
    c.restore();
  }
  return {prepare,sample,draw};
})();
