// The Canvas drawing vocabulary shared by model exports and battle snapshots.
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
class SnapshotContext {
  constructor() {
    this.state = { m: [1, 0, 0, 1, 0, 0], alpha: 1, fillStyle: '#000', strokeStyle: '#000', lineWidth: 1, groups: 0,
      lineCap: 'butt', lineJoin: 'miter', dash: [], globalCompositeOperation: 'source-over', font: '10px sans-serif', textAlign: 'start', textBaseline: 'alphabetic' };
    this.prefix = '';
    this.stack = [];
    this.body = [];
    this.defs = [];
    this.ids = 0;
    this.path = [];
    this.current = null;
    this.start = null;
    this.box = [Infinity, Infinity, -Infinity, -Infinity];
    this.pathBox = [Infinity, Infinity, -Infinity, -Infinity];
  }

  get fillStyle() { return this.state.fillStyle; }
  set fillStyle(v) { this.state.fillStyle = v; }
  get strokeStyle() { return this.state.strokeStyle; }
  set strokeStyle(v) { this.state.strokeStyle = v; }
  get lineWidth() { return this.state.lineWidth; }
  set lineWidth(v) { this.state.lineWidth = v; }
  get globalAlpha() { return this.state.alpha; }
  set globalAlpha(v) { this.state.alpha = v; }
  get globalCompositeOperation() { return this.state.globalCompositeOperation; }
  set globalCompositeOperation(v) { this.state.globalCompositeOperation = v; }
  get lineCap() { return this.state.lineCap; }
  set lineCap(v) { this.state.lineCap = v; }
  get lineJoin() { return this.state.lineJoin; }
  set lineJoin(v) { this.state.lineJoin = v; }
  get font() { return this.state.font; }
  set font(v) { this.state.font = v; }
  get textAlign() { return this.state.textAlign; }
  set textAlign(v) { this.state.textAlign = v; }
  get textBaseline() { return this.state.textBaseline; }
  set textBaseline(v) { this.state.textBaseline = v; }
  setLineDash(v) { this.state.dash = v.slice(); }
  setTransform(...m) { this.state.m = m; }

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
    const old = [this.path, this.current, this.start, this.pathBox];
    this.beginPath();
    this.moveTo(x, y);
    this.lineTo(x + w, y);
    this.lineTo(x + w, y + h);
    this.lineTo(x, y + h);
    this.closePath();
    this.fill();
    [this.path, this.current, this.start, this.pathBox] = old;
  }

  strokeRect(x, y, w, h) {
    const old = [this.path, this.current, this.start, this.pathBox];
    this.beginPath();
    this.moveTo(x, y); this.lineTo(x + w, y); this.lineTo(x + w, y + h); this.lineTo(x, y + h);
    this.closePath(); this.stroke();
    [this.path, this.current, this.start, this.pathBox] = old;
  }

  ellipse(x, y, rx, ry, rotation, a0, a1, anticlockwise = false) {
    const turn = Math.PI * 2;
    let sweep = anticlockwise ? a0 - a1 : a1 - a0;
    sweep = sweep >= turn ? turn : ((sweep % turn) + turn) % turn;
    const at = (a) => [x + rx * Math.cos(a) * Math.cos(rotation) - ry * Math.sin(a) * Math.sin(rotation),
      y + rx * Math.cos(a) * Math.sin(rotation) + ry * Math.sin(a) * Math.cos(rotation)];
    const first = at(a0);
    if (this.current) this.lineTo(...first); else this.moveTo(...first);
    const parts = Math.max(1, Math.ceil(sweep / (Math.PI / 2)));
    for (let i = 0; i < parts; i++) {
      const a = a0 + (anticlockwise ? -1 : 1) * sweep * i / parts;
      const b = a0 + (anticlockwise ? -1 : 1) * sweep * (i + 1) / parts;
      const k = 4 / 3 * Math.tan((b - a) / 4);
      const derivative = (t) => [-rx * Math.sin(t) * Math.cos(rotation) - ry * Math.cos(t) * Math.sin(rotation),
        -rx * Math.sin(t) * Math.sin(rotation) + ry * Math.cos(t) * Math.cos(rotation)];
      const p = at(a), q = at(b), dp = derivative(a), dq = derivative(b);
      const c1 = this.point(p[0] + k * dp[0], p[1] + k * dp[1]);
      const c2 = this.point(q[0] - k * dq[0], q[1] - k * dq[1]);
      const end = this.point(...q);
      this.path.push(`C${c1.map(n).join(' ')} ${c2.map(n).join(' ')} ${end.map(n).join(' ')}`);
      this.current = q;
    }
  }

  createLinearGradient(x0, y0, x1, y1) {
    const gradient = new Gradient(x0, y0, 0, x1, y1, 0);
    gradient.linear = true;
    return gradient;
  }

  createRadialGradient(x0, y0, r0, x1, y1, r1) {
    return new Gradient(x0, y0, r0, x1, y1, r1);
  }

  // A fill or stroke as an SVG paint, defining a gradient where it is one.
  paintOf(style) {
    if (!(style instanceof Gradient)) return paint(style);
    const id = `${this.prefix}g${++this.ids}`;
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
    if (style.linear) this.defs.push(`<linearGradient id="${id}" gradientUnits="userSpaceOnUse" x1="${n(fx)}" y1="${n(fy)}" x2="${n(cx)}" y2="${n(cy)}">${stops.join('')}</linearGradient>`);
    else this.defs.push(
      `<radialGradient id="${id}" gradientUnits="userSpaceOnUse" cx="${n(cx)}" cy="${n(cy)}" r="${n(style.r1 * s)}" fx="${n(fx)}" fy="${n(fy)}" fr="${n(style.r0 * s)}">${stops.join('')}</radialGradient>`,
    );
    return [`url(#${id})`, 1];
  }

  opacity(extra, blend = true) {
    const a = this.state.alpha * extra;
    return (a === 1 ? '' : ` opacity="${n(a)}"`) +
      (blend && this.globalCompositeOperation === 'lighter' ? ' style="mix-blend-mode:plus-lighter"' : '');
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
    const cap = this.lineCap === 'butt' ? '' : ` stroke-linecap="${this.lineCap}"`;
    const join = this.lineJoin === 'miter' ? '' : ` stroke-linejoin="${this.lineJoin}"`;
    const dash = this.state.dash.length ? ` stroke-dasharray="${this.state.dash.map(v => n(v * this.scaleOf())).join(' ')}"` : '';
    this.body.push(`<path d="${this.path.join('')}" fill="none" stroke="${colour}" stroke-width="${n(width)}"${cap}${join}${dash}${this.opacity(a)}/>`);
  }

  clip() {
    const id = `${this.prefix}c${++this.ids}`;
    this.defs.push(`<clipPath id="${id}"><path d="${this.path.join('')}"/></clipPath>`);
    this.body.push(`<g clip-path="url(#${id})">`);
    this.state.groups += 1;
  }

  fillText(text, x, y) {
    const anchor = { center: 'middle', right: 'end', end: 'end' }[this.textAlign] || 'start';
    const baseline = { middle: 'central', top: 'text-before-edge', bottom: 'text-after-edge' }[this.textBaseline] || 'alphabetic';
    const [colour, a] = this.paintOf(this.fillStyle);
    const blend = this.globalCompositeOperation === 'lighter' ? ';mix-blend-mode:plus-lighter' : '';
    this.body.push(`<text x="${n(x)}" y="${n(y)}" transform="matrix(${this.state.m.map(n).join(' ')})" text-anchor="${anchor}" dominant-baseline="${baseline}" style="font:${SnapshotContext.escape(this.font)}${blend}" fill="${colour}"${this.opacity(a, false)}>${SnapshotContext.escape(text)}</text>`);
  }

  drawImage(source, x, y) {
    if (!(source instanceof SnapshotContext)) throw new Error('SVG export requires a vector image');
    this.body.push(`<g transform="matrix(${this.state.m.map(n).join(' ')})"${this.opacity(1)}><svg x="${n(x)}" y="${n(y)}" width="${source.width}" height="${source.height}" viewBox="0 0 ${source.width} ${source.height}" overflow="hidden"><defs>${source.defs.join('')}</defs>${source.body.join('')}</svg></g>`);
  }

  static escape(value) {
    return String(value).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
  }

  snapshot(title, width, height) {
    const pad = 0.25;
    const [x0, y0, x1, y1] = this.box;
    const [x, y, w, h] = width === undefined ? [x0 - pad, y0 - pad, x1 - x0 + 2 * pad, y1 - y0 + 2 * pad] : [0, 0, width, height];
    return [
      `<svg xmlns="http://www.w3.org/2000/svg" viewBox="${n(x)} ${n(y)} ${n(w)} ${n(h)}" width="${Math.round(width === undefined ? w * 16 : w)}" height="${Math.round(height === undefined ? h * 16 : h)}">`,
      `<title>${SnapshotContext.escape(title)}</title>`,
      `<defs>${this.defs.join('')}</defs>`,
      ...this.body,
      '</svg>',
      '',
    ].join('\n');
  }
}
