// Committed line-skill previews. A rounded band frames the impact circles'
// extent; the vector symbols follow the native SBUI vocabulary.
'use strict';

const SkillPreview = (() => {
  function prepare(d) {
    const x0 = d.x0 / 100, y0 = -d.z0 / 100;
    const x1 = d.x1 / 100, y1 = -d.z1 / 100;
    const r = d.radius / 100;
    const distance = Math.hypot(x1 - x0, y1 - y0);
    // CalculateAttackPositions places equally spaced circles, including both
    // control points. The band spans their extent, rather than tracing cells.
    const points = Array.from({ length: d.count }, (_, i) => {
      const t = d.count > 1 ? i / (d.count - 1) : 0;
      return { x: x0 + (x1 - x0) * t, y: y0 + (y1 - y0) * t };
    });
    const x = (x0 + x1) / 2, y = (y0 + y1) / 2;
    return { ...d, x, y, r, points,
      angle: Math.atan2(y1 - y0, x1 - x0), length: distance + r * 2,
      start: d.start_ms / 1000, interval: d.interval_ms / 1000 };
  }

  function state(d, tick, tps) {
    const elapsed = (tick - 1) / tps;
    // Native DoUpdate includes an unrecorded randomRangeRecord allowance.
    // Use its zero-jitter presentation here, so seeking and SVG agree.
    if (elapsed < 0 || elapsed > d.start + d.interval * d.count) return null;
    return { elapsed, countdown: elapsed < d.start,
      progress: d.start > 0 ? Math.min(1, elapsed / d.start) : 1 };
  }

  function polygon(c, points) {
    c.beginPath();
    points.forEach(([x, y], i) => i ? c.lineTo(x, y) : c.moveTo(x, y));
    c.closePath();
  }

  // Curved silhouettes sampled to paths using the shared Canvas vocabulary.
  function silhouette(c, start, curves) {
    c.beginPath(); c.moveTo(...start);
    let [x0, y0] = start;
    for (const [x1, y1, x2, y2, x3, y3] of curves) {
      for (let i = 1; i <= 10; i++) {
        const t = i / 10, u = 1 - t;
        c.lineTo(u ** 3 * x0 + 3 * u * u * t * x1 + 3 * u * t * t * x2 + t ** 3 * x3,
          u ** 3 * y0 + 3 * u * u * t * y1 + 3 * u * t * t * y2 + t ** 3 * y3);
      }
      [x0, y0] = [x3, y3];
    }
    c.closePath();
  }

  function disc(c, x, y, r) {
    c.beginPath(); c.arc(x, y, r, 0, Math.PI * 2); c.fill();
  }

  function oil(c, ink, cut) {
    c.save(); c.scale(1.2, 1.2); c.translate(0, -1);
    const barrel = (x, y, scale) => {
      c.save(); c.translate(x, y); c.scale(scale, scale); c.rotate(-0.35);
      c.fillStyle = ink;
      silhouette(c, [-10, -11], [[-10,-15,10,-15,10,-11], [10,-4,10,8,10,12],
        [8,17,-8,17,-10,12], [-10,4,-10,-5,-10,-11]]); c.fill();
      c.strokeStyle = cut; c.lineWidth = 2;
      c.beginPath(); c.ellipse(0,-10,8,2.3,0,0.05,Math.PI); c.stroke();
      c.fillStyle = cut;
      silhouette(c, [0,-7], [[-2,-3,-6,2,-6,5], [-6,12,6,12,6,5], [6,2,2,-3,0,-7]]); c.fill();
      c.restore();
    };
    barrel(-11, 5, 0.85); barrel(9, -3, 1);
    c.restore();
  }

  function fire(c, ink, cut) {
    c.save(); c.translate(0, 3);
    c.fillStyle = ink;
    silhouette(c, [-17,9], [[-24,-3,-9,-10,-17,-24], [-4,-23,4,-16,5,-5],
      [9,-10,13,-16,9,-20], [30,-10,19,4,15,11], [6,18,-10,18,-17,9]]); c.fill();
    c.fillStyle = cut;
    silhouette(c, [-5,11], [[-11,2,0,0,1,-11], [12,-1,7,4,6,9],
      [3,15,-2,15,-5,11]]); c.fill();
    c.restore();
  }

  function acid(c, ink, cut) {
    c.save(); c.rotate(0.4);
    c.fillStyle = ink;
    silhouette(c, [-14,-7], [[-10,-7,-6,-12,-2,-12], [4,-12,15,-12,19,-8],
      [22,-4,22,10,19,13], [15,16,-7,16,-11,12], [-14,9,-14,2,-14,-7]]); c.fill();
    polygon(c, [[-23,-10],[-16,-10],[-16,7],[-23,7]]); c.fill();
    polygon(c, [[-16,-5],[-9,-5],[-9,4],[-16,4]]); c.fill();
    c.strokeStyle = cut; c.lineWidth = 3.3; c.lineCap = 'round';
    c.beginPath(); c.moveTo(5,-3); c.lineTo(-1,7); c.lineTo(13,9); c.moveTo(5,-3); c.lineTo(13,9); c.stroke();
    c.fillStyle = cut;
    disc(c,5,-3,4); disc(c,-1,7,4); disc(c,13,9,4);
    c.restore();
  }

  function smoke(c, ink, cut) {
    c.save(); c.scale(0.94, 0.94);
    c.fillStyle = ink;
    silhouette(c, [-24,12], [[-24,7,-21,2,-16,1], [-20,-11,-5,-21,2,-12],
      [9,-21,22,-12,18,-4], [27,-5,27,7,20,10], [16,15,-16,16,-24,12]]); c.fill();
    c.strokeStyle = cut; c.lineWidth = 3.5; c.lineCap = 'round';
    c.beginPath(); c.moveTo(-10,-1); c.lineTo(11,-1); c.moveTo(-2,6); c.lineTo(17,6); c.stroke();
    c.restore();
  }

  function ion(c, ink) {
    // Native SW_Ion_SBUI alpha contour, traced with Potrace 1.16.
    // Alpha >= 128; turdsize 0, alphamax 1, opttolerance 0.2; 52-unit width.
    c.fillStyle = ink; c.beginPath();
    c.moveTo(-9.262,-23.319);
    c.bezierCurveTo(-10.265,-23.048,-11.158,-22.777,-11.24,-22.723);
    c.bezierCurveTo(-11.294,-22.642,-8.531,-18.715,-5.065,-13.948);
    c.bezierCurveTo(1.219,-5.335,1.219,-5.335,3.412,-3.9);
    c.lineTo(5.606,-2.492);
    c.lineTo(6.202,-0.081);
    c.bezierCurveTo(6.744,2.085,6.987,2.573,9.019,5.335);
    c.bezierCurveTo(10.238,7.015,11.321,8.423,11.429,8.45);
    c.bezierCurveTo(11.537,8.504,12.458,7.258,13.433,5.688);
    c.lineTo(15.221,2.844);
    c.lineTo(15.058,4.983);
    c.bezierCurveTo(14.977,6.175,14.95,7.069,15.004,7.015);
    c.bezierCurveTo(15.058,6.96,16.142,5.579,17.415,3.954);
    c.lineTo(19.717,0.975);
    c.lineTo(20.042,-2.004);
    c.bezierCurveTo(20.204,-3.656,20.421,-5.308,20.475,-5.688);
    c.bezierCurveTo(20.583,-6.229,20.312,-6.013,19.012,-4.631);
    c.bezierCurveTo(18.146,-3.683,17.36,-3.006,17.279,-3.142);
    c.bezierCurveTo(17.198,-3.277,16.927,-5.308,16.656,-7.665);
    c.bezierCurveTo(16.385,-9.994,16.088,-11.917,16.006,-11.917);
    c.bezierCurveTo(15.925,-11.917,14.923,-9.831,13.785,-7.285);
    c.lineTo(11.7,-2.681);
    c.lineTo(4.469,-13.135);
    c.bezierCurveTo(-0.162,-19.825,-2.925,-23.562,-3.196,-23.562);
    c.bezierCurveTo(-4.604,-23.535,-4.388,-23.183,4.604,-10.292);
    c.bezierCurveTo(6.852,-7.042,7.827,-5.525,6.987,-6.527);
    c.bezierCurveTo(6.798,-6.771,3.765,-10.752,0.271,-15.383);
    c.bezierCurveTo(-6.825,-24.835,-6.013,-24.158,-9.262,-23.319);
    c.closePath();
    c.moveTo(-21.45,-17.929);
    c.bezierCurveTo(-22.425,-17.225,-23.319,-16.575,-23.427,-16.467);
    c.bezierCurveTo(-23.535,-16.385,-22.885,-15.221,-21.992,-13.894);
    c.bezierCurveTo(-18.417,-8.558,-10.86,2.979,-10.969,2.979);
    c.bezierCurveTo(-11.023,2.979,-11.7,2.085,-12.485,1.002);
    c.bezierCurveTo(-18.823,-7.773,-23.915,-14.625,-24.077,-14.625);
    c.bezierCurveTo(-24.212,-14.625,-24.51,-14.408,-24.754,-14.11);
    c.bezierCurveTo(-25.188,-13.623,-25.025,-13.352,-20.8,-7.556);
    c.bezierCurveTo(-13.217,2.817,-9.425,8.152,-9.506,8.233);
    c.bezierCurveTo(-9.56,8.26,-12.54,8.098,-16.115,7.854);
    c.bezierCurveTo(-19.69,7.583,-22.642,7.421,-22.696,7.448);
    c.bezierCurveTo(-22.777,7.529,-20.529,8.748,-15.248,11.537);
    c.bezierCurveTo(-13.867,12.269,-12.729,12.946,-12.729,13.027);
    c.bezierCurveTo(-12.729,13.135,-13.298,13.271,-14.002,13.379);
    c.bezierCurveTo(-16.223,13.677,-18.715,14.137,-18.633,14.219);
    c.bezierCurveTo(-18.579,14.273,-16.467,14.896,-13.948,15.6);
    c.lineTo(-9.371,16.873);
    c.lineTo(-4.956,16.223);
    c.bezierCurveTo(-2.519,15.871,-0.542,15.519,-0.542,15.437);
    c.bezierCurveTo(-0.542,15.329,-1.49,14.977,-2.627,14.598);
    c.bezierCurveTo(-4.523,14.002,-4.658,13.921,-3.927,13.785);
    c.bezierCurveTo(-3.467,13.677,-2.14,13.596,-0.948,13.569);
    c.bezierCurveTo(0.244,13.542,1.652,13.433,2.194,13.325);
    c.lineTo(3.142,13.135);
    c.lineTo(-0.921,7.177);
    c.lineTo(-5.01,1.219);
    c.lineTo(-8.26,-0.731);
    c.bezierCurveTo(-10.021,-1.815,-11.619,-2.844,-11.808,-3.033);
    c.bezierCurveTo(-11.971,-3.223,-12.567,-5.335,-13.135,-7.719);
    c.lineTo(-14.192,-12.052);
    c.lineTo(-16.602,-15.654);
    c.bezierCurveTo(-17.929,-17.604,-19.175,-19.229,-19.337,-19.229);
    c.bezierCurveTo(-19.5,-19.202,-20.448,-18.633,-21.45,-17.929);
    c.closePath();
    c.moveTo(-13.162,-17.875);
    c.bezierCurveTo(-13.081,-17.496,-12.242,-14.246,-11.321,-10.644);
    c.lineTo(-9.615,-4.117);
    c.lineTo(-4.658,-1.165);
    c.lineTo(0.298,1.76);
    c.lineTo(2.925,5.2);
    c.bezierCurveTo(4.36,7.123,5.579,8.558,5.633,8.423);
    c.bezierCurveTo(5.715,8.179,3.385,-1.327,3.196,-1.463);
    c.bezierCurveTo(3.169,-1.517,0.704,-3.115,-2.275,-5.01);
    c.bezierCurveTo(-6.608,-7.827,-7.773,-8.721,-8.315,-9.588);
    c.bezierCurveTo(-8.667,-10.21,-9.94,-12.458,-11.131,-14.625);
    c.bezierCurveTo(-12.35,-16.792,-13.244,-18.254,-13.162,-17.875);
    c.closePath();
    c.moveTo(20.583,3.494);
    c.bezierCurveTo(19.933,3.737,19.933,3.737,20.529,3.765);
    c.bezierCurveTo(23.156,3.873,19.256,8.531,13.162,12.594);
    c.bezierCurveTo(5.904,17.415,-4.333,21.152,-6.5,19.771);
    c.bezierCurveTo(-6.852,19.554,-6.852,19.392,-6.419,18.633);
    c.lineTo(-5.904,17.74);
    c.lineTo(-6.852,18.66);
    c.bezierCurveTo(-7.394,19.148,-8.098,20.096,-8.423,20.719);
    c.bezierCurveTo(-8.965,21.721,-8.992,21.992,-8.721,22.587);
    c.bezierCurveTo(-8.26,23.644,-7.258,23.915,-4.333,23.752);
    c.bezierCurveTo(2.627,23.427,15.14,17.117,21.558,10.725);
    c.bezierCurveTo(24.592,7.665,25.756,5.065,24.565,3.873);
    c.bezierCurveTo(24.077,3.358,21.64,3.142,20.583,3.494);
    c.closePath();
    c.fill();
  }

  const GLYPHS = { sticky_oil_bomb: oil, incendiary_bomb: fire, acid_blast: acid, smoke_bomb: smoke, ion_blast: ion };

  function band(c, half, r, corner) {
    c.beginPath();
    c.moveTo(-half+corner,-r); c.lineTo(half-corner,-r);
    c.arc(half-corner,-r+corner,corner,-Math.PI/2,0);
    c.lineTo(half,r-corner); c.arc(half-corner,r-corner,corner,0,Math.PI/2);
    c.lineTo(-half+corner,r); c.arc(-half+corner,r-corner,corner,Math.PI/2,Math.PI);
    c.lineTo(-half,-r+corner); c.arc(-half+corner,-r+corner,corner,Math.PI,Math.PI*1.5);
    c.closePath();
  }

  function draw(c, d, tick, tps, team, pixelsPerMetre = 1) {
    const stage = state(d, tick, tps);
    const glyph = GLYPHS[d.kind];
    if (!stage || !glyph || d.r <= 0) return;
    const pixel = 1 / Math.max(0.3, pixelsPerMetre);
    // The badge is a screen-sized label; the footprint alone encodes range.
    const R = Math.min(d.r * 0.75, 22 * pixel);
    const half = d.length / 2, corner = d.r * 0.22;
    const rgb = team.rgb;
    const striking = d.kind === 'ion_blast' && !stage.countdown;
    c.save(); c.lineJoin = 'round'; c.lineCap = 'round';
    if (striking) c.globalAlpha *= 0.38;
    c.translate(d.x,d.y); c.rotate(d.angle);
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
    c.restore();

    // The two release controls remain visible inside their true end caps.
    c.save(); c.lineCap = 'round';
    if (striking) c.globalAlpha *= 0.38;
    for (const [i, p] of d.points.entries()) {
      const end = i === 0 || i === d.points.length-1;
      if (d.kind === 'ion_blast' && !end) continue;
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
    c.strokeStyle = `rgba(${rgb},0.35)`; c.lineWidth = pixel;
    c.beginPath(); c.arc(0,0,R,0,Math.PI*2); c.stroke();
    if (stage.countdown) {
      c.strokeStyle = `rgba(${rgb},0.85)`; c.lineWidth = 1.8 * pixel;
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

  return { prepare, state, draw, strike, drawStrike };
})();
