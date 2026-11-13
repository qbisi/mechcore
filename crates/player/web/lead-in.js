// A match round's deployment, replayed before its first tick.
//
// `LeadIn.prepare` reads the timeline's `lead_in`: each side's decisions on
// its own clock, and what each did to the formations. `sample` says where
// every formation stands at a frame before tick 1 and which decision is
// playing on each side; `draw` paints them. A formation that reaches the fight
// is drawn with the recording's own members, their tick-1 places and facings
// carried to wherever the formation stood earlier, so the last frame of the
// lead-in is tick 1. Everything is a function of the frame: seeking and SVG
// export draw the same picture.
const LeadIn = (() => {
  const clamp01 = (t) => Math.max(0, Math.min(1, t));
  const span = (t, a, b) => clamp01((t - a) / (b - a));
  const easeOut = (t) => 1 - (1 - clamp01(t)) ** 3;
  const easeInOut = (t) => {
    const x = clamp01(t);
    return x < 0.5 ? 4 * x * x * x : 1 - (-2 * x + 2) ** 3 / 2;
  };
  const rad = (tenths) => (tenths * Math.PI) / 1800;
  const wrap = (tenths) => ((tenths % 3600) + 5400) % 3600 - 1800;
  const turnTo = (from, to, k) => from + wrap(to - from) * clamp01(k);
  const ANIMATED = new Set(['buy_unit', 'move_unit', 'upgrade_unit', 'upgrade_technology']);
  const TAGS = {
    choose_reinforce_item: 'REINFORCE',
    buy_unit: 'BUY',
    move_unit: 'MOVE',
    upgrade_unit: 'UPGRADE',
    unlock_unit: 'UNLOCK',
    upgrade_technology: 'TECH',
    active_blueprint: 'BLUEPRINT',
    active_energy_tower_skill: 'TOWER',
    strengthen_tower: 'FORTIFY',
    use_equipment: 'EQUIP',
    release_commander_skill: 'SKILL',
    release_contraption: 'CONTRAPTION',
  };
  const metres = (s) => s && { x: s.x / 100, z: s.z / 100, w: s.width / 100, d: s.depth / 100, facing: s.facing, level: s.level };

  function rectPath(c, x, y, w, h) {
    c.moveTo(x, y);
    c.lineTo(x + w, y);
    c.lineTo(x + w, y + h);
    c.lineTo(x, y + h);
    c.closePath();
  }

  function prepare(data, units = []) {
    const lead = data && data.lead_in;
    if (!lead || !lead.ticks) {
      return { active: false, ticks: 0, start: 1, supply: [0, 0], techs: [{}, {}], tracks: [[], []], formations: [] };
    }
    const start = 1 - lead.ticks;
    const techs = [0, 1].map((side) => (lead.techs && lead.techs[side]) || {});
    const formations = lead.formations.map((f, i) => ({
      i, team: f.team, index: f.index, kind: f.kind, opening: metres(f.opening), history: [],
    }));
    const tracks = lead.tracks.map((track, team) => track.map((e) => {
      const entry = { ...e, action: e.action || e.type, team, from: start + e.start, to: start + e.start + e.ticks };
      for (const change of e.changes || []) {
        const f = formations[change.formation];
        const before = f.history.length ? f.history[f.history.length - 1].after : f.opening;
        f.history.push({ entry, before, after: metres(change.to) });
      }
      entry.formations = (e.changes || []).map((c) => formations[c.formation]);
      return entry;
    }));
    for (const f of formations) {
      f.last = f.history.length ? f.history[f.history.length - 1].after : f.opening;
      const source = lead.formations[f.i].formation;
      // The fight's own members at tick 1, relative to where the deployment left them.
      f.members = source == null || !f.last ? [] : units
        .filter((u) => u.from === 1 && u.team === f.team && u.formation === source)
        .map((u) => ({
          dx: u.X[0] - f.last.x,
          dz: u.Z[0] - f.last.z,
          y: u.Y[0],
          body: u.B[0],
          turret: u.R ? u.R[0] : u.B[0],
          radius: u.radius,
          air: u.air,
          kind: u.kind,
          id: u.id,
        }));
      if (!f.members.length) {
        const facing = f.last ? f.last.facing : 0;
        f.members = [{
          dx: 0, dz: 0, y: 0, body: facing, turret: facing, radius: 2, air: false, kind: f.kind, id: f.i,
        }];
      }
      // A member's order of arrival: nearest the formation's centre first.
      const order = f.members.map((m, i) => [Math.hypot(m.dx, m.dz), i]).sort((a, b) => a[0] - b[0]);
      order.forEach(([, i], rank) => { f.members[i].rank = f.members.length > 1 ? rank / (f.members.length - 1) : 0; });
    }
    return { active: true, ticks: lead.ticks, start, supply: lead.supply || [0, 0], techs, tracks, formations };
  }

  function formationAnchor(fs) {
    const { stance, active } = fs;
    if (active && active.entry.action === 'move_unit' && active.before && active.after) {
      const travel = easeInOut(span(active.k, 0.15, 0.85));
      const x = active.before.x + (active.after.x - active.before.x) * travel;
      const z = active.before.z + (active.after.z - active.before.z) * travel;
      const d = active.before.d + (active.after.d - active.before.d) * travel;
      return { x, topZ: z + d / 2 };
    }
    const s = (active && active.after) || stance || (active && active.before);
    return s ? { x: s.x, topZ: s.z + s.d / 2 } : null;
  }

  // Returns the active or activating unit technologies to display above a formation during deployment.
  function formationTechs(model, state, fs) {
    if (!model || !state || !fs || !fs.f) return null;
    const { f, stance, active, doneChanges } = fs;
    const isBuying = !stance && active && active.entry.action === 'buy_unit';
    if (!stance && !isBuying) return null;
    const visAlpha = isBuying ? span(active.k, 0.55, 0.85) : 1;
    const outroAlpha = 1 - span(state.outro, 0.25, 0.85);
    const alpha = visAlpha * outroAlpha;
    if (alpha <= 0.01) return null;
    const anchor = formationAnchor(fs);
    if (!anchor) return null;

    const items = [];
    const seen = new Set();
    const initial = (model.techs && model.techs[f.team] && model.techs[f.team][f.kind]) || [];
    for (const id of initial) {
      if (seen.has(id)) continue;
      seen.add(id);
      items.push({ id, state: 'active', isNew: false, k: 1 });
    }
    for (const e of model.tracks[f.team] || []) {
      if (e.action !== 'upgrade_technology' || e.unit !== f.kind || e.tech == null) continue;
      if (seen.has(e.tech)) continue;
      if (state.t >= e.to) {
        seen.add(e.tech);
        items.push({ id: e.tech, state: 'active', isNew: true, k: 1 });
      } else if (state.t >= e.from && state.t < e.to) {
        const k = (state.t - e.from) / (e.to - e.from);
        if (k >= 0.15) {
          seen.add(e.tech);
          items.push({ id: e.tech, state: k >= 0.45 ? 'activating' : 'charging', isNew: true, k });
        }
      }
    }
    if (!items.length) return null;
    const hasPlate = Boolean(active && active.entry.action === 'upgrade_unit' && active.k >= 0.45);
    const hasLevelPill = Boolean(!active && doneChanges.some((dc) => dc.entry.action === 'upgrade_unit'));
    return { f, team: f.team, anchor, alpha, lift: hasPlate || hasLevelPill ? 24 : 5, items };
  }

  // Where each member stands with its formation at `s`: the tick-1 offsets
  // turned with the region's facing, turned once more if the formation was
  // rotated differently, and fitted to the footprint.
  function membersAt(f, s) {
    const last = f.last;
    const turn = wrap(s.facing - last.facing);
    const a = rad(turn), cos = Math.cos(a), sin = Math.sin(a);
    const quarter = Math.round(turn / 900) % 2 !== 0;
    let w = quarter ? last.d : last.w, d = quarter ? last.w : last.d;
    const square = Math.abs(w - d) < 0.01;
    const crossed = !square && (w > d) !== (s.w > s.d);
    if (crossed) [w, d] = [d, w];
    return f.members.map((m) => {
      let x = m.dx * cos + m.dz * sin;
      let z = -m.dx * sin + m.dz * cos;
      if (crossed) [x, z] = [z, -x];
      x *= w > 0 ? s.w / w : 1;
      z *= d > 0 ? s.d / d : 1;
      return { m, x: s.x + x, y: m.y, z: s.z + z, body: m.body + turn, turret: m.turret + turn };
    });
  }

  function sample(model, cursor) {
    if (!model || !model.active || cursor >= 1) return null;
    const t = cursor;
    const playing = model.tracks.map((track) => {
      const e = track.find((x) => t >= x.from && t < x.to);
      return e ? { entry: e, k: (t - e.from) / (e.to - e.from) } : null;
    });
    const formations = [];
    for (const f of model.formations) {
      let stance = f.opening, active = null;
      const doneChanges = [];
      for (const h of f.history) {
        if (t >= h.entry.to) {
          stance = h.after;
          doneChanges.push(h);
        } else {
          if (t >= h.entry.from) active = { ...h, k: (t - h.entry.from) / (h.entry.to - h.entry.from) };
          break;
        }
      }
      if (stance || active) formations.push({ f, stance, active, doneChanges });
    }
    const done = model.tracks.map((track) => track.filter((e) => t >= e.from));
    const completed = model.tracks.map((track) => track.filter((e) => t >= e.to));
    const outro = model.tracks.reduce((end, track) => Math.max(end, track.length ? track[track.length - 1].to : model.start), model.start);
    return { t, playing, formations, done, completed, outro: span(t, outro, 1), time: (t - model.start) };
  }

  function supplyAt(model, state, team, opening = model?.supply?.[team] ?? 0) {
    if (!state) return null;
    let supply = opening;
    for (const e of model.tracks[team]) {
      if (state.t >= e.from + (e.to - e.from) * 0.5) supply = e.supply;
      else break;
    }
    return supply;
  }

  function describe(entry, names = {}, techs = {}) {
    if (!entry) return null;
    const action = entry.action || entry.type;
    const placeholder = !ANIMATED.has(action);
    const kind = entry.unit || entry.formations?.[0]?.kind || entry.name;
    const unitLabel = (kind && names[kind]) || (kind ? kind.replaceAll('_', ' ') : '');
    let tag = TAGS[action] || action.toUpperCase();
    let label = (entry.name && (names[entry.name] || entry.name.replaceAll('_', ' '))) || unitLabel || tag;
    if (action === 'buy_unit') {
      const stance = entry.changes?.[0]?.to;
      if (stance && stance.facing % 1800 !== 0) tag = 'FLANK';
      label = unitLabel || label;
    } else if (action === 'move_unit') {
      label = unitLabel || 'Formation';
    } else if (action === 'upgrade_unit') {
      const to = entry.changes?.[0]?.to?.level;
      label = to ? `${unitLabel} Lv.${to}` : unitLabel;
    } else if (action === 'upgrade_technology') {
      const techName = (entry.tech != null && techs[entry.tech]?.name) || entry.name || 'Tech';
      label = unitLabel ? `${unitLabel} · ${techName}` : techName;
    }
    const delta = entry.spent > 0 ? `−${entry.spent}` : entry.spent < 0 ? `+${-entry.spent}` : '';
    return { tag, label, delta, placeholder };
  }

  // ------------------------------------------------------------- drawing
  // `h` carries the page's helpers: place(x, z, facing, enlarge), the scale,
  // the team colours, a unit painter and the HUD's natural ui scale.

  function corners(c, x0, y0, x1, y1, len) {
    for (const [x, y, sx, sy] of [[x0, y0, 1, 1], [x1, y0, -1, 1], [x0, y1, 1, -1], [x1, y1, -1, -1]]) {
      c.moveTo(x + sx * len, y);
      c.lineTo(x, y);
      c.lineTo(x, y + sy * len);
    }
  }

  function box(c, h, s, rgb, alpha, pixel, opts = {}) {
    const { place } = h;
    place(0, 0);
    const x0 = s.x - s.w / 2, x1 = s.x + s.w / 2, y0 = -(s.z + s.d / 2), y1 = -(s.z - s.d / 2);
    c.save();
    c.globalAlpha = alpha;
    if (opts.fill) {
      c.fillStyle = `rgba(${rgb},${opts.fill})`;
      c.fillRect(x0, y0, s.w, s.d);
    }
    c.strokeStyle = opts.colour || `rgb(${rgb})`;
    c.lineWidth = (opts.width || 1.5) * pixel;
    c.beginPath();
    if (opts.brackets !== undefined && opts.brackets < 1) {
      corners(c, x0, y0, x1, y1, Math.min(s.w, s.d) * (0.15 + 0.35 * opts.brackets));
    } else if (opts.dashed) {
      c.setLineDash?.([4 * pixel, 3 * pixel]);
      rectPath(c, x0, y0, s.w, s.d);
    } else {
      rectPath(c, x0, y0, s.w, s.d);
    }
    c.stroke();
    c.setLineDash?.([]);
    c.restore();
  }

  function drawFlankArrow(c, h, s, k, pixel) {
    const edgeX = s.x < 0 ? -360 : 360;
    const targetX = s.x < 0 ? s.x - s.w / 2 - 4 : s.x + s.w / 2 + 4;
    const p = span(k, 0, 0.35);
    if (p <= 0 || p >= 1) return;
    const headX = edgeX + (targetX - edgeX) * easeOut(p);
    const dir = s.x < 0 ? 1 : -1;
    const y = -s.z;
    h.place(0, 0);
    c.save();
    c.globalAlpha = 0.85 * Math.sin(Math.PI * p);
    c.strokeStyle = 'rgb(255,196,77)';
    c.lineWidth = 2 * pixel;
    c.setLineDash?.([4 * pixel, 3 * pixel]);
    c.beginPath();
    c.moveTo(edgeX, y);
    c.lineTo(headX, y);
    c.stroke();
    c.setLineDash?.([]);
    c.beginPath();
    c.moveTo(headX - dir * 7, y - 5);
    c.lineTo(headX, y);
    c.lineTo(headX - dir * 7, y + 5);
    c.stroke();
    c.restore();
  }

  function drawFormation(c, h, f, stance, active, air, doneChanges, outro) {
    const pixel = 1 / h.scale;
    const team = h.teams[f.team];
    const type = active && active.entry.action;
    const k = active ? active.k : 1;
    if (!active) {
      if (!air && doneChanges.length) {
        const moveFade = 0.24 * (1 - span(outro, 0.4, 1));
        if (moveFade > 0.01) {
          for (const dc of doneChanges) {
            if (dc.entry.action !== 'move_unit' || !dc.before || !dc.after) continue;
            h.place(0, 0);
            c.save();
            c.globalAlpha = moveFade;
            c.strokeStyle = `rgb(${team.rgb})`;
            c.lineWidth = 1.2 * pixel;
            c.setLineDash?.([3 * pixel, 3 * pixel]);
            c.beginPath();
            c.moveTo(dc.before.x, -dc.before.z);
            c.lineTo(dc.after.x, -dc.after.z);
            c.stroke();
            c.setLineDash?.([]);
            c.restore();
          }
        }
      }
      if (stance) {
        for (const p of membersAt(f, stance)) if (!!p.m.air === air) h.unit(f, p, stance.level, 1);
      }
      return;
    }
    const { before, after } = active;
    if (type === 'buy_unit' && !before && after) {
      const flank = after.facing % 1800 !== 0;
      const boxRgb = flank ? '255,196,77' : team.rgb;
      if (!air) {
        if (flank) drawFlankArrow(c, h, after, k, pixel);
        box(c, h, after, boxRgb, 1 - span(k, 0.75, 1) * 0.6, pixel, {
          brackets: span(k, 0, 0.25),
          fill: 0.08 * (1 - span(k, 0.7, 1)),
        });
      }
      for (const p of membersAt(f, after)) {
        if (!!p.m.air !== air) continue;
        const r = span(k, 0.15 + p.m.rank * 0.3, 0.45 + p.m.rank * 0.3);
        if (r <= 0) continue;
        h.unit(f, p, after.level, 1, { print: r });
      }
      if (!air) {
        const ring = span(k, 0.6, 0.95);
        if (ring > 0 && ring < 1) {
          const grow = 2 + 3 * easeOut(ring);
          box(c, h, { ...after, w: after.w + 2 * grow, d: after.d + 2 * grow }, boxRgb, 0.55 * (1 - ring), pixel, {
            colour: flank ? '#ffd984' : '#e8f6ff',
            width: 1.2,
          });
        }
      }
      return;
    }
    if (type === 'move_unit' && before && after) {
      const travel = easeInOut(span(k, 0.15, 0.85));
      const from = membersAt(f, before), to = membersAt(f, after);
      const dx = after.x - before.x, dz = after.z - before.z;
      const heading = Math.hypot(dx, dz) > 0.5 ? Math.atan2(dx, dz) * 1800 / Math.PI : null;
      if (!air) {
        box(c, h, after, team.rgb, 0.9 * (1 - span(k, 0.85, 1)), pixel, { dashed: k < 0.85 });
        const ghost = 0.35 * (1 - span(k, 0.7, 1));
        box(c, h, before, team.rgb, ghost, pixel, { dashed: true, width: 1 });
        const cx = before.x + dx * travel, cz = before.z + dz * travel;
        const tail = span(k, 0.45, 1);
        h.place(0, 0);
        c.save();
        c.globalAlpha = 0.7 * (1 - span(k, 0.85, 1));
        c.strokeStyle = `rgb(${team.rgb})`;
        c.lineWidth = 1.4 * pixel;
        c.setLineDash?.([3 * pixel, 3 * pixel]);
        c.beginPath();
        c.moveTo(before.x + dx * travel * tail, -(before.z + dz * travel * tail));
        c.lineTo(cx, -cz);
        c.stroke();
        c.setLineDash?.([]);
        c.restore();
      }
      from.forEach((p0, i) => {
        if (!!p0.m.air !== air) return;
        if (k < 0.7) h.unit(f, p0, before.level, ghostAlpha(k), { ghost: true });
        const p1 = to[i];
        let body = p1.body;
        if (heading !== null) {
          body = k < 0.15 ? turnTo(p0.body, heading, k / 0.15)
            : k < 0.85 ? heading
            : turnTo(heading, p1.body, (k - 0.85) / 0.15);
        }
        const walking = k >= 0.15 && k < 0.85;
        h.unit(f, {
          m: p0.m,
          x: p0.x + (p1.x - p0.x) * travel,
          y: p0.y + (p1.y - p0.y) * travel,
          z: p0.z + (p1.z - p0.z) * travel,
          body,
          turret: turnTo(p0.turret, p1.turret, travel),
        }, after.level, 1, { walk: travel * Math.hypot(dx, dz), speed: walking ? 8 : 0 });
      });
      return;
    }
    if (type === 'upgrade_unit' && before && after) {
      if (!air) box(c, h, after, team.rgb, 0.9 * Math.sin(Math.PI * span(k, 0, 1)), pixel, { width: 1.8 });
      const sweep = easeInOut(span(k, 0.15, 0.65));
      // a band sweeping across the formation's box, corner to corner
      const lo = after.x + after.z - (after.w + after.d) / 2 - 3, hi = after.x + after.z + (after.w + after.d) / 2 + 3;
      const edge = lo + (hi - lo) * sweep;
      for (const p of membersAt(f, after)) {
        if (!!p.m.air !== air) continue;
        h.unit(f, p, before.level, 1, { clip: [edge, Infinity] });
        h.unit(f, p, after.level, 1, { clip: [-Infinity, edge] });
      }
      if (!air && sweep > 0 && sweep < 1) {
        h.place(0, 0);
        c.save();
        c.beginPath();
        rectPath(c, after.x - after.w / 2 - 2, -(after.z + after.d / 2 + 2), after.w + 4, after.d + 4);
        c.clip();
        c.globalAlpha = 0.85 * Math.sin(Math.PI * sweep);
        c.strokeStyle = '#ffffff';
        c.lineWidth = 2.2 * pixel;
        const L = after.w + after.d;
        c.beginPath();
        // x + z = edge, drawn in canvas space where y = -z
        c.moveTo(after.x - L, -(edge - after.x + L));
        c.lineTo(after.x + L, -(edge - after.x - L));
        c.stroke();
        c.restore();
      }
      return;
    }
    // a decision the page does not animate yet: its changes fade
    if (!before && after) {
      for (const p of membersAt(f, after)) if (!!p.m.air === air) h.unit(f, p, after.level, k);
    } else if (before && !after) {
      for (const p of membersAt(f, before)) if (!!p.m.air === air) h.unit(f, p, before.level, 1 - k);
    } else if (before && after) {
      const e = easeInOut(k);
      const from = membersAt(f, before), to = membersAt(f, after);
      from.forEach((p0, i) => {
        if (!!p0.m.air !== air) return;
        const p1 = to[i];
        h.unit(f, {
          m: p0.m,
          x: p0.x + (p1.x - p0.x) * e,
          y: p0.y + (p1.y - p0.y) * e,
          z: p0.z + (p1.z - p0.z) * e,
          body: turnTo(p0.body, p1.body, e),
          turret: turnTo(p0.turret, p1.turret, e),
        }, k < 0.5 ? before.level : after.level, 1);
      });
    }
  }

  const ghostAlpha = (k) => 0.35 * (1 - span(k, 0.4, 0.7));

  // A technology's research charges every formation of its type with converging
  // traces and a scan bar, then ignites with a golden sheen and dual shockwaves.
  function drawResearch(c, h, state, play) {
    const { entry, k } = play;
    const team = h.teams[entry.team];
    const pixel = 1 / h.scale;
    const charge = span(k, 0, 0.45);
    const burst = span(k, 0.45, 0.92);
    const burst2 = span(k, 0.52, 1);
    const sweep = span(k, 0.45, 0.72);
    for (const { f, stance } of state.formations) {
      if (f.team !== entry.team || f.kind !== entry.unit || !stance) continue;
      const s = stance;
      const x0 = s.x - s.w / 2, x1 = s.x + s.w / 2;
      const y0 = -(s.z + s.d / 2), y1 = -(s.z - s.d / 2);
      const near = entry.team === 0 ? s.z - s.d / 2 : s.z + s.d / 2;
      const far = entry.team === 0 ? s.z + s.d / 2 : s.z - s.d / 2;
      const halves = [[[x0, near], [x0, far], [x1, far]], [[x0, near], [x1, near], [x1, far]]];
      const baseR = Math.max(s.w, s.d, 10) * 0.5;

      if (charge > 0 && k < 0.75) {
        const holdFade = 1 - span(k, 0.45, 0.75);
        box(c, h, s, team.rgb, (0.25 + 0.65 * charge) * holdFade, pixel, {
          brackets: charge,
          fill: (0.04 + 0.08 * charge) * holdFade,
        });
        h.place(0, 0);
        c.save();
        c.lineCap = 'round';
        const run = span(k, 0.08, 0.45);
        if (run > 0) {
          c.globalAlpha = 0.8 * holdFade;
          c.strokeStyle = `rgb(${team.rgb})`;
          c.lineWidth = 1.6 * pixel;
          for (const half of halves) stroke(c, half, 0, run);
          c.globalAlpha = Math.sin(Math.PI * run) * holdFade;
          c.strokeStyle = '#ffe9a8';
          c.lineWidth = 2.6 * pixel;
          for (const half of halves) stroke(c, half, Math.max(0, run - 0.25), run);
        }
        if (k < 0.48) {
          const scan = easeInOut(span(k, 0.05, 0.45));
          const syLine = -(near + (far - near) * scan);
          c.globalAlpha = 0.75 * Math.sin(Math.PI * span(k, 0.05, 0.48));
          c.strokeStyle = '#bfe6ff';
          c.lineWidth = 1.8 * pixel;
          c.beginPath();
          c.moveTo(x0, syLine);
          c.lineTo(x1, syLine);
          c.stroke();
          const rIn = baseR * (1.25 - 0.72 * easeOut(charge));
          if (rIn > 0.5) {
            c.globalAlpha = 0.55 * charge * (1 - span(k, 0.4, 0.48));
            c.strokeStyle = team.accent;
            c.lineWidth = 1.2 * pixel;
            c.setLineDash?.([4 * pixel, 3 * pixel]);
            c.beginPath();
            c.arc(s.x, -s.z, rIn, 0, Math.PI * 2);
            c.stroke();
            c.setLineDash?.([]);
          }
        }
        c.restore();
      }

      if (burst > 0) {
        const rimFade = 1 - span(k, 0.65, 1);
        if (rimFade > 0.01) {
          box(c, h, s, '255,214,102', 0.92 * rimFade, pixel, {
            colour: '#ffe082',
            width: 2.1,
            fill: 0.16 * (1 - span(k, 0.45, 0.78)),
          });
        }
        h.place(0, 0);
        if (sweep > 0 && sweep < 1) {
          const bandW = Math.max(s.w, s.d) * 0.42;
          const bandX = x0 - s.d * 0.5 - bandW + (s.w + s.d + bandW * 2) * easeInOut(sweep);
          c.save();
          c.beginPath();
          rectPath(c, x0, y0, s.w, s.d);
          c.clip();
          c.globalAlpha = 0.36 * Math.sin(Math.PI * sweep);
          c.fillStyle = '#fff1b8';
          c.beginPath();
          c.moveTo(bandX, y1);
          c.lineTo(bandX + bandW, y1);
          c.lineTo(bandX + bandW + s.d * 0.55, y0);
          c.lineTo(bandX + s.d * 0.55, y0);
          c.closePath();
          c.fill();
          c.restore();
        }
        c.save();
        c.lineCap = 'round';
        const r1 = baseR * (0.35 + 1.55 * easeOut(burst));
        if (r1 > 0.5 && burst < 1) {
          c.globalAlpha = 0.9 * (1 - burst);
          c.strokeStyle = '#ffd666';
          c.lineWidth = (2.6 - 1.5 * burst) * pixel;
          c.beginPath();
          c.arc(s.x, -s.z, r1, 0, Math.PI * 2);
          c.stroke();
        }
        if (burst2 > 0 && burst2 < 1) {
          const r2 = baseR * (0.25 + 1.25 * easeOut(burst2));
          if (r2 > 0.5) {
            c.globalAlpha = 0.75 * (1 - burst2);
            c.strokeStyle = team.accent;
            c.lineWidth = 1.6 * pixel;
            c.beginPath();
            c.arc(s.x, -s.z, r2, 0, Math.PI * 2);
            c.stroke();
          }
        }
        const rayFade = Math.sin(Math.PI * span(k, 0.45, 0.85));
        if (rayFade > 0.01) {
          c.globalAlpha = 0.75 * rayFade;
          c.strokeStyle = '#ffe9a8';
          c.lineWidth = 1.6 * pixel;
          for (const ox of [-s.w * 0.16, 0, s.w * 0.16]) {
            const ry0 = y0 + 2 * pixel;
            const ry1 = y0 - (4 + 8 * easeOut(span(k, 0.45, 0.75))) * pixel;
            c.beginPath();
            c.moveTo(s.x + ox, ry0);
            c.lineTo(s.x + ox, ry1);
            c.stroke();
          }
        }
        c.restore();
      }
    }
  }

  // Strokes the part of polyline `pts` (world x, z) from fraction a to b of its length.
  function stroke(c, pts, a, b) {
    const lengths = [];
    let total = 0;
    for (let i = 1; i < pts.length; i++) {
      const l = Math.hypot(pts[i][0] - pts[i - 1][0], pts[i][1] - pts[i - 1][1]);
      lengths.push(l);
      total += l;
    }
    const from = a * total, to = b * total;
    let at = 0;
    c.beginPath();
    let started = false;
    for (let i = 1; i < pts.length; i++) {
      const l = lengths[i - 1], s0 = Math.max(from, at), s1 = Math.min(to, at + l);
      if (s1 > s0) {
        const p = (s) => [pts[i - 1][0] + (pts[i][0] - pts[i - 1][0]) * ((s - at) / l), pts[i - 1][1] + (pts[i][1] - pts[i - 1][1]) * ((s - at) / l)];
        const [ax, az] = p(s0), [bx, bz] = p(s1);
        if (!started) { c.moveTo(ax, -az); started = true; }
        c.lineTo(bx, -bz);
      }
      at += l;
    }
    c.stroke();
  }

  function draw(c, state, h, air) {
    if (!state) return;
    for (const { f, stance, active, doneChanges } of state.formations) {
      drawFormation(c, h, f, stance, active, air, doneChanges, state.outro);
    }
    if (!air) {
      for (const play of state.playing) {
        if (play && play.entry.action === 'upgrade_technology') drawResearch(c, h, state, play);
      }
    }
  }

  // Screen-space marks above the battlefield: animated transition plates,
  // persistent before/after state indicators during lead-in, placeholder pulses,
  // and the tick-1 outline fade-in.
  function drawOverlay(c, state, h) {
    if (!state) return;
    const ui = h.ui;
    const activeUpgrade = new Set();
    for (const play of state.playing) {
      if (!play) continue;
      const { entry, k } = play;
      const team = h.teams[entry.team];
      if (entry.action === 'upgrade_unit') {
        const f = entry.formations[0];
        if (f) activeUpgrade.add(f.i);
        const hist = f && f.history.find((x) => x.entry === entry);
        if (!hist || !hist.after || !hist.before) continue;
        const s = hist.after;
        const appear = span(k, 0.45, 0.6), fade = 1 - span(k, 0.85, 1);
        if (appear <= 0) continue;
        plate(c, h.sx(s.x), h.sy(s.z + s.d / 2) - 10 * ui - 6 * ui * (1 - easeOut(appear)), ui, appear * fade, team,
          `Lv ${hist.before.level}`, `Lv ${hist.after.level}`, span(k, 0.55, 0.7));
      } else if (!ANIMATED.has(entry.action) && entry.formations?.length) {
        const pulse = Math.sin(Math.PI * k);
        for (const f of entry.formations) {
          const fs = state.formations.find((x) => x.f === f);
          const s = fs?.stance || fs?.active?.after || fs?.active?.before;
          if (!s) continue;
          diamond(c, h.sx(s.x), h.sy(s.z + s.d / 2) - 11 * ui, ui, 0.85 * pulse, team);
        }
      }
    }

    // Persistent promotion badge for formations upgraded earlier in this round.
    const persistAlpha = 1 - span(state.outro, 0.5, 1);
    if (persistAlpha > 0.01) {
      for (const { f, stance, active, doneChanges } of state.formations) {
        if (!stance || active) continue;
        const promoted = doneChanges.some((dc) => dc.entry.action === 'upgrade_unit') && !activeUpgrade.has(f.i);
        if (!promoted) continue;
        const team = h.teams[f.team];
        levelPill(c, h.sx(stance.x), h.sy(stance.z + stance.d / 2) - 11 * ui, ui, 0.88 * persistAlpha, team, `↑Lv.${stance.level}`);
      }
    }

    // The fight's own outline returns over the last moment before tick 1.
    if (state.outro > 0 && h.footprints) {
      c.save();
      c.globalAlpha = state.outro;
      c.strokeStyle = '#fff';
      c.lineWidth = 1.5;
      for (const r of h.footprints) c.strokeRect(h.sx(r.x0 / 100), h.sy(r.z1 / 100), (r.x1 - r.x0) / 100 * h.scale, (r.z1 - r.z0) / 100 * h.scale);
      c.restore();
    }
  }

  // "Lv 2 ›› Lv 3", the new level flipping in (fixed geometry, no measureText).
  function plate(c, x, y, ui, alpha, team, from, to, flip) {
    c.save();
    c.translate(x, y);
    c.scale(ui, ui);
    c.globalAlpha = alpha;
    c.font = '600 11px "Mechcore HUD", system-ui, sans-serif';
    c.textBaseline = 'middle';
    const w = 78, hgt = 18;
    c.fillStyle = 'rgba(12,20,31,0.92)';
    c.fillRect(-w / 2, -hgt / 2, w, hgt);
    c.strokeStyle = team.accent;
    c.lineWidth = 1;
    c.strokeRect(-w / 2, -hgt / 2, w, hgt);
    c.textAlign = 'right';
    c.fillStyle = '#9fb0c4';
    c.fillText(from, -8, 0.5);
    c.strokeStyle = '#ffcf5c';
    c.lineWidth = 1.4;
    c.beginPath();
    c.moveTo(-4, -3); c.lineTo(0, 0.5); c.lineTo(-4, 4);
    c.moveTo(0, -3); c.lineTo(4, 0.5); c.lineTo(0, 4);
    c.stroke();
    c.save();
    c.beginPath();
    rectPath(c, -w / 2, -hgt / 2, w, hgt);
    c.clip();
    c.textAlign = 'left';
    c.fillStyle = '#ffcf5c';
    c.fillText(to, 8, 0.5 + (1 - easeOut(flip)) * hgt);
    c.restore();
    c.restore();
  }

  function levelPill(c, x, y, ui, alpha, team, text) {
    c.save();
    c.translate(x, y);
    c.scale(ui, ui);
    c.globalAlpha = alpha;
    const w = 42, hgt = 15;
    c.fillStyle = 'rgba(12,20,31,0.88)';
    c.fillRect(-w / 2, -hgt / 2, w, hgt);
    c.strokeStyle = team.accent;
    c.lineWidth = 1;
    c.strokeRect(-w / 2, -hgt / 2, w, hgt);
    c.font = '600 10px "Mechcore HUD", system-ui, sans-serif';
    c.textAlign = 'center';
    c.textBaseline = 'middle';
    c.fillStyle = '#ffcf5c';
    c.fillText(text, 0, 0.5);
    c.restore();
  }

  function diamond(c, x, y, ui, alpha, team) {
    c.save();
    c.translate(x, y);
    c.scale(ui, ui);
    c.globalAlpha = alpha;
    c.beginPath();
    c.moveTo(0, -6); c.lineTo(6, 0); c.lineTo(0, 6); c.lineTo(-6, 0);
    c.closePath();
    c.fillStyle = 'rgba(12,20,31,0.85)';
    c.fill();
    c.strokeStyle = team.accent;
    c.lineWidth = 1.2;
    c.stroke();
    c.restore();
  }

  return { prepare, sample, supplyAt, describe, formationTechs, draw, drawOverlay, membersAt, ANIMATED };
})();
