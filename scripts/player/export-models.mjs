// Export the player's sprites as SVG files into `models/`.
//
//     node scripts/player/export-models.mjs [--check]
//
// The sprites are code: `crates/player/web/sprites.js` draws each unit and
// building on a canvas, its moving parts placed by a pose. This runs that file
// unchanged against a context that records the drawing as SVG instead of
// painting it, in each sprite's rest pose and the first team's colours, and
// writes `models/<sprite>.svg`. Lengths stay in metres, the sprite's front up
// the page. `--check` writes nothing and fails when a file differs from what
// the sprites draw now, which is how CI holds the directory to the code.
//
// Node from 18 on, nothing to install.

import { readFileSync, writeFileSync, readdirSync, unlinkSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import vm from 'node:vm';

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '../..');
const SPRITES = join(ROOT, 'crates/player/web/sprites.js');
const OUT = join(ROOT, 'models');
// The sprites the page draws, and whether each is a building.
const MODELS = [
  ['marksman', false], ['arclight', false], ['rhino', false], ['crawler', false],
  ['sledgehammer', false], ['wasp', false], ['energy_tower', true],
  ['research_center', true], ['anti_armor_turret', true], ['rapid_fire_turret', true],
  ['defensive_wall', true],
];
const PIXELS_PER_METRE = 16;

// Two decimals, a centimetre: finer than any sprite's detail, and coarse
// enough that the same drawing writes the same file everywhere.
function n(value) {
  const rounded = Math.round(value * 100) / 100;
  return Object.is(rounded, -0) ? '0' : String(rounded);
}

// A canvas colour as an SVG colour and its opacity.
function paint(colour) {
  const match = /^rgba?\(([^)]+)\)$/.exec(colour.replace(/\s+/g, ''));
  if (!match) return [colour, 1];
  const [r, g, b, a = '1'] = match[1].split(',');
  return [`rgb(${r},${g},${b})`, Number(a)];
}

class Gradient {
  constructor(x0, y0, r0, x1, y1, r1) {
    Object.assign(this, { x0, y0, r0, x1, y1, r1, stops: [] });
  }

  addColorStop(offset, colour) {
    this.stops.push([offset, colour]);
  }
}

// The part of CanvasRenderingContext2D the sprites use, writing SVG. Points
// are transformed as the path is built, as a canvas does; an arc is written
// as an SVG arc, which holds while every transform is a rotation, a
// translation and a uniform scale, the only ones the sprites make.
class SvgContext {
  constructor() {
    this.state = { m: [1, 0, 0, 1, 0, 0], alpha: 1, fillStyle: '#000', strokeStyle: '#000', lineWidth: 1, groups: 0 };
    this.stack = [];
    this.body = [];
    this.defs = [];
    this.ids = 0;
    this.path = [];
    this.current = null;
    this.start = null;
    this.box = [Infinity, Infinity, -Infinity, -Infinity];
    this.pathBox = [Infinity, Infinity, -Infinity, -Infinity];
    this.globalCompositeOperation = 'source-over';
    this.font = '';
    this.textAlign = 'start';
    this.textBaseline = 'alphabetic';
  }

  get fillStyle() { return this.state.fillStyle; }
  set fillStyle(v) { this.state.fillStyle = v; }
  get strokeStyle() { return this.state.strokeStyle; }
  set strokeStyle(v) { this.state.strokeStyle = v; }
  get lineWidth() { return this.state.lineWidth; }
  set lineWidth(v) { this.state.lineWidth = v; }
  get globalAlpha() { return this.state.alpha; }
  set globalAlpha(v) { this.state.alpha = v; }

  save() {
    this.stack.push(this.state);
    this.state = { ...this.state, m: this.state.m.slice(), groups: 0 };
  }

  restore() {
    for (let i = 0; i < this.state.groups; i++) this.body.push('</g>');
    this.state = this.stack.pop();
  }

  transform(a, b, c, d, e, f) {
    const [A, B, C, D, E, F] = this.state.m;
    this.state.m = [A * a + C * b, B * a + D * b, A * c + C * d, B * c + D * d, A * e + C * f + E, B * e + D * f + F];
  }

  translate(x, y) { this.transform(1, 0, 0, 1, x, y); }
  rotate(a) { this.transform(Math.cos(a), Math.sin(a), -Math.sin(a), Math.cos(a), 0, 0); }
  scale(x, y) { this.transform(x, 0, 0, y, 0, 0); }

  point(x, y) {
    const [a, b, c, d, e, f] = this.state.m;
    const p = [a * x + c * y + e, b * x + d * y + f];
    this.grow(p[0], p[1], 0);
    return p;
  }

  // How much the transform scales a length, and how far it turns an angle.
  scaleOf() {
    const [a, b, c, d] = this.state.m;
    return Math.sqrt(Math.abs(a * d - b * c));
  }

