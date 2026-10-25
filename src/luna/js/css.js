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
		// A plugin window's `<link rel="stylesheet">` holds the sheet it
		// loaded (page.js).
		const text = el.localName === "link" ? (el._linkText ?? "") : el.textContent;
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
		for (const e of D.descendants(g.document)) {
			if (e.localName === "style") out.push(e);
			else if (L.page && e.localName === "link") {
				// One a script put in the page loads now.
				if (e._linkState === undefined) L.pageLink?.(e);
				if (typeof e._linkText === "string") out.push(e);
			}
		}
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
	// [class*="round"]`) or an `!important` one (Tidal TUI's) does. A queue
	// row's artist line has TIDAL's own grey, which a theme's catch-all `*
	// { color }` (Meteor Night's white) does not reach in TIDAL and a class
	// rule does (`initial`: the app's own colour). A navigation row has
	// TIDAL's own colour on its class, so a theme's rule for a tag (Synthwave
	// '77's `button { color }`, which the Collection row is) does not reach
	// it in TIDAL and a class rule does. Its words and icon take the row's
	// colour over a theme's catch-all `* { color }` (Meteor Night's white
	// leaves TIDAL's inactive rows dim), while any rule naming them (one
	// class, TidalCustomTheme's `[class^="_marketText_"]`; a tag,
	// its `svg { fill }`) still wins: the rule adds no weight of its own.
	// The playing playlist's name in the sidebar and the queue's playing
	// title keep TIDAL's teal and accent through its own rules for their
	// quality classes (TIDAL 2.46.0, captured under Meteor Night's `* {
	// color }`), which a rule with more weight still beats (Blur's
	// `[class*="_marketText_"] { color: ... !important }`).
	// A track list's playing title has TIDAL's own rule as it is
	// (`._textHigh_bbc1075 { color: var(--wave-color-solid-accent-fill) }`,
	// TIDAL 2.46.0): a theme's accent colours it, a catch-all does not.
	// A context menu's title is in TIDAL's contrast token as it stands at
	// the menu: Cotton Candy Nights sets the token to black on
	// `._contextMenu_` alone, and TIDAL 2.46.0 draws the title black there
	// (white with no theme).
	// TIDAL's play queue panel is grey glass of its own (TIDAL 2.46.0:
	// `._container_b3e8f28 { background: linear-gradient(...) }` with a
	// backdrop blur): a theme's `background-color` lies under it (Blur,
	// Abyss Neptune, Lunallena), its `background` shorthand or
	// `background-image` replaces it (the cotton candy family, Solo v2).
	// Tidalfast draws that glass natively; the host only says it survived.
	const PANEL_GLASS = "linear-gradient(rgba(60, 60, 60, 0.35) 0%, rgba(70, 70, 70, 0.35) 100%)";
	// Its own half-point border too (TIDAL 2.46.0's `._container_b3e8f28 {
	// border: .5px solid rgba(255, 255, 255, .1) }`), drawn natively while
	// it stands: a theme's `border-style` and `border-color` for the panel
	// (the cotton candy family's) recolour TIDAL's line, as in TIDAL.
	const PANEL_BORDER = "0.5px solid rgba(255, 255, 255, 0.1)";
	// TIDAL's layout variables on its root (TIDAL 2.46.0's own sheet, the
	// narrow window's values under 577 points): themes size and shift their
	// boxes by them (the cotton candy family's `.mainContent {
	// padding-bottom: var(--miniPlayerHeight) }`). The container row's own
	// `--sidebarWidth` and `--playQueueWidth` are the app's real widths.
	const TIDAL_ROOT =
		":root { --pageWidth: 1500px; --resonancePageMaxWidth: 92vw; --pageHorizontalMargin: 28px; --pageHorizontalMarginWide: 48px; --pageBottomMargin: 48px; --pageMaxWidth: calc(var(--pageWidth) - var(--pageHorizontalMargin)); --headerHeight: 56px; --footerHeight: 96px; --tabbarHeight: 56px; --sidebarWidth: 240px; --responsiveSidebarWidth: 20vw; --collapsedSidebarWidth: 78px; --windowsTitleBarHeight: 30px; --miniPlayerHeight: 88px; --miniPlayerClearance: calc(var(--miniPlayerHeight) + 24px); --floating-panel-border-radius: 16px; }\n" +
		"@media (max-width: 576px) { :root { --pageHorizontalMargin: 16px; --pageHorizontalMarginWide: 20px; --pageBottomMargin: 20px; --miniPlayerHeight: 72px; --floating-panel-border-radius: 32px; } }\n" +
		".is-desktop-app { --floating-panel-border-radius: 12px; }\n" +
		// Its stacking levels and the profile gradient, which plugins build
		// on (betterFullscreen lays its background at `--z-index-low` and its
		// words at `--z-index-high` over a backdrop of the gradient).
		":root { --user-profile-linear-gradient: linear-gradient(160deg, #50a581 1.22%, #2e4958 40.51%, #0c0c0c 79.07%); --z-index-low: 10; --z-index-header: 20; --z-index-sidebar: 50; --z-index-play-queue: 45; --z-index-floating-aside: 70; --z-index-player: 80; --z-index-floating-aside-over-player: 90; --z-index-play-queue-over-player: 91; --z-index-search-popup: 95; --z-index-modal: 500; --z-index-context-menu: 550; --z-index-drag-layer: 600; --z-index-notifications: 700; --z-index-overlay: 900; --z-index-high-backdrop: 950; --z-index-high: 1000; }\n";
	let tidal = null;
	const tidalSheet = () => {
		if (!tidal) {
			tidal = new CSSStyleSheet();
			tidal.replaceSync(
				TIDAL_ROOT +
					// TIDAL 2.46.0's fullscreen now-playing section, in the root:
					// it covers the window, black, and shows only while the root
					// is `.is-fullscreen` (betterFullscreen takes its place with
					// its own player, copying its class).
					'[class^="_fullscreen_"] { background-color: #000; opacity: 0; pointer-events: none; z-index: 80; position: fixed; inset: 0; }\n' +
					'.is-fullscreen [class^="_fullscreen_"], .is-compact-mode [class^="_fullscreen_"] { opacity: 1; pointer-events: auto; }\n' +
					// TIDAL's own `withBackground` buttons (the queue button) keep
					// their disc: the bare button's look is for plugin buttons
					// that copy its class.
					'[class^="_utilityButtons_"] > button:where(:not(.withBackground)) { display: flex; width: 32px; height: 32px; padding: 1px 6px; border: 0; background-color: transparent; align-items: center; justify-content: center; cursor: pointer; }\n' +
					'[class^="_utilityButtons_"] > button svg { width: 16px; height: 16px; }\n' +
					// A disabled button in the player is at half strength
					// (TIDAL 2.46.0's own rule): OneClickLyrics' button for a
					// song without words.
					":is(._player_8c80ae6 button):disabled { opacity: 0.5; }\n" +
					"._artistVariant_63c8c96 ._round_5776dee { border-radius: 50%; }\n" +
					"._artistText_73966a1 { color: initial; }\n" +
					"._navigationItem_4e9deae { color: initial; }\n" +
					":where(._navigationItem_4e9deae) > ._marketText_1lyag_1 { color: initial; }\n" +
					":where(._navigationItem_4e9deae) > svg { color: initial; }\n" +
					"._qualityHigh_39ef787:is(._textContent_462c76e ._title_8b245d1, ._textContent_462c76e ._subtitle_c0fd197) { color: initial; }\n" +
					"._textHigh_ee6a2ff, ._textHigh_ee6a2ff span { color: initial; }\n" +
					"._textHigh_bbc1075 { color: var(--wave-color-solid-accent-fill); }\n" +
					"._title_2bc5e10 { color: var(--wave-color-solid-contrast-fill); }\n" +
					// TIDAL's medium market buttons (the header's Play and
					// Shuffle) set their text in 16 points, so `em` in a
					// theme's corners for them is 16 (TIDAL 2.46.0, computed).
					":where(._marketButton_1d16h_1._medium_1d16h_4) { font-size: 16px; }\n" +
					// An album's and playlist's title is TIDAL's 32-point
					// title text: `em` in a theme's lengths for it is 32
					// (the cotton candy family's `translateY(-.4em)` is 12.8).
					":where(.wave-text-title-bold) { font-size: 32px; }\n" +
					`._container_b3e8f28 { background: ${PANEL_GLASS}; border: ${PANEL_BORDER}; }`,
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
	// The now-playing header's tabs (TIDAL 2.46.0's `_toggleButton_934359e`,
	// LangRomanizer's button copies the Lyrics tab's classes) look like the pills Tidalfast draws
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
					'[class*="_toggleButton_934359e"] { display: flex; align-items: center; height: 30px; padding: 0 11px; border-radius: 15px; font-size: 14px; font-weight: 600; color: #b4b4b8; }' +
					'[class*="_toggleButton_934359e"][aria-pressed="true"] { color: #101010; background-color: #fff; }',
			);
		}
		return looks;
	};
	// A plugin window's page: the browser's own sheet (page.js), first and
	// outside the document, as Chromium's is not one of `document.styleSheets`.
	let userAgentSheet = null;
	const setUserAgentSheet = (text) => {
		userAgentSheet = new CSSStyleSheet();
		userAgentSheet.replaceSync(String(text));
		D.markDirty?.();
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
		// A plugin window's page has only its own sheets: TIDAL's are the
		// app page's.
		const sheets = [...(L.page ? [userAgentSheet].filter(Boolean) : [tidalSheet(), looksSheet()]), ...els.map(sheetOf), ...(g.document.adoptedStyleSheets ?? [])];
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
		// The custom properties some declaration reads (`var(--x)`, in a rule,
		// a pseudo-element's rule or a keyframe): setting one no rule reads
		// changes no element's style.
		const varNames = new Set();
		const readsVars = (v) => {
			if (v.includes("var(")) for (const m of v.matchAll(/var\(\s*(--[\w-]+)/g)) varNames.add(m[1]);
		};
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
					const decls = [...r._decl._m].filter(([name, d]) => !invalidDecl(name, d.value)).map(([name, d]) => ({ name, value: d.value, important: d.important }));
					if (!decls.length) continue;
					for (const d of decls) readsVars(String(d.value));
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
					for (const [, decls] of frames) for (const [, v] of decls) readsVars(String(v));
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
		index = { byId, byClass, byTag, byAttr, any, fonts, keyframes, hover, styleAttr, structural, hasSteps, hasStyleSteps, styleSiblings, attrNames, varNames, bytes, size: order, pseudo, userSelect };
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
	// A declaration the browser drops as it parses, so that the one before it
	// stays: a negative padding (the cotton candy family's `padding-bottom:
	// -3em` on the album's list panel, which TIDAL 2.46.0 ignores).
	const invalidDecl = (name, value) => /^padding(-(top|right|bottom|left))?$/.test(name) && /(^|\s)-(\d|\.\d)/.test(String(value)) && !/var\(|calc\(|min\(|max\(|clamp\(/i.test(String(value));
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
	// The variables a `@keyframes` body names (kept with the parsed frames).
	const kfVars = new WeakMap();
	const keyframeVars = (frames) => {
		let names = kfVars.get(frames);
		if (names) return names;
		names = new Set();
		for (const [, decls] of frames) for (const [, v] of decls) for (const m of String(v).matchAll(/var\(\s*(--[\w-]+)/g)) names.add(m[1]);
		kfVars.set(frames, names);
		return names;
	};
	const hashString = (s) => {
		let h = 0x811c9dc5;
		for (let i = 0; i < s.length; i++) h = Math.imul(h ^ s.charCodeAt(i), 0x01000193) >>> 0;
		return h.toString(36);
	};
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
	// `--img` is the cover TIDAL sets inline on the album and playlist
	// header (Abyss Neptune's `[class^="_dataContainer_"]::before {
	// background-image: var(--img) }` keeps TIDAL's art).
	const TIDAL_VAR = /^--((market|frosted|player|rounded|desktop|z|floating|os|swiper|spotlight|headliners|onboarding|user)-|img$)/;
	// What an `unset` declaration computes to where Tidalfast has no parent
	// value: TIDAL's body text (14px, weight 500, 20px lines) for inherited
	// properties, the initial value for the boxes the app paints.
	// A colour stays the parent's own: `inherit`, which a painter reads as
	// "the colour around it" (the cotton candy family's `color:
	// var(--SBTXT) !important` on the sidebar's names, with no value, keeps
	// the playing playlist's name the other names' colour, not the accent).
	const UNSET_INHERITED = { "font-size": "14px", "font-weight": "500", "line-height": "20px", color: "inherit" };
	const UNSET_INITIAL = { "background-color": "transparent", "background-image": "none", "border-radius": "0px", "box-shadow": "none" };
	const STYLE_FIELDS = new Set(["display", "visibility", "color", "background-color", "background-image", "font-family", "font-size", "font-weight", "font-style", "text-decoration", "padding", "margin", "margin-top", "margin-bottom", "margin-left", "margin-right", "column-gap", "padding-top", "padding-left", "padding-right", "padding-bottom", "border", "border-radius", "opacity", "width", "height", "gap", "flex-direction", "align-items", "align-self", "justify-content", "cursor", "transform", "filter", "position", "top", "right", "bottom", "left", "z-index", "box-shadow", "min-width", "max-width", "min-height", "max-height", "text-align", "line-height", "overflow", "outline", "outline-offset", "border-color", "letter-spacing", "text-transform", "fill", "white-space", "flex", "flex-wrap", "order", "pointer-events", "background-position", "background-size", "background-repeat", "image-rendering", "text-shadow", "text-overflow", "object-fit", "grid-template-columns", "overflow-y", "list-style-type", "scrollbar-width", "border-image"]);
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
			else if (name === "background-clip" || name === "-webkit-background-clip") set("background-clip", v.trim().toLowerCase());
			else if (name === "-webkit-filter") set("filter", v);
			// Electron's drag regions in a frameless window.
			else if (name === "-webkit-app-region" || name === "app-region") set("app-region", v.toLowerCase());
			// `none` too: a theme that takes TIDAL's own backdrop blur away
			// (Sunken Tidal's play queue) shows what is behind the box
			// unblurred, which the UI reads ("none" parses to no filter).
			else if (name === "backdrop-filter" || name === "-webkit-backdrop-filter") set("backdrop-filter", v);
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
		// A relative font size (`91%`, `.9em`) is of the parent's, so nested
		// ones compound, as in a browser: the cotton candy family's `font-
		// size: 91%` on the bar's time and on its wrapper makes the time 91 %
		// of 91 % (11.6 px in TIDAL 2.46.0). Resolved to points against the
		// size the element inherits, TIDAL's 14-point root where none is set.
		if (st["font-size"] !== undefined) {
			const m = /^(\d*\.?\d+)(%|em)\s*(!important)?$/i.exec(String(st["font-size"]).trim());
			if (m) st["font-size"] = `${+(parseFloat(m[1]) * (m[2] === "%" ? 0.01 : 1) * fontPx(parentStyle?.["font-size"])).toFixed(4)}px`;
		}
		return Object.keys(st).length ? st : null;
	};
	// An inherited font size in points: TIDAL's 14-point root for none (or
	// a word), `rem` of that root.
	const fontPx = (v) => {
		const m = /^(\d*\.?\d+)(px|rem|em|%)?\s*(!important)?$/i.exec(String(v ?? "").trim());
		if (!m) return 14;
		const n = parseFloat(m[1]);
		const unit = (m[2] ?? "px").toLowerCase();
		return unit === "px" ? n : unit === "%" ? (n / 100) * 14 : n * 14;
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
	// Escapes are CSS's: `\f001` (up to six hex digits, and one white space
	// after them) is that code point, an icon font's glyph; a backslash
	// before a line break is nothing; before anything else, that character.
	const unescapeCss = (t) =>
		t.replace(/\\(?:([0-9a-fA-F]{1,6})[ \t\n\r\f]?|(\r\n|[\n\r\f])|([\s\S]))/g, (_, hex, nl, ch) => {
			if (hex) {
				const cp = parseInt(hex, 16);
				return cp === 0 || cp > 0x10ffff || (cp >= 0xd800 && cp <= 0xdfff) ? "�" : String.fromCodePoint(cp);
			}
			return nl ? "" : (ch ?? "");
		});
	const contentText = (v) => {
		let out = "";
		String(v ?? "").replace(/"((?:[^"\\]|\\[\s\S])*)"|'((?:[^'\\]|\\[\s\S])*)'/g, (_, a, b) => {
			out += unescapeCss(a ?? b ?? "");
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
	// The corner radii are TIDAL 2.46.0's (`:root` in its sheet): CatJam's
	// cat is rounded by `var(--wave-border-radius--extra-small)`, 4 px.
	const TIDAL_VARS = [
		["--wave-color-opacity-contrast-fill-thick", "#fff9"],
		["--wave-color-opacity-contrast-fill-ultra-thick", "#fffc"],
		["--wave-border-radius--none", "0"],
		["--wave-border-radius--extra-small", "4px"],
		["--wave-border-radius--small", "8px"],
		["--wave-border-radius--regular", "12px"],
		["--wave-border-radius--full", "1000px"],
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
		const framesWith = (frames, vars) =>
			frames.map(([off, decls]) => {
				const frame = toStyle(decls, vars, null) ?? {};
				// `--rotate: 360deg`: a custom property an animation moves.
				for (const [k, v] of decls) if (k.startsWith("--")) (frame.custom ??= {})[k] = String(v).trim();
				// A keyframe's own easing, for the interval after it.
				if (decls.has("animation-timing-function")) (frame.custom ??= {})["animation-timing-function"] = timingWord(resolveVars(decls.get("animation-timing-function"), vars));
				return [off, frame];
			});
		if (ix.keyframes.size)
			for (const [el, st] of styles) {
				if (!st.animation) continue;
				const list = splitTop(st.animation);
				let renamed = false;
				for (let i = 0; i < list.length; i++) {
					const words = list[i].trim().split(" ");
					const name = words[0];
					const frames = ix.keyframes.get(name);
					if (!frames) continue;
					// Keyframes read variables against the element they
					// animate (`translate: 0 var(--delta)` with the delta set
					// on each element): an element whose own values differ
					// from the root's gets keyframes of its own.
					const own = el._cc?.myVars;
					let key = "";
					if (own && own !== rootVars)
						for (const v of keyframeVars(frames)) {
							const x = own.get(v);
							if (x !== rootVars.get(v)) key += `${v}=${x ?? ""};`;
						}
					if (key) {
						const special = `${name}~${hashString(key)}`;
						keyframes[special] ??= framesWith(frames, own);
						words[0] = special;
						list[i] = words.join(" ");
						renamed = true;
						continue;
					}
					if (keyframes[name]) continue;
					const cached = kfCache.get(name);
					if (cached && cached.frames === frames) {
						keyframes[name] = cached.out;
						continue;
					}
					keyframes[name] = framesWith(frames, rootVars);
					kfCache.set(name, { frames, out: keyframes[name] });
				}
				if (renamed) styles.set(el, { ...st, animation: list.join(",") });
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
	const BLOCK = new Set(["div", "p", "section", "article", "main", "aside", "header", "footer", "nav", "ul", "ol", "h1", "h2", "h3", "h4", "h5", "h6", "form", "figure", "html", "body", "pre"]);
	const NONE = new Set(["head", "style", "script", "meta", "link", "title", "template"]);
	// What the cascade declares for `el` (variables resolved, inherited
	// properties carried down), with no used values: nothing here asks for
	// the element's box. Read only: the maps are shared.
	//
	// Worked out from the parent's and kept on the element (`_cs`) while
	// its own declarations and its parent's result are the same objects (the
	// cascade memo hands out the same Map until something that can change it
	// happens), so a script that scans the whole page (`querySelectorAll("*")`
	// and `getComputedStyle` on each, uwu-ifier on every added node) resolves
	// each element once instead of its whole chain of ancestors on every
	// call. While nothing changed at all (`csToken`), not even the chain is
	// checked again.
	const ROOT_CS = { props: new Map(), vars: new Map(TIDAL_VARS) };
	let csTok = null;
	let csIx = null;
	let csGens = "";
	const csToken = (ix) => {
		const gens = `${D.domGen()}|${D.structGen()}|${D.styleGen()}`;
		if (csTok === null || csIx !== ix || csGens !== gens) {
			csTok = {};
			csIx = ix;
			csGens = gens;
		}
		return csTok;
	};
	// What TIDAL 2.46.0's own sheet gives its classes, for `getComputedStyle`
	// only (Tidalfast draws its own elements itself): an element carrying
	// one, TIDAL's or a plugin's copy of it, computes it under every rule a
	// plugin or theme declares, and what is inside inherits it. Its flex
	// and grid boxes (whose items Chromium blockifies: SongReleaseDate's
	// date in the player bar's title reads `flex`) and the text sizes,
	// weights and (dark) colours that differ from the page's. By exact
	// class, else by the class's name without its hash where every one of
	// TIDAL's classes of that name agrees.
	// Measured in TIDAL 2.46.0's own pages (Home, an album, My Collection's
	// albums and tracks, a track's menu, the now-playing view, at its
	// 2137 x 1183 window) for the classes the anchors carry, where its
	// sheets make the class set the property: a box of its own (`flex`,
	// `grid`, and `inline-flex` where its items' boxes made it `flex`),
	// font sizes and weights, and colours only where the class's own colour
	// is the one shown (a state class may change it).
	const TIDAL_LOOK_ENTRIES = {
		_actionContainer_: { display: "flex" },
		_actionText_d77b98b: { display: "flex", "font-weight": "500" },
		_actionTextInner_: { "font-size": "12px", "font-weight": "600", color: "rgb(255, 255, 255)" },
		_albumColumn_: { display: "flex" },
		_artistColumn_: { display: "flex" },
		_artistText_: { display: "block" },
		_artworkContainer_: { "font-size": "14px", "font-weight": "500" },
		_badge_57d1ccb: { display: "inline-flex" },
		_badge_7b2911e: { display: "flex" },
		_badgeCounter_bd84d2f: { color: "rgb(255, 255, 255)" },
		_bar_: { display: "flex" },
		_button_6808704: { display: "flex" },
		_button_98ab3d0: { display: "flex", color: "rgb(255, 255, 255)" },
		_button_a3412d0: { display: "flex" },
		_button_c25ce4d: { color: "rgba(255, 255, 255, 0.6)" },
		_button_c372e89: { display: "flex" },
		_button_wl3o7_17: { "font-size": "14px", "font-weight": "500" },
		_buttonContainer_a07db07: { display: "flex" },
		_buttonContainer_d207640: { display: "flex" },
		_card_38c6c04: { display: "flex" },
		_cardContainer_: { display: "flex" },
		_cellCoverContainer_: { display: "grid" },
		_cellImage_: { display: "flex" },
		_container_080b5dc: { display: "flex" },
		_container_4454af7: { display: "flex" },
		_container_84abbc3: { display: "flex" },
		_container_ad3e54e: { display: "flex" },
		_container_b3e8f28: { display: "flex" },
		_container_bb78f1d: { display: "flex" },
		_container_cb5cbe4: { display: "flex" },
		_container_f1be359: { display: "flex" },
		_containerRow_: { display: "flex" },
		_contentContainer_6f8427a: { display: "flex" },
		_contentItem_: { display: "flex" },
		_contextColumn_: { display: "flex" },
		_contextMenuButton_: { display: "flex" },
		_contextMenuButton_4c9ff38: { display: "flex", color: "rgba(255, 255, 255, 0.6)" },
		_contextMenuButton_9741d3a: { color: "rgba(255, 255, 255, 0.55)" },
		_creatorImageContainer_274e19d: { display: "grid" },
		_creatorImages_6cd6b4e: { display: "inline-flex" },
		_dataContainer_: { display: "flex" },
		_dateAddedColumn_: { display: "flex" },
		_defaultStyles_a1b9da9: { display: "grid" },
		_explicitBadge_b93510b: { display: "flex" },
		_extraIcon_56196ff: { display: "flex" },
		_feed_412931c: { display: "flex" },
		_flexWrapper_: { display: "flex" },
		_fxHoverControls_cb86dc6: { display: "flex" },
		_gray_c82f8e6: { color: "rgba(255, 255, 255, 0.6)" },
		_grid_97562f5: { display: "grid" },
		_header_2f58a49: { display: "flex" },
		_header_d2e600f: { display: "flex" },
		_headerColumn_fed0322: { display: "flex" },
		_headerContainer_5618a8d: { display: "flex" },
		_iconButton_: { display: "flex" },
		_indexColumn_3ac64ab: { display: "flex" },
		_indexNumber_de640b9: { color: "rgb(121, 119, 127)" },
		_link_86815a7: { display: "block" },
		_link_ce56174: { display: "flex" },
		_list_eaac7af: { display: "grid" },
		_listSection_26d6dae: { display: "flex" },
		_main_a5d9f44: { display: "grid" },
		_mainContainer_: { display: "flex" },
		_marketButton_1d16h_1: { display: "inline-flex" },
		// A market button's size sets its type (`._marketButton_1d16h_1.
		// _small_1d16h_3 { font-size: var(--market-button-small-size-text-
		// size); font-weight: ... }`: 14 px at 600, medium and large 16 px
		// at 600), its rank its colour (Play's primary pill reads rgb(16,
		// 16, 16), a secondary one white; measured).
		_small_1d16h_3: { "font-size": "14px", "font-weight": "600" },
		_medium_1d16h_4: { "font-size": "16px", "font-weight": "600" },
		_large_1d16h_5: { "font-size": "16px", "font-weight": "600" },
		_primary_1d16h_6: { color: "rgb(16, 16, 16)" },
		_secondary_1d16h_7: { color: "rgb(255, 255, 255)" },
		_marketPagingTabList_mibfb_1: { display: "flex" },
		_metaContainer_3980a6e: { display: "grid" },
		_navigationItem_: { display: "flex" },
		_nowPlayingContainer_: { display: "flex" },
		// Written for two classes (`._nowPlayingContainer_88d37df.
		// _nowPlayingHidden_7e74506 { visibility: hidden }`): the closed
		// now-playing view (StarryNight's sky in it is hidden there).
		_nowPlayingHidden_: { visibility: "hidden" },
		_occlusion_de6b9d3: { display: "grid" },
		_overflowButtons_: { display: "flex" },
		_overlayIconWrapper_6331eb1: { display: "flex" },
		_overline_70ce5b5: { "font-size": "12px", "font-weight": "600" },
		_playbackControls_: { display: "flex" },
		_player_: { display: "grid" },
		_playerContent_: { display: "contents" },
		_playQueueWrapper_: { display: "flex" },
		_profilePicture_1786ea0: { display: "flex" },
		_repeatButton_: { display: "flex", color: "rgba(255, 255, 255, 0.55)" },
		_scrollableContent_4035207: { display: "flex" },
		_scrubberTime_: { color: "rgba(255, 255, 255, 0.35)" },
		_scrubberTimeText_: { color: "rgba(255, 255, 255, 0.35)" },
		_searchField_d9ef617: { color: "rgb(255, 255, 255)" },
		_searchFieldFilter_0f926aa: { color: "rgb(255, 255, 255)" },
		_sectionHeader_ca54638: { display: "flex" },
		_shortcutItem_6c8e7b4: { display: "flex", "font-weight": "600" },
		_shuffleButton_: { display: "flex", color: "rgba(255, 255, 255, 0.55)" },
		_sidebar_: { display: "flex" },
		_sidebarWrapper_: { display: "flex" },
		_sliderContainer_: { display: "flex" },
		_small_wl3o7_73: { "font-size": "14px", "font-weight": "500" },
		_smallButtons_: { display: "flex" },
		_smallHeader_: { display: "flex" },
		_smallText_a636ed6: { "font-size": "12px" },
		_soundOutputButton_a6d011c: { display: "flex" },
		_subMenuIcon_fbc18e3: { color: "rgb(120, 120, 135)" },
		_tableHeader_: { display: "flex", color: "rgba(255, 255, 255, 0.6)" },
		_tableRow_: { display: "flex", color: "rgb(255, 255, 255)" },
		_textContent_462c76e: { display: "flex" },
		_textWrapper_da25926: { display: "flex" },
		_thumbnail_f82abfa: { display: "flex" },
		_timeColumn_: { display: "flex" },
		_title_2bc5e10: { "font-size": "14px", "font-weight": "600", color: "rgb(255, 255, 255)" },
		_title_3b8740e: { display: "flex" },
		_titleArtistStack_: { display: "flex" },
		_titleColumn_: { display: "flex" },
		_titleContainer_: { display: "flex" },
		_titleGroup_208d294: { display: "flex" },
		_titleText_1d0e44e: { color: "rgba(255, 255, 255, 0.3)" },
		_titleText_51cccae: { display: "block" },
		"_trackcells-sort_ecf19dd": { color: "rgba(255, 255, 255, 0.6)" },
		_trackContent_: { display: "flex" },
		_trackInfo_: { display: "flex" },
		_utilityButtons_: { display: "flex" },
		_utilityContainer_: { display: "flex" },
		"wave-text-capital-demi": { "font-size": "10px", "font-weight": "600" },
		"wave-text-caption-demi": { "font-size": "12px", "font-weight": "600" },
		"wave-text-description-demi": { "font-size": "14px", "font-weight": "600" },
		"wave-text-headline-bold": { "font-weight": "700" },
		"wave-text-subheadline-bold": { "font-size": "20px", "font-weight": "600" },
		"wave-text-title-bold": { "font-size": "32px", "font-weight": "700" },
	};
	const TIDAL_LOOK = new Map(Object.entries(TIDAL_LOOK_ENTRIES));
	// TIDAL's classes that set a market button's own tokens (index.css),
	// which win over its size and rank whatever the order of the classes:
	// [class, the classes of the rule the token feeds, the look]. The
	// now-playing header's icon buttons (`._iconButton_fe68160`: the
	// tertiary icon colour is `--market-core-text-20-color`, #ffffff8c; a
	// plugin's copy of the full screen button reads 55 % white at 600 in
	// TIDAL). The view's tabs, TIDAL's toggle buttons (`._toggleButton_
	// 934359e`): medium type (`--market-core-type-medium-weight`, 500) in
	// the tertiary grey (text-20), and while pressed (`._toggleButtonActive_
	// b595428`) black (`--market-core-fill-black-color`, #000000e6) on white;
	// what a plugin puts in a tab inherits that. A plugin's copy of a tab
	// keeps the look the sheet above gives it.
	const TIDAL_LOOK_TOKENS = [
		["_iconButton_fe68160", ["_tertiary_1d16h_8", "_iconOnly_1d16h_1"], { color: "rgba(255, 255, 255, 0.55)" }],
		["_toggleButton_934359e", ["_small_1d16h_3"], { "font-weight": "500", color: "rgba(255, 255, 255, 0.55)" }],
		["_toggleButtonActive_b595428", ["_toggleButton_934359e", "_tertiary_1d16h_8"], { color: "rgba(0, 0, 0, 0.9)", "background-color": "rgb(255, 255, 255)" }],
	];
	// A form control's own font, from Chromium's sheet (`font:
	// -webkit-small-control`, which TIDAL's normalize sizes back to its
	// parent's): weight 400 whatever its parent's, normal line height, and
	// a field's text in Chromium's field colour (HorizontalVolumeSlider's
	// range reads `rgb(16, 16, 16)` in TIDAL).
	const FORM_LOOK = { "font-weight": "400", "line-height": "normal" };
	const FIELD_LOOK = { ...FORM_LOOK, color: "rgb(16, 16, 16)" };
	const TAG_LOOK = { button: FORM_LOOK, input: FIELD_LOOK, select: FIELD_LOOK, textarea: FIELD_LOOK };
	// TIDAL's `._utilityButtons_4d7aaf9 button { color: var(--market-core-
	// text-20-color); display: flex }`: a plugin's button among the player
	// bar's utility buttons (OneClickLyrics', MusicVideoButton's) is grey.
	const inUtility = (el) => {
		for (let n = el.parentNode, i = 0; n instanceof g.Element && i < 4; n = n.parentNode, i++) if (String(n.getAttribute("class") ?? "").includes("_utilityButtons_")) return true;
		return false;
	};
	const UTILITY_BUTTON = { color: "rgba(255, 255, 255, 0.55)", display: "flex" };
	const lookCache = new Map();
	const pageLooks = new Map();
	const lookOf = (el) => {
		const tag = el.localName;
		// A plugin window's page has Chromium's sheet only.
		if (L.page) {
			const look = TAG_LOOK[tag];
			if (!look) return null;
			let m = pageLooks.get(tag);
			if (!m) pageLooks.set(tag, (m = new Map(Object.entries(look))));
			return m;
		}
		const cls = el.getAttribute?.("class") ?? "";
		const utility = tag === "button" && inUtility(el);
		if (!cls && !(tag in TAG_LOOK)) return null;
		const light = D.colorScheme === "light";
		const key = `${tag in TAG_LOOK ? tag : ""}|${utility ? "u" : ""}|${light ? "l" : ""}|${cls}`;
		const hit = lookCache.get(key);
		if (hit !== undefined) return hit;
		let out = null;
		const put = (look) => {
			for (const k in look) if (!(k === "color" && light)) (out ??= new Map()).set(k, look[k]);
		};
		if (tag in TAG_LOOK) put(TAG_LOOK[tag]);
		if (utility) put(UTILITY_BUTTON);
		const list = cls.split(/\s+/);
		for (const c of list) {
			if (!c) continue;
			const look = TIDAL_LOOK.get(c) ?? TIDAL_LOOK.get(/^(_[A-Za-z0-9]+_)[0-9a-z]{5,8}(?:_\d+)?$/.exec(c)?.[1] ?? "");
			if (look) put(look);
		}
		for (const [c, needs, look] of TIDAL_LOOK_TOKENS) if (list.includes(c) && needs.every((n) => list.includes(n))) put(look);
		if (lookCache.size > 4096) lookCache.clear();
		lookCache.set(key, out);
		return out;
	};
	const cascadedOf = (ix, tok, el) => {
		const m = el._cs;
		if (m !== undefined && m.tok === tok) return m.res;
		const p = el.parentNode;
		const up = p instanceof g.Element ? cascadedOf(ix, tok, p) : ROOT_CS;
		const d = cascade(ix, el);
		const look = lookOf(el);
		if (m !== undefined && m.d === d && m.up === up && m.look === look) {
			m.tok = tok;
			return m.res;
		}
		// Counted for the tests that check what a scan works out again.
		D.styleResolves = (D.styleResolves ?? 0) + 1;
		let vars = up.vars;
		const props = new Map();
		for (const [k, v] of up.props) if (INHERITED.has(k)) props.set(k, v);
		if (look) for (const [k, v] of look) props.set(k, v);
		if (d) {
			for (const [k, v] of d)
				if (k.startsWith("--")) {
					if (vars === up.vars) vars = new Map(vars);
					vars.set(k, v);
				}
			for (const [k, v] of d) {
				if (k.startsWith("--")) continue;
				const r = resolveVars(v, vars);
				if (r !== "inherit") props.set(k, r);
			}
		}
		const res = { props, vars, base: null, look };
		el._cs = { tok, d, up, look, res };
		return res;
	};
	const cascaded = (el) => {
		const ix = buildIndex();
		return cascadedOf(ix, csToken(ix), el);
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
	// The declaration `getComputedStyle` hands out for a cascaded result:
	// built once per result and shared (copied only if a script writes to
	// its copy). The box is read when a size or transform is: as in a
	// browser, a script that only reads colours or images does not make the
	// page lay out (each such read waited for the UI's next layout after a
	// style change).
	const v = (value) => ({ value, important: false });
	// The variables resolved, once per set of them (most elements share
	// their parent's).
	const resolvedVars = new WeakMap();
	const varsOut = (vars) => {
		let out = resolvedVars.get(vars);
		if (!out) {
			out = [];
			for (const [k, x] of vars) out.push([k, v(resolveVars(x, vars).trim())]);
			resolvedVars.set(vars, out);
		}
		return out;
	};
	// What TIDAL 2.46.0's page gives everything in it (captured: `html {
	// font-family: "Square Sans Text VF", ...; font-size: calc(var(
	// --baseFontSize) * 1px) }`, 14 px, and `body { font-weight: 500 }`), so
	// an element that sets none of them computes TIDAL's values. A plugin
	// window's page keeps what it had (16 px, 400).
	const ROOT_FONT_PX = L.page ? 16 : 14;
	const BASE_WEIGHT = L.page ? "400" : "500";
	const BASE_FAMILY = L.page ? "nationale, sans-serif" : '"Square Sans Text VF", "Square Sans Text", Helvetica, Arial, sans-serif';
	const FONT_KEYWORDS = { "xx-small": 9, "x-small": 10, small: 13, medium: 16, large: 18, "x-large": 24, "xx-large": 32, "xxx-large": 48 };
	// A declared `font-size` as pixels, against the parent's (`em`, `%`,
	// `smaller`) and the root's (`rem`); null when it cannot be read.
	const declaredFontPx = (value, parent) => {
		const s = String(value).trim().toLowerCase();
		if (s === "inherit" || s === "unset" || s === "initial" || s === "revert") return parent;
		if (s in FONT_KEYWORDS) return FONT_KEYWORDS[s];
		if (s === "smaller") return parent / 1.2;
		if (s === "larger") return parent * 1.2;
		const m = /^(-?[\d.]+)(px|em|rem|%|pt)?$/.exec(s);
		if (m) {
			const n = parseFloat(m[1]);
			return m[2] === "em" ? n * parent : m[2] === "rem" ? n * ROOT_FONT_PX : m[2] === "%" ? (n * parent) / 100 : m[2] === "pt" ? (n * 4) / 3 : n;
		}
		const calc = /^calc\((.*)\)$/.exec(s);
		if (calc) {
			// `calc(14 * 1px)`, as TIDAL's root writes it: a product of numbers
			// and one length.
			let total = 1;
			for (const part of calc[1].split("*")) {
				const p = declaredFontPx(part, parent);
				if (p === null) return null;
				total *= p;
			}
			return total;
		}
		return null;
	};
	// The element's font size in pixels, as a browser computes it: its own
	// declared size against its parent's computed one, else the parent's.
	// Kept with the cascaded result, which changes when anything can.
	const fontSizeOf = (el) => {
		const m = el._cs;
		if (m === undefined) return ROOT_FONT_PX;
		if (m.res.fontPx !== undefined) return m.res.fontPx;
		const p = el.parentNode;
		const parent = p instanceof g.Element && p._cs !== undefined ? fontSizeOf(p) : ROOT_FONT_PX;
		const own = m.d?.get("font-size") ?? m.res.look?.get("font-size");
		const px = own === undefined ? parent : (declaredFontPx(resolveVars(own, m.res.vars), parent) ?? parent);
		m.res.fontPx = px;
		return px;
	};
	const WEIGHTS = { normal: "400", bold: "700" };
	// Blockification (CSS Display 3): the item of a flex or grid container, an
	// absolutely positioned or floated box and the root are block-level, so
	// their `display` computes to the block-level twin of an inline one
	// (Chromium reports `flex` for SongReleaseDate's `inline-flex` date in
	// the player bar's title, a flex box).
	const BLOCKIFIED = { inline: "block", "inline-block": "block", "inline-flex": "flex", "inline-grid": "grid", "inline-table": "table", "inline-list-item": "list-item", "table-row-group": "block", "table-header-group": "block", "table-footer-group": "block", "table-row": "block", "table-cell": "block", "table-column-group": "block", "table-column": "block", "table-caption": "block", "ruby-base": "block", "ruby-text": "block" };
	const FLEX_OR_GRID = /^(inline-)?(flex|grid)$/;
	// An element's `display` before any rule: Chromium's own sheet, and on
	// TIDAL's page its normalize sheet (`audio, canvas, video {
	// display: inline-block }`, `audio:not([controls])` and `[hidden]` not
	// shown, `details`, `summary` and the like blocks). A picture and an
	// SVG are inline in Chromium.
	const TAG_DISPLAY = { li: "list-item", table: "table", caption: "table-caption", thead: "table-header-group", tbody: "table-row-group", tfoot: "table-footer-group", tr: "table-row", td: "table-cell", th: "table-cell", col: "table-column", colgroup: "table-column-group", button: "inline-block", input: "inline-block", select: "inline-block", textarea: "inline-block", meter: "inline-block", progress: "inline-block" };
	const TIDAL_TAG_BLOCK = new Set(["details", "figcaption", "hgroup", "summary", "dialog", "fieldset", "legend", "hr", "address", "blockquote", "dl", "dt", "dd", "menu"]);
	const tagDisplay = (el) => {
		const t = el.localName;
		if (NONE.has(t) || el.hasAttribute?.("hidden")) return "none";
		if (t === "audio" && !el.hasAttribute?.("controls")) return "none";
		if (BLOCK.has(t)) return "block";
		if (TAG_DISPLAY[t] !== undefined) return TAG_DISPLAY[t];
		if (TIDAL_TAG_BLOCK.has(t)) return "block";
		if (!L.page && (t === "audio" || t === "canvas" || t === "video")) return "inline-block";
		return "inline";
	};
	const word = (x) =>
		String(x ?? "")
			.replace(/!important/i, "")
			.trim()
			.toLowerCase();
	// The element's computed `display`, kept with its cascaded result. A
	// `display: contents` parent passes its own parent's box on: its
	// children are that box's items.
	const displayOf = (el) => {
		const res = el._cs?.res;
		// `hidden` on or off changes no rule: the cached answer is for one.
		const hid = !!el.hasAttribute?.("hidden");
		if (res?.display !== undefined && res.hid === hid) return res.display;
		const props = res?.props;
		const own = word(props?.get("display")) || tagDisplay(el);
		let d = own;
		if (BLOCKIFIED[d] !== undefined) {
			let up = el.parentNode;
			while (up instanceof g.Element && up._cs !== undefined && displayOf(up) === "contents") up = up.parentNode;
			const upDisplay = up instanceof g.Element && up._cs !== undefined ? displayOf(up) : "";
			const out = /^(absolute|fixed)$/.test(word(props?.get("position"))) || /^(left|right|inline-start|inline-end)$/.test(word(props?.get("float")));
			if (FLEX_OR_GRID.test(upDisplay) || out || el === g.document?.documentElement) d = BLOCKIFIED[d];
		}
		if (res) {
			res.display = d;
			res.hid = hid;
		}
		return d;
	};
	// A corner radius as Chromium computes it: lengths in pixels (`0` is
	// `0px`, `em` and `rem` resolved), percentages kept, and the four
	// corners written as briefly as they read (`0 0 12px 12px` is
	// `0px 0px 12px 12px`, `8px 8px` is `8px`).
	const radiusPx = (value, fontPx) => {
		const s = String(value).trim();
		if (s.includes("/") || /var\(|calc\(/i.test(s)) return s;
		const parts = s.split(/\s+/).map((p) => {
			const m = /^(-?[\d.]+)(px|em|rem)?$/i.exec(p);
			if (!m) return p;
			const n = parseFloat(m[1]);
			const px = !m[2] || m[2].toLowerCase() === "px" ? n : m[2].toLowerCase() === "em" ? n * fontPx : n * ROOT_FONT_PX;
			return `${Math.round(px * 1000) / 1000}px`;
		});
		if (parts.length !== 4 && parts.length !== 3 && parts.length !== 2) return parts.join(" ");
		const [a, b, c = a, d = b] = parts;
		if (a === b && b === c && c === d) return a;
		if (a === c && b === d) return `${a} ${b}`;
		if (b === d) return `${a} ${b} ${c}`;
		return `${a} ${b} ${c} ${d}`;
	};
	const computedBase = (el, res) => {
		const { props, vars } = res;
		const m = new Map();
		const box = () => el.getBoundingClientRect();
		m.set("display", v(tagDisplay(el)));
		for (const [k, x] of [
			["visibility", "visible"],
			["opacity", "1"],
			["color", "rgb(255, 255, 255)"],
			["background-color", "rgba(0, 0, 0, 0)"],
			["font-weight", BASE_WEIGHT],
			["font-family", BASE_FAMILY],
			["position", "static"],
			["z-index", "auto"],
		])
			m.set(k, v(x));
		m.set("width", { get value() { return `${box().width}px`; }, important: false });
		m.set("height", { get value() { return `${box().height}px`; }, important: false });
		m.set("line-height", v("normal"));
		m.set("transform", v("none"));
		for (const [k, x] of props) m.set(k, v(/radius$/.test(k) ? radiusPx(x, fontSizeOf(el)) : x));
		m.set("display", v(displayOf(el)));
		// Computed values as a browser reports them: the font size in pixels,
		// a weight as its number, a z-index within a 32-bit integer (oneko's
		// `Number.MAX_SAFE_INTEGER` reads 2147483647), an address quoted.
		const size = fontSizeOf(el);
		m.set("font-size", v(`${Math.round(size * 100) / 100}px`));
		const weight = String(props.get("font-weight") ?? "").trim().toLowerCase();
		if (weight in WEIGHTS) m.set("font-weight", v(WEIGHTS[weight]));
		const z = String(props.get("z-index") ?? "").trim();
		if (/^-?\d+$/.test(z)) m.set("z-index", v(String(Math.max(-2147483648, Math.min(2147483647, Number(z))))));
		const image = props.get("background-image");
		if (image !== undefined) m.set("background-image", v(String(image).replace(/url\(\s*([^"')\s][^)]*?)\s*\)/g, 'url("$1")')));
		// A transform resolves to the matrix a browser reports.
		if (props.has("transform") && L.parseTransform) {
			const t = props.get("transform");
			m.set("transform", {
				get value() {
					try {
						const r = box();
						const { m: x, is2D } = L.parseTransform(t, [r.width, r.height]);
						return t.trim() === "none" ? "none" : is2D ? `matrix(${[x[0], x[1], x[4], x[5], x[12], x[13]].join(", ")})` : `matrix3d(${x.join(", ")})`;
					} catch {
						return t;
					}
				},
				important: false,
			});
		}
		if (props.has("background")) m.set(isImage(props.get("background")) ? "background-image" : "background-color", v(props.get("background")));
		for (const [k, x] of varsOut(vars)) m.set(k, x);
		return m;
	};
	const computedStyle = (el, pseudo) => {
		if (!(el instanceof g.Element) || (pseudo && /::?(before|after|marker|placeholder|selection|-webkit-scrollbar)/.test(String(pseudo)))) return declProxy(new CSSStyleDeclaration());
		const res = cascaded(el);
		res.base ??= computedBase(el, res);
		return declProxy(new CSSStyleDeclaration(null, res.base));
	};

	// Whether any selector names attribute `k` (from the last index; a sheet
	// change since then resets every cascade anyway). No index yet: yes.
	const attrMatters = (k) => !index || index.attrNames.has(String(k).toLowerCase());
	// The custom properties the rules read, as of the last index (`null`
	// before there is one). A sheet that changes since marks the overlay for
	// a build of its own, which reads every inline style afresh.
	const varsRead = () => index?.varNames ?? null;
	// Whether a rule looks at siblings, children or position: then a changed
	// class can change what matches outside the element's own subtree.
	const structural = () => !!buildIndex().structural;
	L.css = { setUserAgentSheet, panelGlass: PANEL_GLASS, panelBorder: PANEL_BORDER, parse: parseRules, sheetOf, documentSheets, compute, computedStyle, cascaded, styleNow, splitDecls, usesHover, attrMatters, varsRead, structural, rangeParts };
})();
