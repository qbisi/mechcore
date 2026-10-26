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

  const GLYPHS = { sticky_oil_bomb: oil, incendiary_bomb: fire, acid_blast: acid, smoke_bomb: smoke };

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
    c.save(); c.lineJoin = 'round'; c.lineCap = 'round';
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
    for (const [i, p] of d.points.entries()) {
      const end = i === 0 || i === d.points.length-1;
      c.fillStyle = `rgba(${rgb},${end ? 0.85 : 0.38})`;
      disc(c, p.x, p.y, (end ? 2.2 : 1.2) * pixel);
      if (end) {
        c.beginPath(); c.moveTo(p.x-5*pixel,p.y); c.lineTo(p.x+5*pixel,p.y);
        c.moveTo(p.x,p.y-5*pixel); c.lineTo(p.x,p.y+5*pixel);
        c.strokeStyle = `rgba(${rgb},0.7)`; c.lineWidth = pixel; c.stroke();
      }
    }
    // Upright glyphs share one quiet ring and one countdown arc on that ring.
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

  return { prepare, state, draw };
})();