  // What the path being built covers; a fill or a stroke adds it to the
  // drawing's bounds, a clip does not.
  grow(x, y, r) {
    const box = this.pathBox;
    box[0] = Math.min(box[0], x - r);
    box[1] = Math.min(box[1], y - r);
    box[2] = Math.max(box[2], x + r);
    box[3] = Math.max(box[3], y + r);
  }

  beginPath() {
    this.path = [];
    this.current = null;
    this.pathBox = [Infinity, Infinity, -Infinity, -Infinity];
  }

  cover(margin) {
    const [x0, y0, x1, y1] = this.pathBox;
    const box = this.box;
    box[0] = Math.min(box[0], x0 - margin);
    box[1] = Math.min(box[1], y0 - margin);
    box[2] = Math.max(box[2], x1 + margin);
    box[3] = Math.max(box[3], y1 + margin);
  }

  moveTo(x, y) {
    const [X, Y] = this.point(x, y);
    this.path.push(`M${n(X)} ${n(Y)}`);
    this.current = [x, y];
    this.start = [x, y];
  }

  lineTo(x, y) {
    if (!this.current) return this.moveTo(x, y);
    const [X, Y] = this.point(x, y);
    this.path.push(`L${n(X)} ${n(Y)}`);
    this.current = [x, y];
  }

  closePath() {
    this.path.push('Z');
    this.current = this.start;
  }

  arcSegment(cx, cy, r, to, clockwise) {
    const [X, Y] = this.point(cx + r * Math.cos(to), cy + r * Math.sin(to));
    const R = r * this.scaleOf();
    this.path.push(`A${n(R)} ${n(R)} 0 0 ${clockwise ? 1 : 0} ${n(X)} ${n(Y)}`);
    this.current = [cx + r * Math.cos(to), cy + r * Math.sin(to)];
  }

  arc(cx, cy, r, a0, a1, anticlockwise = false) {
    const sx = cx + r * Math.cos(a0);
    const sy = cy + r * Math.sin(a0);
    if (this.current) this.lineTo(sx, sy); else this.moveTo(sx, sy);
    const turn = Math.PI * 2;
    let sweep = anticlockwise ? a0 - a1 : a1 - a0;
    sweep = sweep >= turn ? turn : ((sweep % turn) + turn) % turn;
    const [X, Y] = this.point(cx, cy);
    this.grow(X, Y, r * this.scaleOf());
    // no piece may pass half a turn, which an SVG arc could take either way
    const pieces = Math.max(1, Math.ceil(sweep / (Math.PI * 0.99)));
    for (let i = 1; i <= pieces; i++) {
      const at = a0 + (anticlockwise ? -1 : 1) * (sweep * i) / pieces;
      this.arcSegment(cx, cy, r, at, !anticlockwise);
    }
  }

  arcTo(x1, y1, x2, y2, r) {
    if (!this.current) this.moveTo(x1, y1);
    const [x0, y0] = this.current;
    const d1 = [x0 - x1, y0 - y1];
    const d2 = [x2 - x1, y2 - y1];
    const l1 = Math.hypot(...d1);
    const l2 = Math.hypot(...d2);
    const cross = d1[0] * d2[1] - d1[1] * d2[0];
    if (r === 0 || l1 === 0 || l2 === 0 || Math.abs(cross) < 1e-12) return this.lineTo(x1, y1);
    const u1 = [d1[0] / l1, d1[1] / l1];
    const u2 = [d2[0] / l2, d2[1] / l2];
    const angle = Math.acos(Math.max(-1, Math.min(1, u1[0] * u2[0] + u1[1] * u2[1])));
    const t = r / Math.tan(angle / 2);
    this.lineTo(x1 + u1[0] * t, y1 + u1[1] * t);
    const end = [x1 + u2[0] * t, y1 + u2[1] * t];
    const [X, Y] = this.point(end[0], end[1]);
    const R = r * this.scaleOf();
    // with y down the page, a positive turn is clockwise, and so is the arc
    const clockwise = (x1 - x0) * (y2 - y1) - (y1 - y0) * (x2 - x1) > 0;
    this.path.push(`A${n(R)} ${n(R)} 0 0 ${clockwise ? 1 : 0} ${n(X)} ${n(Y)}`);
    this.current = end;
  }

  fillRect(x, y, w, h) {
    this.beginPath();
    this.moveTo(x, y);
    this.lineTo(x + w, y);
    this.lineTo(x + w, y + h);
    this.lineTo(x, y + h);
    this.closePath();
    this.fill();
  }

  createRadialGradient(x0, y0, r0, x1, y1, r1) {
    return new Gradient(x0, y0, r0, x1, y1, r1);
  }

