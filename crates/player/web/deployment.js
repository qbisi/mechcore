// Committed releases: rounded bands for lines and circular single impacts.
// Their vector symbols use the native front-facing card silhouettes.
'use strict';

const SkillPreview = (() => {
  function prepare(d) {
    const x0 = d.x0 / 100, y0 = -d.z0 / 100;
    const x1 = d.x1 / 100, y1 = -d.z1 / 100;
    const r = d.radius / 100;
    const distance = Math.hypot(x1 - x0, y1 - y0);
    // CalculateAttackPositions places equally spaced circles, including both
    // control points. The band spans their extent, rather than tracing cells.
    const pointCount = d.geometry === 'circle' ? 1 : d.count;
    const points = Array.from({ length: pointCount }, (_, i) => {
      const t = pointCount > 1 ? i / (pointCount - 1) : 0;
      return { x: x0 + (x1 - x0) * t, y: y0 + (y1 - y0) * t };
    });
    const x = (x0 + x1) / 2, y = (y0 + y1) / 2;
    const forecast = d.forecast ? {
      radius: d.forecast.radius / 100,
      points: d.forecast.points.map((p,index) => ({tick:p.tick,x:p.x/100,y:-p.z/100,index})),
    } : null;
    let length = 0;
    const route = (d.path || []).map(([x,z]) => ({x:x/100,y:-z/100}));
    const segments = route.slice(1).map((to,i) => {
      const from=route[i], dx=to.x-from.x, dy=to.y-from.y;
      const size=Math.hypot(dx,dy), start=length;
      length+=size;
      return {from,to,start,size,dx:size?dx/size:0,dy:size?dy/size:0};
    }).filter(s=>s.size>0);
    return { ...d, x, y, r, points, forecast, route, segments, routeLength:length,
      angle: Math.atan2(y1 - y0, x1 - x0), length: distance + r * 2,
      start: d.start_ms / 1000, interval: d.interval_ms / 1000 };
  }

  function state(d, tick, tps) {
    const elapsed = (tick - 1) / tps;
    if (d.geometry === 'path') {
      return elapsed < 0 || tick >= d.endTick ? null : {elapsed,countdown:false,opacity:1,progress:0};
    }
    const scattered = d.kind === 'orbital_bombardment' || d.kind === 'lightning_storm';
    const summon = ['underground_threat','rhino_assault','wasp_swarm','mobilize_battleship','vulcans_descent'].includes(d.kind);
    // Random-damage views hide their main frame at the first sub-effect.
    // Use the reconstructed first arrival when available, with a short fade.
    // Preview-only pages retain the nominal start as their fallback.
    const first = scattered && d.forecast?.points[0];
    const start = first ? (first.tick-1)/tps : d.start;
    const fade = 0.25;
    const fading = scattered || summon;
    const end = start + (fading ? fade : d.interval * d.count);
    // Native DoUpdate includes an unrecorded randomRangeRecord allowance.
    // Use its zero-jitter presentation here, so seeking and SVG agree.
    if (elapsed < 0 || (fading ? elapsed >= end : elapsed > end)) return null;
    return { elapsed, countdown: elapsed < start,
      opacity: fading ? Math.min(1, (end - elapsed) / fade) : 1,
      progress: start > 0 ? Math.min(1, elapsed / start) : 1 };
  }

  function disc(c, x, y, r) {
    c.beginPath(); c.arc(x, y, r, 0, Math.PI * 2); c.fill();
  }

  function band(c, half, r, corner) {
    c.beginPath();
    c.moveTo(-half+corner,-r); c.lineTo(half-corner,-r);
    c.arc(half-corner,-r+corner,corner,-Math.PI/2,0);
    c.lineTo(half,r-corner); c.arc(half-corner,r-corner,corner,0,Math.PI/2);
    c.lineTo(-half+corner,r); c.arc(-half+corner,r-corner,corner,Math.PI/2,Math.PI);
    c.lineTo(-half,-r+corner); c.arc(-half+corner,-r+corner,corner,Math.PI,Math.PI*1.5);
    c.closePath();
  }

  function circleArea(c, d, stage, rgb, pixel) {
    const large = d.r >= 100;
    c.fillStyle = `rgba(${rgb},0.045)`; disc(c,0,0,d.r);
    c.beginPath(); c.arc(0,0,d.r,0,Math.PI*2);
    c.strokeStyle = 'rgba(6,12,18,0.48)'; c.lineWidth = 3*pixel; c.stroke();
    c.strokeStyle = `rgba(${rgb},0.58)`; c.lineWidth = 1.15*pixel; c.stroke();
    if (large) {
      // A broken outer rail and graduated ticks distinguish large footprints;
      // the continuous inner contour remains the actual effect radius.
      c.beginPath();
      for (let i=0;i<4;i++) {
        const a=i*Math.PI/2;
        c.moveTo(Math.cos(a+0.12)*(d.r+4*pixel),Math.sin(a+0.12)*(d.r+4*pixel));
        c.arc(0,0,d.r+4*pixel,a+0.12,a+Math.PI/2-0.12);
      }
      c.strokeStyle = `rgba(${rgb},0.3)`; c.lineWidth = 0.8*pixel; c.stroke();
      c.beginPath();
      for (let i=0;i<48;i++) {
        if (i%12===0) continue;
        const a=i*Math.PI/24, x=Math.cos(a), y=Math.sin(a);
        const length=i%4===0?4:2;
        c.moveTo(x*(d.r+4*pixel),y*(d.r+4*pixel));
        c.lineTo(x*(d.r+(4+length)*pixel),y*(d.r+(4+length)*pixel));
      }
      c.strokeStyle = `rgba(${rgb},0.48)`; c.lineWidth = pixel; c.stroke();
    }
    c.beginPath();
    for (let i=0;i<4;i++) {
      const a=i*Math.PI/2, x=Math.cos(a), y=Math.sin(a);
      c.moveTo(x*(d.r-(large?3:2)*pixel),y*(d.r-(large?3:2)*pixel));
      c.lineTo(x*(d.r+(large?9:2)*pixel),y*(d.r+(large?9:2)*pixel));
    }
    c.strokeStyle = `rgba(${rgb},0.8)`; c.lineWidth = 1.6*pixel; c.stroke();
    // The reflection's head tracks the inner countdown arc exactly, making
    // one revolution over the release delay rather than looping independently.
    const a=stage.progress*Math.PI*2-Math.PI/2;
    const g=c.createLinearGradient(Math.cos(a-0.3)*d.r,Math.sin(a-0.3)*d.r,
      Math.cos(a)*d.r,Math.sin(a)*d.r);
    g.addColorStop(0,'rgba(218,240,255,0)');
    g.addColorStop(1,'rgba(218,240,255,0.9)');
    c.beginPath(); c.arc(0,0,d.r,a-0.3,a);
    c.strokeStyle=g; c.lineWidth=(large?2.8:2)*pixel; c.stroke();
    if (large) {
      c.fillStyle='rgba(230,246,255,0.95)';
      disc(c,Math.cos(a)*d.r,Math.sin(a)*d.r,1.5*pixel);
    }
  }

  function draw(c, d, tick, tps, team, pixelsPerMetre = 1) {
    if (d.geometry === 'path') {
      drawFlow(c,d,tick,tps,team,pixelsPerMetre);
      return;
    }
    const stage = state(d, tick, tps);
    const glyph = NativeSkillIcons[d.kind];
    if (!stage || !glyph || d.r <= 0) return;
    const pixel = 1 / Math.max(0.3, pixelsPerMetre);
    // Badge geometry stays in world metres, so glyph, ring and ring weight
    // scale together with the battlefield rather than cancelling camera zoom.
    const R = Math.min(d.r * 0.75, 18), badgePixel = R / 22;
    const half = d.length / 2, corner = d.r * 0.22;
    const rgb = team.rgb;
    const striking = d.kind === 'ion_blast' && !stage.countdown;
    c.save(); c.lineJoin = 'round'; c.lineCap = 'round';
    c.globalAlpha *= stage.opacity;
    if (striking) c.globalAlpha *= 0.38;
    c.translate(d.x,d.y); c.rotate(d.angle);
    if (d.geometry === 'circle') {
      circleArea(c,d,stage,rgb,pixel);
    } else {
      band(c, half, d.r, corner);
      c.fillStyle = `rgba(${rgb},0.055)`; c.fill();
      c.strokeStyle = 'rgba(6,12,18,0.48)'; c.lineWidth = 3 * pixel; c.stroke();
      c.strokeStyle = `rgba(${rgb},0.58)`; c.lineWidth = 1.15 * pixel; c.stroke();
      // Brackets accent both rounded ends of the original placement band.
      for (const side of [-1,1]) {
        const end = side * half, centre = side * (half-corner), inward = side * (half-d.r*0.5);
        c.beginPath(); c.moveTo(inward,-d.r); c.lineTo(centre,-d.r);
        c.arc(centre,-d.r+corner,corner,-Math.PI/2,side > 0 ? 0 : -Math.PI,side < 0);
        c.lineTo(end,d.r-corner);
        c.arc(centre,d.r-corner,corner,side > 0 ? 0 : Math.PI,Math.PI/2,side < 0);
        c.lineTo(inward,d.r);
        c.strokeStyle = `rgba(${rgb},0.86)`; c.lineWidth = 1.8 * pixel; c.stroke();
      }

      // A soft reflection travels in control-point order, including reversed
      // red releases and diagonals. Time alone drives it, so seeking is exact.
      const width = Math.max(8, d.length * 0.12);
      const phase = (stage.elapsed / 1.8) % 1;
      const u = -half - width + phase * (d.length + 2*width);
      const g = c.createLinearGradient(u-width,0,u+width,0);
      g.addColorStop(0, 'rgba(218,240,255,0)');
      g.addColorStop(0.5, 'rgba(218,240,255,0.94)');
      g.addColorStop(1, 'rgba(218,240,255,0)');
      band(c, half, d.r, corner); c.strokeStyle = g; c.lineWidth = 2 * pixel; c.stroke();
    }
    c.restore();

    // The two release controls remain visible inside their true end caps.
    c.save(); c.lineCap = 'round';
    c.globalAlpha *= stage.opacity;
    if (striking) c.globalAlpha *= 0.38;
    for (const [i, p] of d.points.entries()) {
      const end = i === 0 || i === d.points.length-1;
      if (d.geometry === 'circle' || (d.kind === 'ion_blast' && !end)) continue;
      c.fillStyle = `rgba(${rgb},${end ? 0.85 : 0.38})`;
      disc(c, p.x, p.y, (end ? 2.2 : 1.2) * pixel);
      if (end) {
        c.beginPath(); c.moveTo(p.x-5*pixel,p.y); c.lineTo(p.x+5*pixel,p.y);
        c.moveTo(p.x,p.y-5*pixel); c.lineTo(p.x,p.y+5*pixel);
        c.strokeStyle = `rgba(${rgb},0.7)`; c.lineWidth = pixel; c.stroke();
      }
    }
    // Upright glyphs share one quiet ring and one countdown arc on that ring.
    if (striking) c.globalAlpha *= 0.55;
    c.translate(d.x, d.y);
    c.fillStyle = 'rgba(20,32,43,0.86)'; disc(c,0,0,R);
    c.strokeStyle = `rgba(${rgb},0.35)`; c.lineWidth = badgePixel;
    c.beginPath(); c.arc(0,0,R,0,Math.PI*2); c.stroke();
    if (stage.countdown) {
      c.strokeStyle = `rgba(${rgb},0.85)`; c.lineWidth = 1.8 * badgePixel;
      c.beginPath(); c.arc(0,0,R,-Math.PI/2+stage.progress*Math.PI*2,Math.PI*1.5); c.stroke();
    }
    c.save(); const scale = R / 33; c.scale(scale,scale);
    glyph(c, `rgba(${rgb},0.92)`, '#14202b'); c.restore();
    c.restore();
  }

  // A presentation forecast, not reconstructed damage events. MCFR does not
  // identify commander sub-effects, empty impacts or their interception points.
  function strike(d, tick, tps) {
    if (d.kind !== 'ion_blast' || d.count < 1 || d.interval <= 0) return null;
    const elapsed = (tick - 1) / tps;
    const last = d.start + (d.count-1)*d.interval;
    const tail = 0.6;
    if (elapsed < d.start || elapsed >= last+tail) return null;
    const step = Math.max(0, Math.min(d.count-1, (elapsed-d.start)/d.interval));
    const index = Math.min(d.count-1, Math.floor(step+1e-7));
    const from = d.points[Math.floor(step)], to = d.points[Math.min(d.count-1, Math.floor(step)+1)];
    const f = step-Math.floor(step);
    const cursor = { x: from.x+(to.x-from.x)*f, y: from.y+(to.y-from.y)*f };
    const flashes = [];
    for (let i = index; i >= 0; i--) {
      const age = Math.max(0, elapsed-(d.start+i*d.interval));
      if (age >= tail) break;
      flashes.push({ ...d.points[i], age });
    }
    return { cursor, flashes, fade: Math.max(0, 1-Math.max(0,elapsed-last)/tail) };
  }

  function drawStrike(c, d, tick, tps, team, pixelsPerMetre = 1) {
    if (d.kind !== 'ion_blast') {
      drawScatteredStrike(c,d,tick,tps,team,pixelsPerMetre);
      return;
    }
    const s = strike(d, tick, tps);
    if (!s || d.r <= 0) return;
    const pixel = 1 / Math.max(0.3,pixelsPerMetre), rgb = team.rgb;
    c.save(); c.lineCap = 'round'; c.globalAlpha *= s.fade;
    // A quiet moving footprint; its radius stays in world metres as zoom changes.
    c.fillStyle = `rgba(${rgb},0.035)`; disc(c,s.cursor.x,s.cursor.y,d.r);
    c.beginPath(); c.arc(s.cursor.x,s.cursor.y,d.r,0,Math.PI*2);
    c.strokeStyle = `rgba(${rgb},0.32)`; c.lineWidth = pixel; c.stroke();
    for (const p of s.flashes) {
      const residual = (1-p.age/0.6)**2;
      c.beginPath(); c.arc(p.x,p.y,d.r,0,Math.PI*2);
      c.strokeStyle = `rgba(${rgb},${0.18*residual})`; c.lineWidth = pixel; c.stroke();
      const k = Math.max(0,1-p.age/0.15);
      if (!k) continue;
      c.fillStyle = `rgba(${rgb},${0.11*k})`; disc(c,p.x,p.y,d.r);
      c.beginPath(); c.arc(p.x,p.y,d.r*(0.65+0.35*(1-k)),0,Math.PI*2);
      c.strokeStyle = `rgba(${rgb},${0.88*k})`; c.lineWidth = 1.8*pixel; c.stroke();
      const g = c.createRadialGradient(p.x,p.y,0,p.x,p.y,d.r*0.42);
      g.addColorStop(0,`rgba(232,250,255,${0.75*k})`);
      g.addColorStop(0.3,`rgba(${rgb},${0.35*k})`);
      g.addColorStop(1,`rgba(${rgb},0)`);
      c.fillStyle = g; disc(c,p.x,p.y,d.r*0.42);
      c.fillStyle = `rgba(242,253,255,${0.9*k})`; disc(c,p.x,p.y,Math.max(1.1,d.r*0.07));
    }
    c.restore();
  }

  // Seed-reconstructed planned ground arrivals. No interpolation between
  // random points, and no forecast flash changes recorded damage or shields.
  function scatteredStrike(d,tick,tps) {
    if (!d.forecast || !['orbital_bombardment','lightning_storm'].includes(d.kind)) return null;
    const lightning=d.kind==='lightning_storm', lifetime=lightning?0.6:0.85;
    const flashes=d.forecast.points.map(p=>({...p,age:(tick-p.tick)/tps}))
      .filter(p=>p.age>=0 && p.age<lifetime);
    return flashes.length ? {radius:d.forecast.radius,flashes,lightning,lifetime} : null;
  }

  function drawScatteredStrike(c,d,tick,tps,team,pixelsPerMetre) {
    const s=scatteredStrike(d,tick,tps);
    if (!s) return;
    const pixel=1/Math.max(0.3,pixelsPerMetre), r=s.radius, rgb=team.rgb;
    c.save(); c.lineCap='round'; c.lineJoin='round';
    for (const p of s.flashes) {
      const fade=(1-p.age/s.lifetime)**2, pulse=Math.max(0,1-p.age/0.18);
      c.save(); c.translate(p.x,p.y);
      // The quiet circle marks each hit's radius; the expanding ring and
      // central flash distinguish bombardment from the release footprint.
      c.strokeStyle=`rgba(${rgb},${0.48*fade})`; c.lineWidth=1.2*pixel;
      c.beginPath(); c.arc(0,0,r,0,Math.PI*2); c.stroke();
      const glow=c.createRadialGradient(0,0,0,0,0,r*0.5);
      glow.addColorStop(0,`rgba(${s.lightning?'225,248,255':'255,240,199'},${0.85*fade})`);
      glow.addColorStop(0.25,`rgba(${s.lightning?'100,196,255':'255,168,62'},${0.52*fade})`);
      glow.addColorStop(1,`rgba(${s.lightning?'61,141,255':'255,100,32'},0)`);
      c.fillStyle=glow; disc(c,0,0,r*0.5);
      if (s.lightning) {
        if (pulse>0) {
          const bolt=()=>{
            c.beginPath(); c.moveTo(-r*0.12,-r*2.5);
            for (let j=1;j<7;j++) {
              const wobble=Math.sin(p.index*2.3+j*3.7)*r*0.16;
              c.lineTo(wobble,-r*2.5*(1-j/7));
            }
            c.lineTo(0,0);
          };
          bolt(); c.strokeStyle=`rgba(83,162,255,${0.5*pulse})`; c.lineWidth=5*pixel; c.stroke();
          bolt(); c.strokeStyle=`rgba(232,252,255,${pulse})`; c.lineWidth=1.4*pixel; c.stroke();
        }
        c.beginPath();
        for (let i=0;i<5;i++) {
          const a=p.index*0.73+i*Math.PI*0.4, x=Math.cos(a), y=Math.sin(a);
          c.moveTo(0,0); c.lineTo(x*r*0.15-y*r*0.07,y*r*0.15+x*r*0.07);
          c.lineTo(x*r*0.32,y*r*0.32);
        }
        c.strokeStyle=`rgba(156,218,255,${0.7*fade})`; c.lineWidth=pixel; c.stroke();
      } else {
        const wave=Math.min(1,p.age/0.4);
        c.beginPath(); c.arc(0,0,r*(0.25+0.75*wave),0,Math.PI*2);
        c.strokeStyle=`rgba(255,188,92,${0.9*fade})`; c.lineWidth=2*pixel; c.stroke();
        c.fillStyle=`rgba(255,247,219,${pulse})`; disc(c,0,0,r*(0.05+0.11*pulse));
      }
      c.restore();
    }
    c.restore();
  }

  // WayPointInFight scrolls a directional arrow texture over each leg.
  // Its range view uses twice GetSubEffectRange for the band width.
  function flow(d,tick,tps) {
    if (!state(d,tick,tps) || !d.routeLength || d.r<=0) return [];
    const distance=(tick-1)/tps*18, spacing=46;
    return d.segments.map(s=>{
      const phase=((distance-s.start)%spacing+spacing)%spacing;
      const markers=[];
      for (let along=-spacing-d.r+phase;along<s.size+d.r+spacing;along+=spacing) {
        markers.push({along,x:s.from.x+s.dx*along,y:s.from.y+s.dy*along});
      }
      return {...s,markers};
    });
  }

  function capsule(c,length,r) {
    c.beginPath(); c.moveTo(0,-r); c.lineTo(length,-r);
    c.arc(length,0,r,-Math.PI/2,Math.PI/2);
    c.lineTo(0,r); c.arc(0,0,r,Math.PI/2,Math.PI*1.5); c.closePath();
  }

  // MCFR has no command track. Reconstruct only the view's lifetime from
  // recorded positions, following PilotAI's ordered, offset target checks.
  // This does not simulate movement or add units that appear during combat.
  function bindPaths(deployments,units) {
    const assigned=new Set();
    for (const d of deployments) {
      if (d.geometry!=='path') continue;
      d.endTick=1;
      const first=d.route[0];
      if (!first || d.route.length<2) continue;
      for (const u of units) {
        if (u.from!==1 || u.team!==d.team || u.L[0]<=0 || assigned.has(u.id)) continue;
        const ox=u.X[0]-first.x, oy=-u.Z[0]-first.y;
        if (Math.hypot(ox,oy)>d.r+u.radius) continue;
        assigned.add(u.id);
        const targets=d.route.slice(1).map((to,i)=>{
          const from=d.route[i], dx=to.x-from.x, dy=to.y-from.y;
          const length=Math.hypot(dx,dy);
          return {x:to.x+ox+(length?20*dx/length:0),
            y:to.y+oy+(length?20*dy/length:0)};
        });
        let stage=0, end=Infinity;
        for (let i=0;i<u.n;i++) {
          const tick=u.from+i;
          if (u.L[i]<=0 || (u.death && tick>=u.death.t)) { end=tick; break; }
          const target=targets[stage];
          if (Math.hypot(u.X[i]-target.x,-u.Z[i]-target.y)<=20+u.radius) {
            if (++stage===targets.length) { end=tick; break; }
          }
        }
        if (u.death) end=Math.min(end,u.death.t);
        d.endTick=Math.max(d.endTick,end);
      }
    }
  }

  function drawFlow(c,d,tick,tps,team,pixelsPerMetre=1) {
    const pixel=1/Math.max(0.3,pixelsPerMetre), rgb=team.rgb;
    const legs=flow(d,tick,tps);
    for (const [index,s] of legs.entries()) {
      c.save(); c.translate(s.from.x,s.from.y); c.rotate(Math.atan2(s.dy,s.dx));
      c.lineJoin='round'; c.lineCap='round';
      capsule(c,s.size,d.r);
      c.fillStyle=`rgba(${rgb},0.018)`; c.fill();
      c.strokeStyle=`rgba(${rgb},0.23)`; c.lineWidth=1.05*pixel; c.stroke();
      // Each leg keeps its semicircular ends. Clip double chevrons to that
      // same capsule, keeping a shared distance phase.
      c.save(); c.clip();
      for (const marker of s.markers) {
        const head=marker.along, depth=d.r*0.28, gap=d.r*0.2, half=d.r*0.62;
        const glow=c.createLinearGradient(head-gap-depth,0,head,0);
        glow.addColorStop(0,`rgba(${rgb},0.035)`);
        glow.addColorStop(0.65,`rgba(${rgb},0.11)`);
        glow.addColorStop(1,`rgba(${rgb},0.19)`);
        c.beginPath();
        for (const offset of [0,-gap]) {
          const x=head+offset;
          c.moveTo(x-depth,-half); c.lineTo(x,0); c.lineTo(x-depth,half);
        }
        c.strokeStyle=glow; c.lineWidth=1.4; c.stroke();
      }
      c.restore();
      // Complete only the missing inward half at the route's first and last
      // points; the outward half is already part of the capsule outline.
      if (index===0) {
        c.beginPath(); c.arc(0,0,d.r,-Math.PI/2,Math.PI/2); c.stroke();
      }
      if (index===legs.length-1) {
        c.beginPath(); c.arc(s.size,0,d.r,Math.PI/2,Math.PI*1.5); c.stroke();
      }
      c.restore();
    }
  }

  return { prepare, state, draw, strike, drawStrike, scatteredStrike, flow, bindPaths };
})();
