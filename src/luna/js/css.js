// Plugin CSS: <style> elements, constructed stylesheets (adoptedStyleSheets,
// insertRule, replaceSync), the cascade (specificity, order, !important,
// inline styles), custom properties with var() fallbacks, @media and
// @supports, @font-face. dom.js asks compute() for the resolved style of
// every element when it builds the overlay.
"use strict";
(() => {
	const g = globalThis;
	const L = g.__lunaInternal;
	const D = L.dom;
	const { CSSStyleDeclaration, declProxy, splitDecls } = D;
	// Plugin and theme CSS together stop here: every cascade and every
	// overlay flush pays for all of it. A sheet larger than this alone is
	// not even parsed.
	const MAX_CSS_BYTES = 4 << 20;

	// ---------------------------------------------------------------- text helpers
	const stripComments = (s) => {
		let out = "",
			q = null;
		for (let i = 0; i < s.length; i++) {
			const ch = s[i];
			if (q) {
				out += ch;
				if (ch === "\\") out += s[++i] ?? "";
				else if (ch === q) q = null;
			} else if (ch === '"' || ch === "'") {
				q = ch;
				out += ch;
			} else if (ch === "/" && s[i + 1] === "*") {
				const end = s.indexOf("*/", i + 2);
				i = end < 0 ? s.length : end + 1;
			} else out += ch;
		}
		return out;
	};
	// Read from `i` up to the first top-level char in `stops`; returns [text, index].
	const readUntil = (s, i, stops) => {
		let depth = 0,
			q = null;
		const st = i;
		for (; i < s.length; i++) {
			const ch = s[i];
			if (q) {
				if (ch === "\\") i++;
				else if (ch === q) q = null;
			} else if (depth === 0 && stops.includes(ch)) break;
			else if (ch === '"' || ch === "'") q = ch;
			else if (ch === "(" || ch === "[") depth++;
			else if (ch === ")" || ch === "]") depth = Math.max(0, depth - 1);
		}
		return [s.slice(st, i), i];
	};
	// After a "{" at i-1: the block body and the index after the matching "}".
	const readBlock = (s, i) => {
		let depth = 1,
			q = null;
		const st = i;
		for (; i < s.length; i++) {
			const ch = s[i];
			if (q) {
				if (ch === "\\") i++;
				else if (ch === q) q = null;
			} else if (ch === '"' || ch === "'") q = ch;
			else if (ch === "{") depth++;
			else if (ch === "}" && --depth === 0) break;
		}
		return [s.slice(st, i), i + 1];
	};

	// ---------------------------------------------------------------- CSSOM
	class CSSRule {
		constructor(sheet, parent) {
			this.parentStyleSheet = sheet ?? null;
			this.parentRule = parent ?? null;
		}
	}
	Object.assign(CSSRule, { STYLE_RULE: 1, CHARSET_RULE: 2, IMPORT_RULE: 3, MEDIA_RULE: 4, FONT_FACE_RULE: 5, PAGE_RULE: 6, KEYFRAMES_RULE: 7, KEYFRAME_RULE: 8, NAMESPACE_RULE: 10, SUPPORTS_RULE: 12 });
	const ruleChanged = () => D.cssChanged();
	class CSSStyleRule extends CSSRule {
		constructor(sel, body, sheet, parent) {
			super(sheet, parent);
			this._isRule = true;
			this._sel = String(sel).trim();
			this._decl = new CSSStyleDeclaration(this);
			this._decl._setSilently(body);
			this.cssRules = [];
			this._parsed = undefined;
		}
		get type() {
			return 1;
		}
		get selectorText() {
			return this._sel;
		}
		set selectorText(v) {
			this._sel = String(v).trim();
			this._parsed = undefined;
			ruleChanged();
		}
		get style() {
			return (this._proxy ??= declProxy(this._decl));
		}
		set style(v) {
			this._decl.cssText = v;
		}
		_styleChanged() {
			ruleChanged();
		}
		get cssText() {
			return `${this._sel} { ${this._decl.cssText} }`;
		}
		get selectors() {
			if (this._parsed === undefined) {
				try {
					this._parsed = D.sel.parse(this._sel);
				} catch {
					this._parsed = null;
				}
			}
			return this._parsed;
		}
	}
	class CSSGroupingRule extends CSSRule {
		constructor(cond, body, sheet, parent) {
			super(sheet, parent);
			this.conditionText = String(cond).trim();
			this.cssRules = parseRules(body, sheet, this);
		}
		insertRule(text, index = 0) {
			const rs = parseRules(String(text), this.parentStyleSheet, this);
			if (!rs.length) throw new g.DOMException("Failed to parse the rule", "SyntaxError");
			this.cssRules.splice(index, 0, rs[0]);
			ruleChanged();
			return index;
		}
		deleteRule(i) {
			this.cssRules.splice(i, 1);
			ruleChanged();
		}
	}
	class CSSMediaRule extends CSSGroupingRule {
		get type() {
			return 4;
		}
		get media() {
			return { mediaText: this.conditionText, length: 1, item: () => this.conditionText };
		}
		get active() {
			return D.evalMedia(this.conditionText);
		}
		get cssText() {
			return `@media ${this.conditionText} { ${this.cssRules.map((r) => r.cssText).join(" ")} }`;
		}
	}
	class CSSSupportsRule extends CSSGroupingRule {
		get type() {
			return 12;
		}
		get active() {
			return !/^\s*not\b/i.test(this.conditionText);
		}
		get cssText() {
			return `@supports ${this.conditionText} { ${this.cssRules.map((r) => r.cssText).join(" ")} }`;
		}
	}
	class CSSLayerBlockRule extends CSSGroupingRule {
		get type() {
			return 0;
		}
		get active() {
			return true;
		}
		get cssText() {
			return `@layer ${this.conditionText} { ${this.cssRules.map((r) => r.cssText).join(" ")} }`;
		}
	}
	class CSSOtherRule extends CSSRule {
		constructor(name, prelude, body, sheet, parent) {
			super(sheet, parent);
			this.name = name;
			this._prelude = String(prelude).trim();
			this._body = body;
			this.cssRules = [];
		}
		get type() {
			return /keyframes/.test(this.name) ? 7 : this.name === "import" ? 3 : 0;
		}
		get cssText() {
			return this._body === null ? `@${this.name} ${this._prelude};` : `@${this.name} ${this._prelude} { ${this._body} }`;
		}
	}
	class CSSFontFaceRule extends CSSRule {
		constructor(body, sheet, parent) {
			super(sheet, parent);
			this._decl = new CSSStyleDeclaration(this);
			this._decl._setSilently(body);
		}
		get type() {
			return 5;
		}
		get style() {
			return declProxy(this._decl);
		}
		_styleChanged() {
			ruleChanged();
		}
		get cssText() {
			return `@font-face { ${this._decl.cssText} }`;
		}
	}
	// Style rule body: declarations, plus nested rules (CSS nesting).
	const styleRule = (sel, body, sheet, parent, out) => {
		if (!body.includes("{")) {
			out.push(new CSSStyleRule(sel, body, sheet, parent));
			return;
		}
		let decls = "";
		const nested = [];
		let i = 0;
		while (i < body.length) {
			const [pre, j] = readUntil(body, i, ";{");
			if (body[j] === "{") {
				const [inner, k] = readBlock(body, j + 1);
				nested.push([pre.trim(), inner]);
				i = k;
			} else {
				decls += `${pre};`;
				i = j + 1;
			}
		}
		out.push(new CSSStyleRule(sel, decls, sheet, parent));
		const parents = splitTopComma(sel);
		for (const [pre, inner] of nested) {
			if (pre.startsWith("@")) {
				const rs = parseRules(`${pre} { ${sel} { ${inner} } }`, sheet, parent);
				out.push(...rs);
				continue;
			}
			const full = splitTopComma(pre)
				.flatMap((p) => parents.map((par) => (p.includes("&") ? p.replace(/&/g, par) : `${par} ${p}`)))
				.join(", ");
			styleRule(full, inner, sheet, parent, out);
		}
	};
	const splitTopComma = (s) => {
		const out = [];
		let i = 0;
		while (i <= s.length) {
			const [p, j] = readUntil(s, i, ",");
			out.push(p.trim());
			i = j + 1;
		}
		return out.filter(Boolean);
	};
	const parseRules = (text, sheet, parent) => {
		const s = stripComments(String(text ?? ""));
		const out = [];
		let i = 0;
		while (i < s.length) {
			while (i < s.length && /[\s;]/.test(s[i])) i++;
			if (i >= s.length) break;
			if (s[i] === "}") {
				i++;
				continue;
			}
			const [pre, j] = readUntil(s, i, ";{}");
			const prelude = pre.trim();
			if (s[j] !== "{") {
				// statement at-rule (@import, @charset, @layer a, b;) or junk
				if (prelude.startsWith("@")) {
					const m = /^@([\w-]+)\s*([\s\S]*)$/.exec(prelude);
					if (m) out.push(new CSSOtherRule(m[1].toLowerCase(), m[2], null, sheet, parent));
				}
				i = j + 1;
				continue;
			}
			const [body, k] = readBlock(s, j + 1);
			i = k;
			if (prelude.startsWith("@")) {
				const m = /^@([\w-]+)\s*([\s\S]*)$/.exec(prelude);
				if (!m) continue;
				const name = m[1].toLowerCase();
				const cond = m[2];
				if (name === "media") out.push(new CSSMediaRule(cond, body, sheet, parent));
				else if (name === "supports") out.push(new CSSSupportsRule(cond, body, sheet, parent));
				else if (name === "layer" || name === "scope" || name === "document" || name === "-moz-document") out.push(new CSSLayerBlockRule(cond, body, sheet, parent));
				else if (name === "font-face") out.push(new CSSFontFaceRule(body, sheet, parent));
				else out.push(new CSSOtherRule(name, cond, body, sheet, parent));
			} else if (prelude) styleRule(prelude, body, sheet, parent, out);
		}
		return out;
	};

	class StyleSheet {}
	class CSSStyleSheet extends StyleSheet {
		constructor(opts = {}) {
			super();
			this.cssRules = [];
			this.disabled = !!opts.disabled;
			this.ownerNode = null;
			this.ownerRule = null;
			this.href = null;
			this.title = null;
			this.type = "text/css";
			this._media = opts.media ?? "";
		}
		get rules() {
			return this.cssRules;
		}
		get media() {
			return { mediaText: String(this._media), length: this._media ? 1 : 0, item: () => this._media, appendMedium() {}, deleteMedium() {} };
		}
		insertRule(text, index = 0) {
			const rs = parseRules(String(text), this, null);
			if (!rs.length) throw new g.DOMException(`Failed to parse the rule '${text}'`, "SyntaxError");
			if (index < 0 || index > this.cssRules.length) throw new g.DOMException("Index out of range", "IndexSizeError");
			this.cssRules.splice(index, 0, ...rs);
			this._cssom = true;
			ruleChanged();
			return index;
		}
		addRule(sel, style, index) {
			return this.insertRule(`${sel} { ${style} }`, index ?? this.cssRules.length);
		}
		deleteRule(i) {
			this.cssRules.splice(i, 1);
			this._cssom = true;
			ruleChanged();
		}
		removeRule(i) {
			this.deleteRule(i ?? 0);
		}
		replaceSync(text) {
			text = String(text ?? "");
			this.cssRules = text.length > MAX_CSS_BYTES ? [] : parseRules(text, this, null);
			this._cssom = true;
			ruleChanged();
		}
		replace(text) {
			try {
				this.replaceSync(text);
				return Promise.resolve(this);
			} catch (e) {
				return Promise.reject(e);
			}
		}
	}
	Object.assign(g, { CSSRule, CSSStyleRule, CSSMediaRule, CSSSupportsRule, CSSFontFaceRule, CSSGroupingRule, CSSStyleSheet, StyleSheet });

	// A <style> element's sheet: reparsed when its text changes, unless the
	// plugin edited the sheet through the CSSOM since.
	const sheetOf = (el) => {
		const text = el.textContent;
		let sh = el._sheet;
		if (!sh) {
			sh = el._sheet = new CSSStyleSheet({ media: el.getAttribute("media") ?? "" });
			sh.ownerNode = el;
			sh._src = null;
		}
		if (sh._src !== text) {
			sh._src = text;
			sh.cssRules = text.length > MAX_CSS_BYTES ? [] : parseRules(text, sh, null);
			sh._cssom = false;
		}
		sh.disabled = sh.disabled || el.hasAttribute("disabled");
		return sh;
	};
	const styleElements = () => {
		const out = [];
		for (const e of D.descendants(g.document)) if (e.localName === "style") out.push(e);
		return out;
	};
	const documentSheets = () => {
		const list = styleElements().map(sheetOf);
		return Object.assign(list, { item: (i) => list[i] ?? null });
	};

	// ---------------------------------------------------------------- rule index
	// What TIDAL's own stylesheet does to its containers that a plugin can
	// take over by copying their class: the fullscreen now-playing container
	// covers the view (betterFullscreen swaps in its own player). A button a
	// plugin puts among the player bar's utility buttons gets TIDAL's
	// button look there: a transparent 32 x 32 box with its icon at 16 x 16
	// (OneClickLyrics' button, whose SVG says 32px). An artist card's
	// picture is a circle that a theme's plain `[class*="image"]` radius
	// (Meteor Night's, one class deep) does not change in TIDAL, while a
	// deeper rule (Giorgeous's `[class*="_card_"] [class*="_imageContainer_"]
	// [class*="round"]`) or an `!important` one (Tidal TUI's) does.
	let tidal = null;
	const tidalSheet = () => {
		if (!tidal) {
			tidal = new CSSStyleSheet();
			tidal.replaceSync(
				'[class^="_fullscreen_"] { position: absolute; inset: 0; z-index: 10; }\n' +
					'[class^="_utilityButtons_"] > button { display: flex; width: 32px; height: 32px; padding: 1px 6px; border: 0; background-color: transparent; align-items: center; justify-content: center; cursor: pointer; }\n' +
					'[class^="_utilityButtons_"] > button svg { width: 16px; height: 16px; }\n' +
					"._artistVariant_63c8c96 ._round_5776dee { border-radius: 50%; }",
			);
		}
		return tidal;
	};
	// TIDAL's look of its own elements, for plugin elements that copy their
	// class (AltPlay's sidebar link takes `a[data-test=sidebar-music]`'s
	// `_navigationItem_`), measured in TIDAL 2.43.2: a 208 x 40 row (its box
	// with the padding), 16 in
	// from the sidebar's edges, 8 padding and gap, 12 corners, 14-point
	// semibold at 55 % white; white on a 10 % white fill when selected.
	// The now-playing header's tabs (`_tabButton_`, LangRomanizer's button
	// copies the Lyrics tab's classes) look like the pills Tidalfast draws
	// beside them: 30 high, 11 in, 14-point semibold in their grey, a white
	// pill with near-black words when pressed (TIDAL's own tabs all look
	// alike too).
	// Tidalfast draws its own elements itself, so these never apply to them.
	let looks = null;
	const looksSheet = () => {
		if (!looks) {
			looks = new CSSStyleSheet();
			looks.replaceSync(
				'[class*="_navigationItem_"] { display: flex; align-items: center; gap: 8px; padding: 8px; margin: 0 16px; width: 192px; height: 24px; border-radius: 12px; font-size: 14px; font-weight: 600; color: rgba(255, 255, 255, 0.55); }' +
					'[class*="_navigationItem_"][class*="_selected_"] { color: rgb(255, 255, 255); background-color: rgba(255, 255, 255, 0.1); }' +
					'[class*="_tabButton_"] { display: flex; align-items: center; height: 30px; padding: 0 11px; border-radius: 15px; font-size: 14px; font-weight: 600; color: #b4b4b8; }' +
					'[class*="_tabButton_"][aria-pressed="true"] { color: #101010; background-color: #fff; }',
			);
		}
		return looks;
	};
	let index = null;
	let indexKey = "";
	// Nothing in the document changed since the index was checked: reuse it
	// without looking at every style element (getComputedStyle in a loop).
	let warnedCss = false;
	let indexGen = -1;
	let indexSize = "";
	const buildIndex = () => {
		const size = `${g.innerWidth}x${g.innerHeight}|${D.colorScheme}|${D.reducedMotion ? "reduce" : ""}`;
		if (index && indexGen === D.domGen() && indexSize === size) return index;
		indexGen = D.domGen();
		indexSize = size;
		const key = `${D.cssVersion()}|${size}`;
		const els = styleElements();
		const sheets = [tidalSheet(), looksSheet(), ...els.map(sheetOf), ...(g.document.adoptedStyleSheets ?? [])];
		const srcKey = `${key}|${els.length}|${sheets.map((s) => (s._src ?? "").length).join(",")}`;
		if (index && srcKey === indexKey) return index;
		indexKey = srcKey;
		const byId = new Map(),
			byClass = new Map(),
			byTag = new Map(),
			byAttr = new Map(),
			any = [];
		// `::before` / `::after` rules, indexed the same way: the anchors'
		// box layers (TIDAL paints header art and glass in them).
		const pseudoIx = () => ({ byId: new Map(), byClass: new Map(), byTag: new Map(), byAttr: new Map(), any: [], size: 0 });
		// A range input's `::-webkit-slider-runnable-track` and
		// `::-webkit-slider-thumb` (and Firefox's names) go the same way, and
		// `::selection` too: the colours of selected text.
		const pseudo = { before: pseudoIx(), after: pseudoIx(), track: pseudoIx(), thumb: pseudoIx(), selection: pseudoIx() };
		const fonts = [];
		const keyframes = new Map();
		registered.clear();
		let hover = false;
		// Some rule sets `user-select` (copy-lyrics makes the lyrics
		// selectable): the hosts it reaches are marked.
		let userSelect = false;
		// A selector that reads the `style` attribute: inline style changes
		// then may change which rules match anywhere.
		let styleAttr = false;
		// A selector that reads siblings or position (`+`, `~`, `:nth-child`,
		// `:empty`...): inserting or removing nodes may change what it
		// matches anywhere.
		let structural = false;
		// The compounds that carry a `:has(...)` (their other pseudo-classes
		// left out): a change inside an element they match may change whether
		// it matches, and so its whole subtree (scroll-to-playing marks the
		// playing row with `[aria-rowindex]:has(a[href*=...]) span`).
		const hasSteps = [];
		// The same for the rules that also read the style attribute, and
		// whether one of those looks at siblings: all an inline style
		// change can reach beyond its element and what is inside it.
		const hasStyleSteps = [];
		let styleSiblings = false;
		const hasIn = (c) => c.pseudos.some((p) => p.name === "has" || (p.list ?? []).some((x) => x.steps.some((st) => hasIn(st.c))));
		const attrNames = new Set();
		let order = 0;
		let bytes = 0;
		// Rules of `looksSheet`: plugin elements only.
		let pluginOnly = false;
		const add = (m, k, r) => {
			let a = m.get(k);
			if (!a) m.set(k, (a = []));
			a.push(r);
		};
		const walk = (rules) => {
			for (const r of rules) {
				if (r instanceof CSSStyleRule) {
					const list = r.selectors;
					if (!list) continue;
					if (!hover && r._sel.includes(":hover")) hover = true;
					const readsStyle = /\[\s*style\b/i.test(r._sel);
					if (readsStyle) styleAttr = true;
					if (readsStyle && /[+~]/.test(r._sel)) styleSiblings = true;
					if (!structural && /[+~]|:(nth-|first-child|last-child|only-child|empty|first-of-type|last-of-type|only-of-type)/i.test(r._sel)) structural = true;
					for (const m of r._sel.matchAll(/\[\s*([\w-]+)/g)) attrNames.add(m[1].toLowerCase());
					const decls = [...r._decl._m].map(([name, d]) => ({ name, value: d.value, important: d.important }));
					if (!decls.length) continue;
					if (!userSelect && decls.some((d) => /^(-webkit-)?user-select$/.test(d.name))) userSelect = true;
					if (r._sel.includes(":has("))
						for (const sel of list)
							for (const st of sel.steps)
								if (hasIn(st.c)) {
									const step = [{ c: { ...st.c, pseudos: [] } }];
									hasSteps.push(step);
									if (readsStyle) hasStyleSteps.push(step);
								}
					for (const sel of list) {
						const last = sel.steps.at(-1).c;
						let ix = { byId, byClass, byTag, byAttr, any };
						let steps = sel.steps;
						if (last.pe) {
							const p = last.peName && pseudo[last.peName];
							if (!p) continue;
							// Match the element the pseudo-element belongs to.
							steps = [...steps.slice(0, -1), { ...steps.at(-1), c: { ...last, pe: false } }];
							ix = p;
							p.size++;
						}
						const entry = { steps, spec: sel.spec, order: order++, decls, pluginOnly };
						if (last.ids.length) add(ix.byId, last.ids[0], entry);
						else if (last.classes.length) add(ix.byClass, D.canonClass(last.classes[0]), entry);
						else if (last.tag && last.tag !== "*") add(ix.byTag, last.tag.toLowerCase(), entry);
						else if (last.attrs.length) add(ix.byAttr, last.attrs[0].name, entry);
						else ix.any.push(entry);
					}
				} else if (r instanceof CSSGroupingRule) {
					if (r.active) walk(r.cssRules);
				} else if (r instanceof CSSFontFaceRule) {
					const fam = r._decl.getPropertyValue("font-family").replace(/^["']|["']$/g, "").trim();
					const src = r._decl.getPropertyValue("src");
					const m = /url\(\s*(["']?)([^"')]+)\1\s*\)/.exec(src) ?? /local\(\s*(["']?)([^"')]+)\1\s*\)/.exec(src);
					if (fam && m) {
							const weight = r._decl.getPropertyValue("font-weight").trim();
							fonts.push(weight ? { family: fam, src: m[2], weight } : { family: fam, src: m[2] });
						}
				} else if (r instanceof CSSOtherRule && r.type === 7 && r._body) {
					const frames = parseKeyframes(r._body);
					if (frames.length) keyframes.set(r._prelude.replace(/^["']|["']$/g, ""), frames);
					// A variable an animation sets is defined while it runs.
					for (const [, decls] of frames) for (const k of decls.keys()) if (k.startsWith("--")) registered.add(k);
				} else if (r instanceof CSSOtherRule && r.name === "property" && r._prelude.startsWith("--")) {
					registered.add(r._prelude.trim());
				}
			}
		};
		let skipped = 0;
		for (const sh of sheets) {
			if (!sh || sh.disabled) continue;
			if (sh._media && !D.evalMedia(sh._media)) continue;
			pluginOnly = sh === looks;
			if (sh !== tidal && sh !== looks) {
				const size = (sh._src ?? "").length || sh.cssRules.reduce((n, r) => n + (r.cssText?.length ?? 0), 0);
				if (bytes + size > MAX_CSS_BYTES) {
					skipped++;
					continue;
				}
				bytes += size;
			}
			walk(sh.cssRules);
		}
		if (skipped && !warnedCss) L.callOp("log", { plugin: null, level: "warn", msg: `${skipped} style sheet(s) left out: plugin and theme CSS is capped at ${MAX_CSS_BYTES >> 20} MB` });
		warnedCss = skipped > 0;
		index = { byId, byClass, byTag, byAttr, any, fonts, keyframes, hover, styleAttr, structural, hasSteps, hasStyleSteps, styleSiblings, attrNames, bytes, size: order, pseudo, userSelect };
		return index;
	};
	// `@keyframes` body -> [[offset 0..1, Map name -> value]], sorted.
	const parseKeyframes = (body) => {
		const out = [];
		let i = 0;
		while (i < body.length) {
			const [pre, j] = readUntil(body, i, "{");
			if (j >= body.length) break;
			const [inner, k] = readBlock(body, j + 1);
			i = k;
			const decls = new Map();
			for (const part of splitDecls(inner)) {
				const c = part.indexOf(":");
				if (c > 0) decls.set(part.slice(0, c).trim().toLowerCase(), part.slice(c + 1).replace(/!important\s*$/i, "").trim());
			}
			for (const sel of pre.split(",")) {
				const s = sel.trim().toLowerCase();
				const off = s === "from" ? 0 : s === "to" ? 1 : /%$/.test(s) ? parseFloat(s) / 100 : NaN;
				if (Number.isFinite(off)) out.push([Math.min(1, Math.max(0, off)), decls]);
			}
		}
		return out.sort((a, b) => a[0] - b[0]);
	};
	const usesHover = () => !!buildIndex().hover;
	const candidates = (ix, el) => {
		const out = [...ix.any];
		const id = el._attrs.get("id");
		if (id) out.push(...(ix.byId.get(id) ?? []));
		const cls = el._attrs.get("class");
		if (cls) for (const c of new Set(cls.split(/\s+/).map(D.canonClass))) if (c) out.push(...(ix.byClass.get(c) ?? []));
		out.push(...(ix.byTag.get(el.localName.toLowerCase()) ?? []));
		for (const a of el._attrs.keys()) out.push(...(ix.byAttr.get(a.toLowerCase()) ?? []));
		if (el._style?.length && !el._attrs.has("style")) out.push(...(ix.byAttr.get("style") ?? []));
		return out;
	};

	// ---------------------------------------------------------------- cascade
	// The winning declarations of `el`: Map name -> value (unresolved).
	// Cascades by element, good while the document and the index stay as
	// they are (getComputedStyle on every element walks each one's ancestors).
	let memo = new WeakMap();
	// The `::before`/`::after` styles of an anchor, good while its cascade is
	// (forgotten and reset with `memo`).
	let pmemo = new WeakMap();
	let memoIx = null;
	let memoGen = -1;
	let memoStyleGen = -1;
	let memoStructGen = -1;
	const forgetSubtree = (n) => {
		memo.delete(n);
		pmemo.delete(n);
		for (const c of n.childNodes ?? []) forgetSubtree(c);
	};
	// Writes a declaration so the map's order is the cascade's: a property
	// written again moves to the end (a Map keeps a key's first place), and
	// the `background`, `margin` and `padding` shorthands reset their
	// longhands, as in CSS.
	const put = (m, name, value) => {
		m.delete(name);
		if (name === "background") {
			m.delete("background-color");
			m.delete("background-image");
		} else if (name === "margin" || name === "padding") {
			for (const side of ["top", "right", "bottom", "left"]) m.delete(`${name}-${side}`);
		}
		m.set(name, value);
	};
	const cascade = (ix, el) => {
		if (memoIx !== ix || memoGen !== D.domGen() || (memoStructGen !== D.structGen() && ix.structural)) {
			if (L.stat) L.stat(memoIx !== ix ? "recascade: css" : memoGen !== D.domGen() ? "recascade: dom" : "recascade: structure", Date.now());
			D.cascadeResets = (D.cascadeResets ?? 0) + 1;
			memo = new WeakMap();
			pmemo = new WeakMap();
			memoIx = ix;
			memoGen = D.domGen();
			memoStyleGen = D.styleGen();
			memoStructGen = D.structGen();
			D.takeRestyled();
			D.takeInserted();
			D.takeDetached();
		}
		if (memoStructGen !== D.structGen()) {
			// Nodes came and went (or an element's class changed), and no
			// selector looks at siblings: only what was inserted (it may have
			// moved under other ancestors) is matched again, and the subtree of
			// each ancestor a `:has(...)` rule may match.
			memoStructGen = D.structGen();
			const inserted = D.takeInserted();
			const detached = D.takeDetached();
			for (const n of inserted) forgetSubtree(n);
			if (ix.hasSteps.length) {
				const done = new Set();
				const up = (a) => {
					for (let x = a; x instanceof g.Element && !done.has(x); x = x.parentElement) {
						done.add(x);
						if (ix.hasSteps.some((st) => D.sel.matchSteps(x, st, null))) forgetSubtree(x);
					}
				};
				for (const n of inserted) up(n.parentElement);
				for (const p of detached) up(p);
			}
		}
		if (memoStyleGen !== D.styleGen()) {
			// Only inline styles changed: redo those elements.
			memoStyleGen = D.styleGen();
			const els = D.takeRestyled();
			// A selector that reads the style attribute (Lunallena's
			// `[style="width: 240px;"]`) can match differently on the element
			// and what is inside it (`[style] > i`), on the siblings after it
			// (`[style] + i`) and on the ancestors that have it (`:has`): those
			// are matched again and every other element keeps its cascade
			// (TIDAL's progress bar rewrites its style every second, a full
			// recascade of the page each time).
			if (ix.styleAttr) {
				const done = new Set();
				for (const e of els) {
					forgetSubtree(e);
					if (ix.styleSiblings) for (let n = e.nextSibling; n; n = n.nextSibling) forgetSubtree(n);
					for (let x = ix.hasStyleSteps.length ? e.parentElement : null; x instanceof g.Element && !done.has(x); x = x.parentElement) {
						done.add(x);
						if (ix.hasStyleSteps.some((st) => D.sel.matchSteps(x, st, null))) forgetSubtree(x);
					}
				}
			} else for (const e of els) { memo.delete(e); pmemo.delete(e); }
		}
		if (memo.has(el)) return memo.get(el);
		const t0 = L.stat ? Date.now() : 0;
		const out = cascadeNow(ix, el);
		if (L.stat) L.stat("css: cascade one", t0);
		memo.set(el, out);
		return out;
	};
	const cascadeNow = (ix, el) => {
		// Counted for the tests that check what a change keeps.
		D.cascadeRuns = (D.cascadeRuns ?? 0) + 1;
		const live = el.isConnected;
		const matched = ix.size ? candidates(ix, el).filter((r) => !(r.pluginOnly && el._hostKey !== undefined) && D.sel.matchSteps(el, r.steps, null, live)) : [];
		const inline = el._style?._m;
		if (!matched.length && !inline?.size) return null;
		matched.sort((a, b) => D.sel.cmpSpec(a.spec, b.spec) || a.order - b.order);
		const normal = new Map();
		const imp = new Map();
		for (const r of matched) for (const d of r.decls) put(d.important ? imp : normal, d.name, d.value);
		if (inline) for (const [k, d] of inline) put(d.important ? imp : normal, k, d.value);
		for (const [k, v] of imp) put(normal, k, v);
		return normal;
	};
	// The winning declarations of an element's `::before`/`::after` (no
	// inline style: that is the element's own).
	const pseudoCascade = (px, el) => {
		const matched = candidates(px, el).filter((r) => D.sel.matchSteps(el, r.steps, null, true));
		if (!matched.length) return null;
		matched.sort((a, b) => D.sel.cmpSpec(a.spec, b.spec) || a.order - b.order);
		const normal = new Map();
		const imp = new Map();
		for (const r of matched) for (const d of r.decls) put(d.important ? imp : normal, d.name, d.value);
		for (const [k, v] of imp) put(normal, k, v);
		return normal;
	};
	// Resolve var(--x, fallback) against `vars` (Map of unresolved values).
	// Unknown variables without a fallback stay as written, for the UI's theme.
	const resolveVars = (value, vars, depth = 0) => {
		if (!value.includes("var(") || depth > 16) return value;
		let out = "";
		let i = 0;
		for (;;) {
			const at = value.indexOf("var(", i);
			if (at < 0) break;
			out += value.slice(i, at);
			const [inner, end] = readUntil(value, at + 4, ")");
			const comma = readUntil(inner, 0, ",")[1];
			const name = inner.slice(0, comma).trim();
			const fb = comma < inner.length ? inner.slice(comma + 1).trim() : null;
			const v = vars.get(name);
			if (v !== undefined && v.trim() !== "" && v.trim() !== "initial") out += resolveVars(v, vars, depth + 1);
			else if (fb !== null) out += resolveVars(fb, vars, depth + 1);
			else out += `var(${name})`;
			i = end + 1;
		}
		return out + value.slice(i);
	};
	D.resolveVars = resolveVars;
	// A `var(--x)` left in a resolved value names a variable nothing defines:
	// TIDAL's `--wave-*` tokens (resolved later to the app's palette) and a
	// gradient's first argument (an angle `@property` registers, which an
	// animation turns: `linear-gradient(var(--rotate), ...)`) excepted.
	// Variables `@property` registers or a `@keyframes` animates: defined
	// in a browser though no rule declares them (Cotton Candy Twilight's
	// stars, `hsla(56, 100%, 90%, var(--star1))`, twinkle by `--star1`).
	const registered = new Set();
	const undefinedVar = (v) => {
		if (!v.includes("var(--")) return false;
		for (const m of v.matchAll(/var\((--[\w-]+)\)/g)) {
			if (m[1].startsWith("--wave-") || registered.has(m[1])) continue;
			if (/gradient\(\s*$/i.test(v.slice(0, m.index))) continue;
			return m[1];
		}
		return false;
	};
	// Variable families TIDAL itself declares below the root (`--market-*`
	// on its market components, `--frosted-*`, `--player-*`, ...): one a
	// theme names may well be defined in TIDAL where Tidalfast has no value,
	// so a declaration naming one is only left out, never made `unset`.
	const TIDAL_VAR = /^--(market|frosted|player|rounded|desktop|z|floating|os|swiper|spotlight|headliners|onboarding|user)-/;
	// What an `unset` declaration computes to where Tidalfast has no parent
	// value: TIDAL's body text (14px, weight 500, 20px lines) for inherited
	// properties, the initial value for the boxes the app paints.
	// A colour stays the parent's own: `inherit`, which a painter reads as
	// "the colour around it" (the cotton candy family's `color:
	// var(--SBTXT) !important` on the sidebar's names, with no value, keeps
	// the playing playlist's name the other names' colour, not the accent).
	const UNSET_INHERITED = { "font-size": "14px", "font-weight": "500", "line-height": "20px", color: "inherit" };
	const UNSET_INITIAL = { "background-color": "transparent", "background-image": "none", "border-radius": "0px", "box-shadow": "none" };
	const STYLE_FIELDS = new Set(["display", "visibility", "color", "background-color", "background-image", "font-family", "font-size", "font-weight", "font-style", "text-decoration", "padding", "margin", "margin-top", "margin-bottom", "margin-left", "margin-right", "column-gap", "padding-top", "padding-left", "padding-right", "padding-bottom", "border", "border-radius", "opacity", "width", "height", "gap", "flex-direction", "align-items", "justify-content", "cursor", "transform", "filter", "position", "top", "right", "bottom", "left", "z-index", "box-shadow", "min-width", "max-width", "min-height", "max-height", "text-align", "line-height", "overflow", "outline", "outline-offset", "border-color", "letter-spacing", "text-transform", "fill", "white-space", "flex", "flex-wrap", "order", "pointer-events", "background-position", "background-size", "image-rendering", "text-shadow", "text-overflow", "object-fit", "grid-template-columns", "overflow-y", "list-style-type"]);
	const ANIM_LONG = ["animation-name", "animation-duration", "animation-delay", "animation-iteration-count", "animation-direction", "animation-timing-function", "animation-play-state", "animation-fill-mode"];
	const seconds = (t) => (/ms$/i.test(t) ? parseFloat(t) / 1000 : parseFloat(t));
	// A timing function as one word for the lists below: lower case, no spaces
	// around commas, other spaces as `_` (`linear(0, 0.25 75%, 1)` keeps its
	// percentages as `linear(0,0.25_75%,1)`).
	const timingWord = (t) => String(t).trim().toLowerCase().replace(/\s*([(),])\s*/g, "$1").replace(/\s+/g, "_");
	const TIMING = /^(linear|ease|ease-in|ease-out|ease-in-out|step-start|step-end)$|^(steps|cubic-bezier|linear)\(/i;
	// `animation` shorthand or longhands -> "name duration delay count direction timing state fill"
	// per animation, comma separated (count `infinite` or a number).
	const animation = (decls) => {
		const get = (k) => decls.get(k);
		const list = [];
		const fresh = () => ({ name: "", dur: 0, delay: 0, count: "1", dir: "normal", timing: "ease", state: "running", fill: "none" });
		const short = get("animation");
		if (short && !/^none$/i.test(short.trim())) {
			for (const part of splitTop(short)) {
				const a = fresh();
				let times = 0;
				for (const tok of part.match(/[\w-]+\([^)]*\)|[^\s]+/g) ?? []) {
					if (/^-?[\d.]+m?s$/i.test(tok)) {
						if (times++ === 0) a.dur = seconds(tok);
						else a.delay = seconds(tok);
					} else if (/^(infinite|[\d.]+)$/i.test(tok)) a.count = tok.toLowerCase();
					else if (/^(normal|reverse|alternate|alternate-reverse)$/i.test(tok)) a.dir = tok.toLowerCase();
					else if (TIMING.test(tok)) a.timing = timingWord(tok);
					else if (/^(paused|running)$/i.test(tok)) a.state = tok.toLowerCase();
					else if (/^(forwards|backwards|both)$/i.test(tok)) a.fill = tok.toLowerCase();
					else if (/^none$/i.test(tok)) {
						/* fill mode `none` */
					} else a.name = tok.replace(/^["']|["']$/g, "");
				}
				list.push(a);
			}
		}
		const at = (k, i, f) => {
			const v = get(k);
			if (!v) return undefined;
			const p = splitTop(v);
			return f(p[i % p.length].trim());
		};
		const names = get("animation-name");
		if (names) {
			splitTop(names).forEach((n, i) => {
				const a = list[i] ?? (list[i] = fresh());
				a.name = n.trim().replace(/^["']|["']$/g, "");
			});
		}
		// Longhands after the shorthand (or with the names) adjust each one.
		list.forEach((a, i) => {
			a.dur = at("animation-duration", i, seconds) ?? a.dur;
			a.delay = at("animation-delay", i, seconds) ?? a.delay;
			a.count = at("animation-iteration-count", i, (x) => x.toLowerCase()) ?? a.count;
			a.dir = at("animation-direction", i, (x) => x.toLowerCase()) ?? a.dir;
			a.timing = at("animation-timing-function", i, timingWord) ?? a.timing;
			a.fill = at("animation-fill-mode", i, (x) => x.toLowerCase()) ?? a.fill;
			a.state = at("animation-play-state", i, (x) => (/paused/i.test(x) ? "paused" : "running")) ?? a.state;
		});
		const ok = list.filter((a) => a.name && a.name !== "none" && a.dur > 0 && Number.isFinite(a.dur));
		return ok.length ? ok.map((a) => `${a.name} ${a.dur} ${Number.isFinite(a.delay) ? a.delay : 0} ${a.count} ${a.dir} ${a.timing} ${a.state} ${/^(forwards|backwards|both)$/.test(a.fill) ? a.fill : "none"}`).join(",") : null;
	};
	// `transition` shorthand or longhands -> "property duration delay timing" per
	// transition, comma separated (`all`, `opacity`, `transform`...).
	const transition = (decls) => {
		const get = (k) => decls.get(k);
		const list = [];
		const fresh = () => ({ prop: "all", dur: 0, delay: 0, timing: "ease" });
		const short = get("transition");
		if (short && !/^none$/i.test(short.trim())) {
			for (const part of splitTop(short)) {
				const t = fresh();
				let times = 0;
				for (const tok of part.match(/[\w-]+\([^)]*\)|[^\s]+/g) ?? []) {
					if (/^-?[\d.]+m?s$/i.test(tok)) {
						if (times++ === 0) t.dur = seconds(tok);
						else t.delay = seconds(tok);
					} else if (TIMING.test(tok)) t.timing = timingWord(tok);
					else t.prop = tok.toLowerCase();
				}
				list.push(t);
			}
		}
		const props = get("transition-property");
		if (props) {
			// `transition-property: none` stops every transition.
			if (/^none$/i.test(props.trim())) return null;
			splitTop(props).forEach((p, i) => {
				const t = list[i] ?? (list[i] = fresh());
				t.prop = p.trim().toLowerCase();
			});
		}
		const at = (k, i, f) => {
			const v = get(k);
			if (!v) return undefined;
			const p = splitTop(v);
			return f(p[i % p.length].trim());
		};
		list.forEach((t, i) => {
			t.dur = at("transition-duration", i, seconds) ?? t.dur;
			t.delay = at("transition-delay", i, seconds) ?? t.delay;
			t.timing = at("transition-timing-function", i, timingWord) ?? t.timing;
		});
		const ok = list.filter((t) => t.dur > 0 && Number.isFinite(t.dur) && t.prop !== "none");
		return ok.length ? ok.map((t) => `${t.prop} ${t.dur} ${Number.isFinite(t.delay) ? t.delay : 0} ${t.timing}`).join(",") : null;
	};
	const splitTop = (s) => {
		const out = [];
		let i = 0;
		while (i <= s.length) {
			const [p, j] = readUntil(s, i, ",");
			if (p.trim()) out.push(p.trim());
			i = j + 1;
		}
		return out;
	};
	const INHERITED = new Set(["color", "font-family", "font-size", "font-weight", "font-style", "visibility", "line-height", "text-align", "cursor", "letter-spacing", "text-transform", "fill", "white-space", "direction", "list-style-type"]);
	const isImage = (v) => /url\(|gradient\(|image-set\(/i.test(v);
	// The colour in a `background` shorthand without an image (`0 0`,
	// `red`, `center / cover #111`): transparent when it names none.
	const BG_WORDS = /^(top|bottom|left|right|center|repeat|repeat-x|repeat-y|no-repeat|space|round|scroll|fixed|local|cover|contain|auto|border-box|padding-box|content-box|text|\/)$/i;
	const bgColor = (v) => {
		const words = [];
		let depth = 0;
		let cur = "";
		for (const ch of v.trim()) {
			if (ch === "(") depth++;
			if (ch === ")") depth--;
			if (/\s/.test(ch) && depth === 0) {
				if (cur) words.push(cur);
				cur = "";
			} else cur += ch;
		}
		if (cur) words.push(cur);
		const c = words.find((w) => !BG_WORDS.test(w) && !/^-?[\d.]+(px|%|em|rem|vh|vw)?$/i.test(w) && !w.includes("/"));
		return c ?? "transparent";
	};
	const FONT_KEYS = new Set(["font-family", "font-size", "font-weight", "font-style"]);
	// Winning declarations -> the overlay's Style (only what the renderer knows).
	const toStyle = (decls, vars, parentStyle) => {
		const st = {};
		const set = (k, v) => {
			v = String(v).trim();
			if (!v || /^(initial|unset|revert|revert-layer)$/i.test(v)) return;
			if (/^inherit$/i.test(v)) {
				if (parentStyle?.[k] !== undefined) st[k] = parentStyle[k];
				// Said outright, for the UI: a button's font is not the
				// page's unless asked for (`font: inherit`, a reset).
				else if (FONT_KEYS.has(k)) st[k] = "inherit";
				return;
			}
			st[k] = v;
		};
		for (const [name, raw] of decls) {
			if (name.startsWith("--")) continue;
			const v = resolveVars(raw, vars);
			// A variable nothing defines, without a fallback, makes the whole
			// declaration invalid at computed-value time: the property is
			// `unset` (inherited from the parent or its initial value), as in
			// a browser. Tidal TUI's `outline: 14px solid var(--tui-main)`
			// draws nothing in TIDAL when its settings file is not installed.
			const missing = undefinedVar(v);
			if (missing) {
				// Inherited properties take the parent's value; with none
				// known, TIDAL's body text (Tidal TUI's `font-size:
				// var(--font-size)` on every heading and label: 14px in TIDAL).
				// Others take their initial value: its `[role="menu"] {
				// background-color: var(--tui-main) }` leaves TIDAL's menus
				// see-through and `border-radius: var(--border-radius)` its
				// Play and Shuffle square.
				if (INHERITED.has(name)) {
					if (parentStyle?.[name] !== undefined) st[name] = parentStyle[name];
					else if (!TIDAL_VAR.test(missing) && UNSET_INHERITED[name]) st[name] = UNSET_INHERITED[name];
				} else if (!TIDAL_VAR.test(missing)) {
					if (UNSET_INITIAL[name]) st[name] = UNSET_INITIAL[name];
					else if (name === "background") {
						st["background-color"] = "transparent";
						st["background-image"] = "none";
					}
				}
				continue;
			}
			if (name === "background-color" && /^(initial|unset)$/i.test(v.trim())) set(name, "transparent");
			else if (STYLE_FIELDS.has(name)) set(name, v);
			else if (name === "background") {
				if (isImage(v)) set("background-image", v);
				else if (/^none$/i.test(v.trim())) set("background-image", "none");
				// Backgrounds are not inherited: `unset` is `initial`, which is
				// no colour (radiant-lyrics' `main { background: unset }` takes
				// TIDAL's black away so its backdrop shows).
				else if (/^(initial|unset)$/i.test(v.trim())) set("background-color", "transparent");
				else if (/^(inherit|revert|revert-layer)$/i.test(v.trim())) set("background-color", v);
				else set("background-color", bgColor(v));
			} else if (name === "font") {
				// The shorthand: `inherit`, or [style] [weight] size[/line] family.
				const t = v.trim();
				if (/^(inherit|initial|unset|revert|revert-layer)$/i.test(t)) {
					for (const k of FONT_KEYS) set(k, t);
				} else {
					const m = /^((?:(?:italic|oblique|normal|small-caps|bold|bolder|lighter|[1-9]00)\s+)*)([\d.]+(?:px|em|rem|pt|%)|xx-small|x-small|small|medium|large|x-large|xx-large)(?:\s*\/\s*\S+)?\s+(.+)$/i.exec(t);
					if (m) {
						for (const w of m[1].trim().split(/\s+/).filter(Boolean)) {
							if (/^(italic|oblique)$/i.test(w)) set("font-style", w);
							else if (/^(bold|bolder|lighter|[1-9]00)$/i.test(w)) set("font-weight", w);
						}
						set("font-size", m[2]);
						set("font-family", m[3]);
					}
				}
			} else if (name === "inset") {
				const p = v.split(/\s+/);
				const [t, r = t, b = t, l = r] = p;
				set("top", t);
				set("right", r);
				set("bottom", b);
				set("left", l);
			} else if (name === "text-decoration-line") set("text-decoration", v);
			else if (name === "-webkit-filter") set("filter", v);
			else if (name === "backdrop-filter" || name === "-webkit-backdrop-filter") {
				if (!/^none$/i.test(v.trim())) set("backdrop-filter", v);
			}
			else if (name === "translate" || name === "rotate" || name === "scale") {
				if (!/^none$/i.test(v.trim())) set(`-${name}`, v);
			}
			// `list-style: none` (or `square inside`): the marker's type, which
			// list items inherit from their list.
			else if (name === "list-style") {
				const type = v.trim().split(/\s+/).find((w) => !/^(inside|outside|url\(.*)$/i.test(w));
				if (type) set("list-style-type", type);
			}
		}
		// `margin-left` / `-right` (LuckyLP's 16px gap after the title) and
		// the other sides written apart from `margin`: the later of the
		// shorthand and each side wins, in one `margin` value.
		if (decls.has("margin-left") || decls.has("margin-right")) {
			const keys = [...decls.keys()];
			const short = keys.indexOf("margin");
			const [t, r = t, b = t, l = r] = (st.margin ?? "0").trim().split(/\s+/);
			const side = { "margin-top": t, "margin-right": r, "margin-bottom": b, "margin-left": l };
			for (const k of Object.keys(side)) {
				if (!decls.has(k) || keys.indexOf(k) < short) continue;
				const v = resolveVars(decls.get(k), vars).trim();
				if (v && !/^(initial|unset|revert|revert-layer|inherit)$/i.test(v)) side[k] = v;
			}
			st.margin = Object.values(side).join(" ");
		}
		// `outline-style` / `-width` / `-color` written apart (the cotton candy
		// family's thumbnails) make one `outline` value; the width defaults
		// to `medium` (3px) and the colour to `currentcolor`.
		if (decls.has("outline-style") && !decls.has("outline")) {
			const get = (k) => (decls.has(k) ? resolveVars(decls.get(k), vars).trim() : "");
			const style = get("outline-style");
			if (style && !/^(none|hidden)$/i.test(style)) set("outline", `${get("outline-width") || "3px"} ${style} ${get("outline-color") || "currentcolor"}`);
		}
		// `border-style` / `-width` / `-color` written apart (the cotton candy
		// family's track list and queue panels: `border-style: solid;
		// border-width: 2px; border-color: ...`) make one `border` value, as
		// later than a `border` shorthand written before them; the width
		// defaults to `medium` (3px) and the colour to the element's `color`.
		if (decls.has("border-style")) {
			const keys = [...decls.keys()];
			const get = (k) => (decls.has(k) ? resolveVars(decls.get(k), vars).trim() : "");
			const style = get("border-style");
			if (style && keys.indexOf("border-style") > keys.indexOf("border") && !undefinedVar(style)) {
				// One to four sides' styles, top right bottom left.
				const [t, r = t, b = t, l = r] = style.split(/\s+/);
				const sides = [t, r, b, l].map((s) => !/^(none|hidden)$/i.test(s));
				if (!sides.some(Boolean)) delete st.border;
				else {
					let width = get("border-width");
					let colour = get("border-color");
					if (undefinedVar(width)) width = "";
					if (undefinedVar(colour)) colour = "";
					const short = st.border ? String(st.border).trim().split(/\s+/) : [];
					width ||= short.find((w) => /^[\d.]+[a-z%]*$/i.test(w) || /^(thin|medium|thick)$/i.test(w)) ?? "3px";
					// A `border-image` gradient paints the line (the family's
					// row separators fade in from both ends): its first stop
					// that is not see-through.
					if (!colour && decls.has("border-image")) {
						const g = /gradient\((.*)\)/is.exec(get("border-image"));
						const stops = g ? splitTop(g[1]).map((x) => x.trim()) : [];
						const solid = stops.find((x) => !/^(to\s|[\d.-]+(deg|turn|rad)\b)/i.test(x) && !/^(transparent|#0000(0000)?)\b/i.test(x) && !/^#[0-9a-f]{3}0\b/i.test(x));
						if (solid && !undefinedVar(solid)) colour = solid.replace(/\s+[\d.]+%(\s+[\d.]+%)?$/, "");
					}
					colour ||= st.color ?? parentStyle?.color ?? "currentcolor";
					set("border", `${width} ${[t, r, b, l][sides.indexOf(true)]} ${colour}`);
					// Sides with no style draw nothing (the family's track rows:
					// `border-style: solid none none none`, a line on top).
					if (!sides.every(Boolean)) st["border-width"] = sides.map((v) => (v ? width : "0px")).join(" ");
				}
			}
		}
		// A side's own colour (`border-top-color`, a spinner's bright arc, or
		// the colour in a side's shorthand: DownloadManager's nav items,
		// `border-left: 3px solid transparent`) makes `border-color` the four
		// sides' colours, top right bottom left, from `border-color` or the
		// `border` shorthand's colour.
		const SIDES = ["top", "right", "bottom", "left"];
		if (SIDES.some((s) => decls.has(`border-${s}-color`) || decls.has(`border-${s}`))) {
			const words = (v) => {
				const out = [];
				let depth = 0;
				let cur = "";
				for (const ch of String(v).trim()) {
					if (ch === "(") depth++;
					if (ch === ")") depth--;
					if (/\s/.test(ch) && depth === 0) {
						if (cur) out.push(cur);
						cur = "";
					} else cur += ch;
				}
				if (cur) out.push(cur);
				return out;
			};
			const isColour = (w) => /^(#|rgb|hsl|hwb|lab|lch|oklab|oklch|color|var\()/i.test(w) || /^[a-z]+$/i.test(w) && !/^(none|hidden|solid|dashed|dotted|double|groove|ridge|inset|outset|thin|medium|thick)$/i.test(w);
			let base = ["currentcolor"];
			if (st["border-color"]) base = words(st["border-color"]);
			else if (st.border) base = [words(st.border).find(isColour) ?? "currentcolor"];
			const [t, r = t, b = t, l = r] = base;
			const four = [t, r, b, l];
			// In the order they were written, the later winning.
			for (const [k, raw] of decls) {
				const m = /^border-(top|right|bottom|left)(-color)?$/.exec(k);
				if (!m) continue;
				const v = resolveVars(raw, vars).trim();
				const i = SIDES.indexOf(m[1]);
				if (m[2]) four[i] = v;
				else {
					const c = words(v).find(isColour);
					if (c) four[i] = c;
				}
			}
			st["border-color"] = four.join(" ");
		}
		// A side's own width (`border-bottom: 0`, `border-left-width`) makes
		// `border-width` the four sides' widths, top right bottom left, in
		// the order the declarations were written: a CSS triangle (a box with
		// no size and `border: 10px solid transparent; border-bottom: 0`).
		if (st.border && [...decls.keys()].some((k) => k === "border-width" || /^border-(top|right|bottom|left)(-width)?$/.test(k))) {
			const widthOf = (v) => {
				let w = null;
				let none = false;
				for (const word of String(v).trim().split(/\s+/)) {
					if (/^(none|hidden)$/i.test(word)) none = true;
					else if (/^(thin|medium|thick)$/i.test(word)) w = { thin: "1px", medium: "3px", thick: "5px" }[word.toLowerCase()];
					else if (/^-?[\d.]+[a-z%]*$/i.test(word) || /^(calc|min|max|clamp)\(/i.test(word)) w = /^0+(\.0*)?[a-z%]*$/i.test(word) ? "0px" : word;
				}
				return none ? "0px" : w;
			};
			const four = Array(4).fill(widthOf(st.border) ?? "3px");
			for (const [k, raw] of decls) {
				const v = resolveVars(raw, vars);
				if (k === "border") four.fill(widthOf(v) ?? "3px");
				else if (k === "border-width") {
					const p = v.trim().split(/\s+/);
					const [t, r = t, b = t, l = r] = p;
					four.splice(0, 4, t, r, b, l);
				} else {
					const m = /^border-(top|right|bottom|left)(-width)?$/.exec(k);
					if (!m) continue;
					const w = widthOf(v);
					if (w !== null) four[SIDES.indexOf(m[1])] = w;
					else if (!m[2]) four[SIDES.indexOf(m[1])] = "3px";
				}
			}
			if (four.some((w) => w !== four[0])) st["border-width"] = four.join(" ");
		}
		// The individual transform properties apply before `transform`.
		const parts = [];
		if (st["-translate"]) parts.push(`translate(${st["-translate"].trim().split(/\s+/).join(",")})`);
		if (st["-rotate"]) parts.push(`rotate(${st["-rotate"]})`);
		if (st["-scale"]) parts.push(`scale(${st["-scale"].trim().split(/\s+/).join(",")})`);
		delete st["-translate"];
		delete st["-rotate"];
		delete st["-scale"];
		if (parts.length) st.transform = [...parts, st.transform && !/^none$/i.test(st.transform) ? st.transform : ""].join(" ").trim();
		if ([...decls.keys()].some((k) => k.startsWith("transition"))) {
			const m = new Map();
			for (const [k, raw] of decls) if (k.startsWith("transition")) m.set(k, resolveVars(raw, vars));
			const t = transition(m);
			if (t) st.transition = t;
		}
		if ([...decls.keys()].some((k) => k.startsWith("animation"))) {
			const m = new Map();
			for (const [k, raw] of decls) if (k.startsWith("animation")) m.set(k, resolveVars(raw, vars));
			const a = animation(m);
			if (a) st.animation = a;
		}
		return Object.keys(st).length ? st : null;
	};
	const hasContent = (decls) => {
		const c = (decls.get("content") ?? "").trim();
		return c !== "" && !/^(none|normal)$/i.test(c);
	};
	// Anchors TIDAL's stylesheet generates no `::before`/`::after` for.
	const ownsNoPseudo = (el) => /(^|\s)_artistHeader_/.test(el.getAttribute?.("class") ?? "");
	// TIDAL's shortcut tile keeps its `::after` see-through at rest: Tidal
	// Light's `linear-gradient(#0000, #fff)` there leaves its tiles flat grey.
	const restsHidden = (el, kind) => kind === "after" && /(^|\s)_shortcutItem_/.test(el.getAttribute?.("class") ?? "");
	// What a theme's `::before` and `::after` rules say for the anchor `el`.
	const pseudoOf = (ix, el) => {
		D.pseudoRuns = (D.pseudoRuns ?? 0) + 1;
		let o = null;
		for (const kind of ["before", "after", "selection"]) {
			const px = ix.pseudo[kind];
			if (!px.size) continue;
			const decls = pseudoCascade(px, el);
			if (!decls) continue;
			if (kind === "selection") {
				const st = toStyle(decls, el._cc?.myVars ?? NO_VARS, null);
				if (st) (o ??= {}).selection = st;
				continue;
			}
			// A pseudo-element exists only with a `content`. TIDAL's own
			// stylesheet gives most anchors theirs (`_dataContainer_::before`,
			// `_smallHeader_::before`), which a theme restyles without one;
			// the artist header has none, so `[class*="Header"]::before`
			// (Mocha Blue's top-bar fill) draws no box over the portrait.
			if (!hasContent(decls) && ownsNoPseudo(el)) continue;
			if (restsHidden(el, kind)) continue;
			let vars = el._cc?.myVars ?? NO_VARS;
			for (const [k, v] of decls)
				if (k.startsWith("--")) {
					if (vars === (el._cc?.myVars ?? NO_VARS)) vars = new Map(vars);
					vars.set(k, v);
				}
			// The words it draws (Tidal TUI's `content: "Tabs"` box labels).
			const words = hasContent(decls) ? contentText(resolveVars(decls.get("content"), vars)) : "";
			const st = toStyle(decls, vars, el._cc?.inh ?? null) ?? (words.trim() ? {} : null);
			if (!st) continue;
			if (words.trim()) st.content = words;
			// `--img: none` on TIDAL's header art box drops the art its
			// own rule paints from that variable.
			if (/^none$/i.test((decls.get("--img") ?? "").trim()) && st["background-image"] === undefined) st["background-image"] = "none";
			(o ??= {})[kind] = st;
		}
		return o;
	};
	// The styles a range input's track and thumb rules give it, with the
	// variables in scope at the input (`--volume-percentage` in its inline
	// style moves HorizontalVolumeSlider's fill). `null` with no such rule.
	const rangeParts = (el) => {
		const ix = buildIndex();
		if (!ix.pseudo.track.size && !ix.pseudo.thumb.size) return null;
		let o = null;
		for (const kind of ["track", "thumb"]) {
			const px = ix.pseudo[kind];
			if (!px.size) continue;
			const decls = pseudoCascade(px, el);
			if (!decls) continue;
			let vars = el._cc?.myVars ?? NO_VARS;
			for (const [k, v] of decls)
				if (k.startsWith("--")) {
					if (vars === (el._cc?.myVars ?? NO_VARS)) vars = new Map(vars);
					vars.set(k, v);
				}
			const st = toStyle(decls, vars, el._cc?.inh ?? null);
			if (st) (o ??= {})[kind] = st;
		}
		return o;
	};
	// The text a `content` value draws: its quoted strings (`""` is an
	// empty box, a CSS triangle's); anything else draws nothing here.
	const contentText = (v) => {
		let out = "";
		String(v ?? "").replace(/"((?:[^"\\]|\\.)*)"|'((?:[^'\\]|\\.)*)'/g, (_, a, b) => {
			out += (a ?? b ?? "").replace(/\\(.)/g, "$1");
			return "";
		});
		return out;
	};
	// A plugin element's own `::before` / `::after` boxes (lyrics-everywhere's
	// speech-bubble tail): drawn as the element's first and last child, so
	// only a box with a `content` that is shown exists, as in CSS.
	const ownPseudoOf = (ix, el) => {
		let o = null;
		for (const kind of ["before", "after"]) {
			const px = ix.pseudo[kind];
			if (!px.size) continue;
			const decls = pseudoCascade(px, el);
			if (!decls || !hasContent(decls)) continue;
			let vars = el._cc?.myVars ?? NO_VARS;
			for (const [k, v] of decls)
				if (k.startsWith("--")) {
					if (vars === (el._cc?.myVars ?? NO_VARS)) vars = new Map(vars);
					vars.set(k, v);
				}
			const st = toStyle(decls, vars, el._cc?.inh ?? null) ?? {};
			if (/^none$/i.test(String(st.display ?? "").trim())) continue;
			(o ??= {})[kind] = { style: st, text: contentText(resolveVars(decls.get("content"), vars)) };
		}
		return o;
	};
	const ROOTS = () => [g.document.documentElement, g.document.body, D.wimp];
	// Resolved style of every element plugins styled, plus :root variables,
	// the page style and fonts.
	// TIDAL's own `:root` variables, as plugins read them back
	// (`getComputedStyle(document.documentElement)`) or use them in their
	// CSS; measured in TIDAL 2.43.2. They are not a theme's: they never
	// reach the overlay's variables, which recolour Tidalfast.
	const TIDAL_VARS = [
		["--wave-color-opacity-contrast-fill-thick", "#fff9"],
		["--wave-color-opacity-contrast-fill-ultra-thick", "#fffc"],
	];
	const NO_VARS = new Map(TIDAL_VARS);
	let rootMemo = null;
	const compute = () => {
		const t0 = L.stat ? Date.now() : 0;
		const ix = buildIndex();
		if (L.stat) L.stat("css: index", t0);
		const styles = new Map();
		const roots = ROOTS();
		const rootVars = new Map();
		const page = {};
		// `#wimp`'s own background: unlike `html`'s and `body`'s (the
		// canvas), it paints over elements behind the page (negative
		// `z-index`), so it is kept apart from `page` as well.
		let wimpFill;
		// Plugin elements' own `::before` / `::after` boxes.
		const boxes = new Map();
		// Anchors whose own rules set `--img: none` (Tidal TUI's
		// `[class*="dataContainer"] { --img: none }`): TIDAL's `::before`
		// paints the header art from that variable, so the art goes.
		const imgNone = [];
		const ownPseudo = ix.pseudo.before.size > 0 || ix.pseudo.after.size > 0;
		const walk = (el, vars, parentStyle) => {
			if (L.stat) L.stat("css: visit", Date.now());
			const decls = cascade(ix, el);
			// Same declarations, variables and inherited style as last time:
			// the same result, and the same objects, so the children's cache
			// holds too. Only what changed is resolved again.
			const cc = el._cc;
			let myVars, st, inh;
			if (cc && cc.decls === decls && cc.vars === vars && cc.parent === parentStyle) {
				({ myVars, st, inh } = cc);
			} else {
				myVars = vars;
				if (decls) {
					let own = null;
					for (const [k, v] of decls)
						if (k.startsWith("--")) {
							own ??= new Map(vars);
							own.set(k, v);
						}
					if (own) myVars = own;
				}
				st = decls ? toStyle(decls, myVars, parentStyle) : null;
				inh = st ? { ...(parentStyle ?? {}), ...pickInherited(st) } : parentStyle;
				el._cc = { decls, vars, parent: parentStyle, myVars, st, inh };
			}
			if (st) styles.set(el, st);
			if (decls && el._hostKey !== undefined && /^none$/i.test(String(decls.get("--img") ?? "").trim())) imgNone.push(el);
			if (ownPseudo && el._hostKey === undefined && !roots.includes(el)) {
				const hit = pmemo.get(el);
				let o;
				if (hit && hit.cc === el._cc && hit.ix === ix) o = hit.o;
				else {
					o = ownPseudoOf(ix, el);
					pmemo.set(el, { cc: el._cc, ix, o });
				}
				if (o) boxes.set(el, o);
			}
			if (roots.includes(el)) {
				if (decls) for (const [k, v] of decls) if (k.startsWith("--")) rootVars.set(k, v);
				if (st) Object.assign(page, st);
				if (el === D.wimp) wimpFill = st?.["background-color"];
			}
			for (const c of el.childNodes) if (c instanceof g.Element && c.localName !== "head" && c.localName !== "style" && c.localName !== "script") walk(c, myVars, inh);
		};
		const html = g.document.documentElement;
		const t1 = L.stat ? Date.now() : 0;
		if (html) walk(html, NO_VARS, null);
		if (L.stat) L.stat("css: walk", t1);
		// The anchors' `::before` / `::after` boxes, with their element's
		// variables (only anchors: plugin elements draw their own layers).
		// An anchor whose cascade and variables are as they were keeps the
		// boxes it had (the page's clock rewrites the tree every second).
		const pseudo = new Map();
		if (D.hosts && (ix.pseudo.before.size || ix.pseudo.after.size || ix.pseudo.selection.size)) {
			const t2 = L.stat ? Date.now() : 0;
			for (const el of D.hosts.values()) {
				if (!el.isConnected) continue;
				const hit = pmemo.get(el);
				let o;
				if (hit && hit.cc === el._cc && hit.ix === ix) o = hit.o;
				else {
					o = pseudoOf(ix, el);
					pmemo.set(el, { cc: el._cc, ix, o });
				}
				if (o) pseudo.set(el, o);
			}
			if (L.stat) L.stat("css: pseudo", t2);
		}
		for (const el of imgNone) {
			const o = pseudo.get(el) ?? {};
			if (o.before?.["background-image"] !== undefined) continue;
			pseudo.set(el, { ...o, before: { ...(o.before ?? {}), "background-image": "none" } });
		}
		// The root variables, resolved: the same ones as last time give the
		// same answer (a theme like Cotton Candy declares hundreds).
		const t3 = L.stat ? Date.now() : 0;
		let vars;
		const last = rootMemo;
		let same = last !== null && last.src.size === rootVars.size;
		if (same) for (const [k, v] of rootVars) if (last.src.get(k) !== v) { same = false; break; }
		if (same) vars = last.vars;
		else {
			D.rootResolves = (D.rootResolves ?? 0) + 1;
			vars = {};
			for (const [k, v] of rootVars) vars[k] = resolveVars(v, rootVars).trim();
			rootMemo = { src: rootVars, vars, keyframes: new Map() };
		}
		if (L.stat) L.stat("css: root vars", t3);
		// The @keyframes the styled elements use, as [offset, style] lists
		// (kept with the variables they were resolved against).
		const t4 = L.stat ? Date.now() : 0;
		const keyframes = {};
		const kfCache = rootMemo.keyframes;
		if (ix.keyframes.size)
			for (const st of styles.values()) {
				if (!st.animation) continue;
				for (const a of st.animation.split(",")) {
					const name = a.split(" ")[0];
					const frames = ix.keyframes.get(name);
					if (!frames || keyframes[name]) continue;
					const cached = kfCache.get(name);
					if (cached && cached.frames === frames) {
						keyframes[name] = cached.out;
						continue;
					}
					keyframes[name] = frames.map(([off, decls]) => {
						const frame = toStyle(decls, rootVars, null) ?? {};
						// `--rotate: 360deg`: a custom property an animation moves.
						for (const [k, v] of decls) if (k.startsWith("--")) (frame.custom ??= {})[k] = String(v).trim();
						// A keyframe's own easing, for the interval after it.
						if (decls.has("animation-timing-function")) (frame.custom ??= {})["animation-timing-function"] = timingWord(resolveVars(decls.get("animation-timing-function"), rootVars));
						return [off, frame];
					});
					kfCache.set(name, { frames, out: keyframes[name] });
				}
			}
		if (L.stat) L.stat("css: keyframes", t4);
		// The hosts whose text can be selected: the nearest `user-select`
		// up the tree says so (`auto` looks further up), as a browser has it.
		const selectable = new Set();
		if (D.hosts && ix.userSelect)
			for (const el of D.hosts.values()) {
				if (!el.isConnected) continue;
				for (let n = el; n instanceof g.Element; n = n.parentNode) {
					const d = cascade(ix, n);
					const v = String(d?.get("user-select") ?? d?.get("-webkit-user-select") ?? "auto").replace(/!important/i, "").trim().toLowerCase();
					if (v === "auto") continue;
					if (v !== "none") selectable.add(el);
					break;
				}
			}
		return { styles, vars, page, wimpFill, fonts: ix.fonts, bytes: ix.bytes, keyframes, hover: ix.hover, pseudo, boxes, selectable };
	};
	// For the tests that count what a build works out again.
	D.computeOverlayStyles = compute;
	const pickInherited = (st) => {
		const o = {};
		for (const k of Object.keys(st)) if (INHERITED.has(k)) o[k] = st[k];
		return o;
	};

	// ---------------------------------------------------------------- getComputedStyle
	const BLOCK = new Set(["div", "p", "section", "article", "main", "aside", "header", "footer", "nav", "ul", "ol", "li", "h1", "h2", "h3", "h4", "h5", "h6", "form", "figure", "html", "body", "table", "pre"]);
	const NONE = new Set(["head", "style", "script", "meta", "link", "title", "template"]);
	// What the cascade declares for `el` (variables resolved, inherited
	// properties carried down), with no used values: nothing here asks for
	// the element's box.
	const cascaded = (el) => {
		const ix = buildIndex();
		const chain = [];
		for (let n = el; n instanceof g.Element; n = n.parentNode) chain.unshift(n);
		let vars = new Map(TIDAL_VARS);
		const props = new Map();
		for (const n of chain) {
			const d = cascade(ix, n);
			const own = new Map();
			if (d) {
				for (const [k, v] of d) if (k.startsWith("--")) vars.set(k, v);
				for (const [k, v] of d) if (!k.startsWith("--")) own.set(k, resolveVars(v, vars));
			}
			for (const k of [...props.keys()]) if (!INHERITED.has(k)) props.delete(k);
			for (const [k, v] of own) {
				if (v === "inherit") continue;
				props.set(k, v);
			}
			if (n === el) vars = new Map(vars);
		}
		return { props, vars };
	};
	// The overlay style `el` has right now, with what it inherits, when it has
	// a `transition` (else null): the style a browser's style pass would
	// give it before the next change, from which a transition starts.
	const styleNow = (el) => {
		const ix = buildIndex();
		const own = cascade(ix, el);
		if (!own || ![...own.keys()].some((k) => k.startsWith("transition"))) return null;
		const chain = [];
		for (let n = el; n instanceof g.Element; n = n.parentNode) chain.unshift(n);
		let vars = NO_VARS;
		let parent = null;
		let st = null;
		for (const n of chain) {
			const d = n === el ? own : cascade(ix, n);
			if (d) {
				let mine = null;
				for (const [k, v] of d)
					if (k.startsWith("--")) {
						mine ??= new Map(vars);
						mine.set(k, v);
					}
				if (mine) vars = mine;
			}
			st = d ? toStyle(d, vars, parent) : null;
			parent = st ? { ...(parent ?? {}), ...pickInherited(st) } : parent;
		}
		return st;
	};
	const computedStyle = (el, pseudo) => {
		const decl = new CSSStyleDeclaration();
		if (!(el instanceof g.Element) || (pseudo && /::?(before|after|marker|placeholder|selection|-webkit-scrollbar)/.test(String(pseudo)))) return declProxy(decl);
		const { props, vars } = cascaded(el);
		const r = el.getBoundingClientRect();
		const defaults = {
			display: NONE.has(el.localName) ? "none" : BLOCK.has(el.localName) ? "block" : el.localName === "svg" || el.localName === "img" ? "inline-block" : "inline",
			visibility: "visible",
			opacity: "1",
			color: "rgb(255, 255, 255)",
			"background-color": "rgba(0, 0, 0, 0)",
			"font-size": "16px",
			"font-weight": "400",
			"font-family": "nationale, sans-serif",
			position: "static",
			"z-index": "auto",
			width: `${r.width}px`,
			height: `${r.height}px`,
			"line-height": "normal",
			transform: "none",
		};
		for (const [k, v] of Object.entries(defaults)) decl._m.set(k, { value: v, important: false });
		for (const [k, v] of props) decl._m.set(k, { value: v, important: false });
		// A transform resolves to the matrix a browser reports.
		if (props.has("transform") && L.parseTransform) {
			try {
				const { m, is2D } = L.parseTransform(props.get("transform"), [r.width, r.height]);
				const v = props.get("transform").trim() === "none" ? "none" : is2D ? `matrix(${[m[0], m[1], m[4], m[5], m[12], m[13]].join(", ")})` : `matrix3d(${m.join(", ")})`;
				decl._m.set("transform", { value: v, important: false });
			} catch {}
		}
		if (props.has("background")) decl._m.set(isImage(props.get("background")) ? "background-image" : "background-color", { value: props.get("background"), important: false });
		for (const [k, v] of vars) decl._m.set(k, { value: resolveVars(v, vars).trim(), important: false });
		Object.defineProperty(decl, "_owner", { value: null });
		const p = declProxy(decl);
		return p;
	};

	// Whether any selector names attribute `k` (from the last index; a sheet
	// change since then resets every cascade anyway). No index yet: yes.
	const attrMatters = (k) => !index || index.attrNames.has(String(k).toLowerCase());
	// Whether a rule looks at siblings, children or position: then a changed
	// class can change what matches outside the element's own subtree.
	const structural = () => !!buildIndex().structural;
	L.css = { parse: parseRules, sheetOf, documentSheets, compute, computedStyle, cascaded, styleNow, splitDecls, usesHover, attrMatters, structural, rangeParts };
})();
