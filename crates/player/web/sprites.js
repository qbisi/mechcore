// Top-down sprites, drawn after the game's own models.
//
// Every sprite is drawn in metres around the object's centre with its front
// toward -y, so the page turns it by the recorded facing and scales it by the
// zoom. The shapes and proportions follow each model as its prefab assembles
// it in its attack pose, seen from straight above: the parts a unit moves in
// a fight (legs, turret, barrels, arms) are drawn apart so the page
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
    sledgehammer: '#55e8f4', wasp: '#ffd84e', phoenix: '#00eaff', fang: '#e7d512',
    hound: '#ffba19',
    void_eye: '#bc55ff',
    vortex: '#76f9fe',
    mustang: '#12878d',
    steel_ball: '#ffd114',
    stormcaller: '#65f9ff',
    hacker: '#1cfefc',
    fire_badger: '#b18926',
    sabertooth: '#ffff7b',
    tarantula: '#9eff22',
    phantom_ray: '#5a91a7',
    wraith: '#caff98',
    scorpion: '#c8ffef',
    typhoon: '#00b9fa',
    farseer: '#c1b936',
    centurion: '#ff9513',
    fortress: '#ffe525',
    vulcan: '#9dcdc7',
    melting_point: '#ffe116',
    overlord: '#fec21b',
    war_factory: '#b9ca6e',
    sandworm: '#32677e',
    raiden: '#00c2ff',
    abyss: '#f975ff',
    mountain: '#fb9a39',
    larva: '#32677e',
    spider_mine: '#81d01c',
    sentry_missile: '#ff8809',
  };
  // The model's footprint in metres, width by length, which frames an icon.
  const SIZE = {
    marksman: [12, 22], arclight: [18, 13], rhino: [28, 24], crawler: [4.2, 5.2],
    sledgehammer: [7.6, 13], wasp: [7.4, 6.6], phoenix: [18.4, 12.8], fang: [6.6, 5.4],
    hound: [7.8, 10],
    void_eye: [8.8, 8.6],
    vortex: [7.8, 7.0],
    mustang: [5.8, 9.2],
    steel_ball: [10.6, 16],
    stormcaller: [8.8, 11.8],
    hacker: [21.4, 21.8],
    fire_badger: [14.2, 19.4],
    sabertooth: [20.4, 26.4],
    tarantula: [23, 28.6],
    phantom_ray: [15.4, 17.4],
    wraith: [26.8, 26.8],
    scorpion: [27, 24.2],
    typhoon: [21.8, 21.4],
    farseer: [16, 23.6],
    centurion: [19.6, 32.6],
    fortress: [34.8, 28.4],
    vulcan: [30.6, 35.4],
    melting_point: [33.8, 33.2],
    overlord: [44.6, 57],
    war_factory: [67.4, 72.6],
    sandworm: [22.4, 39.8],
    raiden: [30.4, 45.8],
    abyss: [53.4, 54.4],
    mountain: [66.4, 40.6],
    larva: [11.6, 20],
    spider_mine: [4.6, 4.8],
    sentry_missile: [1.6, 15.4],
    energy_tower: [22, 22], research_center: [22, 22],
    interceptor: [8, 9.8],
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

  // Wasp (Bee), seen from above without its abdomen and stinger, which
  // hang under the body where the top view cannot see them, nor the small
  // wings on its engines' flanks: a pointed fuselage with its canopy at the
  // nose and two fins at its tail, a white thruster pod each side of it,
  // and outboard on struts the two long engines, armoured in the team's
  // colour on their outer flanks, that hold it up.
  function wasp(c, p) {
    const T = p.team;
    const bob = Math.sin(p.t * 3 + p.walk) * 0.04;
    c.scale(1 + bob, 1 + bob);
    const pulse = 0.75 + 0.25 * Math.sin(p.t * 29);
    for (const sx of [-1, 1]) {
      // the strut out to the engine
      poly(c, [sx * 1.0, -0.9, sx * 2.3, -0.8, sx * 2.3, 0.4, sx * 1.0, 0.3]);
      paint(c, M.dark);
      // the engine's armour, outboard
      poly(c, [sx * 2.8, -1.25, sx * 3.45, -1.05, sx * 3.55, -0.25, sx * 3.0, 0.15, sx * 2.75, -0.2]);
      paint(c, T.accent);
      poly(c, [sx * 2.85, 0.05, sx * 3.15, 0.3, sx * 2.95, 0.65, sx * 2.7, 0.4]);
      paint(c, M.darker);
      // the engine: a long white body, twin prongs at its head and its
      // nozzles glowing at the back
      for (const dx of [-0.12, 0.12]) {
        rr(c, sx * 2.55 + dx - 0.06, -2.5, 0.12, 0.6, 0.05);
        paint(c, M.steel, M.line, 0.5);
      }
      poly(c, [sx * 2.55, -2.05, sx * 2.9, -1.4, sx * 2.95, 0.6, sx * 2.75, 1.8, sx * 2.35, 1.8, sx * 2.15, 0.6, sx * 2.2, -1.4]);
      paint(c, M.white);
      poly(c, [sx * 2.5, -1.7, sx * 2.6, -1.7, sx * 2.65, 1.6, sx * 2.45, 1.6]);
      paint(c, M.mid, null);
      for (const dx of [-0.12, 0.12]) {
        rr(c, sx * 2.55 + dx - 0.07, 1.75, 0.14, 0.4, 0.05);
        paint(c, M.dark, M.line, 0.5);
      }
      glow(c, sx * 2.55, 2.1, 1.0, GLOW.wasp, 0.75 * pulse);
      // the thruster pod beside the fuselage
      rr(c, sx * 1.3 - 0.35, -1.05, 0.7, 2.35, 0.3);
      paint(c, M.white);
      poly(c, [sx * 1.3, -0.95, sx * 1.48, -0.55, sx * 1.4, -0.2, sx * 1.2, -0.2, sx * 1.12, -0.55]);
      paint(c, M.mid, null);
    }
    // the tail fins
    for (const sx of [-1, 1]) {
      poly(c, [sx * 0.2, 1.3, sx * 0.85, 1.55, sx * 1.1, 3.0, sx * 0.7, 2.95, sx * 0.25, 2.3]);
      paint(c, M.white);
      poly(c, [sx * 0.35, 1.25, sx * 0.75, 1.3, sx * 0.8, 1.75, sx * 0.4, 1.7]);
      paint(c, T.accent);
    }
    // the fuselage: pointed at the nose, widest behind the canopy
    poly(c, sym([0, -3.2, 0.45, -2.5, 0.85, -1.3, 1.0, -0.4, 0.8, 0.9, 0.45, 1.4, 0, 1.5]));
    paint(c, M.white);
    poly(c, sym([0, 0.05, 0.75, -1.1, 0.7, -0.2, 0.3, 0.2]));
    paint(c, M.dark, null);
    // the canopy
    poly(c, sym([0, -2.75, 0.3, -2.6, 0.45, -1.2, 0, -1.1]));
    paint(c, '#bfefff', M.line, 0.6);
    // the back: a white block between the team-coloured mountings the
    // abdomen hangs from
    rr(c, -0.33, -0.25, 0.66, 1.0, 0.1);
    paint(c, M.pale, M.line, 0.6);
    for (const sx of [-1, 1]) {
      poly(c, [sx * 0.4, 0.75, sx * 0.85, 0.7, sx * 0.75, 1.4, sx * 0.4, 1.35]);
      paint(c, T.accent, M.line, 0.6);
    }
  }

  // Phoenix, as its model flies: a long pointed nose over a short body, a
  // tail boom each side of it ending in fins, a white pylon outboard of
  // each boom, and from each pylon a long panel in the team's colour swept
  // back and out. Its exhausts glow in its emissive colour.
  function phoenix(c, p) {
    const T = p.team;
    const bob = Math.sin(p.t * 2.4 + p.walk) * 0.03;
    c.scale(1 + bob, 1 + bob);
    for (const sx of [-1, 1]) {
      // the swept panel, from the pylon back and out to its capped tip
      poly(c, [sx * 4.4, -0.3, sx * 8.7, 2.2, sx * 7.9, 3.8, sx * 4.1, 1.5]);
      paint(c, T.accent);
      line(c, sx * 4.6, 0.6, sx * 8.3, 2.9, T.deep, 0.25);
      poly(c, [sx * 8.55, 2.0, sx * 9.2, 2.4, sx * 8.4, 4.05, sx * 7.75, 3.7]);
      paint(c, T.deep);
      // the strut out to the pylon
      poly(c, [sx * 1.0, -0.9, sx * 4.0, -0.7, sx * 4.0, 0.5, sx * 1.0, 0.6]);
      paint(c, M.dark);
      rr(c, sx * 2.75 - 0.25, -1.7, 0.5, 1.0, 0.1);
      paint(c, T.accent, M.line, 0.6);
      // the pylon
      poly(c, [sx * 3.95, -3.5, sx * 4.6, -3.5, sx * 4.65, 1.9, sx * 4.3, 3.2, sx * 3.9, 2.2]);
      paint(c, M.white);
      rr(c, sx * 4.25 - 0.3, -4.1, 0.6, 0.7, 0.1);
      paint(c, M.mid);
      // the tail boom, widening into its tailplane, red fins at its end
      poly(c, [sx * 1.25, -0.8, sx * 2.15, -0.8, sx * 2.25, 3.2, sx * 3.9, 5.3, sx * 3.8, 5.9, sx * 2.1, 5.2, sx * 1.6, 6.3, sx * 1.35, 3.3]);
      paint(c, M.white);
      poly(c, [sx * 1.5, -0.5, sx * 1.9, -0.5, sx * 1.95, 2.8, sx * 1.6, 2.8]);
      paint(c, M.mid, null);
      for (const [x, y0, y1] of [[1.75, 4.7, 6.3], [3.85, 4.6, 6.0]]) {
        rr(c, sx * x - 0.17, y0, 0.34, y1 - y0, 0.1);
        paint(c, T.accent, M.line, 0.6);
      }
    }
    // the body behind the cockpit, its dark intake under it
    rr(c, -0.65, 0.4, 1.3, 1.6, 0.25);
    paint(c, '#2c3a55');
    // the exhausts under the intake's back, which glow
    for (const sx of [-1, 1]) glow(c, sx * 0.45, 1.9, 0.9, GLOW.phoenix, 0.7 + 0.3 * Math.sin(p.t * 23 + sx));
    rr(c, -0.5, -0.9, 1.0, 1.35, 0.15);
    paint(c, M.white);
    // the collar around the nose's root
    poly(c, sym([0, -1.9, 0.9, -1.6, 1.5, -0.95, 0.9, -0.85, 0, -1.05]));
    paint(c, M.white);
    // the nose, striped in the team's colour, its canopy along the middle
    poly(c, sym([0, -6.3, 0.3, -5.2, 0.75, -2.6, 0.95, -1.0, 0, -0.9]));
    paint(c, M.white);
    for (const sx of [-1, 1]) {
      poly(c, [sx * 0.32, -5.0, sx * 0.55, -3.9, sx * 0.85, -1.9, sx * 0.62, -2.0, sx * 0.4, -3.9]);
      paint(c, T.accent, null);
    }
    poly(c, sym([0, -2.7, 0.24, -2.4, 0.24, -1.0, 0, -0.95]));
    paint(c, '#9fd6f5', M.line, 0.6);
  }

  // Fang, as it fires from the ground: a small biped carrying a heavy gun
  // on its right arm, a big pack on its back to its left, a glass dome for
  // a head and two vents glowing at the back of its chest.
  function fang(c, p) {
    const T = p.team;
    const kick = recoil(p.fireAge, 0.4) * 0.35;
    // the feet
    for (const sx of [-1, 1]) {
      rr(c, 0.1 + sx * 0.38 - 0.28, 1.2, 0.56, 0.75, 0.25);
      paint(c, M.black);
    }
    // the pack, slung behind on the left
    rr(c, -1.95, 0.6, 0.95, 1.3, 0.2);
    paint(c, M.pale);
    poly(c, [-3.7, 0.95, -3.3, 0.65, -1.9, 0.6, -1.85, 2.3, -2.6, 2.45, -3.45, 2.3, -3.75, 1.6]);
    paint(c, M.white);
    poly(c, [-3.7, 0.95, -3.3, 0.65, -2.55, 0.7, -2.9, 1.25, -3.65, 1.3]);
    paint(c, T.accent, null);
    line(c, -3.4, 1.75, -2.1, 1.75, M.mid, 0.12);
    // the left shoulder
    rr(c, -1.8, -0.8, 0.85, 1.4, 0.2);
    paint(c, M.dark);
    rr(c, -1.5, -0.65, 0.22, 0.75, 0.08);
    paint(c, M.black, null);
    // the body: white plates round a dark core, the vents at its back
    poly(c, [-0.8, 0.1, -0.35, -0.2, 0.55, -0.2, 0.85, 0.15, 0.8, 1.35, -0.75, 1.35]);
    paint(c, M.white);
    rr(c, -0.3, 0.1, 0.75, 1.15, 0.12);
    paint(c, M.dark, null);
    for (const x of [-0.4, 0.37]) {
      rr(c, x - 0.08, 0.4, 0.16, 0.4, 0.04);
      paint(c, GLOW.fang, null);
    }
    rr(c, -0.2, 1.2, 0.42, 0.45, 0.08);
    paint(c, T.accent);
    // the right shoulder
    poly(c, [0.82, 0.3, 1.55, 0.25, 1.85, 0.7, 1.7, 1.45, 0.85, 1.4]);
    paint(c, M.white);
    // the dome
    circle(c, 0.13, -0.1, 0.28);
    paint(c, M.glass, M.line, 0.7);
    // the arm and the gun on it, kicked back by its shot
    c.save();
    c.translate(0, kick);
    rr(c, 1.35, -0.6, 0.6, 0.95, 0.15);
    paint(c, M.dark);
    rr(c, 1.9, -1.35, 0.5, 0.85, 0.2);
    paint(c, M.pale);
    poly(c, [1.95, -1.0, 2.6, -0.85, 1.95, -0.6]);
    paint(c, T.accent);
    poly(c, [0.92, -2.62, 1.55, -2.7, 1.98, -0.2, 1.28, -0.12]);
    paint(c, T.accent);
    poly(c, [1.18, -2.35, 1.38, -2.37, 1.62, -0.6, 1.42, -0.58]);
    paint(c, M.darker, null);
    rr(c, 1.05, -2.8, 0.45, 0.25, 0.08);
    paint(c, M.dark);
    flash(c, 1.27, -2.95, 1.0, GLOW.fang, p.fireAge, 0.12, p.fireIndex);
    c.restore();
  }

  // Hound, as it fires: a four-legged walker whose back is one white hood
  // over a twin-barrelled gun, a red ammunition drum on each flank behind
  // the forelegs, and two red plates trailing from its tail.
  function hound(c, p) {
    const T = p.team;
    const stride = Math.min(1, p.speed / 4) * 0.6;
    const ph = p.walk * Math.PI * 2 / 5;
    const kick = recoil(p.fireAge, 0.45) * 0.4;
    // the hind legs and the tail plates
    for (const sx of [-1, 1]) {
      const off = Math.sin(ph + (sx > 0 ? 0 : Math.PI)) * stride;
      poly(c, [sx * 1.6, 3.4 + off, sx * 3.2, 3.5 + off, sx * 3.0, 5.0 + off, sx * 2.2, 5.3 + off, sx * 1.7, 4.7 + off]);
      paint(c, M.dark);
      rr(c, sx * 1.05 - 0.4, 3.9, 0.8, 2.1, 0.12);
      paint(c, T.accent);
      line(c, sx * 1.05, 4.1, sx * 1.05, 5.8, T.deep, 0.15);
    }
    // the forelegs, white, their claws ahead
    for (const sx of [-1, 1]) {
      const off = Math.sin(ph + (sx > 0 ? Math.PI : 0)) * stride;
      const y = sx > 0 ? -0.5 : 0;
      c.save();
      c.translate(0, off);
      for (const dx of [-0.45, 0.45]) {
        rr(c, sx * 2.85 + dx - 0.3, y - 3.3, 0.6, 0.7, 0.15);
        paint(c, M.black);
      }
      rr(c, sx * 2.85 - 0.85, y - 2.75, 1.7, 3.0, 0.5);
      paint(c, M.white);
      rr(c, sx * 2.85 - 0.45, y - 1.6, 0.9, 1.0, 0.2);
      paint(c, M.mid, null);
      c.restore();
    }
    // the body under the hood, and its back
    rr(c, -1.6, 1.4, 3.2, 3.0, 0.4);
    paint(c, M.dark);
    circle(c, 0, 2.95, 0.5);
    paint(c, M.pale);
    rr(c, -0.3, 3.6, 0.6, 0.7, 0.1);
    paint(c, M.darker, null);
    // the drums on the flanks
    for (const sx of [-1, 1]) {
      rr(c, sx * 2.45 - 0.6, 0.55, 1.2, 2.9, 0.5);
      paint(c, T.accent);
      line(c, sx * 2.25, 0.9, sx * 2.25, 3.1, 'rgba(255,255,255,0.45)', 0.18);
    }
    // the hood, kicked back by a shot, the barrels' mouths at its front
    c.save();
    c.translate(0, kick);
    for (const sx of [-1, 1]) line(c, sx * 1.1, -2.4, sx * 1.1, -3.4, M.steel, 0.1);
    poly(c, [-1.35, -2.5, 1.35, -2.5, 1.65, -1.2, 1.95, 0.1, 1.6, 1.3, 0.6, 1.6, -0.6, 1.6, -1.6, 1.3, -1.95, 0.1, -1.65, -1.2]);
    paint(c, M.white);
    line(c, 0, -2.1, 0, 1.4, M.mid, 0.12);
    for (const sx of [-1, 1]) {
      poly(c, [sx * 0.15, -1.8, sx * 1.2, -1.6, sx * 1.6, 0.3, sx * 0.3, 0.9]);
      paint(c, M.pale, null);
      circle(c, sx * 0.5, -2.3, 0.38);
      paint(c, M.darker);
      circle(c, sx * 0.5, -2.3, 0.2);
      paint(c, M.black, null);
    }
    poly(c, [-0.55, 1.1, 0.55, 1.1, 0.45, 2.3, -0.45, 2.3]);
    paint(c, T.accent);
    poly(c, [-0.13, -0.85, 0.13, -0.85, 0, -1.1]);
    paint(c, GLOW.hound, null);
    glow(c, 0, -0.95, 0.5, GLOW.hound, 0.6);
    for (const sx of [-1, 1]) flash(c, sx * 0.5, -2.75, 1.3, '#ffcf70', p.fireAge, 0.14, p.fireIndex + sx);
    c.restore();
  }

  // Void Eye, as it attacks the ground: a flyer whose white head comes to a
  // spike, a red claw curving forward round each side of it, two white pods
  // trailing behind, and its eye glowing under the head's jaw.
  function void_eye(c, p) {
    const T = p.team;
    const bob = Math.sin(p.t * 2.2 + p.walk) * 0.03;
    c.scale(1 + bob, 1 + bob);
    const pulse = 0.7 + 0.3 * Math.sin(p.t * 5);
    // the pods behind
    for (const sx of [-1, 1]) {
      poly(c, [sx * 0.85, 0.35, sx * 1.75, 0.35, sx * 1.7, 2.0, sx * 1.55, 3.4, sx * 1.2, 3.4, sx * 0.9, 2.0]);
      paint(c, M.white);
      line(c, sx * 1.3, 0.8, sx * 1.35, 2.9, M.pale, 0.2);
      circle(c, sx * 2.1, 0.6, 0.35);
      paint(c, M.steel);
    }
    // the body between the pods
    rr(c, -1.0, -0.2, 2.0, 1.8, 0.3);
    paint(c, M.dark);
    rr(c, -0.35, 0.3, 0.7, 0.35, 0.08);
    paint(c, '#8a6234', null);
    // the claws, each round its side, and the white blade inside it
    for (const sx of [-1, 1]) {
      poly(c, [sx * 2.0, -4.9, sx * 1.5, -4.55, sx * 1.6, -1.6, sx * 2.3, -1.8, sx * 2.45, -3.1]);
      paint(c, M.white);
      poly(c, [sx * 1.95, -5.0, sx * 3.0, -4.3, sx * 3.8, -3.2, sx * 4.2, -1.8, sx * 4.3, -0.8, sx * 4.0, 0.6, sx * 3.3, 1.6, sx * 2.5, 1.7, sx * 2.2, 0.8, sx * 2.45, -0.6, sx * 2.9, -1.6, sx * 3.0, -3.0, sx * 2.5, -4.2]);
      paint(c, T.accent);
      poly(c, [sx * 3.9, -2.6, sx * 4.2, -1.6, sx * 4.1, 0.2, sx * 3.75, 0.0, sx * 3.85, -1.6]);
      paint(c, T.deep, null);
      rr(c, sx * 3.2 - 0.2, -2.3, 0.4, 0.75, 0.1);
      paint(c, M.black, null);
      circle(c, sx * 3.15, -3.2, 0.22);
      paint(c, M.steel);
    }
    // the eye under the jaw
    glow(c, 0, -0.35, 1.2, GLOW.void_eye, pulse);
    poly(c, [-0.55, -0.35, 0, -0.55, 0.55, -0.35, 0, -0.2]);
    paint(c, GLOW.void_eye, null);
    // the head, a white shield ending in a spike, a dark slot down its brow
    poly(c, [0, -5.05, 0.45, -3.4, 1.35, -3.15, 1.45, -1.6, 1.15, -0.85, 0.6, -0.45, -0.6, -0.45, -1.15, -0.85, -1.45, -1.6, -1.35, -3.15, -0.45, -3.4]);
    paint(c, M.white);
    poly(c, [-1.4, -1.6, -0.6, -1.9, 0, -1.5, 0.6, -1.9, 1.4, -1.6, 1.1, -0.85, 0.55, -0.5, -0.55, -0.5, -1.1, -0.85]);
    paint(c, M.pale, null);
    rr(c, -0.2, -3.2, 0.45, 1.2, 0.15);
    paint(c, M.dark, null);
    for (let i = 0; i < 6; i++) {
      rr(c, -0.65 + i * 0.25, -0.35, 0.12, 0.25, 0.03);
      paint(c, M.caution, null);
    }
    flash(c, 0, -0.35, 1.6, GLOW.void_eye, p.fireAge, 0.18, p.fireIndex);
  }

  // Vortex, as it attacks: a crab-like walker with two great claws reaching
  // ahead, a leg out to each side and one to each back corner on red feet,
  // a round coil housing at its middle and two coils at its back; its claw
  // tips and coils glow, and arc as it fires.
  function vortex(c, p) {
    const T = p.team;
    const stride = Math.min(1, p.speed / 4) * 0.3;
    const ph = p.walk * Math.PI * 2 / 4;
    const pulse = 0.7 + 0.3 * Math.sin(p.t * 9);
    for (const sx of [-1, 1]) {
      const off = Math.sin(ph + (sx > 0 ? 0 : Math.PI)) * stride;
      // the back leg, out to its corner
      c.save();
      c.translate(sx * 2.4, 2.4 + off);
      c.rotate(sx * Math.PI / 4);
      rr(c, -0.75, -1.1, 1.5, 1.4, 0.3);
      paint(c, M.white);
      rr(c, -0.55, -0.55, 1.1, 1.1, 0.12);
      paint(c, T.accent);
      c.restore();
      poly(c, [sx * 1.2, 1.4, sx * 1.9, 1.7, sx * 2.2, 2.1, sx * 1.6, 2.5, sx * 1.0, 2.0]);
      paint(c, M.dark);
      // the side leg
      poly(c, [sx * 1.6, -1.2, sx * 3.7, -1.25, sx * 3.85, -0.5, sx * 3.6, -0.1, sx * 1.6, -0.1]);
      paint(c, M.white);
      rr(c, sx * 3.05 - 0.4, -1.05 - off, 0.8, 0.85, 0.12);
      paint(c, T.accent);
      // the body plates and the knob at each side
      poly(c, [sx * 1.0, 0.0, sx * 2.4, 0.05, sx * 2.5, 1.0, sx * 1.6, 1.4, sx * 1.0, 1.2]);
      paint(c, M.white);
      for (let i = 0; i < 3; i++) {
        rr(c, sx * (1.0 + i * 0.22) - 0.07, 0.55, 0.14, 0.45, 0.04);
        paint(c, M.mid, null);
      }
      circle(c, sx * 2.35, 0.8, 0.3);
      paint(c, M.pale);
    }
    // the back: a white plate, the coils and a red block
    rr(c, -1.35, 1.2, 2.7, 1.4, 0.4);
    paint(c, M.white);
    rr(c, -0.5, 2.4, 1.0, 0.6, 0.1);
    paint(c, T.accent);
    for (const sx of [-1, 1]) {
      circle(c, sx * 0.85, 2.05, 0.38);
      paint(c, M.pale);
      circle(c, sx * 0.85, 2.05, 0.18);
      paint(c, '#2d6f7a', null);
      glow(c, sx * 0.85, 2.05, 0.7, GLOW.vortex, 0.5 * pulse);
    }
    // the coil housing at the middle, its bar ahead
    circle(c, 0, 0.85, 1.15);
    paint(c, M.darker);
    circle(c, 0, 0.85, 0.48);
    paint(c, M.white);
    circle(c, 0, 0.85, 0.33);
    paint(c, M.dark, null);
    rr(c, -0.28, -0.95, 0.56, 1.2, 0.08);
    paint(c, T.accent);
    rr(c, -0.12, -0.75, 0.24, 0.8, 0.05);
    paint(c, M.darker, null);
    // the claws, white with a stripe of the team's colour down the outside,
    // their tips glowing and arcing as it fires
    for (const sx of [-1, 1]) {
      poly(c, [sx * 2.3, -3.5, sx * 1.45, -3.3, sx * 0.8, -2.2, sx * 0.7, -1.2, sx * 1.0, -0.5, sx * 1.9, -0.1, sx * 2.65, 0.0, sx * 2.85, -1.0, sx * 2.85, -2.3]);
      paint(c, M.white);
      poly(c, [sx * 2.15, -3.1, sx * 2.65, -2.3, sx * 2.65, -0.05, sx * 2.05, -0.1, sx * 1.95, -2.4]);
      paint(c, T.accent, null);
      poly(c, [sx * 1.1, -2.3, sx * 1.5, -2.9, sx * 1.6, -1.4, sx * 1.2, -1.0]);
      paint(c, M.pale, null);
      glow(c, sx * 1.85, -3.4, 0.7, GLOW.vortex, 0.6 * pulse);
      flash(c, sx * 1.85, -3.6, 1.4, GLOW.vortex, p.fireAge, 0.2, p.fireIndex + sx);
    }
  }

  // Mustang, as it fires: a fast wheeled car, a red armoured flipper over
  // each front wheel, big rear wheels under white fenders, and on top a long
  // white gun housing with its cockpit, which turns to its target.
  function mustang(c, p) {
    const T = p.team;
    const roll = (p.walk * 2.0) % 0.8;
    // the wheels
    for (const sx of [-1, 1]) {
      for (const [y, r] of [[-1.6, 0.75], [3.3, 0.95]]) {
        circle(c, sx * 1.6, y, r);
        paint(c, M.black);
        line(c, sx * 1.6 - r * 0.8, y - r + roll, sx * 1.6 + r * 0.8, y - r + roll, M.steel, 0.12);
      }
    }
    for (const sx of [-1, 1]) {
      // the track pod along the side
      rr(c, sx * 1.65 - 0.4, -0.6, 0.8, 1.5, 0.15);
      paint(c, M.dark);
      // the rear fender
      poly(c, [sx * 1.55, 1.4, sx * 2.2, 1.6, sx * 2.25, 4.1, sx * 1.95, 4.35, sx * 1.6, 4.1]);
      paint(c, M.white);
      rr(c, sx * 1.25 - 0.15, 2.5, 0.3, 0.9, 0.08);
      paint(c, T.accent);
      // the flipper over the front wheel, a grey blade ahead of it
      poly(c, [sx * 1.6, -4.6, sx * 2.3, -4.2, sx * 2.5, -3.0, sx * 1.65, -3.0]);
      paint(c, M.mid);
      poly(c, [sx * 2.0, -3.4, sx * 2.6, -3.0, sx * 2.8, -1.0, sx * 2.5, -0.4, sx * 2.0, -0.6]);
      paint(c, T.accent);
      rr(c, sx * 1.8 - 0.22, -3.2, 0.44, 3.9, 0.08);
      paint(c, M.darker);
      rr(c, sx * 1.8 - 0.12, -2.8, 0.24, 0.9, 0.04);
      paint(c, M.mid, null);
    }
    // the chassis under the gun
    rr(c, -1.4, -0.4, 2.8, 3.1, 0.4);
    paint(c, M.dark);
    // the gun housing and cockpit, turning about its mount
    c.save();
    c.translate(0, 0.8);
    c.rotate(p.turret);
    c.translate(0, recoil(p.fireAge, 0.4) * 0.5 - 0.8);
    poly(c, [-0.45, -3.15, 0.45, -3.15, 0.7, -1.5, 1.15, 0.0, 1.35, 1.6, 0.6, 2.2, -0.6, 2.2, -1.35, 1.6, -1.15, 0.0, -0.7, -1.5]);
    paint(c, M.white);
    rr(c, -0.2, -3.25, 0.4, 0.3, 0.05);
    paint(c, M.dark);
    poly(c, [-0.35, -1.1, 0.35, -1.1, 0.3, 0.2, -0.3, 0.2]);
    paint(c, M.glass, M.line, 0.6);
    glow(c, 0, -0.45, 0.8, GLOW.mustang, 0.5);
    rr(c, -0.5, 0.35, 1.0, 1.6, 0.15);
    paint(c, M.darker);
    rr(c, -0.25, 0.75, 0.5, 0.6, 0.08);
    paint(c, M.pale, null);
    flash(c, 0, -3.5, 1.2, '#ffcf70', p.fireAge, 0.12, p.fireIndex);
    c.restore();
  }

  // Steel Ball: a great ball it rolls on, between two tall white blades,
  // a pod each side armoured in the team's colour with prongs reaching
  // ahead, and two long stabilisers trailing from a frame at its back.
  function steel_ball(c, p) {
    const T = p.team;
    const roll = (p.walk * 1.5) % 1.2;
    // the stabilisers trailing behind
    for (const sx of [-1, 1]) {
      poly(c, [sx * 1.35, 6.2, sx * 1.65, 6.2, sx * 1.95, 10.2, sx * 1.75, 10.3]);
      paint(c, M.white);
      circle(c, sx * 1.5, 6.1, 0.3);
      paint(c, M.pale);
    }
    rr(c, -2.0, 4.3, 4.0, 1.3, 0.3);
    paint(c, M.mid);
    for (const sx of [-1, 1]) {
      rr(c, sx * 1.5 - 0.35, 5.2, 0.7, 0.8, 0.15);
      paint(c, M.dark);
    }
    // the ball, its ridges rolling as it goes
    rr(c, -1.5, -4.1, 3.0, 8.5, 1.4);
    paint(c, M.dark);
    c.save();
    rr(c, -1.5, -4.1, 3.0, 8.5, 1.4);
    c.clip();
    for (let y = -4.6 + roll; y < 4.6; y += 1.2) line(c, -1.6, y, 1.6, y, M.steel, 0.3);
    c.restore();
    // the back: a white crest over a ring
    circle(c, 0, 4.2, 0.6);
    paint(c, M.pale);
    circle(c, 0, 4.2, 0.35);
    paint(c, M.darker, null);
    poly(c, [0, 2.4, 0.8, 2.95, 0.5, 3.5, -0.5, 3.5, -0.8, 2.95]);
    paint(c, M.white);
    for (const sx of [-1, 1]) {
      // the rear fins
      poly(c, [sx * 1.5, 1.8, sx * 2.1, 1.9, sx * 2.1, 4.4, sx * 1.6, 4.3]);
      paint(c, T.accent);
      poly(c, [sx * 2.8, 2.6, sx * 4.0, 2.6, sx * 3.6, 4.1, sx * 2.8, 4.1]);
      paint(c, T.accent);
      // the blade beside the ball
      poly(c, [sx * 2.0, -4.75, sx * 1.6, -4.6, sx * 1.4, -1.0, sx * 1.8, 2.0, sx * 2.3, 1.8, sx * 2.45, -1.0]);
      paint(c, M.white);
      // the side pod: a white hexagon with its slots, the armour outboard
      poly(c, [sx * 4.0, -2.45, sx * 4.6, -2.0, sx * 5.25, -1.8, sx * 5.25, 1.6, sx * 4.6, 1.85, sx * 4.0, 2.35, sx * 3.6, 2.2, sx * 3.6, -2.3]);
      paint(c, M.white);
      rr(c, sx * 4.9 - 0.32, -1.9, 0.64, 3.6, 0.2);
      paint(c, T.accent);
      poly(c, [sx * 2.4, -1.4, sx * 3.0, -2.1, sx * 3.6, -1.4, sx * 3.6, 1.0, sx * 3.0, 1.7, sx * 2.4, 1.0]);
      paint(c, M.pale);
      for (const y of [-0.35, 0.35]) {
        rr(c, sx * 3.0 - 0.6, y - 0.12, 1.2, 0.24, 0.06);
        paint(c, T.accent, null);
      }
      // the prong ahead of the pod
      poly(c, [sx * 3.25, -5.6, sx * 3.45, -5.6, sx * 3.6, -3.0, sx * 2.8, -3.0, sx * 2.95, -4.6]);
      paint(c, T.deep);
      rr(c, sx * 3.35 - 0.12, -5.7, 0.24, 0.8, 0.05);
      paint(c, M.white, null);
    }
    if (p.strike >= 0 || p.fireAge < 0.3) glow(c, 0, 0, 3.0, GLOW.steel_ball, 0.6);
  }

  // Stormcaller, as it fires every launcher: a tracked rocket carrier, a rack
  // of red launch tubes each side reaching ahead of it, missiles at their
  // mouths, and between them the cab with its glass canopy.
  function stormcaller(c, p) {
    const T = p.team;
    const kick = recoil(p.fireAge, 0.5) * 0.5;
    for (const sx of [-1, 1]) {
      // the rack's white back
      poly(c, [sx * 2.0, 1.0, sx * 4.1, 1.0, sx * 4.1, 3.2, sx * 3.5, 4.3, sx * 2.0, 4.3]);
      paint(c, M.white);
      rr(c, sx * 3.0 - 0.45, 1.6, 0.9, 2.4, 0.15);
      paint(c, M.pale, null);
      c.save();
      c.translate(0, kick);
      // the missiles in the outer tubes
      poly(c, [sx * 3.7, -6.7, sx * 3.95, -6.0, sx * 3.95, -5.0, sx * 3.45, -5.0, sx * 3.45, -6.0]);
      paint(c, M.white);
      // the tubes
      rr(c, sx * 3.95 - 0.38, -5.1, 0.76, 5.7, 0.2);
      paint(c, T.accent);
      rr(c, sx * 2.95 - 0.38, -5.3, 0.76, 2.6, 0.2);
      paint(c, T.accent);
      rr(c, sx * 2.95 - 0.38, -2.7, 0.76, 3.8, 0.2);
      paint(c, T.accent);
      line(c, sx * 3.95, -4.8, sx * 3.95, 0.3, T.deep, 0.18);
      line(c, sx * 2.95, -2.4, sx * 2.95, 0.8, T.deep, 0.18);
      for (const x of [3.95, 2.95]) flash(c, sx * x, -5.6, 1.2, '#ffb347', p.fireAge, 0.18, p.fireIndex + x);
      c.restore();
    }
    // the cab: a grey wedge, its white front under the canopy
    poly(c, [-0.6, -3.3, 0.6, -3.3, 1.9, -1.6, 1.9, 1.2, 1.2, 2.0, -1.2, 2.0, -1.9, 1.2, -1.9, -1.6]);
    paint(c, M.mid);
    for (const sx of [-1, 1]) {
      rr(c, sx * 1.15 - 0.3, -0.3, 0.6, 1.4, 0.1);
      paint(c, M.pale);
      rr(c, sx * 1.1 - 0.45, 1.5, 0.9, 1.0, 0.1);
      paint(c, M.darker);
      glow(c, sx * 1.0, -2.3, 0.5, GLOW.stormcaller, 0.5);
    }
    poly(c, [-0.65, -2.0, 0.65, -2.0, 0.6, 0.6, -0.6, 0.6]);
    paint(c, M.white);
    for (const x of [-0.25, 0.05]) {
      rr(c, x, -0.6, 0.18, 1.0, 0.04);
      paint(c, M.dark, null);
    }
    rr(c, -0.55, 0.9, 1.1, 1.1, 0.1);
    paint(c, M.dark);
    poly(c, [-0.35, -4.2, 0.35, -4.2, 0.55, -2.6, 0.35, -1.85, -0.35, -1.85, -0.55, -2.6]);
    paint(c, M.dark);
    poly(c, [-0.25, -3.95, 0.25, -3.95, 0.42, -2.6, 0.25, -2.0, -0.25, -2.0, -0.42, -2.6]);
    paint(c, M.glass, M.line, 0.6);
  }

  // Hacker, as it attacks: a round body with a glowing core, a pod angled
  // out ahead each side of it, a tail pod behind, and four black cables
  // looping out from it, the two outer ones raised to spiked heads ringed
  // in its glow.
  const HACKER_CABLES = [
    [[-2.5, 2.0], [-4.0, 3.5], [-9.8, 6.0], [-10.6, 5.5], [-10.1, -2.0], [-10.0, -7.8]],
    [[2.5, 1.5], [4.5, 3.0], [9.0, 7.2], [9.8, 6.8], [8.0, -1.0], [8.2, -6.2]],
    [[-1.8, 3.5], [-3.2, 9.5], [-3.8, 10.3], [-4.9, 8.7], [-4.8, -1.0]],
    [[1.9, 3.6], [3.5, 9.6], [4.1, 10.2], [4.8, 8.5], [4.8, -1.0]],
  ];
  function hacker(c, p) {
    const T = p.team;
    const pulse = 0.7 + 0.3 * Math.sin(p.t * 6);
    const sway = Math.sin(p.t * 1.7) * 0.15;
    // the cables
    c.lineJoin = 'round';
    c.lineCap = 'round';
    for (const cable of HACKER_CABLES) {
      c.beginPath();
      cable.forEach(([x, y], i) => {
        const dx = i > 1 ? sway * i : 0;
        if (i) c.lineTo(x + dx, y); else c.moveTo(x, y);
      });
      c.lineWidth = 0.55;
      c.strokeStyle = M.black;
      c.stroke();
    }
    c.lineCap = 'butt';
    // the spiked heads on the raised cables
    for (const [x, y] of [[-10.0 + sway * 5, -8.3], [8.2 + sway * 5, -6.8]]) {
      circle(c, x, y + 0.1, 0.5);
      paint(c, M.white);
      rr(c, x - 0.45, y - 0.85, 0.9, 0.5, 0.1);
      paint(c, GLOW.hacker);
      poly(c, [x - 0.55, y - 0.85, x + 0.55, y - 0.85, x, y - 2.9]);
      paint(c, M.dark);
      glow(c, x, y - 0.6, 0.9, GLOW.hacker, 0.6 * pulse);
    }
    // the tail pod
    poly(c, [-1.5, 3.0, 1.5, 3.0, 1.3, 8.0, -1.3, 8.0]);
    paint(c, M.white);
    poly(c, [-0.9, 4.2, 0.9, 4.2, 0.7, 7.6, -0.7, 7.6]);
    paint(c, T.accent, null);
    rr(c, -1.15, 7.8, 2.3, 1.1, 0.3);
    paint(c, M.dark);
    // the pods angled out ahead, a cup ringed in its glow under each
    for (const sx of [-1, 1]) {
      c.save();
      c.translate(sx * 4.5, -2.8);
      c.rotate(sx * 0.68);
      rr(c, -1.15, -2.9, 2.3, 4.6, 0.9);
      paint(c, M.white);
      rr(c, -0.8, -2.4, 1.6, 3.1, 0.6);
      paint(c, T.accent, null);
      rr(c, -0.9, -3.3, 1.8, 0.7, 0.3);
      paint(c, M.pale);
      c.restore();
      poly(c, [sx * 4.05, -1.6, sx * 5.15, -1.6, sx * 4.95, -0.4, sx * 4.25, -0.4]);
      paint(c, M.white);
      rr(c, sx * 4.6 - 0.55, -1.75, 1.1, 0.22, 0.05);
      paint(c, GLOW.hacker, null);
      rr(c, sx * 4.6 - 0.12, -3.0, 0.24, 1.3, 0.05);
      paint(c, M.white);
    }
    // the body: a white ring, its red rim and dark face round the core
    circle(c, 0, 1.0, 3.1);
    paint(c, M.white);
    for (let i = 0; i < 12; i++) {
      const a = (i * Math.PI) / 6;
      poly(c, [Math.cos(a) * 2.2, 1.0 + Math.sin(a) * 2.2, Math.cos(a + 0.18) * 3.05, 1.0 + Math.sin(a + 0.18) * 3.05, Math.cos(a - 0.18) * 3.05, 1.0 + Math.sin(a - 0.18) * 3.05]);
      paint(c, M.dark, null);
    }
    circle(c, 0, 1.0, 2.1);
    paint(c, T.accent);
    circle(c, 0, 1.0, 1.8);
    paint(c, M.darker);
    circle(c, -2.85, 1.0, 0.55);
    paint(c, M.pale);
    glow(c, 0, 1.0, 2.0, GLOW.hacker, pulse);
    circle(c, 0, 1.0, 0.95);
    paint(c, '#1b6f8a');
    circle(c, 0, 1.0, 0.6);
    paint(c, GLOW.hacker, null);
    flash(c, 0, 1.0, 2.4, GLOW.hacker, p.fireAge, 0.2, p.fireIndex);
  }

  // Fire Badger, as it attacks: a heavy carrier on four white track pods,
  // an orange radiator drum across its back, and on a red mount at its
  // middle a long flamethrower, its fuel glowing violet through the glass,
  // reaching out past its front, which turns to its target.
  function fire_badger(c, p) {
    const T = p.team;
    const roll = (p.walk * 1.5) % 1.0;
    for (const sx of [-1, 1]) {
      // the front pod: armour, teeth ahead, rollers outboard
      for (const y of [-4.0, -2.9]) {
        rr(c, sx * 6.65 - 0.45, y - 0.45, 0.9, 0.9, 0.3);
        paint(c, M.steel);
      }
      poly(c, [sx * 2.5, -8.6, sx * 6.0, -8.6, sx * 6.3, -6.0, sx * 6.2, -2.2, sx * 2.6, -2.2]);
      paint(c, M.white);
      rr(c, sx * 4.4 - 1.0, -6.5, 2.0, 2.4, 0.2);
      paint(c, M.pale, null);
      for (const x of [3.9, 5.4]) {
        rr(c, sx * x - 0.45, -10.4 + roll * 0.2, 0.9, 1.8, 0.15);
        paint(c, M.white);
        rr(c, sx * x - 0.25, -8.6, 0.5, 0.6, 0.08);
        paint(c, T.accent);
      }
      // the back pod
      poly(c, [sx * 2.5, 0.5, sx * 5.4, 0.5, sx * 5.9, 1.5, sx * 5.9, 6.0, sx * 2.5, 6.0]);
      paint(c, M.white);
      rr(c, sx * 3.85 - 0.7, 2.0, 1.4, 1.5, 0.15);
      paint(c, M.darker);
      for (const x of [1.4, 4.6]) {
        rr(c, sx * x - 0.45, 7.6, 0.9, 1.2, 0.15);
        paint(c, M.dark);
      }
    }
    // the radiator drum across the back
    rr(c, -4.6, 4.6, 9.2, 3.1, 0.5);
    paint(c, T.deep);
    rr(c, -4.2, 4.8, 8.4, 2.6, 0.4);
    paint(c, '#ff7a3c');
    for (let y = 5.0; y < 7.3; y += 0.35) line(c, -4.0, y, 4.0, y, 'rgba(120,30,0,0.45)', 0.08);
    for (const sx of [-1, 1]) {
      rr(c, sx * 1.45 - 0.45, 5.3, 0.9, 1.9, 0.12);
      paint(c, M.dark);
    }
    // the hoses from the mount to the drum
    for (const sx of [-1, 1]) {
      c.beginPath();
      c.moveTo(sx * 0.8, 2.0);
      c.lineTo(sx * 2.0, 3.6);
      c.lineTo(sx * 4.6, 3.8);
      c.lineTo(sx * 5.0, 5.0);
      c.lineWidth = 0.4;
      c.strokeStyle = M.black;
      c.stroke();
    }
    // the mount and the flamethrower on it, turning about the mount
    c.save();
    c.rotate(p.turret);
    poly(c, [-3.4, -1.0, -2.0, -2.3, 2.0, -2.3, 3.4, -1.0, 3.0, 1.6, -3.0, 1.6]);
    paint(c, T.accent);
    poly(c, [-1.2, -1.6, 1.2, -1.6, 1.0, 1.2, -1.0, 1.2]);
    paint(c, T.deep, null);
    for (const sx of [-1, 1]) {
      rr(c, sx * 0.65 - 0.35, -2.6, 0.7, 1.4, 0.2);
      paint(c, '#b77ce8');
    }
    rr(c, -1.3, -3.7, 2.6, 0.55, 0.15);
    paint(c, M.caution);
    rr(c, -1.3, -6.4, 2.6, 2.7, 0.3);
    paint(c, M.mid);
    rr(c, -1.3, -5.3, 2.6, 0.6, 0.1);
    paint(c, M.darker, null);
    rr(c, -0.85, -8.4, 1.7, 2.1, 0.3);
    paint(c, '#d98cf5');
    for (const x of [-0.45, 0, 0.45]) line(c, x, -8.2, x, -6.6, '#f4d6ff', 0.1);
    poly(c, [-1.3, -8.4, 1.3, -8.4, 1.15, -9.4, 0.6, -10.0, -0.6, -10.0, -1.15, -9.4]);
    paint(c, M.pale);
    for (const sx of [-1, 1]) {
      poly(c, [sx * 1.2, -8.0, sx * 2.0, -9.5, sx * 1.7, -10.1, sx * 1.15, -9.2]);
      paint(c, M.dark);
    }
    glow(c, 0, -2.0, 0.8, GLOW.fire_badger, 0.5);
    flash(c, 0, -10.6, 2.4, '#ff8a3c', p.fireAge, 0.3, p.fireIndex);
    c.restore();
  }

  // Sabertooth: a great tank on four white track pods, the front two
  // topped in the team's colour, a pointed white hull between them, and on
  // its turret two long cannons in the team's colour, which turn to their
  // target and kick back in turn as they fire.
  function sabertooth(c, p) {
    const T = p.team;
    for (const sx of [-1, 1]) {
      // the back pod
      rr(c, sx * 5.45 - 1.85, 4.0, 3.7, 8.6, 0.6);
      paint(c, M.white);
      rr(c, sx * 5.5 - 1.3, 5.5, 2.6, 4.0, 0.3);
      paint(c, M.dark);
      for (const y of [10.4, 11.4]) line(c, sx * 4.0, y, sx * 7.0, y, M.mid, 0.2);
      // the side wing and its drum
      poly(c, [sx * 3.0, -2.8, sx * 8.6, -2.8, sx * 9.6, -0.6, sx * 9.6, 3.2, sx * 8.6, 4.2, sx * 3.0, 4.2]);
      paint(c, M.white);
      for (let i = 0; i < 4; i++) line(c, sx * (4.0 + i * 0.9), -2.4, sx * (4.0 + i * 0.9), 3.8, M.pale, 0.25);
      circle(c, sx * 8.4, 1.3, 1.1);
      paint(c, T.accent);
      circle(c, sx * 8.4, 1.3, 0.5);
      paint(c, T.deep, null);
      // the front pod
      rr(c, sx * 6.0 - 2.0, -12.6, 4.0, 8.4, 0.6);
      paint(c, M.white);
      rr(c, sx * 5.95 - 1.35, -12.4, 2.7, 4.2, 0.3);
      paint(c, T.accent);
      rr(c, sx * 5.95 - 1.1, -7.6, 2.2, 1.4, 0.2);
      paint(c, M.dark);
      rr(c, sx * 5.95 - 1.2, -5.8, 2.4, 1.2, 0.2);
      paint(c, M.pale, null);
      // a stripe of the team's colour along the back of the hull
      rr(c, sx * 2.8 - 0.4, 3.6, 0.8, 6.0, 0.15);
      paint(c, T.accent);
    }
    // the hull
    poly(c, [0, -11.3, 2.2, -9.5, 2.4, 9.8, -2.4, 9.8, -2.2, -9.5]);
    paint(c, M.white);
    rr(c, -1.3, 2.6, 2.6, 6.9, 0.3);
    paint(c, M.dark);
    rr(c, -0.6, 3.6, 1.2, 2.0, 0.15);
    paint(c, M.mid, null);
    circle(c, -2.8, 7.8, 0.7);
    paint(c, M.white);
    for (const sx of [-1, 1]) glow(c, sx * 1.6, 4.6, 0.5, GLOW.sabertooth, 0.5);
    // the turret, turning about its middle
    c.save();
    c.translate(0, -1.0);
    c.rotate(p.turret);
    c.translate(0, 1.0);
    rr(c, -2.1, -3.6, 4.2, 5.2, 0.5);
    paint(c, M.mid);
    for (const sx of [-1, 1]) {
      const kick = recoil(p.fireAge, 0.6) * (p.fireIndex % 2 === (sx > 0 ? 1 : 0) ? 1.2 : 0.2);
      c.save();
      c.translate(0, kick);
      rr(c, sx * 2.75 - 0.75, -6.6, 1.5, 9.1, 0.3);
      paint(c, T.accent);
      line(c, sx * 2.75, -6.2, sx * 2.75, 2.0, T.deep, 0.25);
      rr(c, sx * 2.75 - 0.35, -8.3, 0.7, 1.8, 0.1);
      paint(c, '#a8e4ee');
      rr(c, sx * 2.75 - 0.6, -13.2, 1.2, 5.0, 0.2);
      paint(c, T.accent);
      for (let y = -12.6; y < -9.4; y += 0.45) line(c, sx * 2.55, y, sx * 2.95, y, M.darker, 0.15);
      flash(c, sx * 2.75, -13.6, 2.2, '#ffb347', p.fireAge, 0.16, p.fireIndex + sx);
      c.restore();
    }
    poly(c, [0, -7.0, 1.6, -6.1, 1.6, -4.3, 0, -3.4, -1.6, -4.3, -1.6, -6.1]);
    paint(c, M.dark);
    circle(c, 0, -5.3, 0.65);
    paint(c, T.accent);
    c.restore();
  }

  // Tarantula, as it fires: a four-legged walker, a white leg armoured in
  // the team's colour out to each corner, carrying a turret that turns to
  // its target: a white octagonal deck round a spine in the team's colour,
  // two long thin cannons ahead and a pair of launch blocks behind.
  function tarantula(c, p) {
    const T = p.team;
    const stride = Math.min(1, p.speed / 4) * 0.5;
    const ph = p.walk * Math.PI * 2 / 8;
    // the legs, out to the corners
    for (const [sx, sy] of [[-1, -1], [1, -1], [-1, 1], [1, 1]]) {
      const off = Math.sin(ph + (sx * sy > 0 ? 0 : Math.PI)) * stride;
      c.save();
      c.translate(sx * 5.4, sy * (sy < 0 ? 5.2 : 5.4) + off);
      c.rotate(Math.atan2(sx, -sy));
      rr(c, -1.6, -6.0, 3.2, 5.8, 1.3);
      paint(c, M.white);
      rr(c, -1.3, -4.6, 2.6, 2.9, 0.4);
      paint(c, T.accent);
      circle(c, 0, -0.2, 0.9);
      paint(c, M.dark);
      c.restore();
    }
    // the turret
    c.save();
    c.rotate(p.turret);
    const kick = recoil(p.fireAge, 0.5);
    // the launch blocks behind
    for (const sx of [-1, 1]) {
      rr(c, sx * 4.85 - 2.65, 5.4, 5.3, 8.1, 0.6);
      paint(c, T.accent);
      for (const y of [7.6, 9.8, 12.0]) line(c, sx * 2.4, y, sx * 7.3, y, T.deep, 0.3);
      rr(c, sx * 3.05 - 0.6, 5.6, 1.2, 7.6, 0.2);
      paint(c, M.white, null);
    }
    poly(c, [-1.6, 1.0, 1.6, 1.0, 1.3, 10.0, -1.3, 10.0]);
    paint(c, M.white);
    for (let i = 0; i < 6; i++) line(c, -0.8 + i * 0.32, 9.2, -0.8 + i * 0.32, 9.9, M.mid, 0.12);
    // the deck
    poly(c, [-2.0, -5.6, 2.0, -5.6, 6.6, -2.0, 6.6, 1.8, 2.4, 4.4, -2.4, 4.4, -6.6, 1.8, -6.6, -2.0]);
    paint(c, M.white);
    for (const sx of [-1, 1]) {
      poly(c, [sx * 2.6, -4.2, sx * 5.8, -1.6, sx * 3.0, 2.6, sx * 2.0, -1.0]);
      paint(c, M.pale, null);
      for (const y of [-1.6, 1.4]) {
        circle(c, sx * 5.3, y, 0.4);
        paint(c, M.white, M.line, 0.8);
      }
      rr(c, sx * 6.0 - 0.45, -0.9, 0.9, 2.0, 0.1);
      paint(c, M.mid, null);
      rr(c, sx * 2.6 - 0.9, 1.3, 1.8, 0.7, 0.1);
      paint(c, M.dark, null);
    }
    // the cannons, kicked back by their shot
    for (const sx of [-1, 1]) {
      c.save();
      c.translate(0, kick * 1.0);
      rr(c, sx * 2.55 - 0.42, -13.4, 0.84, 7.6, 0.15);
      paint(c, T.accent);
      rr(c, sx * 2.55 - 0.3, -14.3, 0.6, 1.0, 0.1);
      paint(c, T.deep);
      flash(c, sx * 2.55, -14.6, 1.6, '#ffcf70', p.fireAge, 0.14, p.fireIndex + sx);
      c.restore();
    }
    // the spine and its cap
    rr(c, -1.3, -8.8, 2.6, 3.2, 0.5);
    paint(c, M.mid);
    rr(c, -1.45, -5.7, 2.9, 5.7, 0.9);
    paint(c, T.accent);
    for (const sx of [-1, 1]) {
      circle(c, sx * 1.85, -1.1, 0.32);
      paint(c, GLOW.tarantula, null);
      glow(c, sx * 1.85, -1.1, 0.7, GLOW.tarantula, 0.6);
    }
    c.restore();
  }

  // Phantom Ray, as it fires: a flyer with a rounded white body, a wing
  // each side carrying a rack of red missiles, two engine pods in the
  // team's colour behind, and a long boom out to a tail with red fins. Its
  // missiles leave the racks as it fires.
  function phantom_ray(c, p) {
    const T = p.team;
    const bob = Math.sin(p.t * 2.0 + p.walk) * 0.03;
    c.scale(1 + bob, 1 + bob);
    const fired = p.fireAge < 0.4 ? 1 - p.fireAge / 0.4 : 0;
    // the tail
    for (const x of [-0.75, 0.35]) {
      rr(c, x - 0.3, 3.2, 0.6, 4.0, 0.15);
      paint(c, M.white);
    }
    poly(c, [-3.3, 7.6, -1.2, 6.9, 1.2, 6.9, 2.3, 7.6, 1.8, 8.4, -2.8, 8.4]);
    paint(c, M.white);
    for (const x of [-3.5, 2.2]) {
      poly(c, [x, 7.4, x + 0.8, 7.3, x + 1.1, 9.4, x + 0.5, 9.6]);
      paint(c, T.accent);
    }
    // the engine pods
    for (const sx of [-1, 1]) {
      rr(c, sx * 3.4 - 1.2, 0.8, 2.4, 4.0, 0.9);
      paint(c, T.accent);
      rr(c, sx * 3.4 - 0.75, 0.9, 1.5, 0.9, 0.35);
      paint(c, M.darker);
      rr(c, sx * 3.4 - 0.25, 2.5, 0.5, 1.4, 0.15);
      paint(c, M.dark, null);
      rr(c, sx * 3.4 - 0.4, 4.7, 0.8, 0.6, 0.15);
      paint(c, M.black);
    }
    // the wings and their racks
    for (const sx of [-1, 1]) {
      poly(c, [sx * 2.0, -2.3, sx * 5.0, -2.3, sx * 4.4, -1.2, sx * 2.0, -1.0]);
      paint(c, M.white);
      rr(c, sx * 4.65 - 1.85, -4.45, 3.7, 1.8, 0.15);
      paint(c, M.dark);
      rr(c, sx * 4.65 - 1.85, -5.0, 3.7, 0.65, 0.15);
      paint(c, T.accent);
      for (const x of [3.5, 4.4, 5.3]) {
        if (fired && Math.round(x * 2) % 3 === p.fireIndex % 3) continue;
        rr(c, sx * x - 0.25, -4.3, 0.5, 2.0, 0.2);
        paint(c, T.accent);
        circle(c, sx * x, -4.4, 0.25);
        paint(c, M.pale);
      }
      rr(c, sx * 6.9 - 0.45, -5.6, 0.9, 3.5, 0.35);
      paint(c, sx < 0 ? M.white : M.dark);
    }
    // the body and the guns ahead of it
    for (const sx of [-1, 1]) {
      line(c, sx * 2.2, -5.0, sx * 2.2, -7.4, M.mid, 0.12);
      line(c, sx * 1.3, -5.0, sx * 1.3, -6.9, M.mid, 0.12);
      rr(c, sx * 1.65 - 0.45, -5.9, 0.9, 1.2, 0.2);
      paint(c, M.dark);
    }
    rr(c, -1.1, -0.5, 2.2, 3.7, 0.4);
    paint(c, M.mid);
    poly(c, [0, -5.1, 1.2, -4.8, 2.1, -3.0, 2.1, -1.0, 1.4, 0.6, -1.4, 0.6, -2.1, -1.0, -2.1, -3.0, -1.2, -4.8]);
    paint(c, M.white);
    poly(c, [-0.8, -4.95, 0.8, -4.95, 0.5, -4.6, -0.5, -4.6]);
    paint(c, GLOW.phantom_ray, null);
    poly(c, [0, -2.2, 0.75, -1.5, 0.5, -0.4, -0.5, -0.4, -0.75, -1.5]);
    paint(c, T.accent);
    rr(c, -1.3, 2.4, 2.6, 0.9, 0.2);
    paint(c, M.white);
    flash(c, 0, -5.6, 1.4, '#ffcf70', p.fireAge, 0.15, p.fireIndex);
  }

  // Wraith: a flying ring. Its outer ring, four heavy white plates on a dark
  // band with red blades along its rim, turns about a disc crossed by four
  // beams, ringed with yellow armour and holding a toothed disc in the
  // team's colour round its glowing hub.
  function arcBand(c, r, width, a0, a1, colour) {
    c.beginPath();
    c.arc(0, 0, r, a0, a1);
    c.lineWidth = width;
    c.strokeStyle = colour;
    c.stroke();
  }
  function wraith(c, p) {
    const T = p.team;
    const pulse = 0.7 + 0.3 * Math.sin(p.t * 4);
    // the disc
    circle(c, 0, 0, 9.4);
    paint(c, M.dark);
    arcBand(c, 8.6, 0.25, 0, Math.PI * 2, M.white);
    for (let k = 0; k < 4; k++) {
      c.save();
      c.rotate((k * Math.PI) / 2);
      rr(c, -0.5, -8.8, 1.0, 6.8, 0.1);
      paint(c, M.darker);
      rr(c, -1.0, -8.8, 2.0, 0.6, 0.1);
      paint(c, M.white);
      // the yellow armour along each quarter
      for (const a of [-0.17, 0, 0.17, 0.6, 0.8]) {
        const x = Math.sin(a) * 6.5, y = -Math.cos(a) * 6.5;
        poly(c, [x - 0.65, y, x - 0.33, y - 0.56, x + 0.33, y - 0.56, x + 0.65, y, x + 0.33, y + 0.56, x - 0.33, y + 0.56]);
        paint(c, '#e8b53c');
      }
      c.restore();
    }
    // the toothed disc and the hub
    circle(c, 0, 0, 4.0);
    paint(c, T.accent);
    for (let i = 0; i < 16; i++) {
      const a = (i * Math.PI) / 8;
      poly(c, [Math.cos(a - 0.1) * 2.7, Math.sin(a - 0.1) * 2.7, Math.cos(a) * 3.5, Math.sin(a) * 3.5, Math.cos(a + 0.1) * 2.7, Math.sin(a + 0.1) * 2.7]);
      paint(c, T.deep, null);
    }
    circle(c, 0, 0, 2.1);
    paint(c, M.darker);
    glow(c, 0, 0, 2.2, GLOW.wraith, 0.5 * pulse);
    circle(c, 0, 0, 1.0);
    paint(c, M.steel);
    // the outer ring, turning
    c.save();
    c.rotate(p.t * 0.5);
    arcBand(c, 10.2, 1.6, 0, Math.PI * 2, M.darker);
    for (let k = 0; k < 4; k++) {
      const a = (k * Math.PI) / 2 - 1.75;
      arcBand(c, 11.0, 2.8, a, a + 0.95, M.white);
      arcBand(c, 11.0, 0.35, a + 0.15, a + 0.8, M.pale);
      arcBand(c, 12.9, 0.45, a - 0.2, a + 0.75, T.accent);
      for (let i = 0; i < 5; i++) {
        const b = a - 0.15 + i * 0.2;
        poly(c, [Math.cos(b) * 12.6, Math.sin(b) * 12.6, Math.cos(b + 0.08) * 13.4, Math.sin(b + 0.08) * 13.4, Math.cos(b + 0.12) * 12.6, Math.sin(b + 0.12) * 12.6]);
        paint(c, T.accent, null);
      }
      const m = a + 1.25;
      c.save();
      c.rotate(m + Math.PI / 2);
      rr(c, -0.7, -10.6, 1.4, 1.0, 0.15);
      paint(c, M.pale);
      c.restore();
    }
    c.restore();
    flash(c, 0, 0, 3.0, GLOW.wraith, p.fireAge, 0.2, p.fireIndex);
  }

  // Scorpion, as it fires: a six-legged walker, each leg out to a white
  // foot armoured in the team's colour, its white body carrying a long
  // rail launcher in the team's colour that turns to its target and kicks
  // back as it fires.
  const SCORPION_LEGS = [[-3.0, -3.5, -10.2, -10.0], [3.0, -3.5, 9.6, -9.0], [-4.5, 0.8, -12.0, 0.8], [4.5, 0.8, 12.6, 0.9], [-3.0, 5.0, -9.6, 9.6], [3.0, 5.0, 10.0, 8.8]];
  function scorpion(c, p) {
    const T = p.team;
    const stride = Math.min(1, p.speed / 4) * 0.6;
    const ph = p.walk * Math.PI * 2 / 8;
    SCORPION_LEGS.forEach(([hx, hy, fx, fy], i) => {
      const off = Math.sin(ph + (i % 2 ? Math.PI : 0)) * stride;
      const [ex, ey] = [fx, fy + off];
      c.save();
      c.translate(ex, ey);
      c.rotate(Math.atan2(ex - hx, -(ey - hy)));
      const len = Math.hypot(ex - hx, ey - hy);
      rr(c, -1.0, 0.5, 2.0, len - 1.0, 0.4);
      paint(c, M.dark);
      rr(c, -1.6, -2.3, 3.2, 4.2, 0.8);
      paint(c, M.white);
      rr(c, -1.6, -2.3, 0.7, 4.2, 0.3);
      paint(c, T.accent, null);
      rr(c, -0.6, -1.6, 1.2, 1.5, 0.15);
      paint(c, M.darker, null);
      line(c, -0.2, -1.45, -0.2, -0.3, '#9fd8e6', 0.15);
      line(c, 0.2, -1.45, 0.2, -0.3, '#9fd8e6', 0.15);
      c.restore();
    });
    // the body
    poly(c, [-2.9, -6.8, 2.9, -6.8, 5.6, -1.7, 5.6, 1.6, 3.2, 4.4, 3.0, 9.0, -3.0, 9.0, -3.2, 4.4, -5.6, 1.6, -5.6, -1.7]);
    paint(c, M.white);
    for (const sx of [-1, 1]) {
      poly(c, [sx * 3.2, -5.6, sx * 5.0, -1.8, sx * 3.6, -0.4]);
      paint(c, M.darker, null);
    }
    circle(c, -3.8, -0.6, 0.6);
    paint(c, M.pale);
    // the launcher, turning about the body's middle
    c.save();
    c.rotate(p.turret);
    c.translate(0, recoil(p.fireAge, 0.8) * 1.2);
    for (const sx of [-1, 1]) {
      poly(c, [sx * 1.0, 0.5, sx * 2.6, 1.0, sx * 3.6, 7.8, sx * 2.6, 8.6, sx * 1.0, 8.0]);
      paint(c, T.accent);
      line(c, sx * 2.0, -1.8, sx * 2.0, 0.6, '#9fd8e6', 0.3);
      for (const y of [8.6, 9.6]) {
        circle(c, sx * 1.4, y, 0.55);
        paint(c, T.accent);
      }
    }
    rr(c, -0.95, -6.0, 1.9, 17.0, 0.9);
    paint(c, M.darker);
    line(c, 0, -5.4, 0, 10.4, M.steel, 0.25);
    rr(c, -1.25, -6.3, 2.5, 5.3, 0.4);
    paint(c, T.accent);
    rr(c, -0.7, -7.0, 1.4, 0.9, 0.3);
    paint(c, M.dark);
    glow(c, 0, -1.6, 0.8, GLOW.scorpion, 0.5);
    flash(c, 0, -7.4, 2.6, '#ffb347', p.fireAge, 0.2, p.fireIndex);
    c.restore();
  }

  // Typhoon, as it fires: four legs out to its corners on heavy feet, and
  // on them a turret that turns to its target: a white body with a dome at
  // its front, a gatling each side of the dome spinning as it fires, a
  // white shield hung outboard each side and a missile slung on its right.
  function typhoon(c, p) {
    const T = p.team;
    const stride = Math.min(1, p.speed / 4) * 0.5;
    const ph = p.walk * Math.PI * 2 / 8;
    for (const [sx, sy] of [[-1, -1], [1, -1], [-1, 1], [1, 1]]) {
      const off = Math.sin(ph + (sx * sy > 0 ? 0 : Math.PI)) * stride;
      c.save();
      c.translate(sx * 7.6, sy * 7.6 + off);
      c.rotate(Math.atan2(sx, -sy));
      rr(c, -1.1, 1.0, 2.2, 6.0, 0.5);
      paint(c, M.dark);
      rr(c, -2.0, -2.6, 4.0, 4.6, 1.0);
      paint(c, M.white);
      rr(c, -2.0, -2.6, 0.6, 4.6, 0.3);
      paint(c, T.accent, null);
      rr(c, -0.75, -0.6, 1.5, 1.5, 0.2);
      paint(c, T.accent);
      rr(c, -1.3, -2.4, 2.6, 1.3, 0.3);
      paint(c, M.darker, null);
      c.restore();
    }
    c.save();
    c.rotate(p.turret);
    const spin = p.fireAge < 0.6 ? p.t * 40 : 0;
    // the gatlings
    for (const sx of [-1, 1]) {
      rr(c, sx * 4.3 - 0.9, -3.6, 1.8, 2.2, 0.2);
      paint(c, M.darker);
      rr(c, sx * 4.3 - 0.75, -8.6, 1.5, 5.2, 0.3);
      paint(c, M.dark);
      for (let i = 0; i < 3; i++) {
        const x = sx * 4.3 + Math.sin(spin + (i * 2 * Math.PI) / 3) * 0.45;
        line(c, x, -8.4, x, -3.6, M.steel, 0.16);
      }
      rr(c, sx * 4.3 - 0.75, -9.6, 1.5, 1.2, 0.2);
      paint(c, M.white);
      flash(c, sx * 4.3, -10.0, 1.6, '#ffcf70', p.fireAge, 0.1, p.fireIndex + sx * Math.floor(p.t * 12));
    }
    // the shields
    for (const sx of [-1, 1]) {
      rr(c, sx * 6.6 - 1.7, -1.1, 3.4, 4.3, 0.5);
      paint(c, M.white);
      if (sx < 0) {
        rr(c, -8.3, -1.0, 0.6, 4.1, 0.2);
        paint(c, T.accent);
      }
      line(c, sx * 6.6, -0.6, sx * 6.6, 2.8, M.pale, 0.2);
    }
    poly(c, [6.6, -2.8, 7.0, -2.0, 7.05, 3.0, 6.25, 3.0, 6.3, -2.0]);
    paint(c, M.white);
    // the body
    rr(c, -5.2, -2.0, 10.4, 5.6, 0.8);
    paint(c, M.white);
    for (const sx of [-1, 1]) {
      rr(c, sx * 2.4 - 0.6, -3.6, 1.2, 2.0, 0.2);
      paint(c, T.accent);
      poly(c, [sx * 0.9, 0.0, sx * 1.6, 0.0, sx * 1.6, 2.2, sx * 1.1, 2.4]);
      paint(c, '#f2d544', null);
      line(c, sx * 1.4, 2.8, sx * 1.4, 5.6, M.mid, 0.12);
    }
    rr(c, -1.0, -0.4, 2.0, 3.6, 0.2);
    paint(c, M.dark, null);
    circle(c, 0, -3.1, 1.25);
    paint(c, '#b98a62');
    circle(c, 0, -3.1, 0.6);
    paint(c, M.white, null);
    glow(c, 0, -3.1, 0.6, GLOW.typhoon, 0.4);
    rr(c, -2.6, 3.2, 5.2, 0.4, 0.2);
    paint(c, M.pale, null);
    c.restore();
  }

  // Farseer: an eight-wheeled rover, a red bumper and wheel guards at its
  // front, a white hull carrying a small turret and a red sensor ball ahead,
  // a cable reel on its right, and at its back, between two red blocks, a
  // faceted dome whose facets turn.
  const FARSEER_FACETS = ['#c7a6f0', '#f2d544', '#3f5fa8', '#e8ecf1', '#9fd6f2', '#d9d0b0'];
  function farseer(c, p) {
    const T = p.team;
    const roll = (p.walk * 1.5) % 1.1;
    // the wheels
    for (const sx of [-1, 1]) {
      for (const y of [-9.0, -3.3, 2.6, 8.3]) {
        rr(c, sx * 6.5 - 1.0, y - 2.2, 2.0, 4.4, 0.8);
        paint(c, '#6b5a48');
        c.save();
        rr(c, sx * 6.5 - 1.0, y - 2.2, 2.0, 4.4, 0.8);
        c.clip();
        for (let k = y - 2.6 + roll; k < y + 2.2; k += 1.1) line(c, sx * 5.5, k, sx * 7.5, k, '#4a3d31', 0.25);
        c.restore();
      }
    }
    // the bumper and the guards at the front
    rr(c, -7.9, -10.6, 15.8, 1.2, 0.2);
    paint(c, T.accent);
    for (const sx of [-1, 1]) {
      rr(c, sx * 7.45 - 0.45, -12.3, 0.9, 5.7, 0.2);
      paint(c, T.accent);
      circle(c, sx * 7.2, -8.6, 0.25);
      paint(c, M.pale);
    }
    // the hull
    poly(c, [-3.4, -11.3, 3.4, -11.3, 5.4, -7.0, 5.4, 2.6, 3.0, 3.6, 3.0, 10.2, -3.0, 10.2, -3.0, 3.6, -5.4, 2.6, -5.4, -7.0]);
    paint(c, M.white);
    rr(c, -1.2, -10.2, 2.4, 0.6, 0.1);
    paint(c, '#6b4a2a', null);
    for (const sx of [-1, 1]) {
      rr(c, sx * 2.6 - 0.9, -9.0, 1.8, 1.0, 0.15);
      paint(c, M.darker, null);
      rr(c, sx * 2.0 - 0.6, -1.9, 1.2, 0.7, 0.1);
      paint(c, T.accent);
    }
    poly(c, [-1.6, -2.4, 1.6, -2.4, 1.2, 2.4, -1.2, 2.4]);
    paint(c, M.dark, null);
    poly(c, [-2.2, -6.3, 2.2, -6.3, 2.0, -2.8, -2.0, -2.8]);
    paint(c, T.accent);
    // the reel on the right
    circle(c, 5.6, -0.6, 1.2);
    paint(c, M.dark);
    rr(c, 4.5, -0.85, 2.2, 0.5, 0.1);
    paint(c, T.accent, null);
    circle(c, 5.6, -0.6, 0.4);
    paint(c, M.pale);
    // the sensor ball and the turret ahead
    circle(c, -3.6, -4.3, 1.4);
    paint(c, T.accent);
    rr(c, -3.9, -7.6, 0.6, 2.6, 0.1);
    paint(c, M.pale);
    c.save();
    c.translate(0, -7.4);
    c.rotate(p.turret);
    rr(c, -0.2, -2.4, 0.4, 1.6, 0.1);
    paint(c, M.mid);
    poly(c, [0, -1.5, 1.3, -0.75, 1.3, 0.75, 0, 1.5, -1.3, 0.75, -1.3, -0.75]);
    paint(c, T.accent);
    flash(c, 0, -2.6, 1.0, '#ffcf70', p.fireAge, 0.1, p.fireIndex);
    c.restore();
    // the back: red blocks, the dome between them
    for (const sx of [-1, 1]) {
      poly(c, [sx * 2.5, 3.8, sx * 5.3, 4.2, sx * 5.3, 9.3, sx * 2.5, 9.3]);
      paint(c, T.accent);
      poly(c, [sx * 3.0, 5.0, sx * 4.8, 7.0, sx * 3.0, 9.0]);
      paint(c, T.deep, null);
    }
    rr(c, -2.7, 9.5, 5.4, 1.0, 0.2);
    paint(c, M.white);
    circle(c, 0, 6.8, 2.05);
    paint(c, M.white);
    const turn = p.t * 0.6;
    for (let i = 0; i < 6; i++) {
      const a = turn + (i * Math.PI) / 3;
      poly(c, [0, 6.8, Math.cos(a) * 1.9, 6.8 + Math.sin(a) * 1.9, Math.cos(a + 0.7) * 1.9, 6.8 + Math.sin(a + 0.7) * 1.9]);
      paint(c, FARSEER_FACETS[i], null);
    }
    glow(c, 0, 6.8, 1.6, GLOW.farseer, 0.35);
  }

  // Centurion, as it fires: a great biped, its right arm one long gun in
  // the team's colour, its left reaching out to a white fist, a heavy
  // shield in the team's colour on its back and a lance slung behind it.
  // Its gun's vents glow, and kick back as it fires.
  function centurion(c, p) {
    const T = p.team;
    const kick = recoil(p.fireAge, 0.6) * 0.8;
    // the lance
    c.save();
    c.translate(2.5, 13.8);
    c.rotate(0.42);
    rr(c, -0.55, -5.4, 1.1, 10.4, 0.55);
    paint(c, '#e8b48c');
    rr(c, -0.7, -1.2, 1.4, 1.2, 0.3);
    paint(c, M.dark);
    c.restore();
    // the shield on its back
    c.save();
    c.translate(-3.6, 5.6);
    c.rotate(-0.45);
    rr(c, -4.2, -4.8, 8.4, 9.6, 1.6);
    paint(c, T.accent);
    rr(c, -2.4, -3.0, 2.0, 2.4, 0.3);
    paint(c, M.dark);
    rr(c, 0.4, -1.0, 2.0, 2.6, 0.3);
    paint(c, M.white);
    line(c, -3.0, 2.6, 3.0, 2.6, T.deep, 0.3);
    c.restore();
    // the legs and the torso
    for (const sx of [-1, 1]) {
      rr(c, 1.0 + sx * 2.2 - 1.3, 0.5, 2.6, 6.0, 0.5);
      paint(c, M.white);
    }
    rr(c, -3.4, -3.8, 8.2, 8.0, 1.4);
    paint(c, M.pale);
    rr(c, -1.6, -2.4, 4.4, 4.2, 0.6);
    paint(c, M.dark, null);
    // the right shoulder
    poly(c, [5.0, -1.8, 8.0, -1.0, 9.6, 2.4, 9.2, 7.8, 6.4, 9.0, 4.6, 6.0]);
    paint(c, T.accent);
    poly(c, [6.0, 0.0, 7.6, 0.6, 8.1, 3.6, 6.6, 4.4]);
    paint(c, M.white, null);
    // the left arm out to its fist
    c.save();
    c.translate(-2.2, -2.8);
    c.rotate(-0.9);
    rr(c, -1.0, -7.2, 2.0, 7.2, 0.5);
    paint(c, M.dark);
    rr(c, -1.3, -5.6, 2.6, 1.0, 0.3);
    paint(c, T.accent);
    rr(c, -1.3, -2.8, 2.6, 1.0, 0.3);
    paint(c, T.accent);
    rr(c, -1.7, -9.6, 3.4, 2.8, 0.6);
    paint(c, M.white);
    rr(c, -1.9, -9.9, 3.8, 0.9, 0.3);
    paint(c, T.accent);
    c.restore();
    // the head
    rr(c, -0.4, -4.6, 2.0, 1.8, 0.5);
    paint(c, T.accent);
    rr(c, 0.1, -4.3, 1.0, 0.5, 0.1);
    paint(c, M.darker, null);
    // the gun arm, kicked back as it fires
    c.save();
    c.translate(0, kick);
    rr(c, 3.6, -11.5, 2.8, 12.0, 0.5);
    paint(c, T.accent);
    rr(c, 4.2, -9.6, 1.6, 3.4, 0.2);
    paint(c, M.darker, null);
    rr(c, 4.3, -13.4, 1.4, 2.0, 0.3);
    paint(c, M.white);
    rr(c, 3.5, -0.4, 3.0, 0.8, 0.1);
    paint(c, M.darker);
    for (let i = 0; i < 6; i++) {
      rr(c, 3.7 + i * 0.45, -0.3, 0.2, 0.6, 0.04);
      paint(c, GLOW.centurion, null);
    }
    glow(c, 5.0, 0, 1.2, GLOW.centurion, 0.5);
    flash(c, 5.0, -13.9, 2.4, '#ffb347', p.fireAge, 0.15, p.fireIndex);
    c.restore();
  }

  // Fortress, braced to fire: a giant biped, a long cannon in the team's
  // colour on each shoulder, a heavy armoured block on each arm and a long
  // white blade outboard of it, its great feet in the team's colour behind.
  // Its cannons kick back in turn as they fire.
  function fortress(c, p) {
    const T = p.team;
    for (const sx of [-1, 1]) {
      // the foot
      rr(c, sx * 6.6 - 3.0, 4.0, 6.0, 7.6, 0.8);
      paint(c, T.accent);
      for (const x of [5.0, 6.6, 8.2]) line(c, sx * x, 4.6, sx * x, 11.0, T.deep, 0.3);
      // the wing between body and arm
      poly(c, [sx * 3.4, -4.0, sx * 9.6, -3.4, sx * 9.6, 4.0, sx * 3.4, 4.2]);
      paint(c, M.mid);
      circle(c, sx * 11.6, 6.0, 0.6);
      paint(c, M.steel);
      // the blade
      poly(c, [sx * 13.2, -12.6, sx * 12.0, -12.0, sx * 13.4, 0.0, sx * 15.2, 5.0, sx * 16.6, 3.0, sx * 15.6, -3.0]);
      paint(c, M.white);
      poly(c, [sx * 12.9, -11.0, sx * 13.9, 0.0, sx * 14.6, -0.2, sx * 13.6, -10.5]);
      paint(c, M.dark, null);
      poly(c, [sx * 15.8, -1.8, sx * 14.5, -1.4, sx * 14.6, 0.4, sx * 15.9, 0.4]);
      paint(c, T.accent, null);
      // the arm's armour
      rr(c, sx * 11.6 - 1.7, -8.3, 3.4, 3.0, 0.4);
      paint(c, T.accent);
      rr(c, sx * 11.4 - 1.9, -3.0, 3.8, 6.3, 0.6);
      paint(c, T.accent);
      rr(c, sx * 11.4 - 0.9, -1.6, 1.8, 3.4, 0.2);
      paint(c, M.white, null);
    }
    // the body
    poly(c, [0, -6.8, 3.4, -5.2, 3.4, 4.0, -3.4, 4.0, -3.4, -5.2]);
    paint(c, M.mid);
    rr(c, -1.3, -2.4, 2.6, 3.2, 0.3);
    paint(c, M.darker);
    circle(c, 0, -0.6, 0.6);
    paint(c, M.glass);
    glow(c, 0, 6.1, 0.8, GLOW.fortress, 0.6);
    // the cannons
    for (const sx of [-1, 1]) {
      const kick = recoil(p.fireAge, 0.7) * (p.fireIndex % 2 === (sx > 0 ? 1 : 0) ? 1.6 : 0.3);
      c.save();
      c.translate(0, kick);
      rr(c, sx * 5.6 - 2.1, -9.0, 4.2, 10.0, 0.6);
      paint(c, T.accent);
      rr(c, sx * 5.6 - 1.0, -7.2, 2.0, 1.2, 0.2);
      paint(c, M.darker);
      poly(c, [sx * 5.6, -4.6, sx * 6.3, -1.2, sx * 4.9, -1.2]);
      paint(c, M.white, null);
      rr(c, sx * 5.6 - 1.5, -10.6, 3.0, 0.8, 0.2);
      paint(c, T.accent);
      rr(c, sx * 5.6 - 0.6, -12.6, 1.2, 2.2, 0.1);
      paint(c, M.dark);
      rr(c, sx * 5.6 - 0.85, -16.6, 1.7, 4.2, 0.3);
      paint(c, T.accent);
      flash(c, sx * 5.6, -17.2, 2.8, '#ffb347', p.fireAge, 0.16, p.fireIndex + sx);
      c.restore();
    }
  }

  // Vulcan: a giant biped whose arms are flamethrowers reaching ahead, each
  // ending in a clawed head in the team's colour and a long spike, red
  // armour on its shoulders, a white chest and two orange fuel tanks on its
  // back round a glowing exhaust. Its claws flare as it fires.
  function vulcan(c, p) {
    const T = p.team;
    // the fuel tanks and the back
    for (const sx of [-1, 1]) {
      for (const [x, y] of [[12.5, 4.5], [11.0, 10.2]]) {
        circle(c, sx * x, y, 0.8);
        paint(c, M.pale);
      }
      circle(c, sx * 8.5, 6.2, 3.6);
      paint(c, '#f27a2a');
      circle(c, sx * 8.5, 6.2, 2.6);
      paint(c, '#ff9a4a', null);
      line(c, sx * 8.5, 6.2, sx * 9.8, 3.4, M.dark, 0.35);
      circle(c, sx * 8.5, 6.2, 0.55);
      paint(c, T.accent);
    }
    rr(c, -2.4, 2.8, 4.8, 4.4, 0.4);
    paint(c, M.mid);
    circle(c, 0, 8.6, 1.6);
    paint(c, M.dark);
    circle(c, 0, 8.6, 0.8);
    paint(c, M.darker, null);
    glow(c, 0, 8.6, 1.4, GLOW.vulcan, 0.5);
    // the chest
    for (const sx of [-1, 1]) {
      rr(c, sx * 6.4 - 1.9, -6.0, 3.8, 8.4, 0.4);
      paint(c, M.white);
      poly(c, [sx * 4.5, -1.6, sx * 8.3, -1.6, sx * 8.3, 2.4, sx * 4.5, 2.4]);
      paint(c, M.pale, null);
      circle(c, sx * 4.0, -1.0, 1.3);
      paint(c, M.pale);
    }
    rr(c, -2.1, -7.6, 4.2, 10.4, 0.4);
    paint(c, M.mid);
    poly(c, [-1.4, -7.6, 1.4, -7.6, 2.1, -4.6, -2.1, -4.6]);
    paint(c, M.white);
    rr(c, -0.6, -2.6, 1.2, 4.2, 0.1);
    paint(c, M.darker, null);
    // the arms
    for (const sx of [-1, 1]) {
      rr(c, sx * 11.4 - 2.2, -6.0, 4.4, 7.4, 0.4);
      paint(c, T.accent);
      for (const y of [-3.0, -1.0]) {
        rr(c, sx * 10.1 - 0.15, y, 0.3, 0.8, 0.05);
        paint(c, M.white, null);
      }
      rr(c, sx * 14.8 - 0.35, -1.6, 0.7, 2.6, 0.2);
      paint(c, M.steel);
      rr(c, sx * 11.4 - 3.6, -13.5, 7.2, 7.2, 0.8);
      paint(c, M.dark);
      rr(c, sx * 11.4 - 2.2, -12.8, 4.4, 4.2, 0.4);
      paint(c, M.white);
      rr(c, sx * 11.4 - 0.9, -8.4, 1.8, 1.4, 0.2);
      paint(c, M.pale, null);
      poly(c, [sx * 9.4, -13.4, sx * 13.6, -13.4, sx * 13.3, -16.4, sx * 12.4, -17.6, sx * 10.6, -17.6, sx * 9.7, -16.4]);
      paint(c, T.accent);
      poly(c, [sx * 10.6, -14.2, sx * 12.2, -14.2, sx * 11.4, -16.8]);
      paint(c, M.white, null);
      poly(c, [sx * 11.3, -17.4, sx * 11.9, -17.4, sx * 11.6, -24.0]);
      paint(c, M.dark);
      flash(c, sx * 11.4, -18.4, 3.6, '#ff8a3c', p.fireAge, 0.3, p.fireIndex + sx);
    }
  }

  // Melting Point, as it fires: a platform on four legs out to its corners,
  // each on a heavy foot armoured in the team's colour, and on it a round
  // body under an arched white frame, a golden emitter across its middle
  // that glows and blazes as it fires, gold canisters round it.
  function melting_point(c, p) {
    const T = p.team;
    const pulse = 0.7 + 0.3 * Math.sin(p.t * 5);
    for (const [sx, sy] of [[-1, -1], [1, -1], [-1, 1], [1, 1]]) {
      c.save();
      c.translate(sx * 13.0, sy * 12.6);
      c.rotate(Math.atan2(sx, -sy));
      rr(c, -2.0, 2.0, 4.0, 10.0, 0.6);
      paint(c, M.mid);
      rr(c, -1.2, 3.0, 2.4, 5.0, 0.3);
      paint(c, M.white, null);
      rr(c, -2.8, -2.8, 5.6, 5.6, 0.6);
      paint(c, M.dark);
      rr(c, -2.4, -2.4, 4.8, 1.2, 0.2);
      paint(c, T.accent);
      rr(c, -2.4, 1.2, 4.8, 1.2, 0.2);
      paint(c, T.accent);
      rr(c, -1.4, -1.0, 2.8, 2.0, 0.6);
      paint(c, M.pale);
      c.restore();
      c.save();
      c.translate(sx * 6.4, sy * 6.3);
      c.rotate(sx * sy * Math.PI / 4);
      rr(c, -0.6, -1.2, 1.2, 2.4, 0.4);
      paint(c, '#e8b53c');
      c.restore();
    }
    // the body
    circle(c, 0, 0, 6.4);
    paint(c, M.darker);
    for (let i = 0; i < 5; i++) {
      const a = -Math.PI / 2 + (i - 2) * 0.55;
      c.save();
      c.rotate(a + Math.PI / 2);
      poly(c, [-0.8, -8.2, 0.8, -8.2, 1.0, -6.6, -1.0, -6.6]);
      paint(c, M.white);
      c.restore();
    }
    for (const sx of [-1, 1]) {
      rr(c, sx * 7.5 - 1.5, -2.5, 3.0, 4.0, 0.3);
      paint(c, T.accent);
      circle(c, sx * 8.3, -3.0, 0.8);
      paint(c, M.pale);
      rr(c, sx * 10.0 - 0.6, -1.5, 1.2, 3.0, 0.2);
      paint(c, M.mid);
    }
    rr(c, -10.5, 2.5, 21.0, 0.5, 0.1);
    paint(c, '#9fd8e6', null);
    circle(c, 0, -0.4, 3.6);
    paint(c, T.deep);
    circle(c, 0, -0.4, 3.1);
    paint(c, M.black, null);
    // the arched frame
    c.beginPath();
    c.arc(0, 0.6, 5.2, Math.PI, 0);
    c.lineWidth = 0.9;
    c.strokeStyle = M.white;
    c.stroke();
    c.beginPath();
    c.arc(0, 0.6, 5.2, Math.PI, 0);
    c.lineWidth = 0.25;
    c.strokeStyle = '#9fd8e6';
    c.stroke();
    for (const sx of [-1, 1]) {
      rr(c, sx * 2.05 - 0.35, -5.0, 0.7, 9.0, 0.1);
      paint(c, M.mid);
      rr(c, sx * 2.05 - 0.45, -5.2, 0.9, 1.2, 0.1);
      paint(c, T.accent);
    }
    // the emitter
    glow(c, 0, 0.25, 3.0, GLOW.melting_point, 0.5 * pulse + (p.fireAge < 0.5 ? 0.5 : 0));
    rr(c, -3.0, -0.5, 6.0, 1.5, 0.2);
    paint(c, '#e8b53c');
    line(c, -2.8, 0.25, 2.8, 0.25, GLOW.melting_point, 0.3);
    flash(c, 0, 0.25, 3.6, GLOW.melting_point, p.fireAge, 0.25, p.fireIndex);
    // the back
    for (const sx of [-1, 1]) {
      rr(c, sx * 2.6 - 1.0, 7.6, 2.0, 3.0, 0.3);
      paint(c, M.dark);
      rr(c, sx * 2.6 - 0.5, 7.9, 1.0, 0.5, 0.1);
      paint(c, T.accent, null);
    }
  }

  // Overlord: a great airship, its long grey prow carrying two guns ahead
  // and a glass core glowing at its middle, a white arc each side of it on
  // a turret pod in the team's colour, and white engines and fins astern.
  function overlord(c, p) {
    const T = p.team;
    const pulse = 0.7 + 0.3 * Math.sin(p.t * 3);
    // the engines and fins astern
    for (const sx of [-1, 1]) {
      poly(c, [sx * 13.0, 9.0, sx * 21.5, 10.4, sx * 21.0, 12.8, sx * 13.0, 13.4]);
      paint(c, M.white);
      for (const [x0, x1] of [[5.0, 8.0], [10.0, 13.0]]) {
        rr(c, sx > 0 ? x0 : -x1, 4.5, x1 - x0, 15.5, 0.4);
        paint(c, M.white);
        rr(c, (sx > 0 ? x0 : -x1) + 0.6, 6.0, x1 - x0 - 1.2, 5.0, 0.2);
        paint(c, M.pale, null);
      }
      rr(c, sx > 0 ? 8.0 : -10.0, 7.0, 2.0, 10.0, 0.2);
      paint(c, M.dark);
    }
    rr(c, -4.5, 3.0, 9.0, 14.0, 0.6);
    paint(c, M.dark);
    rr(c, -1.6, 5.0, 3.2, 3.6, 0.4);
    paint(c, T.accent);
    // the arcs
    for (const sx of [-1, 1]) {
      c.beginPath();
      if (sx < 0) c.arc(0, 1.5, 14.0, Math.PI * 0.78, Math.PI * 1.25);
      else c.arc(0, 1.5, 14.0, -Math.PI * 0.25, Math.PI * 0.22);
      c.lineWidth = 2.2;
      c.strokeStyle = M.white;
      c.stroke();
      circle(c, sx * 13.8, -13.4, 1.9);
      paint(c, T.accent);
      circle(c, sx * 13.8, -13.4, 0.9);
      paint(c, M.darker, null);
      // the red plates and discs by the core
      poly(c, [sx * 6.0, -5.2, sx * 11.2, -5.2, sx * 12.2, -1.0, sx * 6.0, -1.0]);
      paint(c, T.accent);
      for (let i = 0; i < 4; i++) line(c, sx * (7.0 + i), -4.6, sx * (7.4 + i), -1.6, T.deep, 0.2);
      circle(c, sx * 6.6, 0.8, 1.4);
      paint(c, M.mid);
    }
    // the prow
    poly(c, [0, -31.6, 1.4, -30.6, 4.2, -28.8, 5.6, -25.0, 9.4, -21.4, 9.6, -19.6, 5.2, -18.8, 4.6, -12.0, 4.2, -1.0, 2.0, 2.4, -2.0, 2.4, -4.2, -1.0, -4.6, -12.0, -5.2, -18.8, -9.6, -19.6, -9.4, -21.4, -5.6, -25.0, -4.2, -28.8, -1.4, -30.6]);
    paint(c, M.mid);
    poly(c, [0, -29.0, 3.2, -26.0, 3.6, -13.0, 0, -11.0, -3.6, -13.0, -3.2, -26.0]);
    paint(c, M.pale, null);
    for (const sx of [-1, 1]) {
      rr(c, sx * 6.0 - 0.5, -26.0, 1.0, 4.0, 0.15);
      paint(c, T.accent);
    }
    rr(c, -0.55, -29.5, 1.1, 19.0, 0.2);
    paint(c, M.dark, null);
    for (let y = -17; y < -11; y += 1.5) {
      rr(c, -1.6, y, 0.8, 0.8, 0.05);
      paint(c, M.dark, null);
      rr(c, 0.8, y, 0.8, 0.8, 0.05);
      paint(c, M.dark, null);
    }
    poly(c, [0, -31.8, 0.9, -30.2, -0.9, -30.2]);
    paint(c, T.accent);
    // the guns ahead
    for (const sx of [-1, 1]) {
      rr(c, sx * 2.8 - 0.35, -33.0, 0.7, 3.0, 0.1);
      paint(c, M.white);
      rr(c, sx * 2.8 - 0.8, -37.0, 1.6, 4.2, 0.4);
      paint(c, T.accent);
      flash(c, sx * 2.8, -37.6, 2.6, '#ffb347', p.fireAge, 0.15, p.fireIndex + sx);
    }
    // the core
    rr(c, -1.3, -9.5, 2.6, 6.5, 0.6);
    paint(c, '#a8e4ee');
    poly(c, [0, -2.0, 1.3, -1.25, 1.3, 0.25, 0, 1.0, -1.3, 0.25, -1.3, -1.25]);
    paint(c, T.accent);
    glow(c, 0, -0.5, 1.8, GLOW.overlord, pulse);
    poly(c, [0, -1.2, 0.6, 0.0, -0.6, 0.0]);
    paint(c, GLOW.overlord, null);
  }

  // War Factory: a moving fortress. Four white track pods at its corners
  // and a pod of machinery each side, a white wedge at its front, its
  // middle armoured in bands and an X in the team's colour, two turbine
  // fans spinning at its back, and on top a heavy gun that turns to its
  // target.
  function war_factory(c, p) {
    const T = p.team;
    const spin = p.t * 6;
    for (const sx of [-1, 1]) {
      // the corner pods, front and back
      for (const [y0, y1] of [[-33.0, -16.5], [17.5, 35.0]]) {
        rr(c, sx > 0 ? 13.5 : -21.5, y0, 8.0, y1 - y0, 0.8);
        paint(c, M.white);
        rr(c, sx > 0 ? 15.0 : -20.0, y0 + 3.0, 5.0, y1 - y0 - 6.0, 0.4);
        paint(c, M.pale, null);
        line(c, sx * 17.5, y0 + 2.0, sx * 17.5, y1 - 2.0, M.mid, 0.4);
      }
      circle(c, sx * 13.5, -14.8, 1.2);
      paint(c, T.accent);
      circle(c, sx * 13.5, 24.0, 1.2);
      paint(c, T.accent);
      // the side pod
      rr(c, sx > 0 ? 22.0 : -31.0, -13.0, 9.0, 30.5, 1.0);
      paint(c, M.white);
      for (const y of [-1.5, 12.5]) {
        rr(c, sx * 25.5 - 2.5, y - 2.5, 5.0, 5.0, 0.6);
        paint(c, M.dark);
        circle(c, sx * 25.5, y, 1.6);
        paint(c, T.accent);
      }
      circle(c, sx * 32.0, 5.6, 1.5);
      paint(c, M.pale);
      rr(c, sx > 0 ? 15.5 : -22.0, 3.5, 6.5, 4.0, 0.4);
      paint(c, M.dark);
    }
    // the body: the bands, then the armour with its X
    rr(c, -15.0, -13.0, 30.0, 23.0, 1.0);
    paint(c, M.dark);
    rr(c, -12.0, -12.6, 24.0, 8.6, 0.6);
    paint(c, T.accent);
    for (let y = -11.6; y < -4.5; y += 1.6) line(c, -11.4, y, 11.4, y, T.deep, 0.35);
    for (const sx of [-1, 1]) {
      rr(c, sx > 0 ? 2.8 : -14.6, -1.5, 11.8, 11.5, 0.6);
      paint(c, M.white);
      poly(c, [sx * 3.2, -1.0, sx * 8.6, 4.2, sx * 3.2, 9.6]);
      paint(c, T.accent, null);
      poly(c, [sx * 14.2, -1.0, sx * 8.8, 4.2, sx * 14.2, 9.6]);
      paint(c, T.accent, null);
      rr(c, sx > 0 ? 11.8 : -15.6, 4.0, 3.8, 2.2, 0.8);
      paint(c, M.darker);
    }
    // the front wedge
    poly(c, [-6.2, -35.6, 6.2, -35.6, 7.4, -16.0, -7.4, -16.0]);
    paint(c, M.white);
    poly(c, [3.6, -35.0, 6.4, -30.0, -4.2, -16.4, -6.6, -17.6]);
    paint(c, M.darker, null);
    // the back: the fans, a block between them, a white wedge astern
    for (const sx of [-1, 1]) {
      circle(c, sx * 8.7, 18.7, 3.3);
      paint(c, M.white);
      circle(c, sx * 8.7, 18.7, 2.6);
      paint(c, M.darker);
      for (let i = 0; i < 8; i++) {
        const a = spin * sx + (i * Math.PI) / 4;
        line(c, sx * 8.7, 18.7, sx * 8.7 + Math.cos(a) * 2.4, 18.7 + Math.sin(a) * 2.4, M.pale, 0.3);
      }
      glow(c, sx * 8.7, 18.7, 2.4, GLOW.war_factory, 0.3);
    }
    rr(c, -5.0, 14.0, 10.0, 12.0, 0.6);
    paint(c, M.dark);
    rr(c, -1.6, 19.5, 3.2, 2.5, 0.3);
    paint(c, T.accent);
    poly(c, [-6.0, 25.5, 6.0, 25.5, 0, 32.5]);
    paint(c, M.white);
    // the gun
    c.save();
    c.translate(0, -7.0);
    c.rotate(p.turret);
    const kick = recoil(p.fireAge, 0.8) * 2.0;
    circle(c, 0, 0, 4.2);
    paint(c, M.dark);
    circle(c, 0, 0, 2.8);
    paint(c, T.accent);
    rr(c, -1.0, -17.0 + kick, 2.0, 15.0, 0.4);
    paint(c, M.darker);
    rr(c, -1.4, -19.0 + kick, 2.8, 3.0, 0.4);
    paint(c, T.accent);
    flash(c, 0, -19.8 + kick, 4.0, '#ffb347', p.fireAge, 0.2, p.fireIndex);
    c.restore();
  }

  // Sandworm, risen to strike: a segmented worm seen from above, its white
  // segments plated in the team's colour trailing behind, its long head
  // between two arms that each end in a toothed saw, spinning faster as it
  // strikes.
  const SANDWORM_SEGMENTS = [[0, 4.5, 5.0], [-0.5, 10.5, 4.4], [-0.7, 15.5, 3.8], [-0.4, 19.6, 3.2]];
  function sandworm(c, p) {
    const T = p.team;
    const sway = Math.sin(p.t * 1.2) * 0.4;
    // the segments, the last first
    for (let i = SANDWORM_SEGMENTS.length - 1; i >= 0; i--) {
      const [x, y, r] = SANDWORM_SEGMENTS[i];
      const dx = x + sway * i * 0.6;
      rr(c, dx - r, y - r * 0.8, r * 2, r * 1.6, r * 0.4);
      paint(c, M.white);
      rr(c, dx - r - 0.3, y - r * 0.4, 1.2, r * 0.8, 0.3);
      paint(c, T.accent);
      rr(c, dx + r - 0.9, y - r * 0.4, 1.2, r * 0.8, 0.3);
      paint(c, T.accent);
      poly(c, [dx, y - r * 0.6, dx + r * 0.45, y + r * 0.4, dx - r * 0.45, y + r * 0.4]);
      paint(c, M.darker, null);
    }
    // the arms out to the saws
    const spin = p.t * (p.strike >= 0 || p.fireAge < 0.6 ? 14 : 4);
    for (const sx of [-1, 1]) {
      c.beginPath();
      c.moveTo(sx * 2.5, -1.5);
      c.lineTo(sx * 7.5, -4.0);
      c.lineTo(sx * 7.0, -7.0);
      c.lineWidth = 1.6;
      c.strokeStyle = M.white;
      c.stroke();
      c.save();
      c.translate(sx * 6.6, -7.6);
      c.rotate(sx * 0.4);
      rr(c, -1.3, -2.0, 2.6, 4.0, 0.5);
      paint(c, T.accent);
      c.restore();
      // the saw
      c.save();
      c.translate(sx * 5.6, -11.5);
      c.rotate(spin * sx);
      for (let i = 0; i < 10; i++) {
        c.save();
        c.rotate((i * Math.PI) / 5);
        poly(c, [-0.5, -2.6, 0.6, -2.6, 1.1, -3.6]);
        paint(c, M.pale, null);
        c.restore();
      }
      circle(c, 0, 0, 2.7);
      paint(c, M.white);
      circle(c, 0, 0, 1.9);
      paint(c, M.darker);
      circle(c, 0, 0, 1.4);
      paint(c, T.accent);
      rr(c, -0.4, -1.4, 0.8, 2.8, 0.1);
      paint(c, T.deep, null);
      c.restore();
    }
    // the head
    poly(c, [0, -12.6, 1.8, -11.0, 2.6, -4.0, 1.6, -1.5, -1.6, -1.5, -2.6, -4.0, -1.8, -11.0]);
    paint(c, M.white);
    for (const sx of [-1, 1]) {
      poly(c, [sx * 1.9, -10.5, sx * 2.6, -4.2, sx * 2.1, -4.2, sx * 1.4, -10.5]);
      paint(c, T.accent, null);
      poly(c, [sx * 3.0, -4.6, sx * 3.8, -3.0, sx * 3.0, -2.6]);
      paint(c, M.dark);
    }
    rr(c, -0.6, -10.0, 1.2, 6.5, 0.2);
    paint(c, M.pale, null);
    rr(c, -1.0, -3.0, 2.0, 1.6, 0.2);
    paint(c, T.accent);
    glow(c, 0, -2.2, 1.0, GLOW.sandworm, 0.5);
  }

  // Raiden: a flyer shaped like a fork, two white prongs lined in the
  // team's colour reaching ahead, each with a rail gun along its inside
  // whose coils glow and blaze as it fires, a dark ring joining them to a
  // white body with a dome and twin guns, an arm each side, and two engines
  // in the team's colour astern, their exhausts glowing.
  function raiden(c, p) {
    const T = p.team;
    const bob = Math.sin(p.t * 1.6 + p.walk) * 0.02;
    c.scale(1 + bob, 1 + bob);
    const pulse = 0.7 + 0.3 * Math.sin(p.t * 7);
    // the engines astern
    rr(c, -2.0, 12.5, 4.0, 8.5, 0.4);
    paint(c, M.mid);
    for (const sx of [-1, 1]) {
      rr(c, sx > 0 ? 3.5 : -8.5, 14.5, 5.0, 7.0, 0.6);
      paint(c, T.accent);
      rr(c, sx > 0 ? 3.8 : -8.2, 14.6, 4.4, 1.6, 0.3);
      paint(c, M.darker);
      for (const x of [4.3, 7.0]) {
        glow(c, sx * x, 22.0, 1.4, GLOW.raiden, pulse);
        rr(c, sx * x - 0.7, 21.3, 1.4, 1.0, 0.3);
        paint(c, '#bfefff', null);
      }
    }
    // the ring
    c.beginPath();
    c.arc(0, 3.0, 10.0, Math.PI * 0.95, Math.PI * 2.05);
    c.lineWidth = 2.2;
    c.strokeStyle = M.dark;
    c.stroke();
    // the body
    poly(c, [-12.0, 3.5, 12.0, 3.5, 12.4, 9.0, 9.6, 13.4, -9.6, 13.4, -12.4, 9.0]);
    paint(c, M.white);
    poly(c, [-9.0, 4.4, 9.0, 4.4, 9.0, 9.0, -9.0, 9.0]);
    paint(c, M.pale, null);
    // the arms out to the sides
    for (const sx of [-1, 1]) {
      rr(c, sx > 0 ? 10.8 : -14.2, -7.0, 3.4, 9.0, 0.6);
      paint(c, M.mid);
      line(c, sx * 13.2, -7.0, sx * 13.2, -9.4, M.pale, 0.15);
      poly(c, [sx * 13.6, -1.2, sx * 14.7, 0.5, sx * 13.6, 2.2, sx * 12.5, 0.5]);
      paint(c, T.accent);
    }
    // the prongs and their rail guns
    for (const sx of [-1, 1]) {
      for (const y of [-15.2, -12.6]) {
        circle(c, sx * 9.9, y, 0.9);
        paint(c, M.darker);
      }
      poly(c, [sx * 9.0, -21.0, sx * 6.0, -21.0, sx * 4.4, -13.0, sx * 4.0, -6.0, sx * 5.6, -3.8, sx * 9.6, -3.8, sx * 11.0, -8.0, sx * 10.4, -14.0]);
      paint(c, M.white);
      poly(c, [sx * 6.4, -20.2, sx * 5.4, -20.2, sx * 4.6, -6.4, sx * 5.8, -5.2]);
      paint(c, T.accent, null);
      rr(c, sx * 3.9 - 0.5, -21.2, 1.0, 19.0, 0.3);
      paint(c, M.white);
      rr(c, sx * 3.9 - 0.25, -16.4, 0.5, 6.4, 0.1);
      paint(c, GLOW.raiden, null);
      for (let y = -8.4; y < -2.6; y += 0.9) {
        rr(c, sx * 3.9 - 0.5, y, 1.0, 0.4, 0.05);
        paint(c, M.dark, null);
      }
      glow(c, sx * 3.9, -13.2, 1.2, GLOW.raiden, 0.4 * pulse);
      flash(c, sx * 3.9, -21.8, 2.4, GLOW.raiden, p.fireAge, 0.2, p.fireIndex + sx);
    }
    // the dome and its guns
    rr(c, -0.4, -5.2, 0.8, 4.2, 0.15);
    paint(c, M.pale);
    for (const sx of [-1, 1]) {
      rr(c, sx * 1.1 - 0.4, 0.0, 0.8, 3.6, 0.2);
      paint(c, M.white);
      rr(c, sx > 0 ? 3.0 : -6.0, 5.0, 3.0, 3.5, 0.3);
      paint(c, M.mid);
      for (let i = 0; i < 3; i++) line(c, sx * (3.6 + i * 0.8), 5.3, sx * (3.6 + i * 0.8), 8.2, '#a06a4a', 0.25);
    }
    circle(c, 0, 7.3, 2.8);
    paint(c, M.white);
    circle(c, 0, 7.3, 1.6);
    paint(c, M.pale, null);
  }

  // Abyss: a great flying ship. A white crested head at its front, at its
  // middle a ring in the team's colour round a dark core glowing violet, a
  // ringed pod and a drum out each side, white wings and turbines astern
  // and two long feelers trailing behind.
  function abyss(c, p) {
    const T = p.team;
    const pulse = 0.7 + 0.3 * Math.sin(p.t * 3);
    const trail = Math.sin(p.t * 1.3) * 0.6;
    // the feelers
    for (const sx of [-1, 1]) {
      poly(c, [sx * 16.4, 19.5, sx * 17.2, 19.5, sx * 19.2 + trail, 32.6, sx * 18.8 + trail, 32.6]);
      paint(c, T.accent);
    }
    // the wings and turbines astern
    for (const sx of [-1, 1]) {
      poly(c, [sx * 10.0, 12.0, sx * 20.0, 13.0, sx * 24.0, 17.0, sx * 21.0, 25.0, sx * 10.0, 24.0]);
      paint(c, M.white);
      for (let i = 0; i < 5; i++) line(c, sx * (12.0 + i * 2.0), 15.0, sx * (12.5 + i * 2.0), 23.0, M.pale, 0.4);
      rr(c, sx * 8.6 - 3.0, 14.0, 6.0, 10.6, 1.4);
      paint(c, M.mid);
      for (let y = 15.0; y < 24.0; y += 1.2) line(c, sx * 6.2, y, sx * 11.0, y, M.dark, 0.3);
    }
    rr(c, -3.6, 11.0, 7.2, 14.0, 0.8);
    paint(c, T.accent);
    for (let y = 13.5; y < 24.0; y += 2.0) line(c, -2.6, y, 2.6, y, T.deep, 0.5);
    // the body round the core
    rr(c, -16.0, -1.0, 32.0, 13.0, 3.0);
    paint(c, M.dark);
    for (const sx of [-1, 1]) {
      // the drum outboard
      rr(c, sx > 0 ? 21.6 : -25.6, -0.8, 4.0, 11.6, 1.6);
      paint(c, T.accent);
      rr(c, sx > 0 ? 18.5 : -21.5, 3.2, 3.0, 3.6, 0.3);
      paint(c, M.darker);
      // the ringed pod
      rr(c, sx > 0 ? 7.0 : -12.0, 2.4, 5.0, 4.0, 1.0);
      paint(c, M.mid);
      circle(c, sx * 13.8, 4.2, 3.2);
      paint(c, T.accent);
      circle(c, sx * 13.8, 4.2, 1.8);
      paint(c, M.darker);
    }
    circle(c, 0, 5.0, 7.6);
    paint(c, T.accent);
    circle(c, 0, 5.0, 5.0);
    paint(c, M.black);
    glow(c, 0, 5.0, 4.6, GLOW.abyss, 0.6 * pulse);
    circle(c, 0, 5.0, 1.6);
    paint(c, GLOW.abyss, null);
    for (let i = 0; i < 8; i++) {
      const a = (i * Math.PI) / 4;
      rr(c, Math.cos(a) * 6.3 - 0.4, 5.0 + Math.sin(a) * 6.3 - 0.4, 0.8, 0.8, 0.1);
      paint(c, T.deep, null);
    }
    flash(c, 0, 5.0, 6.0, GLOW.abyss, p.fireAge, 0.25, p.fireIndex);
    // the shoulders and the head
    for (const sx of [-1, 1]) {
      poly(c, [sx * 13.0, -12.0, sx * 19.5, -10.0, sx * 21.5, -6.0, sx * 19.5, -2.0, sx * 13.0, -2.0]);
      paint(c, M.mid);
      poly(c, [sx * 19.8, -9.0, sx * 22.6, -7.0, sx * 22.6, -2.4, sx * 20.0, -2.4]);
      paint(c, T.accent);
      rr(c, sx * 10.2 - 1.0, -9.0, 2.0, 3.6, 0.6);
      paint(c, '#f2a66b');
      circle(c, sx * 16.4, -4.6, 1.4);
      paint(c, M.steel);
    }
    poly(c, [0, -21.6, 4.6, -20.6, 8.0, -17.0, 12.0, -14.0, 15.4, -10.0, 13.0, -3.0, 6.0, -1.0, -6.0, -1.0, -13.0, -3.0, -15.4, -10.0, -12.0, -14.0, -8.0, -17.0, -4.6, -20.6]);
    paint(c, M.white);
    poly(c, [-3.4, -21.4, 3.4, -21.4, 4.4, -19.2, -4.4, -19.2]);
    paint(c, T.accent);
    for (const sx of [-1, 1]) {
      poly(c, [sx * 6.0, -16.0, sx * 9.0, -12.0, sx * 6.4, -10.6]);
      paint(c, M.darker, null);
      poly(c, [sx * 9.0, -9.0, sx * 13.0, -7.0, sx * 9.6, -5.6]);
      paint(c, M.darker, null);
    }
    poly(c, [0, -15.0, 2.6, -12.6, 2.0, -6.0, -2.0, -6.0, -2.6, -12.6]);
    paint(c, T.accent, null);
    rr(c, -1.4, -10.6, 2.8, 8.0, 0.4);
    paint(c, M.mid);
    rr(c, -5.0, -6.4, 10.0, 1.6, 0.4);
    paint(c, M.mid);
  }

  // Mountain: a titan with a rank of eight launch pods across its
  // shoulders, four each side, the outermost and innermost in the team's
  // colour; between them a pointed white head over a hexagonal core whose
  // orange ring glows, and its feet behind. Each pod's mouth flashes as it
  // fires.
  const MOUNTAIN_PODS = [[6.2, 2.6, -14.5, 10.0, true], [14.5, 2.0, -13.5, 12.0, false], [19.8, 1.8, -13.0, 12.0, false], [27.5, 2.6, -19.0, 13.0, true]];
  function mountain(c, p) {
    const T = p.team;
    // the feet
    for (const [x, w] of [[-11, 6], [11, 6], [0, 4]]) {
      rr(c, x - w / 2, 15.6, w, 5.6, 0.6);
      paint(c, M.pale);
    }
    rr(c, -0.8, 13.6, 1.6, 7.6, 0.2);
    paint(c, M.white);
    // the shoulders under the pods
    rr(c, -30.0, -6.0, 60.0, 12.0, 2.0);
    paint(c, M.dark);
    for (const sx of [-1, 1]) {
      poly(c, [sx * 30.0, 3.0, sx * 33.0, 4.4, sx * 32.4, 11.0, sx * 30.0, 11.4]);
      paint(c, T.accent);
      MOUNTAIN_PODS.forEach(([x, hw, y0, y1, red], i) => {
        rr(c, sx * x - hw, y0, hw * 2, y1 - y0, 0.6);
        paint(c, red ? T.accent : M.mid);
        rr(c, sx * x - hw * 0.55, y0 + (y1 - y0) * 0.35, hw * 1.1, (y1 - y0) * 0.2, 0.2);
        paint(c, M.white, null);
        rr(c, sx * x - hw * 0.7, y0 + 0.6, hw * 1.4, 2.2, 0.3);
        paint(c, red ? T.deep : M.darker, null);
        rr(c, sx * x - hw, y1 - 3.0, hw * 2, 2.4, 0.3);
        paint(c, red ? M.white : T.accent, null);
        flash(c, sx * x, y0 - 0.4, 2.6, '#ffb347', p.fireAge, 0.18, p.fireIndex + i * sx);
      });
    }
    // the head and the core
    poly(c, [0, -18.6, 2.6, -17.0, 6.2, -13.0, 6.6, -6.0, 3.4, -2.6, -3.4, -2.6, -6.6, -6.0, -6.2, -13.0, -2.6, -17.0]);
    paint(c, M.white);
    rr(c, -0.7, -16.6, 1.4, 3.0, 0.2);
    paint(c, M.white, M.line, 0.8);
    poly(c, [-2.4, -10.0, 2.4, -10.0, 2.0, -3.4, -2.0, -3.4]);
    paint(c, M.mid, null);
    poly(c, [0, 2.0, 3.0, 3.7, 3.0, 7.3, 0, 9.0, -3.0, 7.3, -3.0, 3.7]);
    paint(c, M.darker);
    c.beginPath();
    c.moveTo(0, 2.4);
    [[2.7, 3.9], [2.7, 7.1], [0, 8.6], [-2.7, 7.1], [-2.7, 3.9]].forEach(([x, y]) => c.lineTo(x, y));
    c.closePath();
    c.lineWidth = 0.35;
    c.strokeStyle = GLOW.mountain;
    c.stroke();
    glow(c, 0, 5.5, 2.4, GLOW.mountain, 0.5 + 0.2 * Math.sin(p.t * 3));
    circle(c, 0, 5.5, 0.9);
    paint(c, T.accent);
  }

  // Larva: a Sandworm's young, its model the Sandworm's own mesh at half its
  // size (Mech_Default_1001_1, 11.5 m by 19.8 m against 22.3 m by 39.6 m).
  function larva(c, p) {
    c.scale(0.5, 0.5);
    sandworm(c, p);
  }

  // Spider Mine: a red drum on four splayed legs, each foot white with an
  // orange lamp, a slot across the drum's top; it lights as it goes off.
  function spider_mine(c, p) {
    const T = p.team;
    const stride = Math.min(1, p.speed / 4) * 0.25;
    const ph = p.walk * Math.PI * 2 / 2;
    for (const [sx, sy] of [[-1, -1], [1, -1], [-1, 1], [1, 1]]) {
      const off = Math.sin(ph + (sx * sy > 0 ? 0 : Math.PI)) * stride;
      c.save();
      c.translate(sx * 1.45, sy * 1.45 + off);
      c.rotate(Math.atan2(sx, -sy));
      rr(c, -0.12, 0.1, 0.24, 0.9, 0.05);
      paint(c, M.dark);
      rr(c, -0.42, -0.75, 0.84, 1.0, 0.2);
      paint(c, M.white);
      rr(c, -0.22, -0.15, 0.44, 0.3, 0.05);
      paint(c, M.caution, null);
      c.restore();
    }
    circle(c, 0, 0, 0.92);
    paint(c, T.accent);
    circle(c, 0, 0.05, 0.55);
    paint(c, T.deep, null);
    rr(c, -0.12, -0.9, 0.24, 1.5, 0.05);
    paint(c, M.darker, null);
    glow(c, 0, -0.1, 0.6, GLOW.spider_mine, 0.4 + (p.strike >= 0 ? 0.6 : 0));
  }

  // Sentry Missile, in flight: a long dark missile with an orange nose and
  // band, its engine at the tail burning; it is drawn along its flight.
  function sentry_missile(c, p) {
    const T = p.team;
    glow(c, 0, 7.6, 2.0, GLOW.sentry_missile, 0.9);
    rr(c, -0.7, 3.4, 1.4, 3.0, 0.2);
    paint(c, M.dark);
    poly(c, [-0.2, 3.9, 0.2, 3.9, 0, 5.9]);
    paint(c, GLOW.sentry_missile, null);
    rr(c, -0.55, 6.5, 1.1, 0.8, 0.1);
    paint(c, M.darker);
    rr(c, -0.45, 1.6, 0.9, 1.6, 0.1);
    paint(c, '#e8894a');
    rr(c, -0.62, -6.1, 1.24, 7.6, 0.3);
    paint(c, M.mid);
    rr(c, -0.12, -6.0, 0.24, 7.4, 0.05);
    paint(c, M.white, null);
    poly(c, [0, -7.6, 0.6, -7.0, 0.62, -6.0, -0.62, -6.0, -0.6, -7.0]);
    paint(c, T.accent);
    poly(c, [0, -7.6, 0.2, -7.3, 0.2, -6.0, -0.2, -6.0, -0.2, -7.3]);
    paint(c, M.white, null);
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

  const UNITS = { marksman, arclight, rhino, crawler, sledgehammer, wasp, phoenix, fang, hound, void_eye, vortex, mustang, steel_ball, stormcaller, hacker, fire_badger, sabertooth, tarantula, phantom_ray, wraith, scorpion, typhoon, farseer, centurion, fortress, vulcan, melting_point, overlord, war_factory, sandworm, raiden, abyss, mountain, larva, spider_mine, sentry_missile };

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

  // Missile Interceptor: a pod landed on four splayed legs, white plates
  // round a domed top, and a gatling in the team's colour that turns to the
  // missile it shoots down.
  function interceptor(c, p) {
    const T = p.team;
    for (const [x, y, a] of [[-2.7, -2.6, -0.8], [2.7, -2.6, 0.8], [-3.5, 0.9, -1.9], [3.5, 0.9, 1.9], [0, 3.3, Math.PI]]) {
      c.save();
      c.translate(x, y);
      c.rotate(a);
      rr(c, -0.55, -0.7, 1.1, 1.4, 0.2);
      paint(c, M.dark);
      rr(c, -0.35, 0.6, 0.7, 1.6, 0.1);
      paint(c, M.steel);
      c.restore();
    }
    rr(c, -2.6, -1.2, 1.4, 2.6, 0.3);
    paint(c, M.white);
    rr(c, 1.4, -0.5, 1.4, 1.2, 0.3);
    paint(c, M.white);
    for (const [x, y] of [[-1.6, -1.6], [1.6, -1.6], [-1.6, 1.7], [1.6, 1.7]]) {
      circle(c, x, y, 0.35);
      paint(c, M.mid);
    }
    c.save();
    c.translate(0, 0.4);
    c.rotate(p.turret);
    const spin = p.fireAge < 0.3 ? p.t * 40 : 0;
    rr(c, -0.45, -6.4, 0.9, 4.0, 0.15);
    paint(c, M.darker);
    for (let i = 0; i < 3; i++) {
      const x = Math.sin(spin + (i * 2 * Math.PI) / 3) * 0.25;
      line(c, x, -6.3, x, -2.6, M.steel, 0.1);
    }
    poly(c, [-0.55, -2.9, 0.55, -2.9, 0.75, -1.3, -0.75, -1.3]);
    paint(c, T.accent);
    for (const sx of [-1, 1]) {
      poly(c, [sx * 0.55, -2.4, sx * 0.95, -2.6, sx * 0.9, -2.0]);
      paint(c, M.white);
    }
    circle(c, 0, 0, 1.45);
    paint(c, M.white);
    circle(c, 0, 0, 0.6);
    paint(c, M.pale, null);
    flash(c, 0, -6.7, 1.2, '#ffcf70', p.fireAge, 0.08, p.fireIndex);
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
    interceptor,
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
