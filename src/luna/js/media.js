// Pictures and sound for plugins: <canvas> 2D contexts and Path2D, a
// WebGL2 context that records what it draws, Image/<img> loading,
// createImageBitmap, and Web Audio (AudioContext, AnalyserNode, channel
// splitters, media stream sources) fed by a tap on Tidalfast's output.
// The document also gets a <video> in <head> standing in for TIDAL's
// player element: its state follows playback and it fires the media
// events TIDAL's element would.
//
// Pixels live on the native side (src/luna/canvas.rs, `__canvas`); paths
// are sent there in device space. A canvas publishes what it shows after
// the animation frame that drew on it, and the UI draws it
// (src/ui/luna_canvas.rs).
"use strict";
(() => {
	const g = globalThis;
	const L = g.__lunaInternal;
	const N = g.__canvas;
	const M = (L.media = {});
	const TAU = Math.PI * 2;
	const reg = typeof g.FinalizationRegistry === "function" ? new g.FinalizationRegistry((id) => N.free(id)) : null;
	const domErr = (msg, name) => new (g.DOMException ?? Error)(msg, name);

	// ---------------------------------------------------------------- colours
	const NAMED = {
		black: "000000", white: "ffffff", red: "ff0000", green: "008000", lime: "00ff00", blue: "0000ff", yellow: "ffff00", cyan: "00ffff", aqua: "00ffff", magenta: "ff00ff", fuchsia: "ff00ff",
		gray: "808080", grey: "808080", silver: "c0c0c0", maroon: "800000", olive: "808000", purple: "800080", teal: "008080", navy: "000080", orange: "ffa500", pink: "ffc0cb", hotpink: "ff69b4",
		deeppink: "ff1493", gold: "ffd700", brown: "a52a2a", violet: "ee82ee", indigo: "4b0082", coral: "ff7f50", tomato: "ff6347", salmon: "fa8072", khaki: "f0e68c", crimson: "dc143c", orchid: "da70d6",
		plum: "dda0dd", turquoise: "40e0d0", skyblue: "87ceeb", steelblue: "4682b4", royalblue: "4169e1", dodgerblue: "1e90ff", deepskyblue: "00bfff", lightblue: "add8e6", darkblue: "00008b",
		darkgreen: "006400", lightgreen: "90ee90", limegreen: "32cd32", seagreen: "2e8b57", springgreen: "00ff7f", darkred: "8b0000", darkorange: "ff8c00", lightgray: "d3d3d3", lightgrey: "d3d3d3",
		darkgray: "a9a9a9", darkgrey: "a9a9a9", dimgray: "696969", dimgrey: "696969", whitesmoke: "f5f5f5", gainsboro: "dcdcdc", beige: "f5f5dc", ivory: "fffff0", lavender: "e6e6fa", slategray: "708090",
		chartreuse: "7fff00", tan: "d2b48c", chocolate: "d2691e", firebrick: "b22222", mediumpurple: "9370db", rebeccapurple: "663399", lightpink: "ffb6c1", aquamarine: "7fffd4", midnightblue: "191970",
	};
	const num = (s, max) => {
		s = s.trim();
		if (s.endsWith("%")) return (parseFloat(s) / 100) * max;
		return parseFloat(s);
	};
	const hue = (s) => {
		s = s.trim();
		const v = parseFloat(s);
		if (s.endsWith("turn")) return v * 360;
		if (s.endsWith("rad")) return (v * 180) / Math.PI;
		return v;
	};
	const hsl = (h, s, l) => {
		h = (((h % 360) + 360) % 360) / 360;
		const f = (n) => {
			const k = (n + h * 12) % 12;
			return l - s * Math.min(l, 1 - l) * Math.max(-1, Math.min(k - 3, 9 - k, 1));
		};
		return [f(0), f(8), f(4)];
	};
	const colorCache = new Map();
	// A CSS colour as [r, g, b, a] in 0..1, or null.
	const parseColor = (v) => {
		const key = String(v);
		let c = colorCache.get(key);
		if (c !== undefined) return c;
		c = parseColorUncached(key.trim().toLowerCase());
		if (colorCache.size > 512) colorCache.clear();
		colorCache.set(key, c);
		return c;
	};
	const parseColorUncached = (s) => {
		if (s === "transparent") return [0, 0, 0, 0];
		if (s === "currentcolor") return [0, 0, 0, 1];
		if (NAMED[s]) s = "#" + NAMED[s];
		if (s[0] === "#") {
			let h = s.slice(1);
			if (!/^[0-9a-f]+$/.test(h)) return null;
			if (h.length === 3 || h.length === 4) h = [...h].map((x) => x + x).join("");
			if (h.length !== 6 && h.length !== 8) return null;
			const p = (i) => parseInt(h.slice(i, i + 2), 16) / 255;
			return [p(0), p(2), p(4), h.length === 8 ? p(6) : 1];
		}
		const m = /^(rgba?|hsla?)\((.*)\)$/.exec(s);
		if (!m) return null;
		const parts = m[2].split(/[\s,/]+/).filter(Boolean);
		if (parts.length < 3) return null;
		const a = parts.length > 3 ? Math.max(0, Math.min(1, num(parts[3], 1))) : 1;
		if (m[1].startsWith("rgb")) {
			const [r, gg, b] = parts.slice(0, 3).map((x) => Math.max(0, Math.min(255, num(x, 255))) / 255);
			return [r, gg, b, a].some(Number.isNaN) ? null : [r, gg, b, a];
		}
		const rgb = hsl(hue(parts[0]), num(parts[1], 1) / (parts[1].endsWith("%") ? 1 : 100), num(parts[2], 1) / (parts[2].endsWith("%") ? 1 : 100));
		return [...rgb, a].some(Number.isNaN) ? null : [...rgb, a];
	};
	M.parseColor = parseColor;
	const colorString = (c) => {
		const h = (x) => Math.round(x * 255).toString(16).padStart(2, "0");
		return c[3] >= 1 ? `#${h(c[0])}${h(c[1])}${h(c[2])}` : `rgba(${Math.round(c[0] * 255)}, ${Math.round(c[1] * 255)}, ${Math.round(c[2] * 255)}, ${+c[3].toFixed(3)})`;
	};

	// ---------------------------------------------------------------- matrices
	// [a, b, c, d, e, f]: x' = a x + c y + e, y' = b x + d y + f.
	const mul = (m, n) => [m[0] * n[0] + m[2] * n[1], m[1] * n[0] + m[3] * n[1], m[0] * n[2] + m[2] * n[3], m[1] * n[2] + m[3] * n[3], m[0] * n[4] + m[2] * n[5] + m[4], m[1] * n[4] + m[3] * n[5] + m[5]];
	const ID = [1, 0, 0, 1, 0, 0];
	const finite = (...v) => v.every(Number.isFinite);

	// ---------------------------------------------------------------- paths
	// Ops (numbers): 0 moveTo x y, 1 lineTo x y, 2 quadTo cx cy x y,
	// 3 cubicTo c1x c1y c2x c2y x y, 4 close. Points go through `m` (the
	// context's transform, or none for Path2D) as they are added; the
	// current point is kept in the caller's coordinates.
	class PathRec {
		constructor() {
			this.ops = [];
			this.has = false;
			this.cx = this.cy = this.sx = this.sy = 0;
		}
		pt(m, x, y) {
			return m ? [m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5]] : [x, y];
		}
		move(m, x, y) {
			if (!finite(x, y)) return;
			const [a, b] = this.pt(m, x, y);
			this.ops.push(0, a, b);
			this.has = true;
			this.cx = this.sx = x;
			this.cy = this.sy = y;
		}
		ensure(m, x, y) {
			if (!this.has) this.move(m, x, y);
		}
		line(m, x, y) {
			if (!finite(x, y)) return;
			if (!this.has) return this.move(m, x, y);
			const [a, b] = this.pt(m, x, y);
			this.ops.push(1, a, b);
			this.cx = x;
			this.cy = y;
		}
		quad(m, cx, cy, x, y) {
			if (!finite(cx, cy, x, y)) return;
			this.ensure(m, cx, cy);
			const [a, b] = this.pt(m, cx, cy);
			const [c, d] = this.pt(m, x, y);
			this.ops.push(2, a, b, c, d);
			this.cx = x;
			this.cy = y;
		}
		cubic(m, c1x, c1y, c2x, c2y, x, y) {
			if (!finite(c1x, c1y, c2x, c2y, x, y)) return;
			this.ensure(m, c1x, c1y);
			const [a, b] = this.pt(m, c1x, c1y);
			const [c, d] = this.pt(m, c2x, c2y);
			const [e, f] = this.pt(m, x, y);
			this.ops.push(3, a, b, c, d, e, f);
			this.cx = x;
			this.cy = y;
		}
		close() {
			if (!this.has) return;
			this.ops.push(4);
			this.cx = this.sx;
			this.cy = this.sy;
		}
		rect(m, x, y, w, h) {
			if (!finite(x, y, w, h)) return;
			this.move(m, x, y);
			this.line(m, x + w, y);
			this.line(m, x + w, y + h);
			this.line(m, x, y + h);
			this.close();
			this.move(m, x, y);
		}
		// Elliptical arc as cubic segments of at most a quarter turn.
		ellipse(m, x, y, rx, ry, rot, a0, a1, ccw) {
			if (!finite(x, y, rx, ry, rot, a0, a1)) return;
			if (rx < 0 || ry < 0) throw domErr("The radius provided is negative.", "IndexSizeError");
			let sweep = a1 - a0;
			if (!ccw) sweep = sweep >= TAU ? TAU : ((sweep % TAU) + TAU) % TAU;
			else sweep = -sweep >= TAU ? -TAU : -((((-sweep) % TAU) + TAU) % TAU);
			const cr = Math.cos(rot),
				sr = Math.sin(rot);
			const at = (t) => {
				const ex = rx * Math.cos(t),
					ey = ry * Math.sin(t);
				return [x + ex * cr - ey * sr, y + ex * sr + ey * cr];
			};
			const d = (t) => {
				const ex = -rx * Math.sin(t),
					ey = ry * Math.cos(t);
				return [ex * cr - ey * sr, ex * sr + ey * cr];
			};
			const [sx, sy] = at(a0);
			if (this.has) this.line(m, sx, sy);
			else this.move(m, sx, sy);
			const n = Math.max(1, Math.ceil(Math.abs(sweep) / (Math.PI / 2) - 1e-9));
			const step = sweep / n;
			const k = (4 / 3) * Math.tan(step / 4);
			let t = a0;
			for (let i = 0; i < n; i++) {
				const t2 = t + step;
				const [p0x, p0y] = at(t),
					[p1x, p1y] = at(t2),
					[d0x, d0y] = d(t),
					[d1x, d1y] = d(t2);
				this.cubic(m, p0x + k * d0x, p0y + k * d0y, p1x - k * d1x, p1y - k * d1y, p1x, p1y);
				t = t2;
			}
		}
		arcTo(m, x1, y1, x2, y2, r) {
			if (!finite(x1, y1, x2, y2, r)) return;
			if (r < 0) throw domErr("The radius provided is negative.", "IndexSizeError");
			if (!this.has) return this.move(m, x1, y1);
			const x0 = this.cx,
				y0 = this.cy;
			const v1x = x0 - x1,
				v1y = y0 - y1,
				v2x = x2 - x1,
				v2y = y2 - y1;
			const l1 = Math.hypot(v1x, v1y),
				l2 = Math.hypot(v2x, v2y);
			const cross = v1x * v2y - v1y * v2x;
			if (r === 0 || l1 === 0 || l2 === 0 || Math.abs(cross) < 1e-9) return this.line(m, x1, y1);
			const cos = (v1x * v2x + v1y * v2y) / (l1 * l2);
			const angle = Math.acos(Math.max(-1, Math.min(1, cos)));
			const dist = r / Math.tan(angle / 2);
			const t1x = x1 + (v1x / l1) * dist,
				t1y = y1 + (v1y / l1) * dist,
				t2x = x1 + (v2x / l2) * dist,
				t2y = y1 + (v2y / l2) * dist;
			// The centre is along the bisector, r / sin(angle/2) from the corner.
			const bx = v1x / l1 + v2x / l2,
				by = v1y / l1 + v2y / l2;
			const bl = Math.hypot(bx, by);
			const cd = r / Math.sin(angle / 2);
			const ccx = x1 + (bx / bl) * cd,
				ccy = y1 + (by / bl) * cd;
			const a0 = Math.atan2(t1y - ccy, t1x - ccx),
				a1 = Math.atan2(t2y - ccy, t2x - ccx);
			this.line(m, t1x, t1y);
			this.ellipse(m, ccx, ccy, r, r, 0, a0, a1, cross > 0);
		}
		roundRect(m, x, y, w, h, radii) {
			if (!finite(x, y, w, h)) return;
			let r = radii === undefined ? [0] : Array.isArray(radii) || (radii && typeof radii === "object" && typeof radii.length === "number") ? [...radii] : [radii];
			r = r.map((v) => (typeof v === "object" && v ? [+v.x || 0, +v.y || 0] : [+v || 0, +v || 0]));
			if (r.length < 1 || r.length > 4) throw domErr("Bad radii", "RangeError");
			const [tl, tr, br, bl] = r.length === 1 ? [r[0], r[0], r[0], r[0]] : r.length === 2 ? [r[0], r[1], r[0], r[1]] : r.length === 3 ? [r[0], r[1], r[2], r[1]] : r;
			// Scale down radii that overlap (as the spec does).
			const aw = Math.abs(w),
				ah = Math.abs(h);
			const f = Math.min(1, aw / (tl[0] + tr[0] || 1), aw / (bl[0] + br[0] || 1), ah / (tl[1] + bl[1] || 1), ah / (tr[1] + br[1] || 1));
			const s = (p) => [p[0] * f, p[1] * f];
			const [a, b, c, d] = [s(tl), s(tr), s(br), s(bl)];
			const sx = Math.sign(w) || 1,
				sy = Math.sign(h) || 1;
			this.move(m, x + a[0] * sx, y);
			this.line(m, x + w - b[0] * sx, y);
			if (b[0] && b[1]) this.ellipse(m, x + w - b[0] * sx, y + b[1] * sy, b[0], b[1], 0, -Math.PI / 2, 0, false);
			this.line(m, x + w, y + h - c[1] * sy);
			if (c[0] && c[1]) this.ellipse(m, x + w - c[0] * sx, y + h - c[1] * sy, c[0], c[1], 0, 0, Math.PI / 2, false);
			this.line(m, x + d[0] * sx, y + h);
			if (d[0] && d[1]) this.ellipse(m, x + d[0] * sx, y + h - d[1] * sy, d[0], d[1], 0, Math.PI / 2, Math.PI, false);
			this.line(m, x, y + a[1] * sy);
			if (a[0] && a[1]) this.ellipse(m, x + a[0] * sx, y + a[1] * sy, a[0], a[1], 0, Math.PI, Math.PI * 1.5, false);
			this.close();
			this.move(m, x, y);
		}
	}
	// Ops through matrix `m` (Path2D drawn by a context).
	const mapOps = (ops, m) => {
		const out = new Float32Array(ops.length);
		for (let i = 0; i < ops.length; ) {
			const op = ops[i];
			out[i] = op;
			const n = op === 0 || op === 1 ? 2 : op === 2 ? 4 : op === 3 ? 6 : 0;
			for (let k = 0; k < n; k += 2) {
				const x = ops[i + 1 + k],
					y = ops[i + 2 + k];
				out[i + 1 + k] = m[0] * x + m[2] * y + m[4];
				out[i + 2 + k] = m[1] * x + m[3] * y + m[5];
			}
			i += 1 + n;
		}
		return out.buffer;
	};

	// SVG path data (for `new Path2D("M ...")`).
	const svgPath = (p, d) => {
		const toks = String(d).match(/[a-df-z]|[-+]?(?:\d*\.\d+|\d+\.?)(?:e[-+]?\d+)?/gi) ?? [];
		let i = 0,
			cmd = "",
			x = 0,
			y = 0,
			lcx = 0,
			lcy = 0,
			lq = null;
		const n = () => parseFloat(toks[i++]);
		const more = () => i < toks.length && !/^[a-z]$/i.test(toks[i]);
		while (i < toks.length) {
			if (/^[a-z]$/i.test(toks[i])) cmd = toks[i++];
			else if (!cmd) break;
			const rel = cmd === cmd.toLowerCase();
			const C = cmd.toUpperCase();
			const ox = rel ? x : 0,
				oy = rel ? y : 0;
			let prevCubic = null;
			switch (C) {
				case "M":
					x = ox + n();
					y = oy + n();
					p.move(null, x, y);
					cmd = rel ? "l" : "L";
					break;
				case "L":
					x = ox + n();
					y = oy + n();
					p.line(null, x, y);
					break;
				case "H":
					x = ox + n();
					p.line(null, x, y);
					break;
				case "V":
					y = oy + n();
					p.line(null, x, y);
					break;
				case "C": {
					const a = ox + n(),
						b = oy + n(),
						c = ox + n(),
						e = oy + n();
					x = ox + n();
					y = oy + n();
					p.cubic(null, a, b, c, e, x, y);
					prevCubic = [c, e];
					break;
				}
				case "S": {
					const a = 2 * x - lcx,
						b = 2 * y - lcy;
					const c = ox + n(),
						e = oy + n();
					x = ox + n();
					y = oy + n();
					p.cubic(null, a, b, c, e, x, y);
					prevCubic = [c, e];
					break;
				}
				case "Q": {
					const a = ox + n(),
						b = oy + n();
					x = ox + n();
					y = oy + n();
					p.quad(null, a, b, x, y);
					lq = [a, b];
					break;
				}
				case "T": {
					const [a, b] = lq ? [2 * x - lq[0], 2 * y - lq[1]] : [x, y];
					x = ox + n();
					y = oy + n();
					p.quad(null, a, b, x, y);
					lq = [a, b];
					break;
				}
				case "A": {
					let rx = Math.abs(n()),
						ry = Math.abs(n());
					const phi = (n() * Math.PI) / 180,
						large = !!n(),
						sweep = !!n();
					const x2 = ox + n(),
						y2 = oy + n();
					if (!rx || !ry) p.line(null, x2, y2);
					else {
						const cp = Math.cos(phi),
							sp = Math.sin(phi);
						const dx = (x - x2) / 2,
							dy = (y - y2) / 2;
						const x1p = cp * dx + sp * dy,
							y1p = -sp * dx + cp * dy;
						const lam = (x1p * x1p) / (rx * rx) + (y1p * y1p) / (ry * ry);
						if (lam > 1) {
							rx *= Math.sqrt(lam);
							ry *= Math.sqrt(lam);
						}
						const num2 = rx * rx * ry * ry - rx * rx * y1p * y1p - ry * ry * x1p * x1p;
						const den = rx * rx * y1p * y1p + ry * ry * x1p * x1p;
						let co = Math.sqrt(Math.max(0, num2 / den));
						if (large === sweep) co = -co;
						const cxp = (co * rx * y1p) / ry,
							cyp = (-co * ry * x1p) / rx;
						const cx = cp * cxp - sp * cyp + (x + x2) / 2,
							cy = sp * cxp + cp * cyp + (y + y2) / 2;
						const ang = (ux, uy, vx, vy) => Math.atan2(ux * vy - uy * vx, ux * vx + uy * vy);
						const t1 = ang(1, 0, (x1p - cxp) / rx, (y1p - cyp) / ry);
						let dt = ang((x1p - cxp) / rx, (y1p - cyp) / ry, (-x1p - cxp) / rx, (-y1p - cyp) / ry);
						if (!sweep && dt > 0) dt -= TAU;
						if (sweep && dt < 0) dt += TAU;
						p.ellipse(null, cx, cy, rx, ry, phi, t1, t1 + dt, dt < 0);
					}
					x = x2;
					y = y2;
					break;
				}
				case "Z":
					p.close();
					x = p.cx;
					y = p.cy;
					break;
				default:
					i++;
			}
			if (C !== "Q" && C !== "T") lq = null;
			if (prevCubic) [lcx, lcy] = prevCubic;
			else [lcx, lcy] = [x, y];
			if (C === "Z" && !more()) continue;
		}
	};

	class Path2D {
		constructor(src) {
			this._p = new PathRec();
			if (src instanceof Path2D) {
				this._p.ops = src._p.ops.slice();
				Object.assign(this._p, { has: src._p.has, cx: src._p.cx, cy: src._p.cy, sx: src._p.sx, sy: src._p.sy });
			} else if (typeof src === "string") svgPath(this._p, src);
		}
		addPath(p, t) {
			if (!(p instanceof Path2D)) return;
			const m = t ? [t.a ?? 1, t.b ?? 0, t.c ?? 0, t.d ?? 1, t.e ?? 0, t.f ?? 0] : ID;
			for (const v of new Float32Array(mapOps(p._p.ops, m))) this._p.ops.push(v);
			this._p.has ||= p._p.has;
		}
		closePath() {
			this._p.close();
		}
		moveTo(x, y) {
			this._p.move(null, +x, +y);
		}
		lineTo(x, y) {
			this._p.line(null, +x, +y);
		}
		quadraticCurveTo(a, b, x, y) {
			this._p.quad(null, +a, +b, +x, +y);
		}
		bezierCurveTo(a, b, c, d, x, y) {
			this._p.cubic(null, +a, +b, +c, +d, +x, +y);
		}
		arc(x, y, r, a0, a1, ccw) {
			this._p.ellipse(null, +x, +y, +r, +r, 0, +a0, +a1, !!ccw);
		}
		ellipse(x, y, rx, ry, rot, a0, a1, ccw) {
			this._p.ellipse(null, +x, +y, +rx, +ry, +rot, +a0, +a1, !!ccw);
		}
		arcTo(a, b, c, d, r) {
			this._p.arcTo(null, +a, +b, +c, +d, +r);
		}
		rect(x, y, w, h) {
			this._p.rect(null, +x, +y, +w, +h);
		}
		roundRect(x, y, w, h, r) {
			this._p.roundRect(null, +x, +y, +w, +h, r);
		}
	}
	g.Path2D = Path2D;

	// ---------------------------------------------------------------- styles
	class CanvasGradient {
		constructor(kind, args) {
			this._kind = kind;
			this._args = args;
			this._stops = [];
		}
		addColorStop(offset, color) {
			offset = +offset;
			if (!(offset >= 0 && offset <= 1)) throw domErr("The provided value is outside the range [0, 1].", "IndexSizeError");
			const c = parseColor(color);
			if (!c) throw new SyntaxError(`'${color}' could not be parsed as a color.`);
			this._stops.push([offset, c]);
			this._stops.sort((a, b) => a[0] - b[0]);
		}
	}
	class CanvasPattern {
		constructor(src, rep) {
			this._src = src;
			this._rep = { repeat: 0, "repeat-x": 1, "repeat-y": 2, "no-repeat": 3 }[rep || "repeat"] ?? 0;
			this._m = ID;
		}
		setTransform(t) {
			this._m = t ? [t.a ?? 1, t.b ?? 0, t.c ?? 0, t.d ?? 1, t.e ?? 0, t.f ?? 0] : ID;
		}
	}
	g.CanvasGradient = CanvasGradient;
	g.CanvasPattern = CanvasPattern;
	// A fill/stroke style as numbers for the native side (see `shader` in
	// canvas.rs).
	const encodePaint = (style, alpha, m) => {
		if (style instanceof CanvasGradient) {
			const st = [style._stops.length];
			for (const [o, c] of style._stops) st.push(o, c[0], c[1], c[2], c[3] * alpha);
			if (style._kind === "linear") return new Float32Array([1, ...style._args, ...m, ...st]).buffer;
			if (style._kind === "radial") return new Float32Array([2, ...style._args, ...m, ...st]).buffer;
			// Conic gradients are drawn as their middle colour.
			const mid = style._stops[style._stops.length >> 1]?.[1] ?? [0, 0, 0, 0];
			return new Float32Array([0, mid[0], mid[1], mid[2], mid[3] * alpha]).buffer;
		}
		if (style instanceof CanvasPattern) {
			const id = surfOf(style._src);
			if (id == null) return new Float32Array([0, 0, 0, 0, 0]).buffer;
			return new Float32Array([3, id, style._rep, ...mul(m, style._m), alpha]).buffer;
		}
		const c = style;
		return new Float32Array([0, c[0], c[1], c[2], c[3] * alpha]).buffer;
	};
	const COMPOSITE = ["source-over", "source-in", "source-out", "source-atop", "destination-over", "destination-in", "destination-out", "destination-atop", "lighter", "copy", "xor", "multiply", "screen", "overlay", "darken", "lighten", "color-dodge", "color-burn", "hard-light", "soft-light", "difference", "exclusion", "hue", "saturation", "color", "luminosity"];
	const CLEAR = 26;

	// The native surface behind an image source, or null.
	const surfOf = (src) => {
		if (!src) return null;
		if (src instanceof g.HTMLImageElement) return src._surf ?? null;
		if (src instanceof g.HTMLCanvasElement || src instanceof OffscreenCanvas) {
			const ctx = src._ctx;
			return ctx instanceof CanvasRenderingContext2D ? ctx._id : null;
		}
		if (src instanceof ImageBitmap) return src._surf ?? null;
		return null;
	};
	const sizeOf = (src) => {
		if (src instanceof g.HTMLImageElement) return [src._nw ?? 0, src._nh ?? 0];
		return [src?.width ?? 0, src?.height ?? 0];
	};

	// ---------------------------------------------------------------- fonts
	const parseFont = (f) => {
		const s = String(f);
		const m = /(?:^|\s)([\d.]+)(px|pt|em|rem|%)(?:\s*\/\s*\S+)?\s+(.+)$/.exec(s);
		if (!m) return null;
		let px = parseFloat(m[1]);
		if (m[2] === "pt") px *= 4 / 3;
		else if (m[2] === "em" || m[2] === "rem") px *= 16;
		else if (m[2] === "%") px *= 0.16;
		const pre = s.slice(0, m.index + (m[0].startsWith(" ") ? 1 : 0)).toLowerCase();
		let weight = 400;
		const w = /\b([1-9]00)\b/.exec(pre);
		if (w) weight = +w[1];
		else if (/\bbold(er)?\b/.test(pre)) weight = 700;
		else if (/\blighter\b/.test(pre)) weight = 300;
		const italic = /\b(italic|oblique)\b/.test(pre);
		return { px, weight, italic, family: m[3], text: s };
	};

	// ---------------------------------------------------------------- publishing
	// Canvases that drew since their last publish, sent after the current
	// task (so after all of a frame's callbacks).
	const pending = new Set();
	let flushArmed = false;
	const flush = () => {
		flushArmed = false;
		for (const c of pending) {
			try {
				c._present();
			} catch (err) {
				L.reportError(err);
			}
		}
		pending.clear();
	};
	const schedule = (ctx) => {
		pending.add(ctx);
		if (!flushArmed) {
			flushArmed = true;
			L.internalTimeout(flush, 0);
		}
	};

	// ---------------------------------------------------------------- 2D context
	const defaults = () => ({
		fill: [0, 0, 0, 1],
		fillStyle: "#000000",
		stroke: [0, 0, 0, 1],
		strokeStyle: "#000000",
		globalAlpha: 1,
		lineWidth: 1,
		lineCap: "butt",
		lineJoin: "miter",
		miterLimit: 10,
		dash: [],
		lineDashOffset: 0,
		font: "10px sans-serif",
		fontInfo: { px: 10, weight: 400, italic: false },
		textAlign: "start",
		textBaseline: "alphabetic",
		direction: "ltr",
		op: 0,
		globalCompositeOperation: "source-over",
		imageSmoothingEnabled: true,
		imageSmoothingQuality: "low",
		shadowBlur: 0,
		shadowColor: "rgba(0, 0, 0, 0)",
		shadowOffsetX: 0,
		shadowOffsetY: 0,
		filter: "none",
		letterSpacing: "0px",
		wordSpacing: "0px",
		fontKerning: "auto",
		m: ID,
	});
	class CanvasRenderingContext2D {
		constructor(canvas, attrs) {
			this._canvas = canvas;
			this._attrs = attrs ?? {};
			this._id = N.create(canvas.width, canvas.height);
			reg?.register(this, this._id);
			this._s = defaults();
			this._stack = [];
			this._path = new PathRec();
		}
		get canvas() {
			return this._canvas instanceof OffscreenCanvas && this._canvas._el ? this._canvas._el : this._canvas;
		}
		_resized() {
			N.resize(this._id, this._canvas.width, this._canvas.height);
			this._s = defaults();
			this._stack = [];
			this._path = new PathRec();
			this._changed();
		}
		_changed() {
			const el = this.canvas;
			if (el instanceof g.HTMLCanvasElement) schedule(this);
		}
		_present() {
			N.publish(this._id);
		}
		getContextAttributes() {
			return { alpha: true, colorSpace: "srgb", desynchronized: false, willReadFrequently: false, ...this._attrs };
		}
		// state
		save() {
			this._stack.push({ ...this._s, dash: this._s.dash.slice() });
			N.save(this._id);
		}
		restore() {
			const s = this._stack.pop();
			if (!s) return;
			this._s = s;
			N.restore(this._id);
		}
		reset() {
			N.reset(this._id);
			this._s = defaults();
			this._stack = [];
			this._path = new PathRec();
			this._changed();
		}
		isContextLost() {
			return false;
		}
		// transforms
		translate(x, y) {
			if (finite(x, y)) this._s.m = mul(this._s.m, [1, 0, 0, 1, +x, +y]);
		}
		scale(x, y) {
			if (finite(x, y)) this._s.m = mul(this._s.m, [+x, 0, 0, +y, 0, 0]);
		}
		rotate(a) {
			if (!finite(a)) return;
			const c = Math.cos(a),
				s = Math.sin(a);
			this._s.m = mul(this._s.m, [c, s, -s, c, 0, 0]);
		}
		transform(a, b, c, d, e, f) {
			if (finite(a, b, c, d, e, f)) this._s.m = mul(this._s.m, [+a, +b, +c, +d, +e, +f]);
		}
		setTransform(a, b, c, d, e, f) {
			if (a === undefined) return this.resetTransform();
			if (typeof a === "object") {
				const t = a;
				this._s.m = [t.a ?? t.m11 ?? 1, t.b ?? t.m12 ?? 0, t.c ?? t.m21 ?? 0, t.d ?? t.m22 ?? 1, t.e ?? t.m41 ?? 0, t.f ?? t.m42 ?? 0];
				return;
			}
			if (finite(a, b, c, d, e, f)) this._s.m = [+a, +b, +c, +d, +e, +f];
		}
		resetTransform() {
			this._s.m = ID;
		}
		getTransform() {
			const [a, b, c, d, e, f] = this._s.m;
			return g.DOMMatrix ? new g.DOMMatrix([a, b, c, d, e, f]) : { a, b, c, d, e, f, is2D: true, isIdentity: a === 1 && b === 0 && c === 0 && d === 1 && e === 0 && f === 0 };
		}
		// styles
		get fillStyle() {
			return this._s.fillStyle;
		}
		set fillStyle(v) {
			if (v instanceof CanvasGradient || v instanceof CanvasPattern) {
				this._s.fill = v;
				this._s.fillStyle = v;
				return;
			}
			const c = parseColor(v);
			if (!c) return;
			this._s.fill = c;
			this._s.fillStyle = colorString(c);
		}
		get strokeStyle() {
			return this._s.strokeStyle;
		}
		set strokeStyle(v) {
			if (v instanceof CanvasGradient || v instanceof CanvasPattern) {
				this._s.stroke = v;
				this._s.strokeStyle = v;
				return;
			}
			const c = parseColor(v);
			if (!c) return;
			this._s.stroke = c;
			this._s.strokeStyle = colorString(c);
		}
		get globalAlpha() {
			return this._s.globalAlpha;
		}
		set globalAlpha(v) {
			v = +v;
			if (v >= 0 && v <= 1) this._s.globalAlpha = v;
		}
		get globalCompositeOperation() {
			return this._s.globalCompositeOperation;
		}
		set globalCompositeOperation(v) {
			const i = COMPOSITE.indexOf(String(v));
			if (i < 0) return;
			this._s.op = i;
			this._s.globalCompositeOperation = COMPOSITE[i];
		}
		get lineWidth() {
			return this._s.lineWidth;
		}
		set lineWidth(v) {
			v = +v;
			if (v > 0 && Number.isFinite(v)) this._s.lineWidth = v;
		}
		get miterLimit() {
			return this._s.miterLimit;
		}
		set miterLimit(v) {
			v = +v;
			if (v > 0 && Number.isFinite(v)) this._s.miterLimit = v;
		}
		get lineDashOffset() {
			return this._s.lineDashOffset;
		}
		set lineDashOffset(v) {
			if (Number.isFinite(+v)) this._s.lineDashOffset = +v;
		}
		setLineDash(a) {
			if (!a || [...a].some((x) => !(x >= 0) || !Number.isFinite(+x))) return;
			const d = [...a].map(Number);
			this._s.dash = d.length % 2 ? [...d, ...d] : d;
		}
		getLineDash() {
			return this._s.dash.slice();
		}
		get font() {
			return this._s.font;
		}
		set font(v) {
			const f = parseFont(v);
			if (!f) return;
			this._s.font = f.text;
			this._s.fontInfo = f;
		}
		createLinearGradient(x0, y0, x1, y1) {
			return new CanvasGradient("linear", [+x0, +y0, +x1, +y1]);
		}
		createRadialGradient(x0, y0, r0, x1, y1, r1) {
			if (r0 < 0 || r1 < 0) throw domErr("The radius provided is negative.", "IndexSizeError");
			return new CanvasGradient("radial", [+x0, +y0, +r0, +x1, +y1, +r1]);
		}
		createConicGradient(a, x, y) {
			return new CanvasGradient("conic", [+a, +x, +y]);
		}
		createPattern(src, rep) {
			return surfOf(src) == null && !(src instanceof g.HTMLImageElement) ? null : new CanvasPattern(src, rep);
		}
		// paths
		beginPath() {
			this._path = new PathRec();
		}
		closePath() {
			this._path.close();
		}
		moveTo(x, y) {
			this._path.move(this._s.m, +x, +y);
		}
		lineTo(x, y) {
			this._path.line(this._s.m, +x, +y);
		}
		quadraticCurveTo(a, b, x, y) {
			this._path.quad(this._s.m, +a, +b, +x, +y);
		}
		bezierCurveTo(a, b, c, d, x, y) {
			this._path.cubic(this._s.m, +a, +b, +c, +d, +x, +y);
		}
		arc(x, y, r, a0, a1, ccw) {
			this._path.ellipse(this._s.m, +x, +y, +r, +r, 0, +a0, +a1, !!ccw);
		}
		ellipse(x, y, rx, ry, rot, a0, a1, ccw) {
			this._path.ellipse(this._s.m, +x, +y, +rx, +ry, +rot, +a0, +a1, !!ccw);
		}
		arcTo(a, b, c, d, r) {
			this._path.arcTo(this._s.m, +a, +b, +c, +d, +r);
		}
		rect(x, y, w, h) {
			this._path.rect(this._s.m, +x, +y, +w, +h);
		}
		roundRect(x, y, w, h, r) {
			this._path.roundRect(this._s.m, +x, +y, +w, +h, r);
		}
		_ops(p) {
			return p instanceof Path2D ? mapOps(p._p.ops, this._s.m) : new Float32Array(this._path.ops).buffer;
		}
		_scale() {
			const m = this._s.m;
			return Math.sqrt(Math.abs(m[0] * m[3] - m[1] * m[2])) || 0;
		}
		fill(a, b) {
			const p = a instanceof Path2D ? a : null;
			const rule = (p ? b : a) === "evenodd";
			N.fill(this._id, this._ops(p), encodePaint(this._s.fill, this._s.globalAlpha, this._s.m), rule, this._s.op);
			this._changed();
		}
		stroke(p) {
			const s = this._s;
			const k = this._scale();
			const opts = [s.lineWidth * k, s.lineCap === "round" ? 1 : s.lineCap === "square" ? 2 : 0, s.lineJoin === "round" ? 1 : s.lineJoin === "bevel" ? 2 : 0, s.miterLimit, s.op, s.lineDashOffset * k, ...s.dash.map((x) => x * k)];
			N.stroke(this._id, this._ops(p instanceof Path2D ? p : null), encodePaint(s.stroke, s.globalAlpha, s.m), opts);
			this._changed();
		}
		clip(a, b) {
			const p = a instanceof Path2D ? a : null;
			N.clip(this._id, this._ops(p), (p ? b : a) === "evenodd");
		}
		isPointInPath() {
			return false;
		}
		isPointInStroke() {
			return false;
		}
		_rect(x, y, w, h) {
			const r = new PathRec();
			r.rect(this._s.m, +x, +y, +w, +h);
			return new Float32Array(r.ops).buffer;
		}
		fillRect(x, y, w, h) {
			if (!finite(x, y, w, h) || !w || !h) return;
			N.fill(this._id, this._rect(x, y, w, h), encodePaint(this._s.fill, this._s.globalAlpha, this._s.m), false, this._s.op);
			this._changed();
		}
		strokeRect(x, y, w, h) {
			if (!finite(x, y, w, h)) return;
			const s = this._s;
			const k = this._scale();
			N.stroke(this._id, this._rect(x, y, w, h), encodePaint(s.stroke, s.globalAlpha, s.m), [s.lineWidth * k, 0, s.lineJoin === "round" ? 1 : s.lineJoin === "bevel" ? 2 : 0, s.miterLimit, s.op, 0]);
			this._changed();
		}
		clearRect(x, y, w, h) {
			if (!finite(x, y, w, h) || !w || !h) return;
			N.fill(this._id, this._rect(x, y, w, h), new Float32Array([0, 0, 0, 0, 1]).buffer, false, CLEAR);
			this._changed();
		}
		// images
		drawImage(src, ...a) {
			if (src instanceof g.HTMLImageElement && src._nw === undefined && !src._broken) return;
			const id = surfOf(src);
			const [iw, ih] = sizeOf(src);
			if (id == null || !iw || !ih) return;
			let sx = 0,
				sy = 0,
				sw = iw,
				sh = ih,
				dx,
				dy,
				dw,
				dh;
			if (a.length >= 8) [sx, sy, sw, sh, dx, dy, dw, dh] = a;
			else if (a.length >= 4) [dx, dy, dw, dh] = a;
			else {
				[dx, dy] = a;
				dw = iw;
				dh = ih;
			}
			if (!finite(sx, sy, sw, sh, dx, dy, dw, dh)) return;
			N.drawImage(this._id, [id, +sx, +sy, +sw, +sh, +dx, +dy, +dw, +dh, ...this._s.m, this._s.globalAlpha, this._s.op, this._s.imageSmoothingEnabled ? 1 : 0]);
			this._changed();
		}
		createImageData(w, h) {
			if (typeof w === "object") return new ImageData(w.width, w.height);
			return new ImageData(Math.abs(Math.floor(w)), Math.abs(Math.floor(h)));
		}
		getImageData(x, y, w, h) {
			if (!w || !h) throw domErr("The source width is 0.", "IndexSizeError");
			if (w < 0) {
				x += w;
				w = -w;
			}
			if (h < 0) {
				y += h;
				h = -h;
			}
			w = Math.floor(w);
			h = Math.floor(h);
			return new ImageData(new Uint8ClampedArray(N.getData(this._id, x, y, w, h)), w, h);
		}
		putImageData(img, dx, dy, x = 0, y = 0, w = img.width, h = img.height) {
			if (!img?.data) return;
			const d = img.data;
			const buf = d.byteOffset === 0 && d.byteLength === d.buffer.byteLength ? d.buffer : d.slice().buffer;
			N.putData(this._id, buf, [+dx, +dy, img.width, img.height, +x, +y, +w, +h]);
			this._changed();
		}
		// text
		_text(text, x, y, maxWidth, stroke) {
			const s = this._s;
			const f = s.fontInfo;
			const rtl = s.direction === "rtl";
			const align = { left: 0, center: 1, right: 2, start: rtl ? 2 : 0, end: rtl ? 0 : 2 }[s.textAlign] ?? 0;
			const base = { alphabetic: 0, top: 1, middle: 2, bottom: 3, hanging: 4, ideographic: 5 }[s.textBaseline] ?? 0;
			const str = String(text).replace(/[\t\n\f\r]/g, " ");
			const k = this._scale();
			N.text(this._id, str, [+x, +y, f.px, f.weight, f.italic ? 1 : 0, align, base, maxWidth === undefined ? -1 : +maxWidth, ...s.m], encodePaint(stroke ? s.stroke : s.fill, s.globalAlpha, s.m), [stroke ? 1 : 0, s.lineWidth * k, s.op], f.family ?? "");
			this._changed();
		}
		fillText(t, x, y, mw) {
			if (finite(x, y)) this._text(t, x, y, mw, false);
		}
		strokeText(t, x, y, mw) {
			if (finite(x, y)) this._text(t, x, y, mw, true);
		}
		measureText(t) {
			const f = this._s.fontInfo;
			const [w, asc, desc] = N.measure(String(t), f.px, f.weight, f.family ?? "");
			return { width: w, actualBoundingBoxLeft: 0, actualBoundingBoxRight: w, actualBoundingBoxAscent: asc, actualBoundingBoxDescent: desc, fontBoundingBoxAscent: asc, fontBoundingBoxDescent: desc, emHeightAscent: asc, emHeightDescent: desc, hangingBaseline: asc * 0.8, alphabeticBaseline: 0, ideographicBaseline: -desc };
		}
		drawFocusIfNeeded() {}
		scrollPathIntoView() {}
	}
	for (const k of ["lineCap", "lineJoin", "textAlign", "textBaseline", "direction", "imageSmoothingEnabled", "imageSmoothingQuality", "shadowBlur", "shadowColor", "shadowOffsetX", "shadowOffsetY", "filter", "letterSpacing", "wordSpacing", "fontKerning", "fontStretch", "fontVariantCaps", "textRendering"])
		Object.defineProperty(CanvasRenderingContext2D.prototype, k, {
			get() {
				return this._s[k];
			},
			set(v) {
				this._s[k] = v;
			},
			configurable: true,
		});
	g.CanvasRenderingContext2D = CanvasRenderingContext2D;
	g.OffscreenCanvasRenderingContext2D = CanvasRenderingContext2D;

	// ---------------------------------------------------------------- WebGL2
	// Records state and draw calls; each frame's draws go to the UI, which
	// replays them with OpenGL. Queries answer optimistically (shaders
	// compile, programs link). Textures keep their pixels on the Rust side;
	// draws into a framebuffer name its colour texture, and the UI keeps
	// those as real GL framebuffers. Depth/stencil attachments, 3D textures
	// and blits are not supported.
	const GL = {
		DEPTH_BUFFER_BIT: 0x100, STENCIL_BUFFER_BIT: 0x400, COLOR_BUFFER_BIT: 0x4000, POINTS: 0, LINES: 1, LINE_LOOP: 2, LINE_STRIP: 3, TRIANGLES: 4, TRIANGLE_STRIP: 5, TRIANGLE_FAN: 6,
		ZERO: 0, ONE: 1, SRC_COLOR: 0x300, ONE_MINUS_SRC_COLOR: 0x301, SRC_ALPHA: 0x302, ONE_MINUS_SRC_ALPHA: 0x303, DST_ALPHA: 0x304, ONE_MINUS_DST_ALPHA: 0x305, DST_COLOR: 0x306, ONE_MINUS_DST_COLOR: 0x307, SRC_ALPHA_SATURATE: 0x308,
		CONSTANT_COLOR: 0x8001, ONE_MINUS_CONSTANT_COLOR: 0x8002, CONSTANT_ALPHA: 0x8003, ONE_MINUS_CONSTANT_ALPHA: 0x8004, FUNC_ADD: 0x8006, FUNC_SUBTRACT: 0x800a, FUNC_REVERSE_SUBTRACT: 0x800b, MIN: 0x8007, MAX: 0x8008,
		ARRAY_BUFFER: 0x8892, ELEMENT_ARRAY_BUFFER: 0x8893, UNIFORM_BUFFER: 0x8a11, STREAM_DRAW: 0x88e0, STATIC_DRAW: 0x88e4, DYNAMIC_DRAW: 0x88e8, BUFFER_SIZE: 0x8764, BUFFER_USAGE: 0x8765,
		CULL_FACE: 0xb44, BLEND: 0xbe2, DITHER: 0xbd0, STENCIL_TEST: 0xb90, DEPTH_TEST: 0xb71, SCISSOR_TEST: 0xc11, POLYGON_OFFSET_FILL: 0x8037, SAMPLE_ALPHA_TO_COVERAGE: 0x809e, SAMPLE_COVERAGE: 0x80a0, RASTERIZER_DISCARD: 0x8c89,
		NO_ERROR: 0, INVALID_ENUM: 0x500, INVALID_VALUE: 0x501, INVALID_OPERATION: 0x502, OUT_OF_MEMORY: 0x505, CW: 0x900, CCW: 0x901, FRONT: 0x404, BACK: 0x405, FRONT_AND_BACK: 0x408,
		VIEWPORT: 0xba2, SCISSOR_BOX: 0xc10, COLOR_CLEAR_VALUE: 0xc22, MAX_TEXTURE_SIZE: 0xd33, MAX_VIEWPORT_DIMS: 0xd3a, MAX_VERTEX_ATTRIBS: 0x8869, MAX_VERTEX_UNIFORM_VECTORS: 0x8dfb, MAX_FRAGMENT_UNIFORM_VECTORS: 0x8dfd, MAX_VARYING_VECTORS: 0x8dfc, MAX_TEXTURE_IMAGE_UNITS: 0x8872, MAX_COMBINED_TEXTURE_IMAGE_UNITS: 0x8b4d, MAX_RENDERBUFFER_SIZE: 0x84e8,
		VENDOR: 0x1f00, RENDERER: 0x1f01, VERSION: 0x1f02, SHADING_LANGUAGE_VERSION: 0x8b8c, ALIASED_LINE_WIDTH_RANGE: 0x846e, ALIASED_POINT_SIZE_RANGE: 0x846d, RED_BITS: 0xd52, GREEN_BITS: 0xd53, BLUE_BITS: 0xd54, ALPHA_BITS: 0xd55, DEPTH_BITS: 0xd56, STENCIL_BITS: 0xd57,
		BYTE: 0x1400, UNSIGNED_BYTE: 0x1401, SHORT: 0x1402, UNSIGNED_SHORT: 0x1403, INT: 0x1404, UNSIGNED_INT: 0x1405, FLOAT: 0x1406, HALF_FLOAT: 0x140b,
		DEPTH_COMPONENT: 0x1902, ALPHA: 0x1906, RGB: 0x1907, RGBA: 0x1908, LUMINANCE: 0x1909, LUMINANCE_ALPHA: 0x190a, RED: 0x1903, RGBA8: 0x8058, R8: 0x8229, R32F: 0x822e, RGBA32F: 0x8814, RGBA16F: 0x881a,
		FRAGMENT_SHADER: 0x8b30, VERTEX_SHADER: 0x8b31, COMPILE_STATUS: 0x8b81, LINK_STATUS: 0x8b82, VALIDATE_STATUS: 0x8b83, DELETE_STATUS: 0x8b80, ATTACHED_SHADERS: 0x8b85, ACTIVE_UNIFORMS: 0x8b86, ACTIVE_ATTRIBUTES: 0x8b89, SHADER_TYPE: 0x8b4f, CURRENT_PROGRAM: 0x8b8d,
		TEXTURE_2D: 0xde1, TEXTURE_3D: 0x806f, TEXTURE_2D_ARRAY: 0x8c1a, TEXTURE_CUBE_MAP: 0x8513, TEXTURE0: 0x84c0, TEXTURE1: 0x84c1, TEXTURE2: 0x84c2, TEXTURE3: 0x84c3, ACTIVE_TEXTURE: 0x84e0, TEXTURE_MAG_FILTER: 0x2800, TEXTURE_MIN_FILTER: 0x2801, TEXTURE_WRAP_S: 0x2802, TEXTURE_WRAP_T: 0x2803,
		NEAREST: 0x2600, LINEAR: 0x2601, NEAREST_MIPMAP_NEAREST: 0x2700, LINEAR_MIPMAP_NEAREST: 0x2701, NEAREST_MIPMAP_LINEAR: 0x2702, LINEAR_MIPMAP_LINEAR: 0x2703, REPEAT: 0x2901, CLAMP_TO_EDGE: 0x812f, MIRRORED_REPEAT: 0x8370,
		FRAMEBUFFER: 0x8d40, RENDERBUFFER: 0x8d41, READ_FRAMEBUFFER: 0x8ca8, DRAW_FRAMEBUFFER: 0x8ca9, COLOR_ATTACHMENT0: 0x8ce0, DEPTH_ATTACHMENT: 0x8d00, FRAMEBUFFER_COMPLETE: 0x8cd5, DEPTH_COMPONENT16: 0x81a5,
		UNPACK_FLIP_Y_WEBGL: 0x9240, UNPACK_PREMULTIPLY_ALPHA_WEBGL: 0x9241, UNPACK_ALIGNMENT: 0xcf5, PACK_ALIGNMENT: 0xd05, CONTEXT_LOST_WEBGL: 0x9242, BROWSER_DEFAULT_WEBGL: 0x9244,
		LESS: 0x201, LEQUAL: 0x203, EQUAL: 0x202, GREATER: 0x204, ALWAYS: 0x207, NEVER: 0x200, KEEP: 0x1e00, REPLACE: 0x1e01, INCR: 0x1e02, DECR: 0x1e03,
		FLOAT_VEC2: 0x8b50, FLOAT_VEC3: 0x8b51, FLOAT_VEC4: 0x8b52, INT_VEC2: 0x8b53, BOOL: 0x8b56, FLOAT_MAT2: 0x8b5a, FLOAT_MAT3: 0x8b5b, FLOAT_MAT4: 0x8b5c, SAMPLER_2D: 0x8b5e, VERTEX_ARRAY_BINDING: 0x85b5,
	};
	// Buffer and texture keys live in process-wide maps: unique across
	// runtimes (a Luna restart, tests running side by side).
	let glSeq = (Date.now() % 1e9) * 4096 + Math.floor(Math.random() * 4096);
	class WebGLObject {}
	const glClass = (name) => {
		const C = class extends WebGLObject {};
		Object.defineProperty(C, "name", { value: name });
		g[name] = C;
		return C;
	};
	const WebGLShader = glClass("WebGLShader");
	const WebGLProgram = glClass("WebGLProgram");
	const WebGLBuffer = glClass("WebGLBuffer");
	const WebGLVertexArrayObject = glClass("WebGLVertexArrayObject");
	const WebGLUniformLocation = glClass("WebGLUniformLocation");
	const WebGLTexture = glClass("WebGLTexture");
	const WebGLFramebuffer = glClass("WebGLFramebuffer");
	const WebGLRenderbuffer = glClass("WebGLRenderbuffer");
	glClass("WebGLQuery");
	glClass("WebGLSampler");
	glClass("WebGLSync");
	glClass("WebGLTransformFeedback");
	g.WebGLActiveInfo ??= class WebGLActiveInfo {};
	g.WebGLShaderPrecisionFormat ??= class WebGLShaderPrecisionFormat {};
	const bufReg = typeof g.FinalizationRegistry === "function" ? new g.FinalizationRegistry((k) => N.glBufferFree(k)) : null;
	const texReg = typeof g.FinalizationRegistry === "function" ? new g.FinalizationRegistry((k) => N.glTexFree(k)) : null;
	// A texture source's pixels for N.glTexImage: [size, rgba buffer, surface].
	const texSource = (src) => {
		if (src?.data instanceof Uint8ClampedArray && src.width >= 0) return [[src.width, src.height], src.data.buffer.slice(src.data.byteOffset, src.data.byteOffset + src.data.byteLength), -1];
		const [w, h] = sizeOf(src);
		return [[w, h], null, surfOf(src) ?? -1];
	};
	// Typed pixels of `format`/`type` as straight RGBA bytes (null: none,
	// or not 8-bit).
	const texBytes = (w, h, format, type, px) => {
		if (!px || !ArrayBuffer.isView(px) || type !== GL.UNSIGNED_BYTE) return null;
		const src = new Uint8Array(px.buffer, px.byteOffset, px.byteLength);
		const n = w * h;
		if (format === GL.RGBA) return src.slice(0, n * 4).buffer;
		const out = new Uint8Array(n * 4);
		for (let i = 0; i < n; i++) {
			const o = i * 4;
			if (format === GL.RGB) {
				out[o] = src[i * 3];
				out[o + 1] = src[i * 3 + 1];
				out[o + 2] = src[i * 3 + 2];
				out[o + 3] = 255;
			} else if (format === GL.LUMINANCE || format === GL.RED) {
				out[o] = out[o + 1] = out[o + 2] = src[i];
				out[o + 3] = 255;
			} else if (format === GL.ALPHA) out[o + 3] = src[i];
			else if (format === GL.LUMINANCE_ALPHA) {
				out[o] = out[o + 1] = out[o + 2] = src[i * 2];
				out[o + 3] = src[i * 2 + 1];
			}
		}
		return out.buffer;
	};
	const TEX_PARAMS = [GL.TEXTURE_MIN_FILTER, GL.TEXTURE_MAG_FILTER, GL.TEXTURE_WRAP_S, GL.TEXTURE_WRAP_T];
	const UNIFORM_KINDS = ["1f", "2f", "3f", "4f", "1i", "2i", "3i", "4i", "1ui", "2ui", "3ui", "4ui"];
	class WebGL2RenderingContext {
		constructor(canvas, attrs, version) {
			this._canvas = canvas;
			this._attrs = { alpha: true, antialias: true, depth: true, stencil: false, premultipliedAlpha: true, preserveDrawingBuffer: false, powerPreference: "default", failIfMajorPerformanceCaveat: false, desynchronized: false, ...(attrs ?? {}) };
			this._version = version;
			this._id = N.create(0, 0);
			reg?.register(this, this._id);
			this._cmds = [];
			this._prog = null;
			this._defaultVao = new WebGLVertexArrayObject();
			this._defaultVao._attribs = new Map();
			this._vao = this._defaultVao;
			this._array = null;
			this._blend = false;
			this._factors = [1, 0, 1, 0];
			this._clear = [0, 0, 0, 0];
			this._viewport = [0, 0, canvas.width, canvas.height];
			this._scissor = [0, 0, canvas.width, canvas.height];
			this._caps = new Set([GL.DITHER]);
			this._active = 0;
			this._units = new Map();
			this._fb = null;
			this._flipY = false;
		}
		get canvas() {
			return this._canvas;
		}
		get drawingBufferWidth() {
			return this._canvas.width;
		}
		get drawingBufferHeight() {
			return this._canvas.height;
		}
		get drawingBufferColorSpace() {
			return "srgb";
		}
		_resized() {
			this._viewport = [0, 0, this._canvas.width, this._canvas.height];
		}
		_cmd(c) {
			this._cmds.push(c);
			if (this._cmds.length > 4096) this._cmds.splice(0, this._cmds.length - 4096);
			schedule(this);
		}
		_present() {
			if (!(this._canvas instanceof g.HTMLCanvasElement)) return void (this._cmds = []);
			const cmds = this._cmds;
			this._cmds = [];
			// Draws WebGL would refuse (reading past a buffer) are dropped
			// natively; their error is what getError reports next.
			const err = N.glPublish(this._id, this._canvas.width, this._canvas.height, JSON.stringify(cmds));
			if (err && !this._glErr) this._glErr = err;
		}
		getContextAttributes() {
			return { ...this._attrs };
		}
		isContextLost() {
			return false;
		}
		getError() {
			const e = this._glErr ?? 0;
			this._glErr = 0;
			return e;
		}
		getExtension() {
			return null;
		}
		getSupportedExtensions() {
			return [];
		}
		getParameter(p) {
			switch (p) {
				case GL.VERSION:
					return this._version === 2 ? "WebGL 2.0 (OpenGL ES 3.0)" : "WebGL 1.0 (OpenGL ES 2.0)";
				case GL.SHADING_LANGUAGE_VERSION:
					return this._version === 2 ? "WebGL GLSL ES 3.00" : "WebGL GLSL ES 1.0";
				case GL.VENDOR:
					return "Tidalfast";
				case GL.RENDERER:
					return "Tidalfast WebGL";
				case GL.MAX_TEXTURE_SIZE:
				case GL.MAX_RENDERBUFFER_SIZE:
					return 4096;
				case GL.MAX_VIEWPORT_DIMS:
					return new Int32Array([4096, 4096]);
				case GL.MAX_VERTEX_ATTRIBS:
					return 16;
				case GL.MAX_VERTEX_UNIFORM_VECTORS:
				case GL.MAX_FRAGMENT_UNIFORM_VECTORS:
					return 1024;
				case GL.MAX_VARYING_VECTORS:
					return 15;
				case GL.MAX_TEXTURE_IMAGE_UNITS:
				case GL.MAX_COMBINED_TEXTURE_IMAGE_UNITS:
					return 16;
				case GL.VIEWPORT:
					return new Int32Array(this._viewport);
				case GL.SCISSOR_BOX:
					return new Int32Array(this._scissor);
				case GL.COLOR_CLEAR_VALUE:
					return new Float32Array(this._clear);
				case GL.CURRENT_PROGRAM:
					return this._prog;
				case GL.VERTEX_ARRAY_BINDING:
					return this._vao === this._defaultVao ? null : this._vao;
				case GL.ALIASED_LINE_WIDTH_RANGE:
				case GL.ALIASED_POINT_SIZE_RANGE:
					return new Float32Array([1, 1]);
				case GL.RED_BITS:
				case GL.GREEN_BITS:
				case GL.BLUE_BITS:
				case GL.ALPHA_BITS:
					return 8;
				default:
					return this._caps.has(p) ? true : null;
			}
		}
		getShaderPrecisionFormat() {
			return { rangeMin: 127, rangeMax: 127, precision: 23 };
		}
		// shaders and programs
		createShader(type) {
			const s = new WebGLShader();
			s._type = type;
			s._src = "";
			return s;
		}
		shaderSource(s, src) {
			if (s) s._src = String(src);
		}
		compileShader() {}
		getShaderParameter(s, p) {
			if (p === GL.COMPILE_STATUS) return true;
			if (p === GL.SHADER_TYPE) return s?._type;
			if (p === GL.DELETE_STATUS) return !!s?._deleted;
			return null;
		}
		getShaderInfoLog() {
			return "";
		}
		getShaderSource(s) {
			return s?._src ?? null;
		}
		deleteShader(s) {
			if (s) s._deleted = true;
		}
		isShader(s) {
			return s instanceof WebGLShader;
		}
		createProgram() {
			const p = new WebGLProgram();
			p._shaders = [];
			p._bind = new Map();
			p._uniforms = new Map();
			p._locs = new Map();
			return p;
		}
		attachShader(p, s) {
			if (p && s) p._shaders.push(s);
		}
		detachShader(p, s) {
			if (p) p._shaders = p._shaders.filter((x) => x !== s);
		}
		bindAttribLocation(p, i, name) {
			p?._bind.set(String(name), i);
		}
		linkProgram(p) {
			if (!p) return;
			const vs = p._shaders.find((s) => s._type === GL.VERTEX_SHADER)?._src ?? "";
			const fs = p._shaders.find((s) => s._type === GL.FRAGMENT_SHADER)?._src ?? "";
			const inputs = N.glInputs(vs);
			const names = [];
			for (const [n, i] of p._bind) if (inputs.includes(n) && i >= 0 && i < 16) names[i] = n;
			for (const n of inputs) {
				if (names.includes(n)) continue;
				let i = 0;
				while (names[i] !== undefined) i++;
				names[i] = n;
			}
			for (let i = 0; i < names.length; i++) names[i] ??= "";
			p._attribs = names;
			p._key = N.glProgram(vs, fs, names);
			p._uniforms = new Map();
			p._linked = true;
		}
		getProgramParameter(p, q) {
			if (q === GL.LINK_STATUS || q === GL.VALIDATE_STATUS) return !!p?._linked;
			if (q === GL.DELETE_STATUS) return !!p?._deleted;
			if (q === GL.ATTACHED_SHADERS) return p?._shaders.length ?? 0;
			if (q === GL.ACTIVE_ATTRIBUTES) return p?._attribs?.filter(Boolean).length ?? 0;
			if (q === GL.ACTIVE_UNIFORMS) return 0;
			return null;
		}
		getProgramInfoLog() {
			return "";
		}
		validateProgram() {}
		deleteProgram(p) {
			if (p) p._deleted = true;
		}
		isProgram(p) {
			return p instanceof WebGLProgram;
		}
		useProgram(p) {
			this._prog = p ?? null;
		}
		getAttribLocation(p, name) {
			return p?._attribs ? p._attribs.indexOf(String(name)) : -1;
		}
		getUniformLocation(p, name) {
			if (!p?._linked) return null;
			name = String(name).replace(/\[0\]$/, "");
			let l = p._locs.get(name);
			if (!l) {
				l = new WebGLUniformLocation();
				l._p = p;
				l._name = name;
				p._locs.set(name, l);
			}
			return l;
		}
		getActiveAttrib(p, i) {
			const n = p?._attribs?.filter(Boolean)[i];
			return n ? { name: n, size: 1, type: GL.FLOAT_VEC2 } : null;
		}
		getActiveUniform() {
			return null;
		}
		_u(l, kind, v) {
			if (!l?._p) return;
			for (let i = 0; i < v.length; i++) if (!Number.isFinite(v[i])) v[i] = 0;
			l._p._uniforms.set(l._name, [l._name, kind, v]);
		}
		uniformMatrix2fv(l, t, v) {
			this._u(l, "m2", Array.from(v));
		}
		uniformMatrix3fv(l, t, v) {
			this._u(l, "m3", Array.from(v));
		}
		uniformMatrix4fv(l, t, v) {
			this._u(l, "m4", Array.from(v));
		}
		// buffers and vertex arrays
		createBuffer() {
			const b = new WebGLBuffer();
			b._key = glSeq++;
			b._version = 0;
			b._data = new Uint8Array(0);
			bufReg?.register(b, b._key);
			return b;
		}
		deleteBuffer(b) {
			if (b) N.glBufferFree(b._key);
		}
		isBuffer(b) {
			return b instanceof WebGLBuffer;
		}
		bindBuffer(target, b) {
			if (target === GL.ARRAY_BUFFER) this._array = b ?? null;
			else if (target === GL.ELEMENT_ARRAY_BUFFER) this._vao._elements = b ?? null;
		}
		_bound(target) {
			if (target === GL.ARRAY_BUFFER) return this._array;
			return target === GL.ELEMENT_ARRAY_BUFFER ? (this._vao._elements ?? null) : null;
		}
		bufferData(target, data, usage) {
			const b = this._bound(target);
			if (!b) return;
			if (typeof data === "number") b._data = new Uint8Array(data);
			else if (data instanceof ArrayBuffer) b._data = new Uint8Array(data.slice(0));
			else if (ArrayBuffer.isView(data)) b._data = new Uint8Array(data.buffer.slice(data.byteOffset, data.byteOffset + data.byteLength));
			else return;
			N.glBuffer(b._key, ++b._version, b._data.slice().buffer);
		}
		bufferSubData(target, offset, data) {
			const b = this._bound(target);
			if (!b || !data) return;
			const src = data instanceof ArrayBuffer ? new Uint8Array(data) : new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
			if (offset + src.length > b._data.length) return;
			b._data.set(src, offset);
			N.glBuffer(b._key, ++b._version, b._data.slice().buffer);
		}
		createVertexArray() {
			const v = new WebGLVertexArrayObject();
			v._attribs = new Map();
			return v;
		}
		deleteVertexArray() {}
		isVertexArray(v) {
			return v instanceof WebGLVertexArrayObject;
		}
		bindVertexArray(v) {
			this._vao = v ?? this._defaultVao;
		}
		_attrib(i) {
			let a = this._vao._attribs.get(i);
			if (!a) this._vao._attribs.set(i, (a = { on: false, ptr: null }));
			return a;
		}
		enableVertexAttribArray(i) {
			this._attrib(i).on = true;
		}
		disableVertexAttribArray(i) {
			this._attrib(i).on = false;
		}
		vertexAttribPointer(i, size, type, norm, stride, offset) {
			if (type !== GL.FLOAT || !this._array) return;
			this._attrib(i).ptr = { loc: i, buffer: this._array._key, size, stride, offset };
		}
		// state
		enable(cap) {
			this._caps.add(cap);
			if (cap === GL.BLEND) this._blend = true;
		}
		disable(cap) {
			this._caps.delete(cap);
			if (cap === GL.BLEND) this._blend = false;
		}
		isEnabled(cap) {
			return this._caps.has(cap);
		}
		blendFunc(s, d) {
			this._factors = [s, d, s, d];
		}
		blendFuncSeparate(s, d, sa, da) {
			this._factors = [s, d, sa, da];
		}
		viewport(x, y, w, h) {
			this._viewport = [x, y, w, h];
		}
		scissor(x, y, w, h) {
			this._scissor = [x, y, w, h];
		}
		clearColor(r, gg, b, a) {
			this._clear = [+r, +gg, +b, +a];
		}
		// The bound framebuffer's colour texture key: null for the canvas,
		// undefined for a framebuffer without one (its draws go nowhere).
		_target() {
			if (!this._fb) return null;
			return this._fb._color?._key;
		}
		clear(mask) {
			const fb = this._target();
			if (mask & GL.COLOR_BUFFER_BIT && fb !== undefined) this._cmd({ k: "clear", color: this._clear.slice(), fb });
		}
		_draw(mode, first, count, elements) {
			const p = this._prog;
			const fb = this._target();
			if (!p?._linked || count <= 0 || fb === undefined) return;
			const attribs = [];
			for (const a of this._vao._attribs.values()) if (a.on && a.ptr) attribs.push(a.ptr);
			const tex = [];
			for (const [unit, t] of this._units) if (t && !t._deleted) tex.push({ unit, tex: t._key, params: t._params.slice() });
			this._cmd({ k: "draw", prog: p._key, mode, first, count, uniforms: [...p._uniforms.values()], attribs, blend: this._blend ? this._factors.slice() : null, fb, vp: fb === null ? null : this._viewport.slice(), tex, elements });
		}
		drawArrays(mode, first, count) {
			this._draw(mode, first, count, null);
		}
		drawElements(mode, count, type, offset) {
			const e = this._vao._elements;
			if (e) this._draw(mode, 0, count, [e._key, type, offset]);
		}
		drawElementsInstanced(mode, count, type, offset) {
			this.drawElements(mode, count, type, offset);
		}
		// textures
		createTexture() {
			const t = new WebGLTexture();
			t._key = glSeq++;
			t._params = [GL.NEAREST_MIPMAP_LINEAR, GL.LINEAR, GL.REPEAT, GL.REPEAT];
			t._w = t._h = 0;
			texReg?.register(t, t._key);
			return t;
		}
		deleteTexture(t) {
			if (!t || t._deleted) return;
			t._deleted = true;
			N.glTexFree(t._key);
			for (const [u, b] of this._units) if (b === t) this._units.delete(u);
		}
		isTexture(t) {
			return t instanceof WebGLTexture;
		}
		activeTexture(unit) {
			this._active = Math.max(0, unit - GL.TEXTURE0);
		}
		bindTexture(target, t) {
			if (target === GL.TEXTURE_2D) this._units.set(this._active, t ?? null);
		}
		_tex(target) {
			return target === GL.TEXTURE_2D ? (this._units.get(this._active) ?? null) : null;
		}
		texParameteri(target, pname, v) {
			const t = this._tex(target);
			const i = TEX_PARAMS.indexOf(pname);
			if (t && i >= 0) t._params[i] = v;
		}
		texParameterf(target, pname, v) {
			this.texParameteri(target, pname, v);
		}
		getTexParameter(target, pname) {
			const t = this._tex(target);
			const i = TEX_PARAMS.indexOf(pname);
			return t && i >= 0 ? t._params[i] : null;
		}
		pixelStorei(pname, v) {
			if (pname === GL.UNPACK_FLIP_Y_WEBGL) this._flipY = !!v;
		}
		_upload(t, size, buf, surf, at) {
			const version = N.glTexImage(t._key, size, buf, surf, this._flipY, at ?? []);
			if (!version) return;
			if (!at) [t._w, t._h] = size;
			this._cmd({ k: "tex", tex: t._key, w: t._w, h: t._h, version });
		}
		texImage2D(target, level, ifmt, ...a) {
			const t = this._tex(target);
			if (!t || level !== 0) return;
			if (a.length >= 6) {
				const [w, h, , format, type, px] = a;
				this._upload(t, [w, h], texBytes(w, h, format, type, px), -1, null);
			} else {
				const [size, buf, surf] = texSource(a[2]);
				this._upload(t, size, buf, surf, null);
			}
		}
		texSubImage2D(target, level, x, y, ...a) {
			const t = this._tex(target);
			if (!t || level !== 0) return;
			if (a.length >= 5) {
				const [w, h, format, type, px] = a;
				const buf = texBytes(w, h, format, type, px);
				if (buf) this._upload(t, [w, h], buf, -1, [x, y]);
			} else {
				const [size, buf, surf] = texSource(a[2]);
				this._upload(t, size, buf, surf, [x, y]);
			}
		}
		texStorage2D(target, levels, ifmt, w, h) {
			const t = this._tex(target);
			if (t) this._upload(t, [w, h], null, -1, null);
		}
		generateMipmap(target) {
			const t = this._tex(target);
			if (t) this._cmd({ k: "mip", tex: t._key });
		}
		// framebuffers
		createFramebuffer() {
			const f = new WebGLFramebuffer();
			f._color = null;
			return f;
		}
		bindFramebuffer(target, f) {
			if (target === GL.FRAMEBUFFER || target === GL.DRAW_FRAMEBUFFER) this._fb = f ?? null;
		}
		framebufferTexture2D(target, attachment, textarget, t, level) {
			if (attachment === GL.COLOR_ATTACHMENT0 && this._fb) this._fb._color = t ?? null;
		}
		deleteFramebuffer(f) {
			if (f && this._fb === f) this._fb = null;
		}
		isFramebuffer(f) {
			return f instanceof WebGLFramebuffer;
		}
		drawArraysInstanced(mode, first, count) {
			this.drawArrays(mode, first, count);
		}
		readPixels(x, y, w, h, f, t, out) {
			out?.fill?.(0);
		}
		createRenderbuffer() {
			return new WebGLRenderbuffer();
		}
		checkFramebufferStatus() {
			return GL.FRAMEBUFFER_COMPLETE;
		}
		fenceSync() {
			return {};
		}
		clientWaitSync() {
			return 0x911a;
		}
	}
	for (const [k, v] of Object.entries(GL)) {
		WebGL2RenderingContext.prototype[k] = v;
		WebGL2RenderingContext[k] = v;
	}
	for (const kind of UNIFORM_KINDS) {
		const n = +kind[0];
		WebGL2RenderingContext.prototype[`uniform${kind}`] = function (l, ...v) {
			this._u(l, kind, v.slice(0, n).map(Number));
		};
		WebGL2RenderingContext.prototype[`uniform${kind}v`] = function (l, v) {
			if (v) this._u(l, `${kind}v`, Array.from(v, Number));
		};
	}
	// Everything else WebGL has does nothing.
	for (const k of ["activeTexture", "bindTexture", "texImage2D", "texSubImage2D", "texImage3D", "texStorage2D", "texParameteri", "texParameterf", "generateMipmap", "deleteTexture", "bindFramebuffer", "framebufferTexture2D", "deleteFramebuffer", "bindRenderbuffer", "renderbufferStorage", "framebufferRenderbuffer", "deleteRenderbuffer", "pixelStorei", "depthFunc", "depthMask", "colorMask", "cullFace", "frontFace", "lineWidth", "polygonOffset", "stencilFunc", "stencilOp", "stencilMask", "blendEquation", "blendEquationSeparate", "blendColor", "clearDepth", "clearStencil", "hint", "flush", "finish", "drawElements", "drawElementsInstanced", "drawBuffers", "vertexAttribDivisor", "vertexAttrib1f", "vertexAttrib2f", "vertexAttrib3f", "vertexAttrib4f", "vertexAttribIPointer", "bindBufferBase", "uniformBlockBinding", "deleteSync", "invalidateFramebuffer", "blitFramebuffer", "readBuffer", "bindSampler", "sampleCoverage"])
		WebGL2RenderingContext.prototype[k] ??= /^(texImage3D|texSubImage3D|texStorage3D|blitFramebuffer)/.test(k)
			? function () {
					// A context that needs 3D textures or blits would draw
					// garbage here: it is not shown at all.
					this._unsupported = true;
				}
			: function () {};
	g.WebGL2RenderingContext = WebGL2RenderingContext;
	g.WebGLRenderingContext = class WebGLRenderingContext extends WebGL2RenderingContext {};

	// ---------------------------------------------------------------- canvas elements
	const contextFor = (canvas, type, attrs) => {
		type = String(type).toLowerCase();
		const want2d = type === "2d";
		const wantGl = type === "webgl2" || type === "webgl" || type === "experimental-webgl";
		if (!want2d && !wantGl) return null;
		const cur = canvas._ctx;
		if (cur) {
			if (want2d) return cur instanceof CanvasRenderingContext2D ? cur : null;
			return cur instanceof WebGL2RenderingContext ? cur : null;
		}
		canvas._ctx = want2d ? new CanvasRenderingContext2D(canvas, attrs) : type === "webgl2" ? new WebGL2RenderingContext(canvas, attrs, 2) : new g.WebGLRenderingContext(canvas, attrs, 1);
		if (canvas instanceof g.HTMLCanvasElement) L.dom?.markDirty?.();
		return canvas._ctx;
	};
	const dataUrl = (ctx, w, h) => {
		if (!w || !h) return "data:,";
		let id = ctx instanceof CanvasRenderingContext2D ? ctx._id : null;
		let tmp = false;
		if (id == null) {
			id = N.create(w, h);
			tmp = true;
		}
		const png = N.png(id);
		if (tmp) N.free(id);
		return "data:image/png;base64," + L.b64encRaw(png, 0, png.byteLength);
	};
	const pngBlob = (ctx, w, h) => {
		let id = ctx instanceof CanvasRenderingContext2D ? ctx._id : null;
		let tmp = false;
		if (id == null) {
			id = N.create(w, h);
			tmp = true;
		}
		const png = N.png(id);
		if (tmp) N.free(id);
		return new Blob([new Uint8Array(png)], { type: "image/png" });
	};
	const CP = g.HTMLCanvasElement.prototype;
	Object.assign(CP, {
		getContext(type, attrs) {
			return contextFor(this, type, attrs);
		},
		toDataURL() {
			return dataUrl(this._ctx, this.width, this.height);
		},
		toBlob(cb) {
			const b = pngBlob(this._ctx, this.width, this.height);
			setTimeout(() => cb(b));
		},
		captureStream() {
			return new MediaStream([new MediaStreamTrack("video")]);
		},
		transferControlToOffscreen() {
			const o = new OffscreenCanvas(this.width, this.height);
			o._el = this;
			return o;
		},
	});
	for (const k of ["width", "height"])
		Object.defineProperty(CP, k, {
			get() {
				const v = parseInt(this.getAttribute(k) ?? "", 10);
				return v >= 0 ? v : k === "width" ? 300 : 150;
			},
			set(v) {
				this.setAttribute(k, String(Math.max(0, Math.floor(Number(v)) || 0)));
			},
			configurable: true,
		});
	{
		const base = g.Element.prototype._setAttr;
		CP._setAttr = function (k, v) {
			base.call(this, k, v);
			if ((k === "width" || k === "height") && this._ctx) this._ctx._resized();
		};
	}
	// The id the UI draws a canvas element by (dom.js puts it in the VNode).
	M.canvasKey = (el) => {
		const ctx = el._ctx;
		if (!ctx || ctx._unsupported) return null;
		if (ctx instanceof CanvasRenderingContext2D) N.publish(ctx._id);
		return ctx._id;
	};

	class OffscreenCanvas extends g.EventTarget {
		constructor(w, h) {
			super();
			this._w = Math.max(0, Math.floor(w) || 0);
			this._h = Math.max(0, Math.floor(h) || 0);
		}
		get width() {
			return this._el ? this._el.width : this._w;
		}
		set width(v) {
			if (this._el) this._el.width = v;
			else this._w = Math.max(0, Math.floor(v) || 0);
			this._ctx?._resized();
		}
		get height() {
			return this._el ? this._el.height : this._h;
		}
		set height(v) {
			if (this._el) this._el.height = v;
			else this._h = Math.max(0, Math.floor(v) || 0);
			this._ctx?._resized();
		}
		getContext(type, attrs) {
			if (this._el) return contextFor(this._el, type, attrs);
			return contextFor(this, type, attrs);
		}
		convertToBlob() {
			return Promise.resolve(pngBlob(this._el?._ctx ?? this._ctx, this.width, this.height));
		}
		transferToImageBitmap() {
			return bitmapFrom(this._el ?? this);
		}
	}
	g.OffscreenCanvas = OffscreenCanvas;

	class ImageData {
		constructor(a, b, c) {
			if (a instanceof Uint8ClampedArray) {
				const w = b,
					h = c ?? a.length / 4 / b;
				if (a.length !== w * h * 4) throw domErr("The input data length is not equal to (4 * width * height).", "IndexSizeError");
				Object.assign(this, { data: a, width: w, height: h });
			} else {
				if (!(a > 0) || !(b > 0)) throw domErr("The source width is 0.", "IndexSizeError");
				Object.assign(this, { data: new Uint8ClampedArray(a * b * 4), width: a, height: b });
			}
			this.colorSpace = "srgb";
		}
	}
	g.ImageData = ImageData;

	class ImageBitmap {
		get width() {
			return this._w;
		}
		get height() {
			return this._h;
		}
		close() {
			if (this._surf != null) N.free(this._surf);
			this._surf = null;
			this._w = this._h = 0;
		}
	}
	g.ImageBitmap = ImageBitmap;
	const bitmapFrom = (src) => {
		const bm = new ImageBitmap();
		const [w, h] = src instanceof ImageData ? [src.width, src.height] : sizeOf(src);
		bm._w = w;
		bm._h = h;
		bm._surf = N.create(w, h);
		reg?.register(bm, bm._surf);
		if (src instanceof ImageData) N.putData(bm._surf, src.data.slice().buffer, [0, 0, w, h, 0, 0, w, h]);
		else {
			const id = surfOf(src);
			if (id != null) N.drawImage(bm._surf, [id, 0, 0, w, h, 0, 0, w, h, ...ID, 1, 0, 1]);
		}
		return bm;
	};

	// ---------------------------------------------------------------- images
	const fetchBytes = async (src) => {
		if (src.startsWith("data:")) {
			const comma = src.indexOf(",");
			if (comma < 0) throw new Error("Bad data URL");
			const meta = src.slice(5, comma);
			const body = src.slice(comma + 1);
			if (/;base64$/i.test(meta)) return L.b64decRaw(decodeURIComponent(body).replace(/\s+/g, ""));
			return L.utf8enc(decodeURIComponent(body)).buffer;
		}
		if (src.startsWith("blob:")) {
			const b = L.blobUrls.get(src);
			if (!b) throw new Error("Unknown blob URL");
			return (await b.arrayBuffer());
		}
		if (/^https?:/i.test(src)) {
			const r = await g.fetch(src);
			if (!r.ok) throw new Error(`HTTP ${r.status}`);
			return await r.arrayBuffer();
		}
		throw new Error("Unsupported image URL");
	};
	const IP = g.HTMLImageElement.prototype;
	const decoded = (img, out) => {
		const [id, w, h] = out;
		const old = img._holder;
		if (old) {
			reg?.unregister(old);
			N.free(old.id);
		}
		img._holder = { id };
		reg?.register(img, id, img._holder);
		img._surf = id;
		img._nw = w;
		img._nh = h;
	};
	IP._load = function () {
		const gen = (this._gen = (this._gen ?? 0) + 1);
		this._loading = true;
		this._broken = false;
		const src = this.getAttribute("src") ? this.src : "";
		const done = (ok) => {
			if (gen !== this._gen) return;
			this._loading = false;
			this._broken = !ok;
			// A drawn <img> without a CSS size takes its natural one.
			if (this.isConnected) L.dom?.markDirty?.();
			if (!ok) {
				this._nw = this._nh = 0;
				this._surf = null;
			}
			for (const w of this._waiters?.splice(0) ?? []) ok ? w.resolve() : w.reject(domErr("The source image cannot be decoded.", "EncodingError"));
			this.dispatchEvent(new Event(ok ? "load" : "error"));
			if (ok) this.dispatchEvent(new Event("loadend"));
		};
		if (!src) {
			this._nw = this._nh = 0;
			this._surf = null;
			this._loading = false;
			return;
		}
		(async () => {
			await new Promise((r) => setTimeout(r, 0));
			if (gen !== this._gen) return;
			try {
				const bytes = await fetchBytes(src);
				if (gen !== this._gen) return;
				decoded(this, N.decode(bytes));
				done(true);
			} catch (err) {
				done(false);
			}
		})();
	};
	{
		const base = g.Element.prototype._setAttr;
		IP._setAttr = function (k, v) {
			const old = this.getAttribute(k);
			base.call(this, k, v);
			if (k === "src" && (old !== v || !this._gen)) this._load();
		};
		const baseRemove = g.Element.prototype._removeAttr;
		IP._removeAttr = function (k) {
			baseRemove.call(this, k);
			if (k === "src") this._load();
		};
	}
	Object.defineProperties(IP, {
		complete: {
			get() {
				return !this._loading;
			},
			configurable: true,
		},
		naturalWidth: {
			get() {
				return this._nw ?? 0;
			},
			configurable: true,
		},
		naturalHeight: {
			get() {
				return this._nh ?? 0;
			},
			configurable: true,
		},
		width: {
			get() {
				const v = parseInt(this.getAttribute("width") ?? "", 10);
				return v >= 0 ? v : (this._nw ?? 0);
			},
			set(v) {
				this.setAttribute("width", String(Math.floor(+v) || 0));
			},
			configurable: true,
		},
		height: {
			get() {
				const v = parseInt(this.getAttribute("height") ?? "", 10);
				return v >= 0 ? v : (this._nh ?? 0);
			},
			set(v) {
				this.setAttribute("height", String(Math.floor(+v) || 0));
			},
			configurable: true,
		},
		currentSrc: {
			get() {
				return this._nw ? this.src : "";
			},
			configurable: true,
		},
	});
	IP.decode = function () {
		if (!this._loading) return this._broken || !this._nw ? Promise.reject(domErr("The source image cannot be decoded.", "EncodingError")) : Promise.resolve();
		return new Promise((resolve, reject) => (this._waiters ??= []).push({ resolve, reject }));
	};

	// A bitmap scaled to `resizeWidth`/`resizeHeight` (one alone keeps the
	// aspect): "pixelated" picks pixels, "low" blends neighbours, "medium"
	// and "high" first halve the picture by averaging while it is at least
	// twice the size, as a browser's high-quality scaler does.
	const resized = (bm, o) => {
		let tw = Math.round(Number(o.resizeWidth) || 0);
		let th = Math.round(Number(o.resizeHeight) || 0);
		if (tw <= 0 && th <= 0) return bm;
		if (tw <= 0) tw = Math.max(1, Math.round((bm._w * th) / bm._h));
		if (th <= 0) th = Math.max(1, Math.round((bm._h * tw) / bm._w));
		const quality = String(o.resizeQuality ?? "low");
		const smooth = quality === "pixelated" ? 0 : 1;
		let cur = bm;
		const step = (w, h) => {
			const next = new ImageBitmap();
			next._w = w;
			next._h = h;
			next._surf = N.create(w, h);
			reg?.register(next, next._surf);
			N.drawImage(next._surf, [cur._surf, 0, 0, cur._w, cur._h, 0, 0, w, h, ...ID, 1, 0, smooth]);
			cur.close();
			cur = next;
		};
		if (quality === "medium" || quality === "high") while (cur._w >= tw * 2 && cur._h >= th * 2) step(Math.ceil(cur._w / 2), Math.ceil(cur._h / 2));
		if (cur._w !== tw || cur._h !== th) step(tw, th);
		return cur;
	};
	g.createImageBitmap = async (src, ...rest) => {
		const last = rest.at(-1);
		const opts = last !== null && typeof last === "object" ? rest.pop() : null;
		const bm = await makeBitmap(src, rest);
		return opts ? resized(bm, opts) : bm;
	};
	const makeBitmap = async (src, rest) => {
		if (src instanceof Blob) {
			const out = N.decode(await src.arrayBuffer());
			const bm = new ImageBitmap();
			[bm._surf, bm._w, bm._h] = out;
			reg?.register(bm, bm._surf);
			return bm;
		}
		if (src instanceof g.HTMLImageElement) await src.decode();
		const bm = bitmapFrom(src);
		if (rest.length >= 4) {
			const [sx, sy, sw, sh] = rest;
			const cut = new ImageBitmap();
			cut._w = Math.abs(sw);
			cut._h = Math.abs(sh);
			cut._surf = N.create(cut._w, cut._h);
			reg?.register(cut, cut._surf);
			N.drawImage(cut._surf, [bm._surf, sx, sy, sw, sh, 0, 0, cut._w, cut._h, ...ID, 1, 0, 1]);
			bm.close();
			return cut;
		}
		return bm;
	};

	// ---------------------------------------------------------------- media streams
	let trackSeq = 1;
	class MediaStreamTrack extends g.EventTarget {
		constructor(kind = "audio", tap = false) {
			super();
			this.kind = kind;
			this.id = `luna-track-${trackSeq++}`;
			this.label = kind === "audio" ? "Tidalfast output" : "";
			this.enabled = true;
			this.muted = false;
			this.readyState = "live";
			this._tap = tap;
		}
		stop() {
			if (this.readyState === "ended") return;
			this.readyState = "ended";
		}
		clone() {
			return new MediaStreamTrack(this.kind, this._tap);
		}
		getSettings() {
			return this.kind === "audio" ? { sampleRate: audioRate(), channelCount: 2 } : {};
		}
		getConstraints() {
			return {};
		}
		applyConstraints() {
			return Promise.resolve();
		}
	}
	class MediaStream extends g.EventTarget {
		constructor(tracks = []) {
			super();
			this.id = `luna-stream-${trackSeq++}`;
			this._t = tracks instanceof MediaStream ? tracks.getTracks() : [...tracks];
		}
		get active() {
			return this._t.some((t) => t.readyState === "live");
		}
		getTracks() {
			return this._t.slice();
		}
		getAudioTracks() {
			return this._t.filter((t) => t.kind === "audio");
		}
		getVideoTracks() {
			return this._t.filter((t) => t.kind === "video");
		}
		getTrackById(id) {
			return this._t.find((t) => t.id === id) ?? null;
		}
		addTrack(t) {
			if (!this._t.includes(t)) this._t.push(t);
		}
		removeTrack(t) {
			this._t = this._t.filter((x) => x !== t);
		}
		clone() {
			return new MediaStream(this._t.map((t) => t.clone()));
		}
	}
	g.MediaStream = MediaStream;
	g.MediaStreamTrack = MediaStreamTrack;

	// ---------------------------------------------------------------- Web Audio
	const audioRate = () => N.audioInfo()[0] || 48000;
	class AudioParam {
		constructor(v, min = -3.4e38, max = 3.4e38) {
			this.value = v;
			this.defaultValue = v;
			this.minValue = min;
			this.maxValue = max;
			this.automationRate = "a-rate";
		}
		setValueAtTime(v) {
			this.value = v;
			return this;
		}
		linearRampToValueAtTime(v) {
			this.value = v;
			return this;
		}
		exponentialRampToValueAtTime(v) {
			this.value = v;
			return this;
		}
		setTargetAtTime(v) {
			this.value = v;
			return this;
		}
		setValueCurveAtTime(c) {
			if (c?.length) this.value = c[c.length - 1];
			return this;
		}
		cancelScheduledValues() {
			return this;
		}
		cancelAndHoldAtTime() {
			return this;
		}
	}
	g.AudioParam = AudioParam;
	class AudioNode extends g.EventTarget {
		constructor(ctx, ins = 1, outs = 1) {
			super();
			this.context = ctx;
			this.numberOfInputs = ins;
			this.numberOfOutputs = outs;
			this.channelCount = 2;
			this.channelCountMode = "max";
			this.channelInterpretation = "speakers";
			this._in = [];
		}
		connect(dest, output = 0) {
			if (dest instanceof AudioNode) {
				if (!dest._in.some((x) => x.node === this && x.output === output)) dest._in.push({ node: this, output });
				return dest;
			}
			return undefined;
		}
		disconnect(dest) {
			const nodes = dest instanceof AudioNode ? [dest] : [...this.context._nodes];
			for (const n of nodes) n._in = n._in.filter((x) => x.node !== this);
		}
		// Which tapped channel reaches this node: 0, 1, -1 (both mixed), or
		// null for none.
		_channel(seen = new Set()) {
			if (seen.has(this)) return null;
			seen.add(this);
			for (const { node, output } of this._in) {
				const c = node._source(output, seen);
				if (c !== null) return c;
			}
			return null;
		}
		_source(output, seen) {
			return this._channel(seen);
		}
	}
	g.AudioNode = AudioNode;
	class AudioScheduledSourceNode extends AudioNode {
		start() {}
		stop() {
			setTimeout(() => this.dispatchEvent(new Event("ended")));
		}
	}
	g.AudioScheduledSourceNode = AudioScheduledSourceNode;
	class ChannelSplitterNode extends AudioNode {
		constructor(ctx, opts = {}) {
			super(ctx, 1, opts.numberOfOutputs ?? 6);
		}
		_source(output, seen) {
			const c = this._channel(seen);
			if (c === null) return null;
			return output === 0 || output === 1 ? output : null;
		}
	}
	class ChannelMergerNode extends AudioNode {
		constructor(ctx, opts = {}) {
			super(ctx, opts.numberOfInputs ?? 6, 1);
		}
	}
	class MediaStreamAudioSourceNode extends AudioNode {
		constructor(ctx, opts = {}) {
			super(ctx, 0, 1);
			this.mediaStream = opts.mediaStream;
			if (!this.mediaStream?.getAudioTracks?.().length) throw domErr("MediaStream has no audio track", "InvalidStateError");
		}
		_source() {
			return this.mediaStream.getAudioTracks().some((t) => t._tap && t.readyState === "live") ? -1 : null;
		}
	}
	class MediaElementAudioSourceNode extends AudioNode {
		constructor(ctx, opts = {}) {
			super(ctx, 0, 1);
			this.mediaElement = opts.mediaElement;
		}
		_source() {
			return this.mediaElement === player ? -1 : null;
		}
	}
	const dbOf = (x) => (x > 0 ? 20 * Math.log10(x) : -Infinity);
	class AnalyserNode extends AudioNode {
		constructor(ctx, opts = {}) {
			super(ctx, 1, 1);
			this._fft = 2048;
			this.minDecibels = opts.minDecibels ?? -100;
			this.maxDecibels = opts.maxDecibels ?? -30;
			this.smoothingTimeConstant = opts.smoothingTimeConstant ?? 0.8;
			if (opts.fftSize) this.fftSize = opts.fftSize;
			this._smooth = null;
			this._at = 0;
		}
		get fftSize() {
			return this._fft;
		}
		set fftSize(v) {
			v = Math.floor(+v);
			if (!(v >= 32 && v <= 32768) || (v & (v - 1)) !== 0) throw domErr(`The value provided (${v}) is not a power of two between 32 and 32768.`, "IndexSizeError");
			this._fft = v;
			this._smooth = null;
		}
		get frequencyBinCount() {
			return this._fft / 2;
		}
		_spectrum() {
			// Computed at most once per few milliseconds (a browser computes
			// once per render quantum), so smoothing does not run twice for
			// two reads in one frame.
			const now = g.performance.now();
			const n = this._fft / 2;
			if (this._smooth && this._smooth.length === n && now - this._at < 8) return this._smooth;
			this._at = now;
			const ch = this._channel();
			const mags = ch === null ? new Float32Array(n) : new Float32Array(N.audioFft(this._fft, ch));
			const t = Math.max(0, Math.min(1, this.smoothingTimeConstant));
			let s = this._smooth;
			if (!s || s.length !== n) s = this._smooth = new Float32Array(n);
			for (let k = 0; k < n; k++) {
				const v = t * s[k] + (1 - t) * (mags[k] || 0);
				s[k] = Number.isFinite(v) ? v : 0;
			}
			return s;
		}
		_time() {
			const ch = this._channel();
			if (ch === null) return new Float32Array(this._fft);
			return new Float32Array(N.audioRead(this._fft, ch));
		}
		getFloatFrequencyData(out) {
			const s = this._spectrum();
			const n = Math.min(out.length, s.length);
			for (let k = 0; k < n; k++) out[k] = dbOf(s[k]);
		}
		getByteFrequencyData(out) {
			const s = this._spectrum();
			const n = Math.min(out.length, s.length);
			const lo = this.minDecibels,
				range = this.maxDecibels - this.minDecibels || 1;
			for (let k = 0; k < n; k++) {
				const v = Math.floor((255 / range) * (dbOf(s[k]) - lo));
				out[k] = v < 0 ? 0 : v > 255 ? 255 : v;
			}
		}
		getFloatTimeDomainData(out) {
			const d = this._time();
			const n = Math.min(out.length, d.length);
			for (let i = 0; i < n; i++) out[i] = d[i];
		}
		getByteTimeDomainData(out) {
			const d = this._time();
			const n = Math.min(out.length, d.length);
			for (let i = 0; i < n; i++) {
				const v = Math.floor(128 * (1 + d[i]));
				out[i] = v < 0 ? 0 : v > 255 ? 255 : v;
			}
		}
	}
	class GainNode extends AudioNode {
		constructor(ctx, opts = {}) {
			super(ctx);
			this.gain = new AudioParam(opts.gain ?? 1);
		}
	}
	class BiquadFilterNode extends AudioNode {
		constructor(ctx) {
			super(ctx);
			this.type = "lowpass";
			this.frequency = new AudioParam(350);
			this.detune = new AudioParam(0);
			this.Q = new AudioParam(1);
			this.gain = new AudioParam(0);
		}
		getFrequencyResponse() {}
	}
	class OscillatorNode extends AudioScheduledSourceNode {
		constructor(ctx) {
			super(ctx, 0, 1);
			this.type = "sine";
			this.frequency = new AudioParam(440);
			this.detune = new AudioParam(0);
		}
		setPeriodicWave() {}
	}
	class AudioBuffer {
		constructor(o = {}) {
			this.numberOfChannels = o.numberOfChannels ?? 1;
			this.length = o.length ?? 0;
			this.sampleRate = o.sampleRate ?? 48000;
			this.duration = this.length / this.sampleRate;
			this._d = Array.from({ length: this.numberOfChannels }, () => new Float32Array(this.length));
		}
		getChannelData(c) {
			return this._d[c];
		}
		copyFromChannel(dst, c, o = 0) {
			dst.set(this._d[c].subarray(o, o + dst.length));
		}
		copyToChannel(src, c, o = 0) {
			this._d[c].set(src, o);
		}
	}
	class AudioBufferSourceNode extends AudioScheduledSourceNode {
		constructor(ctx) {
			super(ctx, 0, 1);
			this.buffer = null;
			this.loop = false;
			this.playbackRate = new AudioParam(1);
			this.detune = new AudioParam(0);
		}
	}
	const passthrough = (name, params = {}) => {
		const C = class extends AudioNode {
			constructor(ctx) {
				super(ctx);
				for (const [k, v] of Object.entries(params)) this[k] = new AudioParam(v);
			}
		};
		Object.defineProperty(C, "name", { value: name });
		g[name] = C;
		return C;
	};
	const DynamicsCompressorNode = passthrough("DynamicsCompressorNode", { threshold: -24, knee: 30, ratio: 12, attack: 0.003, release: 0.25 });
	const StereoPannerNode = passthrough("StereoPannerNode", { pan: 0 });
	const DelayNode = passthrough("DelayNode", { delayTime: 0 });
	const ConvolverNode = passthrough("ConvolverNode");
	const WaveShaperNode = passthrough("WaveShaperNode");
	const PannerNode = passthrough("PannerNode", { positionX: 0, positionY: 0, positionZ: 0 });
	class AudioDestinationNode extends AudioNode {
		constructor(ctx) {
			super(ctx, 1, 0);
			this.maxChannelCount = 2;
		}
	}
	class BaseAudioContext extends g.EventTarget {
		constructor() {
			super();
			this._nodes = new Set();
			this._t0 = g.performance.now();
			this.state = "running";
			this.destination = new AudioDestinationNode(this);
			this.listener = { positionX: new AudioParam(0), positionY: new AudioParam(0), positionZ: new AudioParam(0), setPosition() {}, setOrientation() {} };
			this.onstatechange = null;
		}
		get sampleRate() {
			return audioRate();
		}
		get currentTime() {
			return this.state === "closed" ? this._tEnd ?? 0 : (g.performance.now() - this._t0) / 1000;
		}
		_add(n) {
			this._nodes.add(n);
			return n;
		}
		_state(s) {
			if (this.state === s) return;
			this.state = s;
			const ev = new Event("statechange");
			this.dispatchEvent(ev);
			try {
				this.onstatechange?.(ev);
			} catch (err) {
				L.reportError(err);
			}
		}
		createAnalyser() {
			return this._add(new AnalyserNode(this));
		}
		createChannelSplitter(n = 6) {
			return this._add(new ChannelSplitterNode(this, { numberOfOutputs: n }));
		}
		createChannelMerger(n = 6) {
			return this._add(new ChannelMergerNode(this, { numberOfInputs: n }));
		}
		createGain() {
			return this._add(new GainNode(this));
		}
		createBiquadFilter() {
			return this._add(new BiquadFilterNode(this));
		}
		createOscillator() {
			return this._add(new OscillatorNode(this));
		}
		createBufferSource() {
			return this._add(new AudioBufferSourceNode(this));
		}
		createBuffer(c, len, rate) {
			return new AudioBuffer({ numberOfChannels: c, length: len, sampleRate: rate });
		}
		createDynamicsCompressor() {
			return this._add(new DynamicsCompressorNode(this));
		}
		createStereoPanner() {
			return this._add(new StereoPannerNode(this));
		}
		createDelay() {
			return this._add(new DelayNode(this));
		}
		createConvolver() {
			return this._add(new ConvolverNode(this));
		}
		createWaveShaper() {
			return this._add(new WaveShaperNode(this));
		}
		createPanner() {
			return this._add(new PannerNode(this));
		}
		createScriptProcessor() {
			const n = this._add(new AudioNode(this));
			n.onaudioprocess = null;
			return n;
		}
		createPeriodicWave() {
			return {};
		}
		decodeAudioData(buf, ok, fail) {
			const err = domErr("Decoding audio is not supported.", "EncodingError");
			if (fail) setTimeout(() => fail(err));
			return Promise.reject(err);
		}
	}
	class AudioContext extends BaseAudioContext {
		constructor(opts) {
			super();
			this.baseLatency = 0.01;
			this.outputLatency = 0.02;
			this.sinkId = "";
			this._opts = opts;
		}
		createMediaStreamSource(stream) {
			return this._add(new MediaStreamAudioSourceNode(this, { mediaStream: stream }));
		}
		createMediaElementSource(el) {
			if (el?._sourced) throw domErr("HTMLMediaElement already connected previously to a different MediaElementSourceNode.", "InvalidStateError");
			if (el) el._sourced = true;
			return this._add(new MediaElementAudioSourceNode(this, { mediaElement: el }));
		}
		createMediaStreamDestination() {
			const n = this._add(new AudioNode(this, 1, 0));
			n.stream = new MediaStream([new MediaStreamTrack("audio")]);
			return n;
		}
		getOutputTimestamp() {
			return { contextTime: this.currentTime, performanceTime: g.performance.now() };
		}
		resume() {
			if (this.state === "closed") return Promise.reject(domErr("The AudioContext is closed.", "InvalidStateError"));
			this._state("running");
			return Promise.resolve();
		}
		suspend() {
			if (this.state !== "closed") this._state("suspended");
			return Promise.resolve();
		}
		close() {
			this._tEnd = this.currentTime;
			this._state("closed");
			return Promise.resolve();
		}
		setSinkId() {
			return Promise.resolve();
		}
	}
	Object.assign(g, { AudioContext, webkitAudioContext: AudioContext, BaseAudioContext, AnalyserNode, ChannelSplitterNode, ChannelMergerNode, GainNode, BiquadFilterNode, OscillatorNode, AudioBuffer, AudioBufferSourceNode, AudioDestinationNode, MediaStreamAudioSourceNode, MediaElementAudioSourceNode });

	// ---------------------------------------------------------------- <video>
	// A plugin's own <video> (CatJam's dancing cat). Given a src, the clip is
	// fetched and read by __video (src/luna/video.rs); the interface decodes
	// and draws it (src/ui/luna_video.rs). The clock runs here and there from
	// the same anchor: play, pause, seeking and the rate send it over. Only
	// VP9 WebM plays; anything else fails with an "error" event, as in a
	// browser without the codec. Clips play muted (they have no sound track
	// here).
	const V = g.__video;
	const VP = g.HTMLVideoElement.prototype;
	const vreg = typeof g.FinalizationRegistry === "function" ? new g.FinalizationRegistry((id) => V.free(id)) : null;
	const vs = (el) => (el._vs ??= { id: null, holder: null, w: 0, h: 0, dur: NaN, ready: 0, loading: false, paused: true, time: 0, at: 0, rate: 1, gen: 0, ended: false, error: null, endTimer: null, muted: null });
	const vfire = (el, type) => el.dispatchEvent(new Event(type));
	const vnow = (el) => {
		const s = vs(el);
		let t = s.time;
		if (!s.paused) t += ((g.performance.now() - s.at) / 1000) * s.rate;
		if (!(s.dur > 0)) return Math.max(0, t);
		if (el.hasAttribute("loop")) return t >= 0 && t < s.dur ? t : ((t % s.dur) + s.dur) % s.dur;
		return Math.max(0, Math.min(t, s.dur));
	};
	// Re-anchors the clock at now and hands it to the native side.
	const vsync = (el) => {
		const s = vs(el);
		s.time = vnow(el);
		s.at = g.performance.now();
		if (s.endTimer) {
			clearTimeout(s.endTimer);
			s.endTimer = null;
		}
		if (s.id == null) return;
		const loop = el.hasAttribute("loop");
		V.state(s.id, s.paused || !(s.rate > 0), s.time, s.rate > 0 ? s.rate : 1, loop);
		// An unlooped clip ends: paused, "pause" then "ended".
		if (!s.paused && !loop && s.rate > 0 && s.dur > 0)
			s.endTimer = setTimeout(() => {
				s.endTimer = null;
				if (s.paused || el.hasAttribute("loop")) return;
				s.time = s.dur;
				s.paused = true;
				s.ended = true;
				V.state(s.id, true, s.dur, 1, false);
				vfire(el, "timeupdate");
				vfire(el, "pause");
				vfire(el, "ended");
			}, Math.max(0, ((s.dur - s.time) / s.rate) * 1000));
	};
	VP._load = function () {
		if (this._player) return;
		const s = vs(this);
		const gen = ++s.gen;
		if (s.id != null) {
			vreg?.unregister(s.holder);
			V.free(s.id);
			s.id = null;
		}
		const had = s.ready || s.error;
		Object.assign(s, { ready: 0, w: 0, h: 0, dur: NaN, error: null, ended: false, time: 0, at: g.performance.now(), loading: false });
		if (had) vfire(this, "emptied");
		L.dom?.markDirty?.();
		const src = this.getAttribute("src") ? this.src : "";
		if (!src) return;
		s.loading = true;
		(async () => {
			await new Promise((r) => setTimeout(r, 0));
			if (gen !== s.gen) return;
			vfire(this, "loadstart");
			try {
				const bytes = await fetchBytes(src);
				if (gen !== s.gen) return;
				const [id, w, h, dur] = V.load(bytes);
				s.id = id;
				s.holder = { id };
				vreg?.register(this, id, s.holder);
				Object.assign(s, { w, h, dur, ready: 4, loading: false });
				vsync(this);
				L.dom?.markDirty?.();
				const playing = !s.paused;
				for (const t of ["durationchange", "loadedmetadata", "loadeddata", "canplay", "canplaythrough"]) vfire(this, t);
				if (playing) vfire(this, "playing");
				else if (s.paused && this.hasAttribute("autoplay")) this.play();
			} catch (err) {
				if (gen !== s.gen) return;
				s.loading = false;
				s.error = { code: 4, message: String(err?.message ?? err) };
				console.warn(`<video> ${src}: ${s.error.message}`);
				vfire(this, "error");
			}
		})();
	};
	{
		const base = g.Element.prototype._setAttr;
		VP._setAttr = function (k, v) {
			const old = this.getAttribute(k);
			base.call(this, k, v);
			if (this._player) return;
			if (k === "src" && (old !== v || !vs(this).gen)) this._load();
			else if (k === "loop" && old === null) vsync(this);
		};
		const baseRemove = g.Element.prototype._removeAttr;
		VP._removeAttr = function (k) {
			const had = this.hasAttribute(k);
			baseRemove.call(this, k);
			if (this._player || !had) return;
			if (k === "src") this._load();
			else if (k === "loop") vsync(this);
		};
	}
	VP.play = function () {
		if (this._player) return Promise.resolve();
		const s = vs(this);
		if (s.error) return Promise.reject(domErr("The element has no supported sources.", "NotSupportedError"));
		if (s.ended || (!this.hasAttribute("loop") && s.dur > 0 && vnow(this) >= s.dur)) {
			s.time = 0;
			s.at = g.performance.now();
			s.ended = false;
		}
		if (s.paused) {
			s.time = vnow(this);
			s.paused = false;
			vsync(this);
			// As in a browser, after the task that called play().
			const ready = s.ready;
			setTimeout(() => {
				vfire(this, "play");
				if (ready) vfire(this, "playing");
			}, 0);
		}
		return Promise.resolve();
	};
	VP.pause = function () {
		if (this._player) return;
		const s = vs(this);
		if (s.paused) return;
		s.time = vnow(this);
		s.paused = true;
		vsync(this);
		vfire(this, "timeupdate");
		vfire(this, "pause");
	};
	VP.load = function () {
		if (!this._player) this._load();
	};
	VP.canPlayType = function (type) {
		const t = String(type).toLowerCase();
		if (!/^video\/webm\b/.test(t)) return "";
		const codecs = /codecs\s*=\s*"?([^";]*)/.exec(t)?.[1];
		if (!codecs) return "maybe";
		return codecs.split(",").every((c) => /^\s*(vp9|vp09[\w.]*)\s*$/.test(c)) ? "probably" : "";
	};
	VP.getVideoPlaybackQuality = function () {
		return { creationTime: g.performance.now(), totalVideoFrames: 0, droppedVideoFrames: 0, corruptedVideoFrames: 0 };
	};
	const vprop = (name, get, set) => Object.defineProperty(VP, name, { get, set, configurable: true });
	vprop("paused", function () {
		return vs(this).paused;
	});
	vprop("ended", function () {
		return vs(this).ended;
	});
	vprop("readyState", function () {
		return vs(this).ready;
	});
	vprop("networkState", function () {
		const s = vs(this);
		return s.loading ? 2 : s.ready || s.error ? 1 : 0;
	});
	vprop("duration", function () {
		return vs(this).dur;
	});
	vprop("videoWidth", function () {
		return vs(this).w;
	});
	vprop("videoHeight", function () {
		return vs(this).h;
	});
	vprop("error", function () {
		const e = vs(this).error;
		return e ? { code: e.code, message: e.message, MEDIA_ERR_SRC_NOT_SUPPORTED: 4 } : null;
	});
	vprop("currentSrc", function () {
		return vs(this).ready ? this.src : "";
	});
	vprop(
		"currentTime",
		function () {
			return vnow(this);
		},
		function (v) {
			const s = vs(this);
			v = +v;
			if (!Number.isFinite(v)) return;
			s.time = Math.max(0, s.dur > 0 ? Math.min(v, s.dur) : v);
			s.at = g.performance.now();
			s.ended = false;
			vsync(this);
			setTimeout(() => {
				vfire(this, "seeking");
				vfire(this, "seeked");
				vfire(this, "timeupdate");
			}, 0);
		},
	);
	vprop(
		"playbackRate",
		function () {
			return vs(this).rate;
		},
		function (v) {
			const s = vs(this);
			v = +v;
			if (!Number.isFinite(v) || v === s.rate) return;
			s.time = vnow(this);
			s.at = g.performance.now();
			s.rate = v;
			vsync(this);
			vfire(this, "ratechange");
		},
	);
	vprop(
		"defaultPlaybackRate",
		function () {
			return 1;
		},
		function () {},
	);
	vprop(
		"muted",
		function () {
			const s = vs(this);
			return s.muted ?? this.hasAttribute("muted");
		},
		function (v) {
			vs(this).muted = !!v;
		},
	);
	vprop(
		"volume",
		function () {
			return 1;
		},
		function () {},
	);
	for (const name of ["loop", "autoplay", "controls", "playsInline", "defaultMuted"]) {
		const attr = name === "playsInline" ? "playsinline" : name === "defaultMuted" ? "muted" : name;
		vprop(
			name,
			function () {
				return this.hasAttribute(attr);
			},
			function (v) {
				if (v) this.setAttribute(attr, "");
				else this.removeAttribute(attr);
			},
		);
	}
	// The id the interface draws a video by (dom.js puts it in the VNode).
	M.videoKey = (el) => (el._player ? null : (el._vs?.id ?? null));

	// ---------------------------------------------------------------- the player element
	// Stands in for TIDAL's player element: in <head>, so it is never drawn,
	// but querySelector("video, audio") finds it and document capture
	// listeners hear its events.
	const player = g.document.createElement("video");
	player.setAttribute("id", "video-one");
	player.setAttribute("data-test", "video");
	player._player = true;
	const demo = !!N.demo();
	const pstate = { paused: !demo, readyState: demo ? 4 : 0, currentTime: 0, duration: NaN, volume: 1, muted: false, ended: false, src: "", sync: g.performance.now() };
	const fire = (type) => player.dispatchEvent(new Event(type));
	// Muted by a plugin (`player.muted = true`), apart from the volume.
	let mutedByPlugin = false;
	let scheduleSwap = () => {};
	Object.defineProperties(player, {
		paused: { get: () => pstate.paused, configurable: true },
		ended: { get: () => pstate.ended, configurable: true },
		readyState: { get: () => pstate.readyState, configurable: true },
		networkState: { get: () => (pstate.readyState ? 1 : 0), configurable: true },
		currentTime: {
			get: () => pstate.currentTime + (pstate.paused ? 0 : (g.performance.now() - pstate.sync) / 1000),
			set() {},
			configurable: true,
		},
		duration: { get: () => pstate.duration, configurable: true },
		volume: {
			get: () => pstate.volume,
			set() {},
			configurable: true,
		},
		muted: {
			get: () => pstate.muted || mutedByPlugin,
			set(v) {
				if (mutedByPlugin === !!v) return;
				mutedByPlugin = !!v;
				scheduleSwap();
			},
			configurable: true,
		},
		currentSrc: { get: () => pstate.src, configurable: true },
		srcObject: { get: () => null, set() {}, configurable: true },
	});
	player.captureStream = () => new MediaStream([new MediaStreamTrack("audio", true)]);
	player.mozCaptureStream = player.captureStream;
	g.document.head?.appendChild(player);
	M.player = player;
	let lastTrack = null;
	let timeTimer = null;

	// ------------------------------------------------------------ a plugin's <audio> for the song
	// AltPlay mutes TIDAL's player element and plays the song from the
	// listener's own server in an <audio> of its own, kept in step with
	// the player. Here that <audio> is no second player: while a plugin
	// has muted the player and an <audio> was given a src for the
	// playing song, Tidalfast's engine plays that address for the song
	// (src/tidal_engine: the same output, EQ and controls), and the
	// element reports the player's clock and state. A plugin's own
	// loopback proxy is followed to the address it proxies
	// (L.resolveLocalStream). A source that cannot be played gives way
	// to TIDAL's stream and the element fires "error", as a failed
	// <audio> does; the plugin then unmutes the player.
	const AP = g.HTMLAudioElement.prototype;
	const base = Object.getPrototypeOf(AP);
	let swap = null; // { el, track, src, failed }
	let lastSrc = null; // the <audio> given a src last
	let ownLength = NaN; // the length of the plugin's own song, as the engine found it
	let endedCount = 0; // plays of a plugin's own song to its end, as the engine counts
	let queued = false;
	const swapped = (el) => swap !== null && swap.el === el && swap.track === (lastTrack == null ? null : String(lastTrack));
	const check = () => {
		queued = false;
		const el = lastSrc?.getAttribute("src") ? lastSrc : null;
		const track = lastTrack == null ? null : String(lastTrack);
		const want = (mutedByPlugin || el?._own) && el && track && /^\d+$/.test(track) && el._srcTrack === track ? { el, track, src: el.getAttribute("src"), failed: false, own: !!el._own && !mutedByPlugin } : null;
		if (want?.el === swap?.el && want?.track === swap?.track && want?.src === swap?.src && want?.own === swap?.own) return;
		const prev = swap;
		swap = want;
		try {
			if (want) L.callOp("host.replace_audio", { track, url: L.resolveLocalStream?.(want.el.src) ?? want.el.src, own: want.own });
			else if (prev && prev.track === track) L.callOp("host.replace_audio", { track, url: null });
			if (!want && prev?.own) prev.el._own = false;
		} catch (e) {
			if (want) {
				swap = null;
				want.el._audioError = { code: 4, message: String(e?.message ?? e), MEDIA_ERR_SRC_NOT_SUPPORTED: 4 };
				want.el.dispatchEvent(new Event("error"));
			}
		}
	};
	scheduleSwap = () => {
		if (queued) return;
		queued = true;
		queueMicrotask(check);
	};
	const srcChanged = (el) => {
		el._audioError = null;
		if (!el.getAttribute("src")) el._own = false;
		el._srcTrack = lastTrack == null ? null : String(lastTrack);
		if (el.getAttribute("src")) lastSrc = el;
		scheduleSwap();
	};
	AP._setAttr = function (k, v) {
		base._setAttr.call(this, k, v);
		if (k === "src") srcChanged(this);
	};
	AP._removeAttr = function (k) {
		const had = this._attrs.has(k);
		base._removeAttr.call(this, k);
		if (k === "src" && had) srcChanged(this);
	};
	const own = (el) => (el._au ??= { time: 0 });
	Object.defineProperties(AP, {
		paused: {
			get() {
				return swapped(this) ? player.paused : true;
			},
			configurable: true,
		},
		ended: {
			get() {
				return swapped(this) ? player.ended : false;
			},
			configurable: true,
		},
		readyState: {
			get() {
				return swapped(this) ? 4 : 0;
			},
			configurable: true,
		},
		duration: {
			get() {
				// A song of the plugin's own has the length the engine found in it.
				if (swapped(this) && this._own) return ownLength;
				return swapped(this) ? player.duration : NaN;
			},
			configurable: true,
		},
		currentSrc: {
			get() {
				return this.src;
			},
			configurable: true,
		},
		error: {
			get() {
				return this._audioError ?? null;
			},
			configurable: true,
		},
		currentTime: {
			get() {
				return swapped(this) ? player.currentTime : own(this).time;
			},
			// The engine follows TIDAL's clock; a seek is the player's, and a
			// song of the plugin's own takes it.
			set(v) {
				own(this).time = Number(v) || 0;
				if (swapped(this) && this._own && Number.isFinite(+v)) L.reduxStore?.dispatch?.({ type: "playbackControls/SEEK", payload: Math.max(0, +v) });
			},
			configurable: true,
		},
	});
	// An <audio> that plays a song from an address of its own (AltPlay's
	// library) while TIDAL's player is not playing is the plugin's own
	// player: the engine plays that address, and play, pause and seek on
	// the element go to it. TIDAL's player still playing means a sound
	// effect, which is not played.
	AP.play = function () {
		const src = this.getAttribute("src") || "";
		this._wantsPlay = true;
		if (!mutedByPlugin && !this._own && /^https?:/i.test(src)) {
			setTimeout(() => {
				if (this._wantsPlay && !this._own && pstate.paused && lastTrack != null && this.getAttribute("src") === src) {
					this._own = true;
					this._srcTrack = String(lastTrack);
					scheduleSwap();
				}
			}, 400);
		}
		if (swapped(this) && this._own && swap.src === src && pstate.paused) L.reduxStore?.dispatch?.({ type: "playbackControls/PLAY" });
		scheduleSwap();
		return Promise.resolve();
	};
	AP.pause = function () {
		this._wantsPlay = false;
		if (swapped(this) && this._own && swap.src === this.getAttribute("src") && !pstate.paused) L.reduxStore?.dispatch?.({ type: "playbackControls/PAUSE" });
	};
	AP.load = function () {
		scheduleSwap();
	};
	const swapFailed = (pc) => {
		if (swap && !swap.failed && swap.track === (lastTrack == null ? null : String(lastTrack)) && pc.tidalfastReplacedAudio === false) {
			swap.failed = true;
			const el = swap.el;
			el._audioError = { code: 2, message: "The source could not be played; TIDAL's stream plays", MEDIA_ERR_NETWORK: 2 };
			el.dispatchEvent(new Event("error"));
		}
	};
	// TIDAL's main process tells the page where the song is, about once a
	// second while it plays (`client.playback.playersignal` with
	// `media.currenttime`, in seconds): Total Play Time, lyrics and
	// miniplayer plugins count from it. Only while something listens.
	const SIGNAL = "client.playback.playersignal";
	let signalled = null;
	const playerSignal = (force) => {
		if (lastTrack == null || !L.ipcListening?.(SIGNAL)) return;
		const time = player.currentTime;
		const at = `${lastTrack}|${Math.floor(time)}`;
		if (!force && at === signalled) return;
		signalled = at;
		L.lunaIpc(SIGNAL, JSON.stringify([{ signal: "media.currenttime", time }]));
	};
	M.playerSignalNow = () => playerSignal(true);
	const update = () => {
		let pc;
		try {
			pc = L.reduxStore?.getState()?.playbackControls;
		} catch {
			return;
		}
		if (!pc) return;
		const playing = demo || pc.playbackState === "PLAYING";
		const track = pc.mediaProduct?.productId ?? null;
		const now = g.performance.now();
		const time = +pc.latestCurrentTime || 0;
		const wasPaused = pstate.paused;
		if (track !== lastTrack) {
			lastTrack = track;
			scheduleSwap();
			pstate.src = track ? `blob:https://desktop.tidal.com/${track}` : "";
			pstate.readyState = track || demo ? 4 : 0;
			pstate.duration = NaN;
			if (track) for (const t of ["loadstart", "durationchange", "loadedmetadata", "loadeddata", "canplay", "canplaythrough"]) fire(t);
			else fire("emptied");
		}
		const items = L.reduxStore?.getState()?.content?.mediaItems;
		const d = track && items ? +(items[track]?.item?.duration ?? NaN) : NaN;
		if (d !== pstate.duration && !(Number.isNaN(d) && Number.isNaN(pstate.duration))) {
			pstate.duration = d;
			fire("durationchange");
		}
		swapFailed(pc);
		ownLength = pc.tidalfastReplacedDuration > 0 ? +pc.tidalfastReplacedDuration : NaN;
		const ended = +pc.tidalfastReplaceEnded || 0;
		if (ended !== endedCount) {
			endedCount = ended;
			if (swap?.own) swap.el.dispatchEvent(new Event("ended"));
		}
		pstate.volume = Math.max(0, Math.min(1, (+pc.volume || 0) / 100));
		pstate.muted = !!pc.muted;
		const timeChanged = Math.abs(time - pstate.currentTime) > 0.001;
		pstate.currentTime = time;
		pstate.sync = now;
		pstate.paused = !playing;
		pstate.ended = !playing && pc.playbackState === "NOT_PLAYING" && pstate.duration > 0 && time >= pstate.duration - 0.5;
		if (wasPaused && playing) {
			fire("play");
			fire("playing");
		} else if (!wasPaused && !playing) {
			fire("pause");
			if (pstate.ended) fire("ended");
		}
		if (timeChanged) {
			fire("timeupdate");
			playerSignal(false);
		}
		// While playing, TIDAL's element reports the time about four times a
		// second; only kept up while someone listens for it.
		const wants = !!(player._ls?.has("timeupdate") || g.document._ls?.has("timeupdate") || player._on?.timeupdate);
		if (playing && wants && !timeTimer)
			timeTimer = setInterval(() => {
				if (pstate.paused) {
					clearInterval(timeTimer);
					timeTimer = null;
					return;
				}
				fire("timeupdate");
			}, 250);
	};
	L.reduxStore?.subscribe?.(update);
	setTimeout(() => {
		update();
		if (demo) {
			fire("play");
			fire("playing");
		}
	}, 0);
})();
