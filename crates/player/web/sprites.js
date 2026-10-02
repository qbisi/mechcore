// Top-down sprites, drawn after the game's own models.
//
// Every sprite is drawn in metres around the object's centre with its front
// toward -y, so the page turns it by the recorded facing and scales it by the
// zoom. The shapes and proportions follow each model as its prefab assembles
// it in its attack pose, seen from straight above: the parts a unit moves in
// a fight (legs, turret, barrels, arms, stinger) are drawn apart so the page
// can move them. The colours follow the models' textures: white armour over
// gunmetal, the team colour where the texture masks it, and each unit's own
// emissive colour.
//
// scripts/player/export-models.mjs runs this file to write `models/`, one SVG
// per sprite, through a context that records the canvas calls made
// here as SVG. A sprite draws only with the calls it records, and turns, moves
// and scales its parts uniformly.

'use strict';

const Sprites = (() => {
  const TEAMS = [
    { name: 'Blue', accent: '#3d8dff', deep: '#1d4fa6', light: '#a6d2ff', rgb: '61,141,255' },
    { name: 'Red', accent: '#ff4f3c', deep: '#a8261b', light: '#ffb4a8', rgb: '255,79,60' },
  ];
  const M = {
    white: '#e6e9ee', pale: '#c3c9d1', mid: '#7f8691', steel: '#575e68',
    dark: '#363b43', darker: '#23272d', black: '#121418', line: '#0a0c0f',
    glass: '#9cc6e6', sand: '#b7a47a', caution: '#f2a640', gold: '#e8c23a',
  };
  // Each unit's emissive colour, read off its model's emission texture.
  const GLOW = {
    marksman: '#5ff2ff', arclight: '#b3f2ff', rhino: '#ffa236', crawler: '#ffb648',
    sledgehammer: '#55e8f4', wasp: '#ffd84e',
  };
  // The model's footprint in metres, width by length, which frames an icon.
  const SIZE = {
    marksman: [12, 22], arclight: [18, 13], rhino: [28, 24], crawler: [4.2, 5.2],
    sledgehammer: [7.6, 13], wasp: [9, 11], energy_tower: [22, 22], research_center: [22, 22],
    anti_armor_turret: [24, 24], rapid_fire_turret: [14, 20], defensive_wall: [10, 10],
  };

  let LW = 0.15; // outline width in metres, set from the zoom

  function setScale(metresPerPixel) {
    LW = Math.max(0.1, metresPerPixel * 0.9);
  }

  // ----------------------------------------------------------------- helpers
  function rr(c, x, y, w, h, r) {
    r = Math.min(r, w / 2, h / 2);
    c.beginPath();
    c.moveTo(x + r, y);
    c.arcTo(x + w, y, x + w, y + h, r);
    c.arcTo(x + w, y + h, x, y + h, r);
    c.arcTo(x, y + h, x, y, r);
    c.arcTo(x, y, x + w, y, r);
    c.closePath();
  }
  function poly(c, pts) {
    c.beginPath();
    c.moveTo(pts[0], pts[1]);
    for (let i = 2; i < pts.length; i += 2) c.lineTo(pts[i], pts[i + 1]);
    c.closePath();
  }
  // Mirror a half outline drawn on the right (+x) side into a whole one.
  function sym(half) {
    const out = half.slice();
    for (let i = half.length - 2; i >= 0; i -= 2) out.push(-half[i], half[i + 1]);
    return out;
  }
  function circle(c, x, y, r) {
    c.beginPath();
    c.arc(x, y, r, 0, Math.PI * 2);
  }
  function paint(c, fill, stroke = M.line, width = 1) {
    if (fill) { c.fillStyle = fill; c.fill(); }
    if (stroke) { c.lineWidth = LW * width; c.strokeStyle = stroke; c.stroke(); }
  }
  function line(c, x1, y1, x2, y2, colour, width) {
    c.beginPath();
    c.moveTo(x1, y1);
    c.lineTo(x2, y2);
    c.lineWidth = width;
    c.strokeStyle = colour;
    c.stroke();
  }
  function glow(c, x, y, r, colour, alpha) {
    if (alpha <= 0.01) return;
    const g = c.createRadialGradient(x, y, 0, x, y, r);
    g.addColorStop(0, colour);
    g.addColorStop(1, 'rgba(0,0,0,0)');
    c.save();
    c.globalAlpha = Math.min(1, alpha);
    c.globalCompositeOperation = 'lighter';
    c.fillStyle = g;
    c.fillRect(x - r, y - r, r * 2, r * 2);
    c.restore();
  }
  // A star-shaped flash, for a muzzle.
  function flash(c, x, y, r, colour, age, life, seed = 0) {
    if (age < 0 || age > life) return;
    const k = 1 - age / life;
    glow(c, x, y, r * 1.6, colour, k);
    c.save();
    c.globalCompositeOperation = 'lighter';
    c.globalAlpha = k;
    c.fillStyle = '#ffffff';
    c.beginPath();
    const spikes = 6;
    for (let i = 0; i < spikes * 2; i++) {
      const a = (i / (spikes * 2)) * Math.PI * 2 + seed;
      const rr2 = i % 2 === 0 ? r * (0.8 + 0.4 * ((seed * 7 + i) % 3) / 2) : r * 0.25;
      const px = x + Math.sin(a) * rr2, py = y - Math.cos(a) * rr2;
      if (i === 0) c.moveTo(px, py); else c.lineTo(px, py);
    }
    c.closePath();
    c.fill();
    c.restore();
  }
  const ease = (t) => (t <= 0 ? 0 : t >= 1 ? 1 : t * t * (3 - 2 * t));
  // How far a weapon has kicked back `age` seconds after firing: a sharp kick
  // and a slower return.
  function recoil(age, back = 0.45) {
    if (!(age >= 0) || age > back + 0.06) return 0;
    if (age < 0.06) return age / 0.06;
    return 1 - ease((age - 0.06) / back);
  }

  // ------------------------------------------------------------------ units
  // A pose says how a unit stands this frame; the page fills it in.
  function pose() {
    return {
      t: 0, team: TEAMS[0], facing: 0, turret: 0, speed: 0, walk: 0, stance: 0,
      fireAge: Infinity, fireIndex: 0, chargeIn: Infinity,
      strike: -1, strikeIndex: 0, attacking: false, altitude: 0, life: 1,
    };
  }

  // Marksman (Longbow): a biped carrying a rail rifle twice its length, the
  // twin prongs forward, shoulder pods to its left and a battery to its back.
  function marksman(c, p) {
    const T = p.team;
    const stride = Math.min(1, p.speed / 4) * 1.5;
    const ph = p.walk * Math.PI * 2 / 6;
    for (const side of [-1, 1]) {
      const off = Math.sin(ph + (side > 0 ? Math.PI : 0)) * stride;
      line(c, side * 1.4, 0.4, side * 2.6, off, M.dark, 1.3);
      poly(c, [side * 1.8, off - 2.2, side * 3.4, off - 1.8, side * 3.6, off + 1.6, side * 1.7, off + 1.9]);
      paint(c, M.darker);
      poly(c, [side * 2.0, off - 2.2, side * 2.7, off - 3.2, side * 3.3, off - 1.9]);
      paint(c, M.steel);
    }
    rr(c, -2, -1.4, 4, 3, 0.6);
    paint(c, M.dark);

    c.save();
    c.rotate(p.turret);
    const kick = recoil(p.fireAge, 0.55);
    c.translate(0, kick * 0.4);
    // battery on the back
    rr(c, 2.9, 0.6, 1.9, 4.6, 0.9);
    paint(c, M.white);
    rr(c, 2.9, 2.0, 1.9, 1.6, 0.2);
    paint(c, '#3f6f93', null);
    // torso
    poly(c, [-2.7, -2.2, -0.9, -3.8, 0.9, -3.8, 2.7, -2.2, 2.9, 2.0, 0.8, 3.4, -0.8, 3.4, -2.9, 2.0]);
    paint(c, M.white);
    poly(c, [-0.8, -3.2, 0.8, -3.2, 1.1, 2.4, -1.1, 2.4]);
    paint(c, M.glass, M.line, 0.6);
    poly(c, [-2.6, -1.6, -1.3, -1.9, -1.4, 1.8, -2.6, 1.6]);
    paint(c, T.accent, null);
    // shoulder pod: the team colour under white facets
    circle(c, -4.3, -0.3, 2.2);
    paint(c, T.accent);
    poly(c, [-4.3, -2.4, -3.1, -0.3, -5.5, -0.3]);
    paint(c, M.white, null);
    poly(c, [-4.3, 1.8, -3.3, 0.3, -5.3, 0.3]);
    paint(c, M.white, null);
    circle(c, -4.3, -0.3, 2.2);
    paint(c, null, M.line, 1.2);
    for (const x of [-2.1, -1.4]) { rr(c, x, -2.9, 0.5, 2.0, 0.2); paint(c, M.pale, M.line, 0.6); }
    // rifle on the right, kicked back by the shot
    c.translate(0, kick * 1.6);
    rr(c, 0.9, -5.2, 2.2, 8.6, 0.5);
    paint(c, M.darker);
    rr(c, 1.15, -4.6, 1.7, 3.6, 0.3);
    paint(c, T.accent, M.line, 0.7);
    const charge = p.chargeIn < 0.6 ? 1 - p.chargeIn / 0.6 : 0;
    if (charge > 0) line(c, 2.0, -6, 2.0, -15.2, GLOW.marksman, 0.25 + charge * 0.5);
    for (const x of [1.25, 2.35]) {
      poly(c, [x, -5.2, x + 0.4, -5.2, x + 0.4, -15.4, x + 0.2, -16.3, x, -15.4]);
      paint(c, M.white, M.line, 0.7);
    }
    rr(c, 1.0, -8.2, 2.0, 0.7, 0.2);
    paint(c, M.dark, null);
    if (charge > 0) glow(c, 2.0, -15.6, 2.2, GLOW.marksman, charge);
    flash(c, 2.0, -16.6, 2.6, GLOW.marksman, p.fireAge, 0.18, p.fireIndex);
    c.restore();
  }

  // Arclight, as its ordinary attack holds it: a biped whose upper body
  // carries the hull, a shield hung on each side and one cannon down its
  // middle, which fires the splashing bolt. The legs show only their striped
  // heels behind the hull; everything above them turns with the turret.
  function arclight(c, p) {
    const T = p.team;
    const stride = Math.min(1, p.speed / 4) * 1.1;
    const ph = p.walk * Math.PI * 2 / 6;
    for (const side of [-1, 1]) {
      const off = Math.sin(ph + (side > 0 ? Math.PI : 0)) * stride;
      rr(c, side * 1.25 - 0.4, 2.4 + off, 0.8, 2.8, 0.3);
      paint(c, M.white);
      rr(c, side * 2.65 - 0.6, 3.0 + off, 1.2, 3.2, 0.3);
      paint(c, M.dark);
      for (let i = 0; i < 3; i++) {
        rr(c, side * 2.65 - 0.55, 3.9 + off + i * 0.7, 1.1, 0.7, 0.1);
        paint(c, i % 2 ? M.white : T.accent, M.line, 0.5);
      }
    }

    c.save();
    c.rotate(p.turret);
    const kick = recoil(p.fireAge, 0.4) * 0.7;
    // the shields, each on an arm from the hull's side
    for (const side of [-1, 1]) {
      rr(c, side * 5.9 - 0.9, 0.9, 1.8, 0.8, 0.2);
      paint(c, M.dark);
      poly(c, [side * 6.5, -4.0, side * 7.6, -4.2, side * 8.5, 3.0, side * 7.4, 4.8, side * 6.7, 4.3]);
      paint(c, T.accent);
      poly(c, [side * 6.5, -4.0, side * 6.9, -4.05, side * 7.1, 4.5, side * 6.7, 4.3]);
      paint(c, M.white, null);
      line(c, side * 7.4, -3.4, side * 7.9, 3.2, T.deep, 0.35);
      rr(c, side * 7.2 - 0.5, 0.5, 1.0, 1.6, 0.3);
      paint(c, M.steel);
    }
    // the hull
    rr(c, -5.3, -5.6, 10.6, 9.7, 0.6);
    paint(c, M.dark);
    poly(c, [-5.2, -4.4, -3.8, -5.6, -1.7, -5.6, -1.7, 4.0, -5.2, 4.0]);
    paint(c, M.white);
    rr(c, -3.9, -3.6, 2.1, 5.4, 0.6);
    paint(c, M.glass, M.line, 0.7);
    rr(c, -5.6, -0.9, 0.7, 2.4, 0.2);
    paint(c, M.mid);
    poly(c, [1.8, -5.6, 3.9, -5.6, 5.2, -4.4, 5.2, 0.6, 1.8, 0.6]);
    paint(c, M.white);
    poly(c, [1.8, 0.6, 5.2, 0.6, 5.2, 4.0, 1.8, 4.0]);
    paint(c, M.steel, M.line, 0.6);
    for (const y of [2.0, 3.0]) { circle(c, 4.6, y, 0.25); paint(c, M.pale, null); }
    // the fuel tank on its right
    rr(c, 2.5, -2.7, 1.5, 5.2, 0.75);
    paint(c, M.mid);
    line(c, 2.95, -2.2, 2.95, 1.9, M.pale, 0.2);
    circle(c, 3.25, -3.1, 0.45);
    paint(c, M.dark);
    // the cannon down the middle, kicked back by its shot
    for (const side of [-1, 1]) {
      rr(c, side * 1.55 - 0.25, -4.6, 0.5, 9.2, 0.2);
      paint(c, M.white);
    }
    rr(c, -1.1, 0.6, 2.2, 1.9, 0.3);
    paint(c, M.white);
    rr(c, -1.2, -1.5, 2.4, 1.8, 0.2);
    paint(c, M.darker);
    for (let i = 0; i < 4; i++) line(c, -1.0, -1.25 + i * 0.42, 1.0, -1.25 + i * 0.42, M.mid, 0.12);
    rr(c, -1.3, -4.9 + kick, 2.6, 3.2, 0.4);
    paint(c, M.white);
    rr(c, -1.45, -5.7 + kick, 2.9, 1.0, 0.45);
    paint(c, T.accent);
    // the charge builds through attack2, the 0.7 s before the shot
    const charge = p.chargeIn < 0.7 ? 1 - p.chargeIn / 0.7 : 0;
    glow(c, 0, -5.4 + kick, 2.4, GLOW.arclight, Math.max(charge, p.attacking ? 0.25 : 0));
    flash(c, 0, -6.2 + kick, 2.6, GLOW.arclight, p.fireAge, 0.14, p.fireIndex);
    c.restore();
  }

  // Rhino: a biped on wheeled feet whose forearms carry chainsaws, long bars
  // clamped beside the hand with a saw flywheel turning on the nose. It walks with both bars held
  // ahead, as its Walk clip poses it, and stands to fight with them hanging
  // at its sides, as its Idle does (`stance`, 0 to 1). It strikes with each
  // arm in turn, as its FiringAL and FiringAR do: the torso winds back and
  // lifts the bar behind the shoulder, then turns through to slash it forward
  // and across, the hips following.
  function rhino(c, p) {
    const T = p.team;
    const pace = Math.min(1, p.speed / 6) * (1 - p.stance);
    const ph = p.walk * Math.PI * 2 / 9;
    // which side strikes, -1 left and 1 right, and the strike's key there
    const side = p.strike >= 0 ? (p.strikeIndex % 2 === 0 ? -1 : 1) : 0;
    const key = slashAt(side ? p.strike : 0, side || -1);
    // the legs, white thighs reaching back to a wheel each, turning with the
    // hips
    c.save();
    c.rotate(key.hips * p.stance);
    for (const leg of [-1, 1]) {
      const off = Math.sin(ph + (leg > 0 ? Math.PI : 0)) * 1.6 * pace;
      rr(c, leg * 3.6 - 1.1, 7.2 + off, 2.2, 5.0, 1.1);
      paint(c, M.black);
      c.save();
      rr(c, leg * 3.6 - 1.1, 7.2 + off, 2.2, 5.0, 1.1);
      c.clip();
      const roll = (p.walk * 1.6) % 1.1;
      for (let y = 6.6 + off + roll; y < 12.8 + off; y += 1.1) line(c, leg * 3.6 - 1.1, y, leg * 3.6 + 1.1, y, M.steel, 0.25);
      c.restore();
      poly(c, [leg * 2.2, 1.0 + off * 0.3, leg * 5.0, 1.4 + off * 0.3, leg * 4.8, 8.4 + off, leg * 3.7, 9.4 + off, leg * 2.5, 8.6 + off]);
      paint(c, M.white);
      poly(c, [leg * 2.7, 6.4 + off, leg * 4.5, 6.2 + off, leg * 4.3, 7.8 + off, leg * 3.0, 8.0 + off]);
      paint(c, M.caution, null);
      line(c, leg * 3.7, 2.0 + off * 0.3, leg * 3.7, 6.0 + off, M.pale, 0.2);
    }
    c.restore();
    // each arm in the torso's frame, between the pose it walks in and the one
    // it fights in; a walking arm swings against its leg
    const arms = [-1, 1].map((arm) => {
      const fight = arm === side || (!side && arm < 0) ? key.striking : key.other;
      const walking = Math.sin(ph + (arm > 0 ? 0 : Math.PI)) * 0.12 * pace;
      return { arm, joints: swung(mixJoints(mirrorJoints(RHINO_WALK, arm), fight, p.stance), arm * walking) };
    });
    if (side && p.stance > 0.5) slashTrail(c, p.strike, side);
    // the torso, turning and shifting about the spine
    c.save();
    c.translate(key.shift[0] * p.stance, RHINO_SPINE + key.shift[1] * p.stance);
    c.rotate(key.torso * p.stance);
    // the striking arm is drawn over the body, the other under it
    for (const a of arms) if (a.arm !== side) rhinoArm(c, a.joints, a.arm, T, p.t, false);
    poly(c, sym([0, -7.4, 1.3, -6.6, 3.7, -4.2, 5.2, -0.4, 5.0, 3.2, 2.4, 5.2, 0, 5.4]));
    paint(c, M.white);
    poly(c, sym([0, -5.8, 2.6, -3.4, 3.3, 0, 2.2, 1.2, 0, 1.2]));
    paint(c, M.pale, null);
    rr(c, -2.2, 1.6, 4.4, 3.3, 0.5);
    paint(c, M.dark);
    poly(c, [0, -7.8, 0.85, -6.8, 0.7, 1.2, -0.7, 1.2, -0.85, -6.8]);
    paint(c, T.accent, M.line, 0.6);
    glow(c, 0, 3.2, 2.0, GLOW.rhino, 0.6 + 0.3 * Math.sin(p.t * 4));
    for (const a of arms) if (a.arm === side) rhinoArm(c, a.joints, a.arm, T, p.t, true);
    // sparks off the nose as it cuts, from the blow on
    if (side && p.strike > 0.39 && p.strike < 0.6) {
      const nose = arms.find((a) => a.arm === side).joints[4];
      sparks(c, nose[0], nose[1], 1 - (p.strike - 0.39) / 0.21, p.t, side);
    }
    c.restore();
  }

  // Where the spine stands, ahead of the body's middle: the torso turns
  // about it.
  const RHINO_SPINE = -0.6;
  // A Rhino's left arm walking, read off its Walk clip by
  // scripts/player/rhino-slash.py: the shoulder, the elbow, the hand, the top
  // of the chainsaw's bar and its nose, from the spine.
  const RHINO_WALK = [[-5.0, 1.2], [-6.3, -0.4], [-6.3, -2.8], [-9.4, 3.3], [-9.1, -9.7]];
  // A Rhino's FiringAL as scripts/player/rhino-slash.py prints it: how far
  // through the clip; how far the torso and the hips have turned, degrees
  // clockwise; how far the spine has moved; then the striking left arm's
  // joints and the right arm's, in the torso's frame. Its first key is the
  // Idle a Rhino fights in.
  const RHINO_SLASH = [
    [0.0, 0, 0, 0.0, 0.0, -5.0, 1.1, -6.4, 3.9, -6.9, 2.9, -8.5, 5.7, -11.1, -0.3, 5.0, 1.1, 6.4, 3.9, 6.9, 2.9, 8.5, 5.7, 11.1, -0.3],
    [0.1, -14, -8, -0.3, 0.3, -4.8, 1.3, -6.3, 4.7, -7.1, 3.7, -8.0, 6.2, -11.7, 0.4, 5.2, 1.2, 6.2, 4.0, 6.8, 3.0, 8.3, 6.3, 11.2, 0.2],
    [0.2, -46, -25, -0.7, 0.9, -4.5, 2.0, -6.1, 6.1, -7.3, 4.9, -6.7, 8.0, -11.4, 1.9, 5.4, 1.7, 5.5, 4.2, 6.3, 3.1, 7.0, 7.8, 11.2, 1.3],
    [0.267, -59, -36, -0.9, 1.3, -4.4, 2.4, -5.8, 6.7, -7.2, 5.4, -5.4, 9.4, -11.0, 2.8, 5.4, 1.9, 5.4, 4.2, 6.3, 3.0, 6.7, 8.3, 11.3, 1.1],
    [0.3, -64, -42, -1.1, 1.7, -4.5, 2.5, -5.8, 6.8, -7.3, 5.4, -5.1, 10.0, -10.8, 2.8, 5.3, 1.9, 5.5, 4.2, 6.3, 2.9, 6.8, 8.4, 11.3, 0.8],
    [0.333, -59, -38, -0.8, 1.5, -4.4, 2.4, -6.1, 6.6, -7.6, 5.1, -5.2, 10.1, -10.8, 2.3, 5.4, 1.9, 5.5, 4.2, 6.3, 2.9, 6.9, 8.2, 11.3, 0.9],
    [0.367, -21, -18, 0.7, 0.3, -4.9, 1.4, -8.1, 3.3, -9.0, 1.0, -9.5, 5.5, -12.1, -6.1, 5.2, 1.4, 6.2, 3.9, 6.7, 2.7, 8.6, 6.6, 10.9, -0.5],
    [0.4, 56, 21, 2.8, -0.9, -5.6, 0.9, -9.1, 0.4, -10.0, -2.3, -11.3, -0.1, -14.0, -14.3, 4.0, 1.3, 7.0, 2.7, 7.1, 1.5, 9.1, 3.4, 9.0, -3.6],
    [0.433, 70, 29, 3.0, -0.9, -5.6, 1.0, -9.0, -0.2, -9.7, -3.1, -11.2, -0.6, -13.0, -15.4, 3.8, 1.5, 7.0, 2.5, 7.0, 1.2, 9.0, 3.1, 8.4, -4.2],
    [0.467, 73, 31, 3.0, -0.9, -5.5, 1.0, -8.7, -0.7, -9.1, -3.7, -10.9, -1.1, -10.9, -16.3, 3.8, 1.5, 7.0, 2.4, 7.0, 1.1, 8.9, 3.1, 8.4, -4.3],
    [0.5, 69, 29, 2.8, -0.9, -5.5, 1.0, -8.3, -1.1, -8.3, -4.1, -10.6, -1.5, -8.4, -16.7, 3.8, 1.5, 7.0, 2.5, 7.0, 1.2, 8.9, 3.2, 8.6, -4.1],
    [0.567, 60, 26, 2.5, -0.8, -5.6, 0.9, -7.6, -1.0, -7.0, -3.9, -10.3, -1.6, -5.1, -15.9, 3.9, 1.4, 6.9, 2.7, 7.1, 1.5, 8.9, 3.6, 9.2, -3.6],
    [0.633, 48, 21, 2.0, -0.7, -5.5, 0.9, -7.2, -0.5, -6.5, -3.3, -10.3, -1.0, -4.4, -14.4, 4.1, 1.2, 6.9, 3.0, 7.2, 1.8, 8.9, 3.9, 9.7, -3.0],
    [0.7, 35, 15, 1.5, -0.6, -5.5, 0.9, -7.0, 0.7, -6.6, -1.8, -10.4, 0.5, -6.0, -11.8, 4.4, 1.1, 6.8, 3.2, 7.2, 2.1, 8.8, 4.4, 10.3, -2.3],
    [0.767, 22, 9, 1.0, -0.4, -5.4, 0.9, -6.8, 2.2, -6.8, 0.2, -10.3, 2.3, -8.6, -7.8, 4.6, 1.1, 6.7, 3.5, 7.1, 2.4, 8.8, 4.8, 10.7, -1.6],
    [0.833, 10, 4, 0.5, -0.2, -5.2, 1.0, -6.5, 3.4, -6.9, 2.2, -9.6, 4.2, -10.7, -2.8, 4.9, 1.0, 6.6, 3.7, 7.0, 2.7, 8.6, 5.3, 11.0, -0.9],
    [0.9, 3, 1, 0.1, 0.0, -5.1, 1.0, -6.4, 3.9, -6.9, 2.9, -8.7, 5.4, -11.0, -0.3, 5.0, 1.0, 6.4, 3.8, 7.0, 2.8, 8.5, 5.6, 11.1, -0.4],
    [1.0, 0, 0, 0.0, 0.0, -5.0, 1.1, -6.4, 3.9, -6.9, 2.9, -8.5, 5.7, -11.1, -0.3, 5.0, 1.1, 6.4, 3.9, 6.9, 2.9, 8.5, 5.7, 11.1, -0.3],
  ];

  // A Rhino's strike `s` of the way through its clip, struck with `side`'s
  // arm, -1 left: FiringAR is FiringAL mirrored. Angles are in radians.
  function slashAt(s, side) {
    let i = 0;
    while (i + 2 < RHINO_SLASH.length && RHINO_SLASH[i + 1][0] <= s) i++;
    const [a, b] = [RHINO_SLASH[i], RHINO_SLASH[i + 1]];
    const k = Math.max(0, Math.min(1, (s - a[0]) / (b[0] - a[0])));
    const v = a.map((x, j) => x + (b[j] - x) * k);
    const flip = -side;
    const joints = (from) => [0, 1, 2, 3, 4].map((j) => [flip * v[from + 2 * j], v[from + 2 * j + 1]]);
    return {
      torso: (flip * v[1] * Math.PI) / 180,
      hips: (flip * v[2] * Math.PI) / 180,
      shift: [flip * v[3], v[4]],
      striking: joints(5),
      other: joints(15),
    };
  }

  function mirrorJoints(joints, arm) {
    return joints.map(([x, y]) => [-arm * x, y]);
  }
  function mixJoints(a, b, k) {
    return a.map(([x, y], j) => [x + (b[j][0] - x) * k, y + (b[j][1] - y) * k]);
  }
  // An arm's joints turned by `angle` about its shoulder.
  function swung(joints, angle) {
    const [sx, sy] = joints[0];
    const c = Math.cos(angle), s = Math.sin(angle);
    return joints.map(([x, y]) => [sx + (x - sx) * c - (y - sy) * s, sy + (x - sx) * s + (y - sy) * c]);
  }

  // One Rhino arm: the upper arm under a team-coloured guard, the forearm,
  // and the chainsaw clamped beside the hand.
  function rhinoArm(c, [shoulder, elbow, hand, top, nose], arm, T, t, striking) {
    segment(c, shoulder, elbow, 2.0, M.dark, M.steel);
    segment(c, elbow, hand, 1.6, M.white, T.deep);
    // the clamp from the hand to the bar
    const [bx, by] = [nose[0] - top[0], nose[1] - top[1]];
    const along = Math.max(0, Math.min(1, ((hand[0] - top[0]) * bx + (hand[1] - top[1]) * by) / (bx * bx + by * by || 1)));
    segment(c, hand, [top[0] + bx * along, top[1] + by * along], 1.4, M.darker, M.mid);
    // the guard over the shoulder, hung from the collarbone as the model's
    // pauldron is, so turning with the torso
    c.save();
    c.translate(shoulder[0], shoulder[1]);
    poly(c, [arm * 0.4, -2.4, arm * 3.0, -3.0, arm * 4.0, 0.0, arm * 3.8, 4.6, arm * 2.0, 5.4, arm * 0.3, 3.8]);
    paint(c, T.accent);
    poly(c, [arm * 0.4, -2.4, arm * 1.5, -2.6, arm * 1.5, 4.4, arm * 0.3, 3.8]);
    paint(c, M.white);
    line(c, arm * 2.6, -2.2, arm * 2.8, 4.0, T.deep, 0.35);
    c.restore();
    chainsaw(c, top, nose, T, t * (striking ? 26 : 5) * arm, striking);
  }

  // A chainsaw: its bar from the top to the nose, and the toothed saw
  // flywheel turning on the nose, a fast one blurred.
  function chainsaw(c, top, nose, T, spin, fast) {
    const length = Math.hypot(nose[0] - top[0], nose[1] - top[1]);
    c.save();
    c.translate(top[0], top[1]);
    c.rotate(Math.atan2(nose[0] - top[0], -(nose[1] - top[1])));
    rr(c, -0.8, -length, 1.6, length, 0.8);
    paint(c, T.accent);
    rr(c, -0.3, -length + 1.0, 0.6, Math.max(0, length - 1.6), 0.3);
    paint(c, T.deep, null);
    c.restore();
    saw(c, nose[0], nose[1], 2.6, spin, T, fast);
  }

  // A toothed saw flywheel turning about its hub; a fast one blurs.
  function saw(c, x, y, r, spin, T, fast) {
    c.save();
    c.translate(x, y);
    if (fast) {
      circle(c, 0, 0, r * 1.22);
      paint(c, 'rgba(230,233,238,0.25)', null);
    }
    c.rotate(spin);
    const teeth = 14;
    c.beginPath();
    for (let i = 0; i < teeth * 2; i++) {
      const a = (i * Math.PI) / teeth;
      const reach = i % 2 === 0 ? r * 1.2 : r;
      const lag = i % 2 === 0 ? 0.12 : 0;
      if (i === 0) c.moveTo(Math.sin(a + lag) * reach, -Math.cos(a + lag) * reach);
      else c.lineTo(Math.sin(a + lag) * reach, -Math.cos(a + lag) * reach);
    }
    c.closePath();
    paint(c, M.pale);
    circle(c, 0, 0, r * 0.8);
    paint(c, M.mid, null);
    for (let k = 0; k < 3; k++) {
      c.rotate((Math.PI * 2) / 3);
      rr(c, -0.22 * r, -0.72 * r, 0.44 * r, 0.34 * r, 0.12 * r);
      paint(c, M.darker, null);
    }
    circle(c, 0, 0, r * 0.34);
    paint(c, T.accent);
    circle(c, 0, 0, r * 0.13);
    paint(c, M.darker, null);
    c.restore();
  }

  // The band a Rhino's bar has swept through over the last tenth of its
  // clip, from its middle to its nose, in the unit's frame.
  function slashTrail(c, s, side) {
    if (s < 0.34 || s > 0.62) return;
    const sweep = (at) => {
      const key = slashAt(at, side);
      const [, , , top, nose] = key.striking;
      const cos = Math.cos(key.torso), sin = Math.sin(key.torso);
      const place = ([x, y]) => [key.shift[0] + x * cos - y * sin, RHINO_SPINE + key.shift[1] + x * sin + y * cos];
      return [place([(top[0] + nose[0]) / 2, (top[1] + nose[1]) / 2]), place(nose)];
    };
    c.save();
    c.globalCompositeOperation = 'lighter';
    const steps = 10;
    const fade = s < 0.5 ? 1 : 1 - (s - 0.5) / 0.12;
    for (let i = 0; i < steps; i++) {
      const [a0, a1] = sweep(Math.max(0.34, s - (0.1 * (steps - i)) / steps));
      const [b0, b1] = sweep(Math.max(0.34, s - (0.1 * (steps - i - 1)) / steps));
      c.globalAlpha = fade * ((i + 1) / steps) * 0.55;
      poly(c, [a0[0], a0[1], a1[0], a1[1], b1[0], b1[1], b0[0], b0[1]]);
      paint(c, '#ffc46b', null);
      // the nose's own path, brightest
      line(c, a1[0], a1[1], b1[0], b1[1], '#fff1d6', 0.5 * ((i + 1) / steps));
    }
    c.restore();
  }

  // One limb from `from` to `to`, `width` across, with a stripe down it.
  function segment(c, from, to, width, fill, stripe) {
    const length = Math.hypot(to[0] - from[0], to[1] - from[1]);
    c.save();
    c.translate(from[0], from[1]);
    c.rotate(Math.atan2(to[0] - from[0], -(to[1] - from[1])));
    rr(c, -width / 2, -length - width / 2, width, length + width, width * 0.45);
    paint(c, fill);
    rr(c, -width * 0.12, -length * 0.85, width * 0.24, length * 0.7, width * 0.1);
    paint(c, stripe, null);
    c.restore();
  }

  // Sparks thrown forward off a saw as it cuts.
  function sparks(c, x, y, strength, t, side) {
    c.save();
    c.globalCompositeOperation = 'lighter';
    for (let i = 0; i < 9; i++) {
      const phase = (t * 7 + i * 0.37) % 1;
      const a = side * (0.25 + 0.5 * ((i * 0.618) % 1)) - Math.PI / 2;
      const d0 = 0.4 + phase * 3.2;
      const d1 = d0 + 0.9;
      c.globalAlpha = strength * (1 - phase);
      line(c, x + Math.cos(a) * d0, y + Math.sin(a) * d0, x + Math.cos(a) * d1, y + Math.sin(a) * d1, '#ffd27a', 0.22);
    }
    c.restore();
    glow(c, x, y, 2.6, GLOW.rhino, strength * 0.9);
  }

  // Crawler: a small four-legged shell led by a spinning drill.
  function crawler(c, p) {
    const T = p.team;
    const lunge = p.strike >= 0 ? Math.sin(Math.min(1, p.strike) * Math.PI) : 0;
    c.translate(0, -lunge * 0.9);
    const ph = p.walk * Math.PI * 2 / 1.6;
    const step = Math.min(1, p.speed / 4) * 0.5;
    for (const [sx, sy, phase] of [[-1, -1, 0], [1, 1, 0], [1, -1, Math.PI], [-1, 1, Math.PI]]) {
      const off = Math.sin(ph + phase) * step;
      const hx = sx * 0.9, hy = sy * 0.5;
      const kx = sx * 1.9, ky = sy * 0.9 + off * 0.5;
      const fx = sx * 2.1, fy = sy * 1.9 + off;
      c.beginPath();
      c.moveTo(hx, hy); c.lineTo(kx, ky); c.lineTo(fx, fy);
      c.lineWidth = 0.42; c.strokeStyle = M.line; c.stroke();
      c.lineWidth = 0.24; c.strokeStyle = M.pale; c.stroke();
    }
    const spin = (p.t * (p.strike >= 0 ? 30 : 8)) % 0.6;
    poly(c, [-0.6, -1.5, 0.6, -1.5, 0, -3.3]);
    paint(c, M.mid, M.line, 0.8);
    c.save();
    poly(c, [-0.6, -1.5, 0.6, -1.5, 0, -3.3]);
    c.clip();
    for (let y = -3.4 + spin; y < -1.4; y += 0.6) line(c, -0.7, y + 0.25, 0.7, y, M.white, 0.14);
    c.restore();
    poly(c, sym([0, -1.8, 1.5, -0.8, 1.3, 1.5, 0.7, 2.0, 0, 2.0]));
    paint(c, M.white);
    poly(c, [-0.35, -1.5, 0.35, -1.5, 0.45, 1.8, -0.45, 1.8]);
    paint(c, M.dark, null);
    for (const sx of [-1, 1]) {
      poly(c, [sx * 1.4, -0.6, sx * 1.25, 1.2, sx * 0.85, 1.4, sx * 0.95, -0.6]);
      paint(c, T.accent, null);
    }
    glow(c, 0, -1.2, 0.9, GLOW.crawler, 0.9);
  }

  // Sledgehammer: a tracked tank whose turret carries one long cannon.
  function sledgehammer(c, p) {
    const T = p.team;
    const roll = (p.walk * 2.5) % 0.9;
    for (const sx of [-1, 1]) {
      rr(c, sx * 3.75 - (sx > 0 ? 1.4 : 0), -5.9, 1.4, 11.8, 0.6);
      paint(c, M.black);
      c.save();
      rr(c, sx * 3.75 - (sx > 0 ? 1.4 : 0), -5.9, 1.4, 11.8, 0.6);
      c.clip();
      for (let y = -6.4 + roll; y < 6.2; y += 0.9) line(c, sx * 2.3, y, sx * 3.8, y, M.steel, 0.22);
      c.restore();
    }
    poly(c, sym([0, -5.4, 2.2, -5.4, 2.5, -3.6, 2.5, 5.0, 0, 5.4]));
    paint(c, M.white);
    for (const sx of [-1, 1]) {
      poly(c, [sx * 1.6, -4.8, sx * 2.4, -3.4, sx * 2.4, 0.6, sx * 1.6, 0.2]);
      paint(c, T.accent, null);
    }
    rr(c, -1.6, 3.0, 3.2, 1.9, 0.3);
    paint(c, M.darker);
    for (let i = 0; i < 3; i++) line(c, -1.2 + i * 1.2, 3.3, -1.2 + i * 1.2, 4.6, GLOW.sledgehammer, 0.25);

    c.save();
    c.rotate(p.turret);
    const kick = recoil(p.fireAge, 0.7);
    circle(c, 0, 0, 2.3);
    paint(c, M.dark);
    rr(c, -0.5, -9.4 + kick * 1.6, 1.0, 8.0, 0.3);
    paint(c, T.accent);
    rr(c, -0.8, -10.2 + kick * 1.6, 1.6, 1.3, 0.3);
    paint(c, M.darker);
    rr(c, -0.65, -5.0 + kick * 1.6, 1.3, 0.8, 0.2);
    paint(c, M.dark, null);
    poly(c, sym([0, -2.6, 1.5, -2.0, 2.0, 0.4, 1.4, 2.0, 0, 2.3]));
    paint(c, M.white);
    circle(c, 0, 0.1, 1.0);
    paint(c, T.accent);
    circle(c, 0, 0.1, 0.45);
    paint(c, M.darker, null);
    line(c, 1.2, 1.2, 2.4, 2.6, M.gold, 0.12);
    flash(c, 0, -10.8 + kick * 1.6, 2.6, '#ffb347', p.fireAge, 0.2, p.fireIndex);
    c.restore();
  }

  // Wasp (Bee): a flying fuselage between two engine pods, with a stinger
  // tail it thrusts forward as it fires.
  function wasp(c, p) {
    const T = p.team;
    const bob = Math.sin(p.t * 3 + p.walk) * 0.04;
    c.scale(1 + bob, 1 + bob);
    // stinger
    const thrust = p.fireAge < 0.5 ? Math.sin(Math.min(1, p.fireAge / 0.5) * Math.PI) : 0;
    c.save();
    c.translate(0, 2.4);
    c.scale(1, 1 - thrust * 1.6);
    poly(c, [-0.5, 0, 0.5, 0, 0.3, 3.4, 0, 4.4, -0.3, 3.4]);
    paint(c, T.accent);
    c.restore();
    // engine pods and their struts
    for (const sx of [-1, 1]) {
      poly(c, [sx * 0.8, -1.0, sx * 3.0, -1.4, sx * 3.0, 0.2, sx * 0.8, 0.8]);
      paint(c, M.dark);
      poly(c, [sx * 3.0, -4.0, sx * 3.6, -3.0, sx * 3.7, 2.6, sx * 3.3, 3.4, sx * 2.9, 2.6, sx * 2.6, -3.0]);
      paint(c, M.white);
      rr(c, sx * 3.15 - 0.25, -2.4, 0.5, 4.0, 0.2);
      paint(c, M.darker, null);
      poly(c, [sx * 3.6, -1.6, sx * 4.6, -0.6, sx * 4.6, 0.4, sx * 3.7, 0.0]);
      paint(c, T.accent);
      glow(c, sx * 3.3, 3.6, 1.6, GLOW.wasp, 0.7 + 0.3 * Math.sin(p.t * 31 + sx));
    }
    poly(c, sym([0, -5.4, 0.8, -3.6, 1.1, 0.4, 0.7, 2.8, 0, 3.1]));
    paint(c, M.white);
    poly(c, sym([0, -4.9, 0.45, -3.6, 0.5, -2.4, 0, -2.1]));
    paint(c, '#bfefff', M.line, 0.6);
    poly(c, sym([0, -1.4, 0.7, -0.6, 0.6, 1.8, 0, 2.2]));
    paint(c, T.accent, null);
    flash(c, 0, 7.0 - thrust * 7.4, 1.4, GLOW.wasp, p.fireAge, 0.15, p.fireIndex);
  }

  // A unit the page has no sprite for: a disc of its collision size, marked.
  function generic(c, p, radius, label) {
    circle(c, 0, 0, radius);
    paint(c, M.dark);
    circle(c, 0, 0, radius * 0.75);
    paint(c, p.team.accent, null);
    poly(c, [0, -radius * 1.05, radius * 0.35, -radius * 0.55, -radius * 0.35, -radius * 0.55]);
    paint(c, M.white);
    if (label) {
      // the name stays upright whichever way the unit faces
      c.save();
      c.rotate(-p.facing);
      c.fillStyle = M.white;
      c.font = `bold ${radius * 0.8}px sans-serif`;
      c.textAlign = 'center';
      c.textBaseline = 'middle';
      c.fillText(label.slice(0, 2).toUpperCase(), 0, 0);
      c.restore();
    }
  }

  const UNITS = { marksman, arclight, rhino, crawler, sledgehammer, wasp };

  function drawUnit(c, kind, p, radius) {
    const draw = UNITS[kind];
    if (draw) draw(c, p);
    else generic(c, p, radius, kind);
  }

  // -------------------------------------------------------------- buildings
  // The Energy Tower: a round base around a glowing core, four pods at its
  // diagonals and a ring turning above them.
  // A tower's outer ring, which is its life: the lit arc runs clockwise from
  // its front and shortens as the tower loses life, over a dim whole ring.
  function lifeRing(c, T, life) {
    circle(c, 0, 0, 9.7);
    c.lineWidth = 0.55;
    c.strokeStyle = `rgba(${T.rgb},0.22)`;
    c.stroke();
    const left = Math.max(0, Math.min(1, life));
    if (left <= 0) return;
    c.beginPath();
    c.arc(0, 0, 9.7, -Math.PI / 2, -Math.PI / 2 + Math.PI * 2 * left);
    c.lineWidth = 0.55;
    c.strokeStyle = T.accent;
    c.stroke();
  }

  function energyTower(c, p) {
    const T = p.team;
    circle(c, 0, 0, 10.6);
    paint(c, M.darker);
    lifeRing(c, T, p.life);
    for (let i = 0; i < 12; i++) {
      const a = i * Math.PI / 6;
      line(c, Math.sin(a) * 3.2, -Math.cos(a) * 3.2, Math.sin(a) * 8.6, -Math.cos(a) * 8.6, M.steel, 0.35);
    }
    for (let i = 0; i < 4; i++) {
      const a = Math.PI / 4 + i * Math.PI / 2;
      const x = Math.sin(a) * 6.4, y = -Math.cos(a) * 6.4;
      circle(c, x, y, 2.6);
      paint(c, M.mid);
      circle(c, x, y, 1.7);
      paint(c, M.pale, M.line, 0.6);
      circle(c, x, y, 0.8);
      paint(c, T.deep, null);
    }
    c.save();
    c.rotate(p.t * 0.4);
    for (let i = 0; i < 6; i++) {
      c.beginPath();
      c.arc(0, 0, 4.6, i * Math.PI / 3 + 0.15, (i + 1) * Math.PI / 3 - 0.15);
      c.lineWidth = 0.9;
      c.strokeStyle = M.white;
      c.stroke();
    }
    c.restore();
    circle(c, 0, 0, 2.8);
    paint(c, M.white);
    const pulse = 0.7 + 0.3 * Math.sin(p.t * 2.2);
    circle(c, 0, 0, 2.0);
    paint(c, '#2fe3ff', null);
    glow(c, 0, 0, 6.5 * pulse, '#53ecff', 0.8 * p.life + 0.1);
    flash(c, 0, 0, 3.4, '#8ff4ff', p.fireAge, 0.2, p.fireIndex);
  }

  // The Research Center, drawn for what it does rather than after its model:
  // on the Energy Tower's round base, a hexagonal laboratory floor holding an
  // atom, three orbits turning slowly around a crystal nucleus, each carrying
  // a bright electron.
  function researchCenter(c, p) {
    const T = p.team;
    circle(c, 0, 0, 10.6);
    paint(c, M.darker);
    lifeRing(c, T, p.life);
    const hex = (r) => {
      const pts = [];
      for (let i = 0; i < 6; i++) {
        const a = (i * Math.PI) / 3;
        pts.push(Math.cos(a) * r, Math.sin(a) * r);
      }
      return pts;
    };
    poly(c, hex(8.5));
    paint(c, M.dark, M.line, 1.2);
    poly(c, hex(7.4));
    paint(c, null, M.steel, 1.2);
    for (let i = 0; i < 6; i++) {
      const a = (i * Math.PI) / 3;
      circle(c, Math.cos(a) * 7.95, Math.sin(a) * 7.95, 0.4);
      paint(c, M.pale, null);
    }
    // an orbit is an ellipse turned about the nucleus, drawn as a polyline
    const orbit = (k, phase) => {
      const tilt = p.t * 0.25 + (k * Math.PI) / 3;
      const point = (u) => {
        const x = Math.cos(u) * 6.1;
        const y = Math.sin(u) * 2.2;
        return [x * Math.cos(tilt) - y * Math.sin(tilt), x * Math.sin(tilt) + y * Math.cos(tilt)];
      };
      c.beginPath();
      for (let i = 0; i <= 48; i++) {
        const [x, y] = point((i / 48) * Math.PI * 2);
        if (i === 0) c.moveTo(x, y); else c.lineTo(x, y);
      }
      return point(phase);
    };
    const electrons = [];
    for (let k = 0; k < 3; k++) {
      const at = orbit(k, p.t * 1.6 + k * 2.1);
      c.lineWidth = 0.7;
      c.strokeStyle = M.line;
      c.stroke();
      c.lineWidth = 0.32;
      c.strokeStyle = T.light;
      c.stroke();
      electrons.push(at);
    }
    for (const [x, y] of electrons) {
      glow(c, x, y, 1.6, `rgba(${T.rgb},1)`, 0.9);
      circle(c, x, y, 0.5);
      paint(c, '#ffffff', M.line, 0.6);
    }
    const gleam = 0.6 + 0.4 * Math.sin(p.t * 1.7);
    glow(c, 0, 0, 4.0, `rgba(${T.rgb},1)`, gleam * 0.7 * p.life + 0.1);
    poly(c, [0, -1.9, 1.5, 0, 0, 1.9, -1.5, 0]);
    paint(c, T.accent);
    poly(c, [0, -1.9, 1.5, 0, 0, 0]);
    paint(c, T.light, null);
  }

  // Anti-Armor Turret: four splayed legs, sandbags and one heavy cannon.
  function antiArmorTurret(c, p) {
    const T = p.team;
    for (let i = 0; i < 4; i++) {
      const a = Math.PI / 4 + i * Math.PI / 2;
      c.save();
      c.rotate(a);
      rr(c, -1.1, -10.2, 2.2, 7.6, 0.6);
      paint(c, M.dark);
      rr(c, -1.7, -11.2, 3.4, 2.2, 0.6);
      paint(c, M.steel);
      rr(c, -0.5, -9.0, 1.0, 2.4, 0.2);
      paint(c, M.caution, null);
      c.restore();
    }
    // sandbags behind the gun
    for (let i = 0; i < 12; i++) {
      const a = (100 + i * 14.5) * Math.PI / 180;
      const x = Math.sin(a) * 8.8, y = -Math.cos(a) * 8.8;
      c.save();
      c.translate(x, y);
      c.rotate(a);
      rr(c, -1.3, -0.7, 2.6, 1.4, 0.6);
      paint(c, M.sand, M.line, 0.6);
      c.restore();
    }
    circle(c, 0, 0, 5.0);
    paint(c, M.mid);
    c.save();
    c.rotate(p.turret);
    const kick = recoil(p.fireAge, 0.8);
    rr(c, -1.1, -13.0 + kick * 2.2, 2.2, 10.4, 0.5);
    paint(c, M.pale);
    for (const y of [-11.2, -8.4]) { rr(c, -1.25, y + kick * 2.2, 2.5, 0.7, 0.2); paint(c, M.dark, null); }
    rr(c, -1.6, -14.2 + kick * 2.2, 3.2, 1.8, 0.4);
    paint(c, M.darker);
    rr(c, -3.6, -3.4, 7.2, 8.0, 1.0);
    paint(c, M.white);
    rr(c, -2.4, -2.4, 4.8, 3.4, 0.6);
    paint(c, M.dark, null);
    for (const sx of [-1, 1]) {
      rr(c, sx * 3.0 - 0.6, -1.0, 1.2, 4.6, 0.3);
      paint(c, M.caution, null);
    }
    rr(c, -2.6, 2.4, 5.2, 1.4, 0.3);
    paint(c, T.accent, null);
    flash(c, 0, -15.2 + kick * 2.2, 3.4, '#ffb347', p.fireAge, 0.22, p.fireIndex);
    c.restore();
  }

  // Rapid-Fire Turret: a light mount with twin barrels firing in turn, fed
  // from two drums.
  function rapidFireTurret(c, p) {
    const T = p.team;
    for (let i = 0; i < 4; i++) {
      c.save();
      c.rotate(Math.PI / 4 + i * Math.PI / 2);
      rr(c, -0.8, -6.6, 1.6, 4.0, 0.4);
      paint(c, M.dark);
      rr(c, -1.3, -7.2, 2.6, 1.4, 0.5);
      paint(c, M.steel);
      c.restore();
    }
    circle(c, 0, 0, 4.2);
    paint(c, M.mid);
    c.save();
    c.rotate(p.turret);
    for (const sx of [-1, 1]) {
      rr(c, sx * 3.0 - (sx > 0 ? 0 : 2.6), -0.6, 2.6, 2.8, 0.6);
      paint(c, M.dark);
      for (let i = 0; i < 4; i++) {
        rr(c, sx * 3.0 - (sx > 0 ? 0 : 2.6) + 0.3 + i * 0.55, -0.3, 0.35, 2.2, 0.15);
        paint(c, M.caution, null);
      }
    }
    for (const [i, sx] of [[0, -1], [1, 1]]) {
      const mine = (p.fireIndex % 2) === i;
      const kick = mine ? recoil(p.fireAge, 0.12) : 0;
      rr(c, sx * 0.95 - 0.4, -11.0 + kick * 1.2, 0.8, 9.2, 0.3);
      paint(c, M.pale);
      rr(c, sx * 0.95 - 0.55, -11.6 + kick * 1.2, 1.1, 1.0, 0.3);
      paint(c, M.darker);
      if (mine) flash(c, sx * 0.95, -12.2, 1.8, '#ffd36b', p.fireAge, 0.1, p.fireIndex);
    }
    rr(c, -2.6, -2.4, 5.2, 6.0, 0.9);
    paint(c, M.white);
    rr(c, -1.6, -1.6, 3.2, 2.2, 0.4);
    paint(c, M.dark, null);
    rr(c, -2.2, 2.2, 4.4, 1.0, 0.3);
    paint(c, T.accent, null);
    c.restore();
  }

  // One block of a Defensive Wall, its capped front toward the enemy.
  function wallBlock(c, p) {
    const T = p.team;
    poly(c, [-4.4, -4.8, 4.4, -4.8, 4.9, -4.2, 4.9, 4.4, 4.2, 4.9, -4.2, 4.9, -4.9, 4.4, -4.9, -4.2]);
    paint(c, M.dark);
    for (const sx of [-1, 1]) {
      poly(c, [sx * 0.9, -3.2, sx * 3.9, -3.2, sx * 3.6, 3.9, sx * 1.1, 3.9]);
      paint(c, T.accent, M.line, 0.6);
      poly(c, [sx * 0.9, -3.2, sx * 1.6, -3.2, sx * 1.8, 3.9, sx * 1.1, 3.9]);
      paint(c, T.deep, null);
      rr(c, sx * 4.0 - (sx > 0 ? 3.2 : 0), -4.5, 3.2, 1.1, 0.2);
      paint(c, M.gold, M.line, 0.5);
    }
    rr(c, -0.6, -3.0, 1.2, 6.6, 0.2);
    paint(c, M.darker, null);
    line(c, -0.2, -2.6, -0.2, 2.0, M.pale, 0.12);
    line(c, 0.2, -2.6, 0.2, 2.0, M.pale, 0.12);
    if (p.life < 0.6) {
      c.save();
      c.globalAlpha = Math.min(1, (0.6 - p.life) * 3);
      line(c, -3.0, -1.4, -1.6, 0.4, M.black, 0.25);
      line(c, -1.6, 0.4, -2.2, 2.0, M.black, 0.2);
      line(c, 2.4, -1.0, 1.4, 1.4, M.black, 0.22);
      c.restore();
    }
  }

  function genericBuilding(c, p, width, depth) {
    rr(c, -width / 2, -depth / 2, width, depth, Math.min(width, depth) * 0.15);
    paint(c, M.dark);
    rr(c, -width / 3, -depth / 3, width * 2 / 3, depth * 2 / 3, Math.min(width, depth) * 0.1);
    paint(c, p.team.accent, null);
  }

  const BUILDINGS = {
    energy_tower: energyTower,
    research_center: researchCenter,
    anti_armor_turret: antiArmorTurret,
    rapid_fire_turret: rapidFireTurret,
    defensive_wall: wallBlock,
  };

  function drawBuilding(c, kind, p, width, depth) {
    const draw = BUILDINGS[kind];
    if (draw) draw(c, p);
    else genericBuilding(c, p, width, depth);
  }

  // The generator at the centre of a deployed shield.
  function shieldGenerator(c, team, t) {
    poly(c, [0, -3.2, 2.8, -1.6, 2.8, 1.6, 0, 3.2, -2.8, 1.6, -2.8, -1.6]);
    paint(c, M.dark);
    poly(c, [0, -2.2, 1.9, -1.1, 1.9, 1.1, 0, 2.2, -1.9, 1.1, -1.9, -1.1]);
    paint(c, M.white, null);
    circle(c, 0, 0, 1.0);
    paint(c, team.accent);
    glow(c, 0, 0, 4, `rgba(${team.rgb},1)`, 0.6 + 0.3 * Math.sin(t * 3));
  }

  // A sprite drawn whole into a small canvas, for the legend.
  function icon(kind, team, size, building) {
    const canvas = document.createElement('canvas');
    const dpr = window.devicePixelRatio || 1;
    canvas.width = canvas.height = Math.round(size * dpr);
    canvas.style.width = canvas.style.height = `${size}px`;
    const c = canvas.getContext('2d');
    const [w, h] = SIZE[kind] || [10, 10];
    const scale = (size * dpr * 0.86) / Math.max(w, h);
    setScale(1 / scale);
    c.translate(canvas.width / 2, canvas.height / 2);
    c.scale(scale, scale);
    const p = pose();
    p.team = TEAMS[team];
    p.t = 0.6;
    if (kind === 'marksman') c.translate(-1, 3);
    if (kind === 'wasp') c.translate(0, -0.6);
    if (building) drawBuilding(c, kind, p, w, h);
    else drawUnit(c, kind, p, 4);
    return canvas;
  }

  return {
    TEAMS, GLOW, SIZE, setScale, pose, drawUnit, drawBuilding, shieldGenerator, icon,
    has: (kind) => kind in UNITS || kind in BUILDINGS,
    glow, flash, ease,
  };
})();
