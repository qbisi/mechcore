// Recorded ground shapes, in metres. The same paths draw on Canvas and in
// SVG snapshots. Nothing persists between frames: seeking rebuilds the look
// from the union of recorded cells, world coordinates and the current tick.
'use strict';

const TerrainEffects = (() => {
  const CELL = 5;
  const HALF = CELL / 2;
  // Ground liquids first, then combustion and the translucent clouds.
  const MATERIALS = ['oil', 'acid', 'fire', 'fog_sand', 'fog'];
  // Cloud-layer opacity multiplies each texture's own translucent paint.
  const CLOUD_OPACITY = { fog: 0.95, fog_sand: 0.9 };
  const isCloud = (kind) => Object.hasOwn(CLOUD_OPACITY, kind);

  // The game's lattice rounds a half to the even integer, including below
  // zero. Math.round would move negative half-cell positions the other way.
  function roundEven(value) {
    const floor = Math.floor(value);
    const fraction = value - floor;
    return fraction < 0.5 ? floor : fraction > 0.5 ? floor + 1 : floor + (floor % 2 !== 0 ? 1 : 0);
  }

  function prepare(frame) {
    const x = frame.x / 100;
    const y = -frame.z / 100;
    const radius = frame.radius / 100;
    const originX = frame.grid ? frame.grid.x / 100 : roundEven((x - radius) / CELL) * CELL + HALF;
    const originZ = frame.grid ? frame.grid.z / 100 : roundEven((-y - radius) / CELL) * CELL + HALF;
    const keys = [];
    const add = (col, row) => {
      keys.push([col, row]);
    };
    if (frame.grid) {
      const g = frame.grid;
      for (let z = 0; z < g.height; z++) {
        for (let x = 0; x < g.width; x++) {
          if ((g.rows[z] >>> x) & 1) add(x, z);
        }
      }
    } else {
      // GridBlockInt builds a radius template at (r, r), then places it on
      // the world lattice. Use that same template for the display of whole
      // circles. Cell lists also admit circles wider than a 32-bit mask.
      const count = Math.floor(radius * 2 / CELL + 1);
      for (let row = 0; row < count; row++) {
        for (let col = 0; col < count; col++) {
          const dx = HALF + col * CELL - radius;
          const dz = HALF + row * CELL - radius;
          if (dx * dx + dz * dz <= radius * radius) add(col, row);
        }
      }
    }
    return footprint({ ...frame, x, y, radius }, originX, originZ, keys);
  }

  function footprint(shape, originX, originZ, keys) {
    const cells = keys.map(([col, row]) => [originX + col * CELL, -originZ - row * CELL]);
    const occupied = new Set(keys.map(([col, row]) => `${col},${row}`));
    // Only exposed sides belong to the rim, including the sides of holes.
    // Increasing world z moves upward on the page.
    const rim = [];
    const bounds = { left: Infinity, right: -Infinity, top: Infinity, bottom: -Infinity };
    keys.forEach(([col, row], i) => {
      const [cx, cy] = cells[i];
      const l = cx - HALF, r = cx + HALF, t = cy - HALF, b = cy + HALF;
      bounds.left = Math.min(bounds.left, l); bounds.right = Math.max(bounds.right, r);
      bounds.top = Math.min(bounds.top, t); bounds.bottom = Math.max(bounds.bottom, b);
      if (!occupied.has(`${col - 1},${row}`)) rim.push([l, t, l, b]);
      if (!occupied.has(`${col + 1},${row}`)) rim.push([r, t, r, b]);
      if (!occupied.has(`${col},${row - 1}`)) rim.push([l, b, r, b]);
      if (!occupied.has(`${col},${row + 1}`)) rim.push([l, t, r, t]);
    });
    return { ...shape, cells, occupied, rim, originX, originZ, bounds, samples: new Map() };
  }

  // One footprint per material: overlap is set union, irrespective of source
  // terrain ids, teams or the order in which circles were recorded.
  function merge(frames) {
    const materials = new Map();
    for (const f of frames) {
      if (!MATERIALS.includes(f.kind)) continue;
      if (!materials.has(f.kind)) materials.set(f.kind, new Map());
      const cells = materials.get(f.kind);
      for (const [x, y] of f.cells) {
        const col = Math.round((x - HALF) / CELL);
        const row = Math.round((-y - HALF) / CELL);
        cells.set(`${col},${row}`, [col, row]);
      }
    }
    const regions = [];
    // Fire remains visible where the recording holds both materials.
    for (const kind of MATERIALS) {
      const cells = materials.get(kind);
      if (!cells || !cells.size) continue;
      const keys = [...cells.values()].sort((a, b) => a[1] - b[1] || a[0] - b[0]);
      regions.push(footprint({ kind }, HALF, HALF, keys));
    }
    return regions;
  }

  function contains(f, x, y) {
    const col = Math.floor((x - f.originX + HALF) / CELL);
    const row = Math.floor((-y - f.originZ + HALF) / CELL);
    return f.occupied.has(`${col},${row}`);
  }

  function path(c, f) {
    c.beginPath();
    for (const [x, y] of f.cells) {
      c.moveTo(x - HALF, y - HALF);
      c.lineTo(x + HALF, y - HALF);
      c.lineTo(x + HALF, y + HALF);
      c.lineTo(x - HALF, y + HALF);
      c.closePath();
    }
  }

  function random(seed) {
    let a = seed >>> 0;
    return () => {
      a = (Math.imul(a, 1664525) + 1013904223) >>> 0;
      return a / 4294967296;
    };
  }

  function flame(c, x, y, width, height, lean) {
    c.beginPath();
    for (const side of [-1, 1]) {
      for (let k = 0; k <= 8; k++) {
        const t = side === -1 ? k / 8 : 1 - k / 8;
        const w = width * Math.sin((1 - t) * Math.PI * 0.7) * (1 - t * 0.6);
        const px = x + lean * t * t + side * w;
        const py = y - height * t;
        if (side === -1 && k === 0) c.moveTo(px, py); else c.lineTo(px, py);
      }
    }
    c.closePath();
    c.fill();
  }

  // The outline as one path, stroked once: within a single stroke the
  // overlapping cell sides do not darken where they meet. Under the clip only
  // the inner half of the stroke shows, an inset band along the edge.
  function inset(c, f, colour, width) {
    c.beginPath();
    for (const [x1, y1, x2, y2] of f.rim) {
      c.moveTo(x1, y1);
      c.lineTo(x2, y2);
    }
    c.strokeStyle = colour;
    c.lineWidth = width;
    c.stroke();
  }

  function glow(c, x, y, r, inner, outer) {
    const g = c.createRadialGradient(x, y, 0, x, y, r);
    g.addColorStop(0, inner);
    g.addColorStop(1, outer);
    c.fillStyle = g;
    c.fillRect(x - r, y - r, r * 2, r * 2);
  }

  // Each world cell has its own repeatable samples. Adding or removing a
  // neighbouring circle never rescatters the texture already on the ground.
  function scatter(f, salt, spacing) {
    const key = `${salt},${spacing}`;
    if (f.samples.has(key)) return f.samples.get(key);
    const points = [];
    for (const [cx, cy] of f.cells) {
      const col = Math.round((cx - HALF) / CELL);
      const row = Math.round((-cy - HALF) / CELL);
      const rnd = random(Math.imul(col, 73856093) ^ Math.imul(row, 19349663) ^ Math.imul(salt, 83492791));
      if (rnd() >= 1 / spacing) continue;
      const x = cx + (rnd() - 0.5) * CELL;
      const y = cy + (rnd() - 0.5) * CELL;
      const a = rnd(), b = rnd(), p = rnd() * Math.PI * 2;
      points.push([x, y, a, b, p]);
    }
    f.samples.set(key, points);
    return points;
  }

  function fill(c, f, colour) {
    const b = f.bounds;
    c.fillStyle = colour;
    c.fillRect(b.left, b.top, b.right - b.left, b.bottom - b.top);
  }

  // Oil: a still, near-black pool a shade cooler than the ground, its rim
  // lifted by a thin meniscus and its surface by a few faint, slowly drifting
  // sheens. Nothing on it is brighter than the deployment grid.
  function oil(c, f, time) {
    fill(c, f, 'rgba(14,18,23,0.91)');
    // The sky, faintly, in the still surface.
    for (const [x, y, a, b, p] of scatter(f, 1, 10)) {
      const dx = Math.sin(time * 0.35 + p) * 0.8;
      const dy = Math.cos(time * 0.27 + p) * 0.8;
      glow(c, x + dx, y + dy, 3 + a * 4,
        b < 0.6 ? 'rgba(78,98,126,0.16)' : 'rgba(122,100,66,0.11)', 'rgba(74,92,118,0)');
    }
    c.lineWidth = 0.25;
    for (const [x, y, a, b, p] of scatter(f, 2, 18)) {
      c.strokeStyle = b < 0.5 ? 'rgba(150,168,196,0.2)' : 'rgba(170,150,112,0.15)';
      c.beginPath();
      c.ellipse(x, y, 1.6 + a * 2.2, 0.5 + a * 0.6, p + Math.sin(time * 0.2 + p) * 0.15, 0.3, 2.4);
      c.stroke();
    }
    c.lineCap = 'square';
    inset(c, f, 'rgba(4,5,7,0.5)', 1.6);
    inset(c, f, 'rgba(140,156,178,0.28)', 0.3);
  }

  // Fire: scorched ground under slow ember beds, a char band and a thin hot
  // front along the edge, small low tongues of flame and flickering sparks.
  // The flames are muted and sparse, so units standing in fire stay legible.
  function fire(c, f, time) {
    fill(c, f, 'rgba(29,16,11,0.83)');
    for (const [x, y, a, , p] of scatter(f, 3, 6)) {
      const pulse = 0.75 + 0.25 * Math.sin(time * 1.6 + p);
      glow(c, x, y, 3 + a * 3.5, `rgba(176,62,22,${(0.26 * pulse).toFixed(3)})`, 'rgba(176,62,22,0)');
    }
    c.lineCap = 'square';
    inset(c, f, 'rgba(12,8,6,0.55)', 2.4);
    inset(c, f, 'rgba(222,112,48,0.3)', 0.35);
    for (const [x, y, a, b, p] of scatter(f, 4, 3)) {
      const size = 0.8 + a * 1.1;
      const pulse = 0.6 + 0.4 * Math.sin(time * (5 + b * 3) + p);
      const height = size * (1.4 + pulse);
      const lean = Math.sin(time * 2.3 + p) * size * 0.25;
      c.globalAlpha = 0.55 + 0.3 * pulse;
      c.fillStyle = 'rgb(196,78,30)';
      flame(c, x, y, size * 0.62, height, lean);
      c.fillStyle = 'rgb(230,140,58)';
      flame(c, x, y, size * 0.36, height * 0.66, lean * 0.6);
      c.fillStyle = 'rgb(246,206,132)';
      flame(c, x, y, size * 0.16, height * 0.34, lean * 0.3);
    }
    for (const [x, y, a, b, p] of scatter(f, 5, 4)) {
      const flicker = Math.sin(time * (7 + b * 6) + p);
      if (flicker <= 0.2) continue;
      c.globalAlpha = flicker * 0.7;
      c.fillStyle = 'rgb(255,178,96)';
      const s = 0.18 + a * 0.2;
      c.fillRect(x - s / 2, y - s / 2 - (time * 2 + p) % 1.5, s, s);
    }
    c.globalAlpha = 1;
  }

  // Acid: a muted green liquid, with slow colour changes and small bubbles.
  // Like oil its meniscus belongs only to the merged footprint's edge.
  function acid(c, f, time) {
    fill(c, f, 'rgba(35,53,19,0.64)');
    for (const [x, y, a, , p] of scatter(f, 6, 8)) {
      const pulse = 0.75 + Math.sin(time * 0.8 + p) * 0.25;
      glow(c, x, y, 3 + a * 4, `rgba(111,149,45,${(0.22 * pulse).toFixed(3)})`, 'rgba(111,149,45,0)');
    }
    c.lineWidth = 0.25;
    for (const [x, y, a, b, p] of scatter(f, 7, 10)) {
      const phase = (time * (0.22 + b * 0.1) + p) % 1;
      const r = 0.35 + a * 0.5 + phase * 0.7;
      c.strokeStyle = `rgba(173,194,93,${((1 - phase) * 0.32).toFixed(3)})`;
      c.beginPath();
      c.ellipse(x, y, r, r * 0.65, p, 0, Math.PI * 2);
      c.stroke();
    }
    c.lineCap = 'square';
    inset(c, f, 'rgba(16,28,9,0.45)', 1.4);
    inset(c, f, 'rgba(134,163,65,0.3)', 0.3);
  }

  // Smoke: cool, translucent banks drifting within the recorded footprint.
  // These wisps are surface detail, never the outline of a source circle.
  function smoke(c, f, time) {
    fill(c, f, 'rgba(99,115,129,0.16)');
    for (const [x, y, a, b, p] of scatter(f, 8, 8)) {
      const dx = Math.sin(time * 0.24 + p) * 3;
      const dy = Math.cos(time * 0.19 + p) * 1.8;
      glow(c, x + dx, y + dy, 7 + a * 8,
        b < 0.5 ? 'rgba(161,176,188,0.16)' : 'rgba(73,87,102,0.15)', 'rgba(122,140,156,0)');
    }
    c.lineWidth = 0.4;
    for (const [x, y, a, , p] of scatter(f, 9, 18)) {
      c.strokeStyle = 'rgba(174,187,197,0.13)';
      c.beginPath();
      c.ellipse(x + Math.sin(time * 0.25 + p) * 2, y,
        3 + a * 4, 1 + a, p * 0.15, 0.2 + time * 0.08, 2.7 + time * 0.08);
      c.stroke();
    }
    c.lineCap = 'square';
    inset(c, f, 'rgba(139,158,173,0.12)', 0.6);
  }

  // Sandstorm: warm haze, long wind trails and a little blowing grit. Its
  // 120-metre circles can span more than 32 cells; the union has no bit cap.
  function sand(c, f, time) {
    fill(c, f, 'rgba(135,101,53,0.2)');
    for (const [x, y, a, , p] of scatter(f, 10, 14)) {
      glow(c, x + Math.sin(time * 0.35 + p) * 4, y + Math.cos(time * 0.2 + p) * 2,
        8 + a * 8, 'rgba(187,145,80,0.13)', 'rgba(187,145,80,0)');
    }
    c.lineWidth = 0.3;
    c.strokeStyle = 'rgba(218,180,117,0.2)';
    for (const [x, y, a, , p] of scatter(f, 11, 10)) {
      const length = 4 + a * 8;
      const drift = Math.sin(time * 0.7 + p) * 3;
      c.beginPath();
      for (let k = 0; k <= 6; k++) {
        const u = k / 6 - 0.5;
        const px = x + u * length + drift;
        const py = y - u * length * 0.16 + Math.sin(u * 3 + p + time * 0.5) * 0.6;
        if (k === 0) c.moveTo(px, py); else c.lineTo(px, py);
      }
      c.stroke();
    }
    c.fillStyle = 'rgba(231,193,129,0.3)';
    for (const [x, y, a, , p] of scatter(f, 12, 6)) {
      const drift = (time * 2 + p) % 4;
      const size = 0.16 + a * 0.2;
      c.fillRect(x + drift - 2, y - drift * 0.15, size * 1.5, size);
    }
    c.lineCap = 'square';
    inset(c, f, 'rgba(173,139,86,0.16)', 0.6);
  }

  const PAINTERS = { oil, acid, fire, fog: smoke, fog_sand: sand };

  function draw(c, f, tick, tps) {
    const paint = PAINTERS[f.kind];
    if (!paint) return;
    if (!f.cells.length) return;
    const time = (tick - 1) / tps;
    c.save();
    c.globalAlpha *= CLOUD_OPACITY[f.kind] ?? 1;
    path(c, f);
    c.clip();
    paint(c, f, time);
    c.restore();
  }

  return { prepare, merge, contains, path, draw, isCloud };
})();