  // A fill or stroke as an SVG paint, defining a gradient where it is one.
  paintOf(style) {
    if (!(style instanceof Gradient)) return paint(style);
    const id = `g${++this.ids}`;
    const s = this.scaleOf();
    const [cx, cy] = this.point(style.x1, style.y1);
    const [fx, fy] = this.point(style.x0, style.y0);
    // SVG blends a gradient's stops without premultiplying their opacity, so
    // a stop fading to transparent black would grey the colour it leaves: it
    // takes its nearest visible neighbour's colour instead.
    const painted = style.stops.map(([offset, colour]) => [offset, ...paint(colour)]);
    const stops = painted.map(([offset, c, a], i) => {
      if (a === 0) {
        const near = [...painted.slice(0, i).reverse(), ...painted.slice(i + 1)].find((stop) => stop[2] > 0);
        if (near) c = near[1];
      }
      return `<stop offset="${n(offset)}" stop-color="${c}"${a === 1 ? '' : ` stop-opacity="${n(a)}"`}/>`;
    });
    this.defs.push(
      `<radialGradient id="${id}" gradientUnits="userSpaceOnUse" cx="${n(cx)}" cy="${n(cy)}" r="${n(style.r1 * s)}" fx="${n(fx)}" fy="${n(fy)}" fr="${n(style.r0 * s)}">${stops.join('')}</radialGradient>`,
    );
    return [`url(#${id})`, 1];
  }

  opacity(extra) {
    const a = this.state.alpha * extra;
    return a === 1 ? '' : ` opacity="${n(a)}"`;
  }

  fill() {
    if (!this.path.length || this.state.alpha <= 0) return;
    const [colour, a] = this.paintOf(this.state.fillStyle);
    this.cover(0);
    this.body.push(`<path d="${this.path.join('')}" fill="${colour}"${this.opacity(a)}/>`);
  }

  stroke() {
    if (!this.path.length || this.state.alpha <= 0) return;
    const [colour, a] = this.paintOf(this.state.strokeStyle);
    const width = this.state.lineWidth * this.scaleOf();
    this.cover(width / 2);
    this.body.push(`<path d="${this.path.join('')}" fill="none" stroke="${colour}" stroke-width="${n(width)}"${this.opacity(a)}/>`);
  }

  clip() {
    const id = `c${++this.ids}`;
    this.defs.push(`<clipPath id="${id}"><path d="${this.path.join('')}"/></clipPath>`);
    this.body.push(`<g clip-path="url(#${id})">`);
    this.state.groups += 1;
  }

  fillText() {
    throw new Error('a sprite with text has no model to export');
  }

  svg(title) {
    const pad = 0.25;
    const [x0, y0, x1, y1] = this.box;
    const [x, y, w, h] = [x0 - pad, y0 - pad, x1 - x0 + 2 * pad, y1 - y0 + 2 * pad];
    return [
      `<svg xmlns="http://www.w3.org/2000/svg" viewBox="${n(x)} ${n(y)} ${n(w)} ${n(h)}" width="${Math.round(w * PIXELS_PER_METRE)}" height="${Math.round(h * PIXELS_PER_METRE)}">`,
      `<title>${title}</title>`,
      `<defs>${this.defs.join('')}</defs>`,
      ...this.body,
      '</svg>',
      '',
    ].join('\n');
  }
}

function load() {
  const sandbox = { Math };
  const code = readFileSync(SPRITES, 'utf8');
  return vm.runInNewContext(`${code}\nSprites;`, sandbox, { filename: SPRITES });
}

function main() {
  const check = process.argv.includes('--check');
  const sprites = load();
  // the outline width the page draws at 10 pixels to the metre
  sprites.setScale(0.1);
  const files = new Map();
  for (const [kind, building] of MODELS) {
    // in blue, the first team's colours; the other team's only differ there
    const c = new SvgContext();
    const pose = sprites.pose();
    const [width, depth] = sprites.SIZE[kind];
    if (building) sprites.drawBuilding(c, kind, pose, width, depth);
    else sprites.drawUnit(c, kind, pose, 4);
    files.set(`${kind}.svg`, c.svg(kind));
  }
  const stale = [];
  const present = new Set(readdirSync(OUT).filter((file) => file.endsWith('.svg')));
  for (const [file, text] of files) {
    const path = join(OUT, file);
    let old = null;
    try { old = readFileSync(path, 'utf8'); } catch { /* not written yet */ }
    if (old !== text) {
      stale.push(file);
      if (!check) writeFileSync(path, text);
    }
    present.delete(file);
  }
  for (const file of present) {
    stale.push(file);
    if (!check) unlinkSync(join(OUT, file));
  }
  if (check && stale.length) {
    console.error(`models/ is not what crates/player/web/sprites.js draws: ${stale.join(', ')}`);
    console.error('run: node scripts/player/export-models.mjs');
    process.exit(1);
  }
  console.log(`${files.size} models${check ? ' match the sprites' : ` written, ${stale.length} changed`}`);
}

main();
