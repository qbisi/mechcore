// Plays the timeline embedded in this page.
//
// Everything drawn is a function of one moment, `tau`, measured in ticks: an
// object's place is read off its tracks between the two ticks around it, and
// every flash, recoil, swing and explosion is a function of how long ago the
// recorded event that causes it happened. Nothing accumulates between frames,
// so the page plays at any speed, backwards when scrubbed, and seeks anywhere
// at once.

'use strict';

(() => {
  const data = JSON.parse(document.getElementById('timeline').textContent);
  const TPS = data.ticks_per_second;
  const LAST = data.ticks;
  const TEAMS = Sprites.TEAMS;
  const NAMES = {
    marksman: ['Marksman', '长弓'], arclight: ['Arclight', '弧光'], rhino: ['Rhino', '犀牛'],
    crawler: ['Crawler', '爬虫'], sledgehammer: ['Sledgehammer', '铁锤'], wasp: ['Wasp', '兵蜂'],
    energy_tower: ['Energy Tower', '能量塔'], research_center: ['Research Center', '研究中心'],
    anti_armor_turret: ['Anti-Armor Turret', '反装甲炮'], rapid_fire_turret: ['Rapid-Fire Turret', '速射炮'],
    defensive_wall: ['Defensive Wall', '防御墙'], magnetic_barrier: ['Magnetic Barrier', '磁力路障'],
  };
  const MOTION = ['idle', 'moving', 'attacking', 'stopped', 'transitioning'];
  // A melee strike's swing: how long before the blow it starts, and how long
  // it lasts, in seconds; the model's attack point and backswing.
  const SWING = { rhino: [0.42, 0.9], crawler: [0.2, 0.5] };
  const DEFAULT_SWING = [0.25, 0.6];
  const LONGEST_EFFECT = 3 * TPS;

  // ------------------------------------------------------------ the tracks
  function decode(steps, n, scale = 1) {
    const out = new Float64Array(n);
    let i = 0;
    let value = 0;
    for (const step of steps) {
      if (typeof step === 'string') {
        const run = Number(step);
        for (let j = 0; j < run && i < n; j++) out[i++] = value * scale;
      } else {
        value += step;
        if (i < n) out[i++] = value * scale;
      }
    }
    while (i < n) out[i++] = value * scale;
    return out;
  }
  const byRef = new Map();
  const ref = (name) => (name ? byRef.get(name) : undefined);

  const units = data.units.map((u) => {
    const n = u.to - u.from + 1;
    const o = {
      what: 'unit', id: u.id, team: u.team, kind: u.kind, air: u.air, formation: u.formation,
      radius: u.radius / 100, maxLife: u.max_life, from: u.from, to: u.to, n,
      X: decode(u.x, n, 0.01), Y: decode(u.y, n, 0.01), Z: decode(u.z, n, 0.01),
      B: decode(u.body, n), R: u.turret ? decode(u.turret, n) : null,
      L: decode(u.life, n), S: u.shield ? decode(u.shield, n) : null,
      Mo: decode(u.motion, n), A: decode(u.aim, n),
      fires: [], strikes: [], hits: [], death: null,
    };
    o.D = new Float64Array(n);
    for (let i = 1; i < n; i++) o.D[i] = o.D[i - 1] + Math.hypot(o.X[i] - o.X[i - 1], o.Z[i] - o.Z[i - 1]);
    byRef.set(`u${u.id}`, o);
    return o;
  });
  const buildings = data.buildings.map((b) => {
    const n = b.to - b.from + 1;
    const o = {
      what: 'building', id: b.id, team: b.team, kind: b.kind, group: b.group,
      x: b.x / 100, z: b.z / 100, width: b.width / 100, depth: b.depth / 100,
      maxLife: b.max_life, from: b.from, to: b.to, n, L: decode(b.life, n),
      fires: [], aims: [], hits: [], fall: null,
      rest: b.team === 0 ? 0 : 1800,
    };
    byRef.set(`b${b.id}`, o);
    return o;
  });
  const projectiles = new Map();
  for (const p of data.projectiles) {
    const n = p.to - p.from + 1;
    const o = {
      what: 'projectile', id: p.id, team: p.team, owner: p.owner, target: p.target,
      from: p.from, to: p.to, n,
      X: decode(p.x, n, 0.01), Y: decode(p.y, n, 0.01), Z: decode(p.z, n, 0.01), gone: null, by: null,
    };
    projectiles.set(p.id, o);
    byRef.set(`p${p.id}`, o);
  }
  const shields = data.shields.map((s) => {
    const n = s.to - s.from + 1;
    const o = {
      what: 'shield', id: s.id, team: s.team, source: s.source, owner: s.owner, from: s.from, to: s.to, n,
      X: decode(s.x, n, 0.01), Z: decode(s.z, n, 0.01), Rad: decode(s.radius, n, 0.01),
      E: decode(s.energy, n), Emax: decode(s.max_energy, n), Act: decode(s.active, n),
      down: null,
    };
    byRef.set(`s${s.id}`, o);
    return o;
  });

  // ------------------------------------------------------------- the cues
  const cues = data.cues;
  const decals = [];
  const fallen = new Map();
  for (let i = 0; i < cues.length; i++) {
    const c = cues[i];
    c.seed = i;
    switch (c.k) {
      case 'fire': {
        const owner = ref(c.by);
        const projectile = projectiles.get(c.p);
        if (owner) owner.fires.push(c.t);
        if (projectile) projectile.by = owner || null;
        if (owner && owner.what === 'building' && projectile) {
          const dx = projectile.X[0] - owner.x;
          const dz = projectile.Z[0] - owner.z;
          if (dx * dx + dz * dz > 0.01) owner.aims.push([c.t, (Math.atan2(dx, dz) * 1800) / Math.PI]);
        }
        break;
      }
      case 'hit': {
        const target = ref(c.at);
        if (target && target.hits) target.hits.push(c.t);
        const by = ref(c.by);
        if (by && by.what === 'unit' && c.p === undefined && by.strikes[by.strikes.length - 1] !== c.t) {
          by.strikes.push(c.t);
        }
        break;
      }
      case 'gone': {
        const projectile = projectiles.get(c.p);
        if (projectile) { projectile.gone = c; c.by = projectile.by; }
        break;
      }
      case 'die': {
        const unit = ref(`u${c.u}`);
        if (unit) { unit.death = c; c.unit = unit; }
        decals.push(c);
        break;
      }
      case 'fall': {
        const building = ref(`b${c.b}`);
        if (building) { building.fall = c; c.building = building; fallen.set(c.b, c); }
        decals.push(c);
        break;
      }
      case 'shield_down': {
        const shield = ref(`s${c.s}`);
        if (shield) shield.down = c;
        break;
      }
      default:
    }
  }

  // ------------------------------------------------------------- sampling
  // The slot of tick `tau` in an object's tracks and how far past it.
  function slot(o, tau) {
    if (tau < o.from || tau >= o.to + 1) return null;
    const k = Math.floor(tau);
    return [k - o.from, tau - k];
  }
  const lerp = (A, i, f, n) => (i + 1 < n ? A[i] + (A[i + 1] - A[i]) * f : A[i]);
  function angle(A, i, f, n) {
    const a = A[i];
    if (i + 1 >= n) return a;
    const d = ((((A[i + 1] - a) % 3600) + 5400) % 3600) - 1800;
    return a + d * f;
  }
  const rad = (tenths) => (tenths * Math.PI) / 1800;
  function blendAngle(a, b, f) {
    const d = ((((b - a) % 3600) + 5400) % 3600) - 1800;
    return a + d * f;
  }
  // The last index of a sorted list at or before `t`, or -1.
  function before(list, t) {
    let lo = 0;
    let hi = list.length - 1;
    let found = -1;
    while (lo <= hi) {
      const mid = (lo + hi) >> 1;
      const at = Array.isArray(list[mid]) ? list[mid][0] : list[mid];
      if (at <= t) { found = mid; lo = mid + 1; } else hi = mid - 1;
    }
    return found;
  }
  function firstCue(t) {
    let lo = 0;
    let hi = cues.length;
    while (lo < hi) {
      const mid = (lo + hi) >> 1;
      if (cues[mid].t < t) lo = mid + 1; else hi = mid;
    }
    return lo;
  }

  function unitAt(u, tau) {
    const s = slot(u, tau);
    if (!s) return null;
    const [i, f] = s;
    const n = u.n;
    const body = angle(u.B, i, f, n);
    return {
      x: lerp(u.X, i, f, n), z: lerp(u.Z, i, f, n), y: lerp(u.Y, i, f, n),
      body, turret: u.R ? angle(u.R, i, f, n) : body,
      life: u.L[i], shield: u.S ? u.S[i] : 0, motion: u.Mo[i], aim: u.A[i],
      walk: lerp(u.D, i, f, n), speed: i + 1 < n ? (u.D[i + 1] - u.D[i]) * TPS : 0,
    };
  }
  // Where an object named by a cue stood at tick `t`, for an effect there.
  function placeOf(name, t) {
    const o = ref(name);
    if (!o) return null;
    if (o.what === 'unit') {
      const s = unitAt(o, Math.min(Math.max(t, o.from), o.to));
      return s && { x: s.x, z: s.z, air: o.air, y: s.y };
    }
    if (o.what === 'building') return { x: o.x, z: o.z };
    if (o.what === 'shield') {
      const i = Math.min(Math.max(t, o.from), o.to) - o.from;
      return { x: o.X[i], z: o.Z[i] };
    }
    return null;
  }
  // A projectile's position, from the muzzle one tick before its first
  // recorded place to where it was removed.
  function projectileAt(p, tau) {
    if (tau < p.from - 1 || tau >= (p.gone ? p.gone.t : p.to + 1)) return null;
    if (tau < p.from) {
      const f = tau - (p.from - 1);
      const back = p.n > 1 ? 1 : 0;
      const ox = p.X[0] - (p.X[back] - p.X[0]);
      const oy = p.Y[0] - (p.Y[back] - p.Y[0]);
      const oz = p.Z[0] - (p.Z[back] - p.Z[0]);
      return { x: ox + (p.X[0] - ox) * f, y: oy + (p.Y[0] - oy) * f, z: oz + (p.Z[0] - oz) * f };
    }
    const k = Math.floor(tau);
    const i = k - p.from;
    const f = tau - k;
    if (i >= p.n - 1) {
      const last = p.n - 1;
      const g = p.gone;
      if (!g) return { x: p.X[last], y: p.Y[last], z: p.Z[last] };
      const end = { x: g.x / 100, y: g.y / 100, z: g.z / 100 };
      return { x: p.X[last] + (end.x - p.X[last]) * f, y: p.Y[last] + (end.y - p.Y[last]) * f, z: p.Z[last] + (end.z - p.Z[last]) * f };
    }
    return { x: lerp(p.X, i, f, p.n), y: lerp(p.Y, i, f, p.n), z: lerp(p.Z, i, f, p.n) };
  }

  // How a unit or building is moving its weapons at `tau`.
  function weaponState(o, tau, pose) {
    // A shot leaves between the tick before its cue and the cue's tick.
    const last = before(o.fires, tau + 1);
    if (last >= 0) {
      pose.fireAge = (tau - (o.fires[last] - 1)) / TPS;
      pose.fireIndex = last;
    }
    const next = last + 1 < o.fires.length ? o.fires[last + 1] - 1 : Infinity;
    pose.chargeIn = (next - tau) / TPS;
    if (o.strikes && o.strikes.length) {
      const [windup, total] = SWING[o.kind] || DEFAULT_SWING;
      const k = before(o.strikes, tau + windup * TPS + 0.5);
      for (const j of [k, k - 1]) {
        if (j < 0) continue;
        const blow = o.strikes[j] - 0.5;
        const s = ((tau - blow) / TPS + windup) / total;
        if (s >= 0 && s <= 1) { pose.strike = s; pose.strikeIndex = j; break; }
      }
    }
    const hit = before(o.hits, tau);
    pose.hurt = hit >= 0 ? Math.max(0, 1 - (tau - o.hits[hit]) / (0.15 * TPS)) : 0;
  }

  // Where a turret construction points: toward each shot it fires, turned
  // in the half second before it.
  function buildingTurret(b, tau) {
    const turn = 0.5 * TPS;
    const k = before(b.aims, tau);
    const prev = k >= 0 ? b.aims[k][1] : b.rest;
    const next = b.aims[k + 1];
    if (next && next[0] - tau < turn) return blendAngle(prev, next[1], Sprites.ease(1 - (next[0] - tau) / turn));
    return prev;
  }

  // ------------------------------------------------------------- the view
  const canvas = document.getElementById('field');
  const ctx = canvas.getContext('2d');
  const view = { x: 0, z: 0, s: 1, w: 1, h: 1, dpr: 1 };
  const options = { life: true, damage: false, aim: false };
  let tau = 1;
  let playing = true;
  let speed = 1;
  let hover = null;
  let pointer = null;

  function fit() {
    const hw = data.field.half_width / 100;
    const hd = data.field.half_depth / 100;
    const top = 12;
    const bottom = 60;
    view.s = Math.max(0.3, Math.min(view.w / (2 * hw), (view.h - top - bottom) / (2 * hd)));
    view.x = 0;
    view.z = ((top - bottom) / 2) / view.s;
  }
  function resize() {
    view.dpr = window.devicePixelRatio || 1;
    view.w = canvas.clientWidth || window.innerWidth;
    view.h = canvas.clientHeight || window.innerHeight;
    canvas.width = Math.round(view.w * view.dpr);
    canvas.height = Math.round(view.h * view.dpr);
  }
  const sx = (x) => view.w / 2 + (x - view.x) * view.s;
  const sy = (z) => view.h / 2 - (z - view.z) * view.s;
  const wx = (px) => view.x + (px - view.w / 2) / view.s;
  const wz = (py) => view.z - (py - view.h / 2) / view.s;
  // Draw in metres at a world point, turned by a recorded facing.
  function place(x, z, facingTenths = 0, enlarge = 1) {
    const k = view.s * view.dpr * enlarge;
    const a = rad(facingTenths);
    const cos = Math.cos(a) * k;
    const sin = Math.sin(a) * k;
    ctx.setTransform(cos, sin, -sin, cos, sx(x) * view.dpr, sy(z) * view.dpr);
  }
  function screen() {
    ctx.setTransform(view.dpr, 0, 0, view.dpr, 0, 0);
  }

  // A deterministic stream of numbers for one effect.
  function random(seed) {
    let a = (seed * 2654435761) >>> 0;
    return () => {
      a = (a + 0x6d2b79f5) >>> 0;
      let t = a;
      t = Math.imul(t ^ (t >>> 15), t | 1);
      t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
      return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
    };
  }

  // ------------------------------------------------------------ the ground
  // The map, simplified: a ground of worn plates and sand, each side's half
  // washed in its colour, and the centre line.
  const ground = (() => {
    const hw = data.field.half_width / 100;
    const hd = data.field.half_depth / 100;
    const res = 3;
    const g = document.createElement('canvas');
    g.width = Math.round(hw * 2 * res);
    g.height = Math.round(hd * 2 * res);
    const c = g.getContext('2d');
    const rnd = random(7);
    c.fillStyle = '#1b2125';
    c.fillRect(0, 0, g.width, g.height);
    const blob = (x, y, r, colour, alpha) => {
      const gr = c.createRadialGradient(x, y, 0, x, y, r);
      gr.addColorStop(0, colour);
      gr.addColorStop(1, 'rgba(0,0,0,0)');
      c.globalAlpha = alpha;
      c.fillStyle = gr;
      c.fillRect(x - r, y - r, r * 2, r * 2);
    };
    for (let i = 0; i < 70; i++) {
      const colour = ['#2b3438', '#13181b', '#33312a', '#262d24'][i % 4];
      blob(rnd() * g.width, rnd() * g.height, (20 + rnd() * 70) * res, colour, 0.55);
    }
    c.globalAlpha = 1;
    // plates: a loose pattern of worn concrete slabs
    for (let i = 0; i < 260; i++) {
      const w = (8 + rnd() * 26) * res;
      const h = (8 + rnd() * 26) * res;
      c.fillStyle = rnd() < 0.5 ? 'rgba(255,255,255,0.025)' : 'rgba(0,0,0,0.06)';
      c.fillRect(Math.round(rnd() * g.width), Math.round(rnd() * g.height), w, h);
    }
    for (let i = 0; i < 5000; i++) {
      c.fillStyle = rnd() < 0.5 ? 'rgba(255,255,255,0.05)' : 'rgba(0,0,0,0.12)';
      c.fillRect(rnd() * g.width, rnd() * g.height, res * 0.5, res * 0.5);
    }
    // the two halves
    for (const [team, top] of [[1, true], [0, false]]) {
      const gr = c.createLinearGradient(0, top ? 0 : g.height, 0, g.height / 2);
      gr.addColorStop(0, `rgba(${TEAMS[team].rgb},0.10)`);
      gr.addColorStop(1, `rgba(${TEAMS[team].rgb},0.0)`);
      c.fillStyle = gr;
      c.fillRect(0, top ? 0 : g.height / 2, g.width, g.height / 2);
    }
    // the centre line
    c.fillStyle = 'rgba(255,255,255,0.035)';
    c.fillRect(0, g.height / 2 - 4 * res, g.width, 8 * res);
    c.strokeStyle = 'rgba(255,255,255,0.16)';
    c.lineWidth = 0.6 * res;
    c.setLineDash([6 * res, 6 * res]);
    c.beginPath();
    c.moveTo(0, g.height / 2);
    c.lineTo(g.width, g.height / 2);
    c.stroke();
    c.setLineDash([]);
    // the edge of the field
    const v = c.createRadialGradient(g.width / 2, g.height / 2, Math.min(g.width, g.height) * 0.35, g.width / 2, g.height / 2, Math.max(g.width, g.height) * 0.75);
    v.addColorStop(0, 'rgba(0,0,0,0)');
    v.addColorStop(1, 'rgba(0,0,0,0.55)');
    c.fillStyle = v;
    c.fillRect(0, 0, g.width, g.height);
    return { canvas: g, hw, hd, res };
  })();

  function drawGround() {
    screen();
    ctx.fillStyle = '#0e1114';
    ctx.fillRect(0, 0, view.w, view.h);
    const k = view.s * view.dpr;
    ctx.setTransform(k / ground.res, 0, 0, k / ground.res, sx(-ground.hw) * view.dpr, sy(ground.hd) * view.dpr);
    ctx.imageSmoothingEnabled = true;
    ctx.drawImage(ground.canvas, 0, 0);
    // the deployment grid
    screen();
    const minor = view.s > 1.6;
    ctx.lineWidth = 1;
    for (const step of minor ? [10, 50] : [50]) {
      ctx.strokeStyle = step === 50 ? 'rgba(255,255,255,0.06)' : 'rgba(255,255,255,0.025)';
      ctx.beginPath();
      for (let x = Math.ceil(-ground.hw / step) * step; x <= ground.hw; x += step) {
        if (step === 10 && x % 50 === 0) continue;
        const p = Math.round(sx(x)) + 0.5;
        ctx.moveTo(p, sy(ground.hd));
        ctx.lineTo(p, sy(-ground.hd));
      }
      for (let z = Math.ceil(-ground.hd / step) * step; z <= ground.hd; z += step) {
        if (step === 10 && z % 50 === 0) continue;
        const p = Math.round(sy(z)) + 0.5;
        ctx.moveTo(sx(-ground.hw), p);
        ctx.lineTo(sx(ground.hw), p);
      }
      ctx.stroke();
    }
  }

  // What fights leave on the ground: a scorch where a unit died, rubble
  // where a building fell.
  function drawDecals() {
    for (const d of decals) {
      if (d.t > tau) break;
      const age = (tau - d.t) / TPS;
      const x = d.x / 100;
      const z = d.z / 100;
      const size = d.k === 'fall'
        ? Math.max(d.building ? d.building.width : 8, 6) * 0.8
        : Math.max(2.5, (d.unit ? d.unit.radius : 3) * 1.3) * (d.unit && d.unit.air ? 0.7 : 1);
      const alpha = Math.max(0.28, 0.75 - age * 0.02);
      place(x, z, (d.seed * 977) % 3600);
      const rnd = random(d.seed);
      ctx.globalAlpha = alpha;
      const g = ctx.createRadialGradient(0, 0, 0, 0, 0, size);
      g.addColorStop(0, 'rgba(8,8,8,0.9)');
      g.addColorStop(0.6, 'rgba(20,18,16,0.5)');
      g.addColorStop(1, 'rgba(0,0,0,0)');
      ctx.fillStyle = g;
      ctx.beginPath();
      ctx.arc(0, 0, size, 0, Math.PI * 2);
      ctx.fill();
      ctx.fillStyle = d.k === 'fall' ? '#3a3f47' : '#2a2d33';
      const pieces = d.k === 'fall' ? 14 : 5;
      for (let i = 0; i < pieces; i++) {
        const a = rnd() * Math.PI * 2;
        const r = rnd() * size * 0.7;
        const w = (0.3 + rnd() * 0.6) * size * 0.25;
        ctx.fillRect(Math.sin(a) * r - w / 2, Math.cos(a) * r - w / 2, w, w * (0.5 + rnd()));
      }
      ctx.globalAlpha = 1;
    }
  }

  // ------------------------------------------------------------ the scene
  const pose = Sprites.pose();
  function resetPose(team) {
    const p = pose;
    p.t = tau / TPS;
    p.team = TEAMS[team] || TEAMS[0];
    p.turret = 0;
    p.speed = 0;
    p.walk = 0;
    p.fireAge = Infinity;
    p.fireIndex = 0;
    p.chargeIn = Infinity;
    p.strike = -1;
    p.strikeIndex = 0;
    p.attacking = false;
    p.life = 1;
    p.hurt = 0;
    p.facing = 0;
    return p;
  }

  function drawBuildings(bars) {
    for (const b of buildings) {
      const s = slot(b, tau);
      if (!s) continue;
      const p = resetPose(b.team);
      p.life = b.L[s[0]] / b.maxLife;
      weaponState(b, tau, p);
      const facing = b.kind === 'defensive_wall' || b.kind === 'research_center' || b.kind === 'energy_tower'
        ? b.rest : buildingTurret(b, tau);
      // a construction's chassis keeps its rest facing; only its gun turns
      p.turret = rad(facing - b.rest);
      place(b.x, b.z, b.rest);
      // shadow
      ctx.fillStyle = 'rgba(0,0,0,0.18)';
      ctx.beginPath();
      ctx.ellipse(1.0, 1.4, b.width * 0.5, b.depth * 0.5, 0, 0, Math.PI * 2);
      ctx.fill();
      Sprites.drawBuilding(ctx, b.kind, p, b.width, b.depth);
      if (p.hurt > 0) hurtFlash(Math.max(b.width, b.depth) * 0.55, p.hurt);
      // a building shows its life once it has lost some, as a unit does
      if (b.L[s[0]] < b.maxLife) {
        bars.push([b.x, b.z, Math.max(b.width, b.depth) * 0.55, b.L[s[0]], b.maxLife, b.team, true]);
      }
    }
  }

  function hurtFlash(r, k) {
    ctx.save();
    ctx.globalCompositeOperation = 'lighter';
    ctx.globalAlpha = 0.55 * k;
    const g = ctx.createRadialGradient(0, 0, 0, 0, 0, r);
    g.addColorStop(0, 'rgba(255,255,255,0.9)');
    g.addColorStop(1, 'rgba(255,255,255,0)');
    ctx.fillStyle = g;
    ctx.beginPath();
    ctx.arc(0, 0, r, 0, Math.PI * 2);
    ctx.fill();
    ctx.restore();
  }

  const AIR_SHADOW = [0.10, -0.16];
  // Zoomed far out a unit would shrink to a few pixels: it is drawn larger,
  // up to half again its size, so its silhouette still reads.
  const enlargement = () => Math.max(1, Math.min(1.6, 2.2 / view.s));
  function drawUnits(air, bars, aims) {
    const big = enlargement();
    for (const u of units) {
      if (u.air !== air) continue;
      const s = unitAt(u, tau);
      if (!s) continue;
      const p = resetPose(u.team);
      p.facing = rad(s.body);
      p.turret = rad(s.turret - s.body);
      p.speed = s.speed;
      p.walk = s.walk + u.id * 1.7;
      p.attacking = s.motion === 2;
      p.life = s.life / u.maxLife;
      weaponState(u, tau, p);
      // a unit fades out over the tick it dies in
      const dying = u.death && tau > u.to ? 1 - (tau - u.to) : 1;
      const alt = air ? s.y : 0;
      if (air) {
        // its shadow on the ground, cast away from the sun by its height
        place(s.x + alt * AIR_SHADOW[0], s.z + alt * AIR_SHADOW[1], s.body, big);
        ctx.globalAlpha = 0.24 * dying;
        ctx.fillStyle = '#000';
        ctx.beginPath();
        ctx.ellipse(0, -1, u.radius * 0.45, u.radius * 1.3, 0, 0, Math.PI * 2);
        ctx.ellipse(0, 0, u.radius * 1.2, u.radius * 0.35, 0, 0, Math.PI * 2);
        ctx.fill();
      } else {
        place(s.x, s.z, s.body, big);
        ctx.globalAlpha = 0.14 * dying;
        ctx.fillStyle = '#000';
        ctx.beginPath();
        ctx.ellipse(0.4, 0.6, u.radius * 0.75, u.radius * 0.85, 0, 0, Math.PI * 2);
        ctx.fill();
      }
      ctx.globalAlpha = dying;
      place(s.x, s.z, s.body, big);
      Sprites.drawUnit(ctx, u.kind, p, u.radius);
      if (p.hurt > 0) hurtFlash(u.radius * 0.9, p.hurt);
      if (s.shield > 0) {
        ctx.globalAlpha = 0.5 * dying;
        ctx.strokeStyle = `rgba(${TEAMS[u.team].rgb},0.9)`;
        ctx.lineWidth = 0.35;
        ctx.beginPath();
        ctx.arc(0, 0, u.radius * 1.15, 0, Math.PI * 2);
        ctx.stroke();
      }
      ctx.globalAlpha = 1;
      if (s.life < u.maxLife) bars.push([s.x, s.z, u.radius * big, s.life, u.maxLife, u.team, false]);
      if (aims && s.aim) aims.push([s, u, s.aim]);
    }
  }

  // A projectile's look follows the weapon that fired it.
  const SHOTS = {
    marksman: { colour: Sprites.GLOW.marksman, trail: 34, head: 0.9, width: 0.7 },
    sledgehammer: { colour: '#ffb347', trail: 9, head: 1.0, width: 0.9 },
    wasp: { colour: Sprites.GLOW.wasp, trail: 7, head: 0.55, width: 0.45 },
    rapid_fire_turret: { colour: '#ffd36b', trail: 10, head: 0.45, width: 0.4 },
    anti_armor_turret: { colour: '#ff9b3d', trail: 12, head: 1.3, width: 1.1 },
    energy_tower: { colour: '#6ff0ff', trail: 14, head: 1.1, width: 0.9 },
  };
  const DEFAULT_SHOT = { colour: '#f4f4f4', trail: 8, head: 0.6, width: 0.5 };

  function lightning(x1, z1, x2, z2, seed, colour, width) {
    const rnd = random(seed);
    const dx = x2 - x1;
    const dz = z2 - z1;
    const len = Math.hypot(dx, dz);
    if (len < 0.5) return;
    const nx = -dz / len;
    const nz = dx / len;
    const steps = Math.max(3, Math.round(len / 3));
    const pts = [];
    for (let i = 0; i <= steps; i++) {
      const f = i / steps;
      const j = i === 0 || i === steps ? 0 : (rnd() - 0.5) * Math.min(4, len * 0.18);
      pts.push([sx(x1 + dx * f + nx * j), sy(z1 + dz * f + nz * j)]);
    }
    screen();
    ctx.save();
    ctx.globalCompositeOperation = 'lighter';
    ctx.lineJoin = 'round';
    for (const [w, a, c] of [[width * 4, 0.25, colour], [width * 1.6, 0.8, colour], [width * 0.6, 1, '#ffffff']]) {
      ctx.beginPath();
      pts.forEach(([px, py], i) => (i ? ctx.lineTo(px, py) : ctx.moveTo(px, py)));
      ctx.lineWidth = Math.max(0.6, w * view.s);
      ctx.strokeStyle = c;
      ctx.globalAlpha = a;
      ctx.stroke();
    }
    ctx.restore();
  }

  // Where an Arclight's bolt leaves: the muzzle of the cannon down the
  // middle of its turret, as sprites.js draws it.
  function muzzle(u, tau2) {
    const s = unitAt(u, Math.min(Math.max(tau2, u.from), u.to));
    if (!s) return null;
    const a = rad(s.turret);
    const forward = 6.0;
    // local -y forward to world (x, z)
    return [s.x + forward * Math.sin(a), s.z + forward * Math.cos(a)];
  }

  function drawProjectiles() {
    const frameSeed = Math.floor(performance.now() / 45);
    for (const p of projectiles.values()) {
      if (tau < p.from - 1 || tau > p.to + 2) continue;
      const head = projectileAt(p, tau);
      if (!head) continue;
      const kind = p.by ? p.by.kind : '';
      if (kind === 'arclight') {
        const from = p.by.what === 'unit' ? muzzle(p.by, tau) : null;
        const start = from || [head.x, head.z];
        lightning(start[0], start[1], head.x, head.z, p.id * 31 + frameSeed, Sprites.GLOW.arclight, 0.35);
        place(head.x, head.z);
        Sprites.glow(ctx, 0, 0, 3.2, Sprites.GLOW.arclight, 1);
        continue;
      }
      const look = SHOTS[kind] || DEFAULT_SHOT;
      const tail = projectileAt(p, Math.max(p.from - 1, tau - look.trail / 40));
      screen();
      ctx.save();
      ctx.globalCompositeOperation = 'lighter';
      ctx.lineCap = 'round';
      const g = ctx.createLinearGradient(sx(tail.x), sy(tail.z), sx(head.x), sy(head.z));
      g.addColorStop(0, 'rgba(255,255,255,0)');
      g.addColorStop(1, look.colour);
      ctx.strokeStyle = g;
      ctx.lineWidth = Math.max(1, look.width * view.s);
      ctx.beginPath();
      ctx.moveTo(sx(tail.x), sy(tail.z));
      ctx.lineTo(sx(head.x), sy(head.z));
      ctx.stroke();
      ctx.restore();
      place(head.x, head.z);
      Sprites.glow(ctx, 0, 0, look.head * 3, look.colour, 1);
      ctx.fillStyle = '#ffffff';
      ctx.beginPath();
      ctx.arc(0, 0, look.head * 0.5, 0, Math.PI * 2);
      ctx.fill();
    }
  }

  function shieldAt(sh, tau2) {
    const s = slot(sh, tau2);
    if (!s) return null;
    const [i, f] = s;
    return {
      x: lerp(sh.X, i, f, sh.n), z: lerp(sh.Z, i, f, sh.n), r: lerp(sh.Rad, i, f, sh.n),
      e: sh.E[i], max: sh.Emax[i], active: sh.Act[i] > 0,
    };
  }

  function drawShields() {
    for (const sh of shields) {
      const s = shieldAt(sh, tau);
      if (!s) continue;
      const T = TEAMS[sh.team] || TEAMS[0];
      const frac = s.max > 0 ? Math.max(0, Math.min(1, s.e / s.max)) : 0;
      if (sh.source === 'contraption') {
        place(s.x, s.z);
        Sprites.shieldGenerator(ctx, T, tau / TPS);
      }
      screen();
      const cx = sx(s.x);
      const cy = sy(s.z);
      const r = s.r * view.s;
      ctx.save();
      if (!s.active) {
        ctx.setLineDash([6, 6]);
        ctx.strokeStyle = `rgba(${T.rgb},0.3)`;
        ctx.lineWidth = 1;
        ctx.beginPath();
        ctx.arc(cx, cy, r, 0, Math.PI * 2);
        ctx.stroke();
        ctx.restore();
        continue;
      }
      const g = ctx.createRadialGradient(cx, cy, r * 0.2, cx, cy, r);
      g.addColorStop(0, `rgba(${T.rgb},0.02)`);
      g.addColorStop(0.85, `rgba(${T.rgb},${0.05 + 0.07 * frac})`);
      g.addColorStop(1, `rgba(${T.rgb},${0.12 + 0.16 * frac})`);
      ctx.fillStyle = g;
      ctx.beginPath();
      ctx.arc(cx, cy, r, 0, Math.PI * 2);
      ctx.fill();
      // the hexagonal skin
      ctx.clip();
      ctx.strokeStyle = `rgba(${T.rgb},${0.05 + 0.06 * frac})`;
      ctx.lineWidth = 1;
      const hex = 7 * view.s;
      if (hex > 4) {
        ctx.beginPath();
        const h = hex * Math.sqrt(3) / 2;
        for (let row = -Math.ceil(r / h) - 1; row <= Math.ceil(r / h) + 1; row++) {
          for (let col = -Math.ceil(r / (hex * 3)) - 1; col <= Math.ceil(r / (hex * 3)) + 1; col++) {
            const hx = cx + col * hex * 3 + (row % 2 ? hex * 1.5 : 0);
            const hy = cy + row * h;
            for (let k = 0; k < 6; k++) {
              const a = (k * Math.PI) / 3;
              const px = hx + Math.cos(a) * hex;
              const py = hy + Math.sin(a) * hex;
              if (k === 0) ctx.moveTo(px, py); else ctx.lineTo(px, py);
            }
            ctx.closePath();
          }
        }
        ctx.stroke();
      }
      ctx.restore();
      const shimmer = 0.5 + 0.2 * Math.sin(tau / TPS * 2 + sh.id);
      ctx.strokeStyle = `rgba(${T.rgb},${(0.35 + 0.45 * frac) * shimmer + 0.15})`;
      ctx.lineWidth = Math.max(1, 0.5 * view.s);
      ctx.beginPath();
      ctx.arc(cx, cy, r, 0, Math.PI * 2);
      ctx.stroke();
      // energy left, as an arc on the rim
      ctx.strokeStyle = `rgba(${T.rgb},0.9)`;
      ctx.lineWidth = Math.max(2, 0.9 * view.s);
      ctx.beginPath();
      ctx.arc(cx, cy, r + Math.max(3, 1.2 * view.s), -Math.PI / 2, -Math.PI / 2 + Math.PI * 2 * frac);
      ctx.stroke();
    }
  }

  // ---------------------------------------------------------- the effects
  function burst(x, z, age, life, radius, colour, seed, debris = 0) {
    if (age < 0 || age > life) return;
    const k = age / life;
    place(x, z);
    ctx.save();
    ctx.globalCompositeOperation = 'lighter';
    const core = radius * (0.4 + 0.8 * Sprites.ease(Math.min(1, k * 3)));
    const g = ctx.createRadialGradient(0, 0, 0, 0, 0, core);
    g.addColorStop(0, `rgba(255,255,240,${1 - k})`);
    g.addColorStop(0.35, colour);
    g.addColorStop(1, 'rgba(0,0,0,0)');
    ctx.globalAlpha = Math.max(0, 1 - k * 1.1);
    ctx.fillStyle = g;
    ctx.beginPath();
    ctx.arc(0, 0, core, 0, Math.PI * 2);
    ctx.fill();
    ctx.globalAlpha = Math.max(0, 0.8 - k);
    ctx.strokeStyle = colour;
    ctx.lineWidth = radius * 0.12 * (1 - k);
    ctx.beginPath();
    ctx.arc(0, 0, radius * (0.6 + 1.6 * k), 0, Math.PI * 2);
    ctx.stroke();
    if (debris) {
      const rnd = random(seed);
      ctx.fillStyle = '#ffd9a0';
      ctx.globalAlpha = Math.max(0, 1 - k);
      for (let i = 0; i < debris; i++) {
        const a = rnd() * Math.PI * 2;
        const v = radius * (1 + rnd() * 2.2);
        const d = v * Sprites.ease(Math.min(1, k * 1.5));
        const sz = radius * 0.06 * (1 + rnd());
        ctx.fillRect(Math.sin(a) * d - sz / 2, -Math.cos(a) * d - sz / 2, sz, sz);
      }
    }
    ctx.restore();
  }

  function smoke(x, z, age, life, radius, seed) {
    if (age < 0 || age > life) return;
    const k = age / life;
    const rnd = random(seed + 99);
    place(x, z);
    for (let i = 0; i < 5; i++) {
      const a = rnd() * Math.PI * 2;
      const d = radius * 0.6 * rnd() * (0.4 + k);
      const r = radius * (0.5 + 0.9 * k) * (0.6 + rnd() * 0.5);
      ctx.globalAlpha = 0.35 * (1 - k);
      ctx.fillStyle = '#2b2b2e';
      ctx.beginPath();
      ctx.arc(Math.sin(a) * d, -Math.cos(a) * d - k * radius * 0.6, r, 0, Math.PI * 2);
      ctx.fill();
    }
    ctx.globalAlpha = 1;
  }

  function drawEffects(numbers) {
    const start = firstCue(tau - LONGEST_EFFECT);
    for (let i = start; i < cues.length; i++) {
      const c = cues[i];
      if (c.t > tau + 1) break;
      const age = (tau - c.t) / TPS;
      switch (c.k) {
        case 'gone': {
          if (age < 0) break;
          const x = c.x / 100;
          const z = c.z / 100;
          if (c.s) {
            shieldRipple(c.s, x, z, age, c.seed);
            break;
          }
          if (c.intercepted) { burst(x, z, age, 0.35, 2.5, '#ffffff', c.seed, 6); break; }
          const kind = c.by ? c.by.kind : '';
          if (kind === 'sledgehammer') { smoke(x, z, age, 1.6, 4.5, c.seed); burst(x, z, age, 0.55, 5, '#ff8a2a', c.seed, 10); }
          else if (kind === 'anti_armor_turret') { smoke(x, z, age, 1.6, 4, c.seed); burst(x, z, age, 0.6, 4.5, '#ff7a2a', c.seed, 10); }
          else if (kind === 'arclight') arcSplash(x, z, age, c.seed);
          else if (kind === 'marksman') burst(x, z, age, 0.35, 3.2, Sprites.GLOW.marksman, c.seed, 6);
          else if (kind === 'wasp') burst(x, z, age, 0.3, 2.0, '#ffcf40', c.seed, 4);
          else burst(x, z, age, 0.25, 1.6, '#ffe08a', c.seed, 3);
          break;
        }
        case 'hit': {
          if (age < 0 || age > 0.9) break;
          if (c.p === undefined && c.by) {
            const by = ref(c.by);
            const at = placeOf(c.at, c.t);
            if (by && at && by.what === 'unit') slash(at.x, at.z, age, by.kind, c.seed, by.team);
          }
          if (numbers) {
            const at = placeOf(c.at, c.t);
            if (at) numbers.push([at.x, at.z, age, c.n, c.seed]);
          }
          break;
        }
        case 'die': {
          if (age < 0) break;
          const u = c.unit;
          const r = Math.max(2.5, (u ? u.radius : 3) * 1.5);
          const x = c.x / 100;
          const z = c.z / 100;
          smoke(x, z, age, 2.4, r * 1.1, c.seed);
          burst(x, z, age, 0.9, r, '#ff9a3a', c.seed, 14);
          burst(x, z, age - 0.08, 0.6, r * 0.6, '#fff1b0', c.seed + 1);
          break;
        }
        case 'fall': {
          if (age < 0) break;
          const b = c.building;
          const r = Math.max(6, b ? Math.max(b.width, b.depth) * 0.7 : 8);
          const x = c.x / 100;
          const z = c.z / 100;
          smoke(x, z, age, 3, r * 1.2, c.seed);
          for (let j = 0; j < 3; j++) {
            const rnd = random(c.seed * 7 + j);
            burst(x + (rnd() - 0.5) * r, z + (rnd() - 0.5) * r, age - j * 0.18, 1.1, r * (0.7 + 0.3 * rnd()), '#ff8f2e', c.seed + j, 12);
          }
          break;
        }
        case 'shield_down': {
          if (age < 0 || age > 1.2) break;
          const sh = ref(`s${c.s}`);
          const last = sh ? sh.n - 1 : 0;
          const r = sh ? sh.Rad[last] : 20;
          const T = TEAMS[sh ? sh.team : 0];
          screen();
          const k = age / 1.2;
          ctx.save();
          ctx.globalCompositeOperation = 'lighter';
          ctx.strokeStyle = `rgba(${T.rgb},${1 - k})`;
          ctx.lineWidth = Math.max(1, 1.5 * view.s * (1 - k));
          ctx.beginPath();
          ctx.arc(sx(c.x / 100), sy(c.z / 100), r * view.s * (1 + 0.15 * k), 0, Math.PI * 2);
          ctx.stroke();
          const rnd = random(c.seed);
          ctx.fillStyle = `rgba(${T.rgb},${0.8 * (1 - k)})`;
          for (let j = 0; j < 40; j++) {
            const a = rnd() * Math.PI * 2;
            const d = r * (0.9 + 0.35 * k * rnd());
            ctx.fillRect(sx(c.x / 100 + Math.sin(a) * d) - 2, sy(c.z / 100 + Math.cos(a) * d) - 2, 4, 4);
          }
          ctx.restore();
          break;
        }
        default:
      }
    }
  }

  function shieldRipple(id, x, z, age, seed) {
    const life = 0.6;
    if (age > life) return;
    const sh = ref(`s${id}`);
    const T = TEAMS[sh ? sh.team : 0];
    const k = age / life;
    place(x, z);
    ctx.save();
    ctx.globalCompositeOperation = 'lighter';
    ctx.globalAlpha = 1 - k;
    ctx.strokeStyle = `rgba(${T.rgb},1)`;
    ctx.lineWidth = 0.5;
    for (let j = 0; j < 2; j++) {
      ctx.beginPath();
      const r = 2 + 9 * Sprites.ease(Math.min(1, k + j * 0.25));
      for (let h = 0; h < 6; h++) {
        const a = (h * Math.PI) / 3 + seed;
        const px = Math.cos(a) * r;
        const py = Math.sin(a) * r;
        if (h === 0) ctx.moveTo(px, py); else ctx.lineTo(px, py);
      }
      ctx.closePath();
      ctx.stroke();
    }
    Sprites.glow(ctx, 0, 0, 5, `rgba(${T.rgb},1)`, 1 - k);
    ctx.restore();
  }

  function arcSplash(x, z, age, seed) {
    const life = 0.3;
    if (age > life) return;
    const k = age / life;
    const rnd = random(seed);
    const frame = Math.floor(performance.now() / 50);
    for (let j = 0; j < 4; j++) {
      const a = rnd() * Math.PI * 2;
      const d = 3 + rnd() * 4;
      lightning(x, z, x + Math.sin(a) * d, z + Math.cos(a) * d, seed * 13 + j + frame, Sprites.GLOW.arclight, 0.2 * (1 - k));
    }
    place(x, z);
    Sprites.glow(ctx, 0, 0, 7, Sprites.GLOW.arclight, 1 - k);
  }

  // A melee blow where it landed: a rhino's blade cuts an arc, a crawler's
  // drill throws sparks.
  function slash(x, z, age, kind, seed, team) {
    const life = 0.3;
    if (age > life) return;
    const k = age / life;
    place(x, z, (seed * 1373) % 3600);
    ctx.save();
    ctx.globalCompositeOperation = 'lighter';
    ctx.globalAlpha = 1 - k;
    const glow = Sprites.GLOW[kind] || '#ffffff';
    if (kind === 'rhino') {
      ctx.strokeStyle = glow;
      ctx.lineWidth = 1.2 * (1 - k);
      ctx.beginPath();
      ctx.arc(0, 0, 5 + 3 * k, -2.2, -0.6);
      ctx.stroke();
      ctx.strokeStyle = '#ffffff';
      ctx.lineWidth = 0.4 * (1 - k);
      ctx.stroke();
    } else {
      const rnd = random(seed);
      ctx.strokeStyle = glow;
      ctx.lineWidth = 0.18;
      ctx.beginPath();
      for (let j = 0; j < 6; j++) {
        const a = rnd() * Math.PI * 2;
        const d = 0.6 + 2.4 * k * (0.5 + rnd());
        ctx.moveTo(Math.sin(a) * d * 0.4, -Math.cos(a) * d * 0.4);
        ctx.lineTo(Math.sin(a) * d, -Math.cos(a) * d);
      }
      ctx.stroke();
    }
    Sprites.glow(ctx, 0, 0, 3, glow, 1 - k);
    ctx.restore();
  }

  // ----------------------------------------------------------- overlays
  function drawBars(bars) {
    if (!options.life) return;
    screen();
    for (const [x, z, r, life, max, team, building] of bars) {
      const w = Math.max(building ? 26 : 14, Math.min(60, r * 2 * view.s));
      const h = building ? 4 : 3;
      const px = sx(x) - w / 2;
      const py = sy(z) - r * view.s - (building ? 10 : 7);
      const frac = Math.max(0, Math.min(1, life / max));
      ctx.fillStyle = 'rgba(0,0,0,0.6)';
      ctx.fillRect(px - 1, py - 1, w + 2, h + 2);
      ctx.fillStyle = frac > 0.5 ? TEAMS[team].accent : frac > 0.25 ? '#f2b33d' : '#ff5a4a';
      ctx.fillRect(px, py, w * frac, h);
    }
  }

  function drawAims(aims) {
    screen();
    ctx.save();
    ctx.setLineDash([3, 4]);
    for (const [s, u, aim] of aims) {
      const target = aim > 0 ? ref(`u${aim}`) : ref(`b${-aim}`);
      if (!target) continue;
      let tx;
      let tz;
      if (target.what === 'unit') {
        const t = unitAt(target, tau);
        if (!t) continue;
        tx = t.x; tz = t.z;
      } else { tx = target.x; tz = target.z; }
      ctx.strokeStyle = `rgba(${TEAMS[u.team].rgb},0.45)`;
      ctx.lineWidth = 1;
      ctx.beginPath();
      ctx.moveTo(sx(s.x), sy(s.z));
      ctx.lineTo(sx(tx), sy(tz));
      ctx.stroke();
    }
    ctx.restore();
  }

  function drawNumbers(numbers) {
    screen();
    ctx.save();
    ctx.textAlign = 'center';
    ctx.font = '600 11px system-ui, sans-serif';
    for (const [x, z, age, n, seed] of numbers) {
      const k = age / 0.9;
      const jitter = ((seed * 37) % 11) - 5;
      ctx.globalAlpha = 1 - k * k;
      ctx.fillStyle = '#000';
      ctx.fillText(String(n), sx(x) + jitter + 1, sy(z) - 10 - k * 22 + 1);
      ctx.fillStyle = '#ffe9a8';
      ctx.fillText(String(n), sx(x) + jitter, sy(z) - 10 - k * 22);
    }
    ctx.restore();
  }

  // ---------------------------------------------------------------- frame
  function frame() {
    Sprites.setScale(1 / (view.s * view.dpr));
    drawGround();
    drawDecals();
    const bars = [];
    const aims = options.aim ? [] : null;
    const numbers = options.damage ? [] : null;
    drawBuildings(bars);
    drawUnits(false, bars, aims);
    drawShields();
    drawProjectiles();
    drawEffects(numbers);
    drawUnits(true, bars, aims);
    screen();
    drawBars(bars);
    if (aims) drawAims(aims);
    if (numbers) drawNumbers(numbers);
    drawHover();
  }

  // --------------------------------------------------------------- the HUD
  const clock = document.getElementById('clock');
  const seek = document.getElementById('seek');
  const playButton = document.getElementById('play');
  seek.max = String(LAST);
  document.getElementById('meta').textContent =
    `${data.producer} · round ${data.round} · ${LAST} ticks · ${(LAST / TPS).toFixed(1)} s`;

  const roster = [0, 1].map((team) => {
    const panel = document.getElementById(`side-${team}`);
    const head = document.createElement('h2');
    const name = document.createElement('b');
    name.textContent = TEAMS[team].name;
    const count = document.createElement('span');
    head.append(name, count);
    const list = document.createElement('div');
    list.className = 'roster';
    const kinds = new Map();
    for (const u of units) {
      if (u.team !== team) continue;
      if (!kinds.has(u.kind)) kinds.set(u.kind, []);
      kinds.get(u.kind).push(u);
    }
    const rows = [];
    for (const [kind, members] of kinds) {
      const row = document.createElement('div');
      row.className = 'kind';
      const label = NAMES[kind] ? `${NAMES[kind][0]} ${NAMES[kind][1]}` : kind;
      row.title = label;
      row.append(Sprites.icon(kind, team, 22, false));
      const n = document.createElement('span');
      row.append(n);
      list.append(row);
      rows.push({ row, n, members, last: '' });
    }
    panel.append(head, list);
    return { count, rows };
  });

  function updateHud() {
    for (const side of roster) {
      let alive = 0;
      let total = 0;
      for (const r of side.rows) {
        let n = 0;
        for (const u of r.members) if (tau >= u.from && tau < u.to + 1) n++;
        alive += n;
        total += r.members.length;
        const text = `${n}/${r.members.length}`;
        if (text !== r.last) {
          r.n.textContent = text;
          r.row.classList.toggle('out', n === 0);
          r.last = text;
        }
      }
      side.count.textContent = `${alive} / ${total} units`;
    }
    const seconds = (tau - 1) / TPS;
    const m = Math.floor(seconds / 60);
    const sec = (seconds - m * 60).toFixed(1).padStart(4, '0');
    clock.textContent = `${m}:${sec} · tick ${Math.floor(tau)}`;
    seek.value = String(tau);
    playButton.textContent = playing ? '❚❚' : '▶';
  }

  // the timeline's marks: who died when
  function drawMarks() {
    const marks = document.getElementById('marks');
    const dpr = window.devicePixelRatio || 1;
    marks.width = Math.round(marks.clientWidth * dpr);
    marks.height = Math.round(marks.clientHeight * dpr);
    const c = marks.getContext('2d');
    c.scale(dpr, dpr);
    const w = marks.clientWidth;
    const h = marks.clientHeight;
    c.fillStyle = 'rgba(255,255,255,0.06)';
    c.fillRect(0, h / 2 - 2, w, 4);
    for (const d of decals) {
      const team = d.unit ? d.unit.team : d.building ? d.building.team : 0;
      const x = ((d.t - 1) / Math.max(1, LAST - 1)) * w;
      c.fillStyle = `rgba(${TEAMS[team].rgb},${d.k === 'fall' ? 1 : 0.55})`;
      const tall = d.k === 'fall' ? 12 : 6;
      if (team === 0) c.fillRect(x, h / 2 + 2, d.k === 'fall' ? 2 : 1, tall);
      else c.fillRect(x, h / 2 - 2 - tall, d.k === 'fall' ? 2 : 1, tall);
    }
  }

  // ------------------------------------------------------------- hovering
  const tooltip = document.getElementById('tooltip');
  function pick(px, py) {
    const x = wx(px);
    const z = wz(py);
    let best = null;
    let bestD = Infinity;
    for (const u of units) {
      const s = unitAt(u, tau);
      if (!s) continue;
      const d = Math.hypot(s.x - x, s.z - z);
      const reach = Math.max(u.radius * 1.2, 8 / view.s);
      if (d < reach && d < bestD) { best = { o: u, state: s }; bestD = d; }
    }
    if (!best) {
      for (const b of buildings) {
        if (!slot(b, tau)) continue;
        if (Math.abs(b.x - x) < Math.max(b.width * 0.6, 5) && Math.abs(b.z - z) < Math.max(b.depth * 0.6, 5)) best = { o: b };
      }
    }
    return best;
  }
  function drawHover() {
    if (!hover || !pointer) { tooltip.hidden = true; return; }
    const o = hover.o;
    let lines;
    if (o.what === 'unit') {
      const s = unitAt(o, tau);
      if (!s) { tooltip.hidden = true; return; }
      screen();
      ctx.strokeStyle = 'rgba(255,255,255,0.8)';
      ctx.lineWidth = 1.5;
      ctx.beginPath();
      ctx.arc(sx(s.x), sy(s.z), Math.max(8, o.radius * 1.4 * view.s), 0, Math.PI * 2);
      ctx.stroke();
      const name = NAMES[o.kind] ? `${NAMES[o.kind][0]} <span class="dim">${NAMES[o.kind][1]}</span>` : o.kind;
      const aim = s.aim > 0 ? `unit ${s.aim}` : s.aim < 0 ? `building ${-s.aim}` : 'nothing';
      lines = [
        `<b>${name}</b> <span class="dim">#${o.id} · ${TEAMS[o.team].name}</span>`,
        `life ${Math.round(s.life)} / ${o.maxLife}${s.shield > 0 ? ` · shield ${Math.round(s.shield)}` : ''}`,
        `${MOTION[s.motion] || s.motion} · aiming at ${aim}`,
        `<span class="dim">x ${s.x.toFixed(1)} · z ${s.z.toFixed(1)}${o.air ? ` · height ${s.y.toFixed(0)}` : ''}</span>`,
      ];
    } else {
      const s = slot(o, tau);
      if (!s) { tooltip.hidden = true; return; }
      const name = NAMES[o.kind] ? `${NAMES[o.kind][0]} <span class="dim">${NAMES[o.kind][1]}</span>` : o.kind;
      lines = [
        `<b>${name}</b> <span class="dim">#${o.id} · ${TEAMS[o.team].name}</span>`,
        `life ${Math.round(o.L[s[0]])} / ${o.maxLife}`,
      ];
    }
    tooltip.innerHTML = lines.join('<br>');
    tooltip.hidden = false;
    const x = Math.min(pointer[0] + 16, window.innerWidth - tooltip.offsetWidth - 8);
    const y = Math.min(pointer[1] + 16, window.innerHeight - tooltip.offsetHeight - 70);
    tooltip.style.left = `${x}px`;
    tooltip.style.top = `${y}px`;
  }

  // -------------------------------------------------------------- control
  function setTau(value) {
    tau = Math.max(1, Math.min(LAST, value));
  }
  function setSpeed(value) {
    speed = value;
    document.getElementById('speed').value = String(value);
  }
  const SPEEDS = [0.25, 0.5, 1, 2, 4, 8];

  playButton.addEventListener('click', () => {
    if (!playing && tau >= LAST) setTau(1);
    playing = !playing;
  });
  seek.addEventListener('input', () => setTau(Number(seek.value)));
  document.getElementById('speed').addEventListener('change', (e) => setSpeed(Number(e.target.value)));
  document.getElementById('fit').addEventListener('click', fit);
  for (const key of ['life', 'damage', 'aim']) {
    const box = document.getElementById(`opt-${key}`);
    box.checked = options[key];
    box.addEventListener('change', () => { options[key] = box.checked; });
  }
  window.addEventListener('keydown', (e) => {
    if (e.target instanceof HTMLInputElement && e.target.type !== 'checkbox' && e.target.type !== 'range') return;
    const step = e.shiftKey ? TPS : 1;
    switch (e.key) {
      case ' ': playButton.click(); break;
      case 'ArrowRight': playing = false; setTau(Math.floor(tau) + step); break;
      case 'ArrowLeft': playing = false; setTau(Math.ceil(tau) - step); break;
      case 'Home': setTau(1); break;
      case 'End': setTau(LAST); break;
      case '+': case '=': setSpeed(SPEEDS[Math.min(SPEEDS.length - 1, SPEEDS.indexOf(speed) + 1)]); break;
      case '-': case '_': setSpeed(SPEEDS[Math.max(0, SPEEDS.indexOf(speed) - 1)]); break;
      case 'f': fit(); break;
      case 'l': case 'd': case 'a': {
        const key = { l: 'life', d: 'damage', a: 'aim' }[e.key];
        options[key] = !options[key];
        document.getElementById(`opt-${key}`).checked = options[key];
        break;
      }
      default: return;
    }
    e.preventDefault();
  });

  const pointers = new Map();
  let pinch = null;
  canvas.addEventListener('pointerdown', (e) => {
    canvas.setPointerCapture(e.pointerId);
    pointers.set(e.pointerId, [e.clientX, e.clientY]);
    canvas.classList.add('dragging');
    if (pointers.size === 2) {
      const [a, b] = [...pointers.values()];
      pinch = { d: Math.hypot(a[0] - b[0], a[1] - b[1]), s: view.s };
    }
  });
  canvas.addEventListener('pointermove', (e) => {
    pointer = [e.clientX, e.clientY];
    const last = pointers.get(e.pointerId);
    if (last) {
      if (pointers.size === 1) {
        view.x -= (e.clientX - last[0]) / view.s;
        view.z += (e.clientY - last[1]) / view.s;
      }
      pointers.set(e.pointerId, [e.clientX, e.clientY]);
      if (pointers.size === 2 && pinch) {
        const [a, b] = [...pointers.values()];
        view.s = Math.max(0.3, Math.min(40, pinch.s * Math.hypot(a[0] - b[0], a[1] - b[1]) / pinch.d));
      }
    }
    hover = pointers.size ? null : pick(e.clientX, e.clientY);
  });
  const release = (e) => {
    pointers.delete(e.pointerId);
    if (pointers.size < 2) pinch = null;
    if (!pointers.size) canvas.classList.remove('dragging');
  };
  canvas.addEventListener('pointerup', release);
  canvas.addEventListener('pointercancel', release);
  canvas.addEventListener('pointerleave', () => { pointer = null; hover = null; });
  canvas.addEventListener('dblclick', fit);
  canvas.addEventListener('wheel', (e) => {
    e.preventDefault();
    const x = wx(e.clientX);
    const z = wz(e.clientY);
    view.s = Math.max(0.3, Math.min(40, view.s * Math.exp(-e.deltaY * 0.0015)));
    view.x = x - (e.clientX - view.w / 2) / view.s;
    view.z = z + (e.clientY - view.h / 2) / view.s;
  }, { passive: false });

  window.addEventListener('resize', () => { resize(); fit(); drawMarks(); });
  resize();
  fit();
  drawMarks();

  let then = performance.now();
  let hudAt = 0;
  function tick(now) {
    const dt = Math.min(0.1, (now - then) / 1000);
    then = now;
    if (playing) {
      setTau(tau + dt * TPS * speed);
      if (tau >= LAST) playing = false;
    }
    try {
      frame();
      if (now - hudAt > 100 || !playing) { updateHud(); hudAt = now; }
    } finally {
      requestAnimationFrame(tick);
    }
  }
  requestAnimationFrame(tick);

  // For a reader of the page who wants the state, and for checking it.
  window.player = {
    data, units, buildings, projectiles, shields,
    seek: (t) => setTau(t), play: () => { playing = true; }, pause: () => { playing = false; },
    get tau() { return tau; }, view, fit,
  };
})();
