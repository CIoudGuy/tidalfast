// A tiny React for plugin settings. Plugins export `Settings`, a React
// component built from @luna/ui setting components. There is no DOM here:
// the tree is rendered (hooks and effects included) into a plain JSON tree of
// setting nodes that Rust turns into `SettingsNode`s for the native Plugins
// page. Input from that page calls the component's own callbacks, then the
// tree is rendered again.
"use strict";
(() => {
	const g = globalThis;
	const L = g.__lunaInternal;

	const ELEMENT = Symbol.for("react.element");
	const TRANSITIONAL = Symbol.for("react.transitional.element");
	const FRAGMENT = Symbol.for("react.fragment");
	const MEMO = Symbol.for("react.memo");
	const FORWARD = Symbol.for("react.forward_ref");
	const PROVIDER = Symbol.for("react.provider");
	const CONTEXT = Symbol.for("react.context");
	const STRICT = Symbol.for("react.strict_mode");
	const SUSPENSE = Symbol.for("react.suspense");
	const PROFILER = Symbol.for("react.profiler");
	const LAZY = Symbol.for("react.lazy");

	const mkElement = (type, key, ref, props) => ({ $$typeof: ELEMENT, type, key: key == null ? null : String(key), ref: ref ?? null, props });
	function createElement(type, config, ...children) {
		const props = {};
		let key = null;
		let ref = null;
		if (config != null) {
			for (const k of Object.keys(config)) {
				if (k === "key") key = config.key;
				else if (k === "ref") ref = config.ref;
				else props[k] = config[k];
			}
		}
		if (children.length === 1) props.children = children[0];
		else if (children.length > 1) props.children = children;
		if (type && type.defaultProps) for (const k of Object.keys(type.defaultProps)) if (props[k] === undefined) props[k] = type.defaultProps[k];
		return mkElement(type, key, ref, props);
	}
	const jsx = (type, config, maybeKey) => {
		const props = {};
		let ref = null;
		let key = maybeKey ?? null;
		if (config != null)
			for (const k of Object.keys(config)) {
				if (k === "key") key = config.key;
				else if (k === "ref") ref = config.ref;
				else props[k] = config[k];
			}
		if (type && type.defaultProps) for (const k of Object.keys(type.defaultProps)) if (props[k] === undefined) props[k] = type.defaultProps[k];
		return mkElement(type, key, ref, props);
	};
	const isValidElement = (o) => typeof o === "object" && o !== null && (o.$$typeof === ELEMENT || o.$$typeof === TRANSITIONAL);
	const cloneElement = (el, config, ...children) => {
		const props = { ...el.props };
		let key = el.key;
		let ref = el.ref;
		if (config) for (const k of Object.keys(config)) k === "key" ? (key = config.key) : k === "ref" ? (ref = config.ref) : (props[k] = config[k]);
		if (children.length === 1) props.children = children[0];
		else if (children.length > 1) props.children = children;
		return mkElement(el.type, key, ref, props);
	};
	const flat = (children) => {
		const out = [];
		const walk = (c) => {
			if (Array.isArray(c)) c.forEach(walk);
			else if (c !== undefined && c !== null && typeof c !== "boolean") out.push(c);
		};
		walk(children);
		return out;
	};
	const Children = {
		map: (c, fn, t) => flat(c).map((x, i) => fn.call(t, x, i)),
		forEach: (c, fn, t) => flat(c).forEach((x, i) => fn.call(t, x, i)),
		count: (c) => flat(c).length,
		toArray: (c) => flat(c),
		only: (c) => {
			const f = flat(c);
			if (f.length !== 1) throw new Error("React.Children.only expected to receive a single React element child.");
			return f[0];
		},
	};
	class Component {
		constructor(props) {
			this.props = props;
			this.state = {};
		}
		setState(partial, cb) {
			const next = typeof partial === "function" ? partial(this.state, this.props) : partial;
			this.state = { ...this.state, ...next };
			this.__root?.invalidate();
			cb?.();
		}
		forceUpdate() {
			this.__root?.invalidate();
		}
	}
	Component.prototype.isReactComponent = {};
	class PureComponent extends Component {}
	const memo = (type, compare) => ({ $$typeof: MEMO, type, compare });
	const forwardRef = (render) => ({ $$typeof: FORWARD, render });
	const createContext = (defaultValue) => {
		const ctx = { $$typeof: CONTEXT, _currentValue: defaultValue, _default: defaultValue };
		ctx.Provider = { $$typeof: PROVIDER, _context: ctx };
		ctx.Consumer = { $$typeof: CONTEXT, _consumer: ctx };
		return ctx;
	};
	const lazy = (load) => ({ $$typeof: LAZY, load });
	const createRef = () => ({ current: null });

	// ---------------------------------------------------------------- hooks
	let frame = null; // { root, path, hooks, index }
	const hookSlot = (init) => {
		if (frame === null) throw new Error("Hooks can only be called inside the body of a function component (Luna settings shim)");
		const i = frame.index++;
		if (frame.hooks.length <= i) frame.hooks.push(init());
		return frame.hooks[i];
	};
	const depsChanged = (a, b) => a === undefined || b === undefined || a.length !== b.length || a.some((x, i) => !Object.is(x, b[i]));
	const useReducer = (reducer, initArg, init) => {
		const root = frame.root;
		const slot = hookSlot(() => {
			const s = { value: init ? init(initArg) : initArg };
			s.dispatch = (action) => {
				const next = reducer(s.value, action);
				if (!Object.is(next, s.value)) {
					s.value = next;
					root.invalidate();
				}
			};
			return s;
		});
		return [slot.value, slot.dispatch];
	};
	const useState = (initial) => useReducer((s, a) => (typeof a === "function" ? a(s) : a), initial, (x) => (typeof x === "function" ? x() : x));
	const useEffect = (fn, deps) => {
		const root = frame.root;
		const slot = hookSlot(() => ({ deps: undefined, cleanup: undefined, effect: true }));
		if (slot.first === undefined || depsChanged(slot.deps, deps)) {
			slot.first = false;
			slot.deps = deps;
			root.pendingEffects.push(() => {
				if (typeof slot.cleanup === "function") slot.cleanup();
				const c = fn();
				slot.cleanup = typeof c === "function" ? c : undefined;
			});
		}
	};
	const useMemo = (fn, deps) => {
		const slot = hookSlot(() => ({ deps: undefined, value: undefined, init: false }));
		if (!slot.init || depsChanged(slot.deps, deps)) {
			slot.value = fn();
			slot.deps = deps;
			slot.init = true;
		}
		return slot.value;
	};
	const useCallback = (fn, deps) => useMemo(() => fn, deps);
	const useRef = (v) => hookSlot(() => ({ current: v }));
	const useContext = (ctx) => (ctx?._consumer ?? ctx)?._currentValue;
	let idSeq = 0;
	const useId = () => hookSlot(() => ({ id: `:l${idSeq++}:` })).id;
	const useSyncExternalStore = (subscribe, getSnapshot) => {
		const root = frame.root;
		const slot = hookSlot(() => ({ unsub: undefined }));
		if (slot.unsub === undefined) {
			try {
				slot.unsub = subscribe(() => root.invalidate()) ?? (() => {});
			} catch {
				slot.unsub = () => {};
			}
			slot.effect = true;
			slot.cleanup = () => slot.unsub?.();
		}
		return getSnapshot();
	};
	const useTransition = () => [false, (fn) => fn()];
	const useDeferredValue = (v) => v;
	const useImperativeHandle = () => {};
	const useDebugValue = () => {};
	const useOptimistic = (v) => [v, () => {}];
	const useActionState = (fn, init) => [init, () => {}, false];
	const useEffectEvent = (fn) => fn;
	const use = (v) => (v && v.$$typeof === CONTEXT ? useContext(v) : v);

	// ---------------------------------------------------------------- renderer
	// Luna UI components carry `__luna` (their node kind). See ui.js.
	const textOf = (node) => {
		const parts = [];
		const links = [];
		const walk = (n) => {
			if (n === null || n === undefined || typeof n === "boolean") return;
			if (typeof n === "string" || typeof n === "number") return void parts.push(String(n));
			if (Array.isArray(n)) return void n.forEach(walk);
			if (!isValidElement(n)) return;
			const t = n.type;
			if (t === "br") return void parts.push("\n");
			const href = n.props?.href;
			if (href) links.push({ text: textOnly(n.props.children), href: String(href) });
			if (typeof t === "function" && !t.__luna && !(t.prototype && t.prototype.isReactComponent)) {
				try {
					return void walk(t(n.props ?? {}));
				} catch {
					return;
				}
			}
			if (t && t.$$typeof === MEMO && typeof t.type === "function" && !t.type.__luna) {
				try {
					return void walk(t.type(n.props ?? {}));
				} catch {
					return;
				}
			}
			walk(n.props?.children ?? (t?.__luna === "link" ? n.props?.href : undefined));
		};
		walk(node);
		return { text: parts.join("").replace(/[ \t]+/g, " ").trim(), links };
	};
	const textOnly = (n) => textOf(n).text;
	const opt = (v) => (v === undefined || v === null ? undefined : v);
	const num = (v) => (v === undefined || v === null || v === "" || isNaN(Number(v)) ? undefined : Number(v));

	// ---- settings from plain elements -------------------------------------
	// Plugins build their Settings from @luna/ui components, from MUI that
	// they bundle, or from plain `<input>`/`<button>`/`<select>` elements.
	// Whatever draws, these tags are what ends up in the tree, so they map to
	// native controls; anything unknown degrades to its children's text.
	const BLOCK = new Set(["div", "p", "section", "article", "header", "footer", "main", "nav", "aside", "form", "fieldset", "ul", "ol", "li", "h1", "h2", "h3", "h4", "h5", "h6", "table", "tr", "thead", "tbody", "dl", "dt", "dd", "pre", "blockquote", "details", "summary", "legend", "figure", "figcaption", "address"]);
	const SKIP = new Set(["svg", "style", "script", "canvas", "img", "picture", "video", "audio", "iframe", "noscript", "template", "link", "meta", "head", "hr", "option", "optgroup", "datalist", "map", "object", "embed", "dialog"]);
	const HEADING = /^h[1-6]$/;
	const TEXTUAL = new Set(["", "text", "search", "email", "url", "tel", "password", "date", "time", "datetime-local", "month", "week", "color"]);
	const mkEvent = (target, extra) => {
		const ev = {
			type: "change",
			target,
			currentTarget: target,
			nativeEvent: { defaultPrevented: false },
			defaultPrevented: false,
			bubbles: true,
			isTrusted: true,
			preventDefault() {
				this.defaultPrevented = true;
			},
			stopPropagation() {},
			persist() {},
			isDefaultPrevented() {
				return this.defaultPrevented;
			},
			isPropagationStopped() {
				return false;
			},
			...extra,
		};
		return ev;
	};
	// `flat` that also looks through fragments, so select items wrapped in
	// one still count.
	const flatItems = (children) => {
		const out = [];
		const walk = (c) => {
			if (Array.isArray(c)) c.forEach(walk);
			else if (isValidElement(c) && (c.type === FRAGMENT || c.type === STRICT)) walk(c.props.children);
			else if (c !== undefined && c !== null && typeof c !== "boolean") out.push(c);
		};
		walk(children);
		return out;
	};
	const optionOf = (it) => {
		const ip = it.props ?? {};
		if (it.type === "optgroup") return null;
		const value = ip.value === undefined && typeof it.type === "string" ? textOnly(ip.children) : ip.value;
		if (value === undefined) return null;
		return { value, label: textOnly(ip.children) || (value === "" ? "None" : String(value)), disabled: !!ip.disabled || undefined };
	};
	const optionsOf = (children) => {
		const options = [];
		const items = [];
		const walk = (list) => {
			for (const it of flatItems(list)) {
				if (!isValidElement(it)) continue;
				if (it.type === "optgroup") {
					walk(it.props.children);
					continue;
				}
				const o = optionOf(it);
				if (o) {
					options.push(o);
					items.push(it);
				}
			}
		};
		walk(children);
		return { options, items };
	};
	const sameJson = (a, b) => JSON.stringify(a) === JSON.stringify(b);

	const isText = (n) => n?.kind === "text" && !n.id;
	class Root {
		constructor(name, Comp) {
			this.name = name;
			this.Comp = Comp;
			this.hooks = new Map();
			this.instances = new Map();
			this.handlers = new Map();
			this.local = new Map();
			this.pendingEffects = [];
			this.scheduled = false;
			this.rendering = false;
			this.dirty = false;
			this.onTree = null;
			this.dead = false;
		}
		invalidate() {
			if (this.dead) return;
			if (this.rendering) {
				this.dirty = true;
				return;
			}
			if (this.scheduled) return;
			this.scheduled = true;
			L.internalTimeout(() => {
				this.scheduled = false;
				if (!this.dead) this.render();
			}, 0);
		}
		render() {
			let tree;
			for (let pass = 0; pass < 20; pass++) {
				this.dirty = false;
				this.rendering = true;
				this.handlers = new Map();
				this.capProps = new Map();
				this.seen = new Set();
				this.byId = new Map();
				this.labelFor = [];
				try {
					tree = this.renderNode(createElement(this.Comp, {}), "r");
				} finally {
					this.rendering = false;
				}
				if (!this.dirty) break;
			}
			for (const [path, hooks] of this.hooks)
				if (!this.seen.has(path)) {
					for (const h of hooks) if (typeof h?.cleanup === "function") h.cleanup();
					this.hooks.delete(path);
				}
			for (const [path, inst] of this.instances)
				if (!this.seen.has(path)) {
					try {
						inst.componentWillUnmount?.();
					} catch {}
					this.instances.delete(path);
				}
			for (const lf of this.labelFor) {
				const c = this.byId.get(lf.id);
				if (c && !c.title) {
					c.title = lf.node.text;
					lf.node._drop = true;
				}
			}
			const effects = this.pendingEffects;
			this.pendingEffects = [];
			for (const e of effects) {
				try {
					e();
				} catch (err) {
					L.reportError(err);
				}
			}
			const out = { kind: "group", children: this.finish(tree) };
			this.onTree?.(out);
			return out;
		}
		// Tidy text, drop what was consumed as a label, strip internal flags.
		finish(nodes) {
			const out = [];
			for (const n of nodes) {
				if (n._drop) continue;
				if (isText(n)) {
					const text = String(n.text).replace(/[ \t\r\f]+/g, " ").replace(/ ?\n ?/g, "\n").replace(/\n{3,}/g, "\n\n").trim();
					if (!text) continue;
					const c = { kind: "text", text };
					if (n.title) c.title = true;
					if (n.desc) c.desc = n.desc;
					out.push(c);
					continue;
				}
				if (n.children) n.children = this.finish(n.children);
				delete n.radio;
				delete n._b;
				delete n._lbl;
				out.push(n);
			}
			return out;
		}
		unmount() {
			this.dead = true;
			for (const hooks of this.hooks.values()) for (const h of hooks) if (typeof h?.cleanup === "function") {
				try {
					h.cleanup();
				} catch {}
			}
			this.hooks.clear();
			for (const inst of this.instances.values()) {
				try {
					inst.componentWillUnmount?.();
				} catch {}
			}
			this.instances.clear();
		}
		handler(path, fn) {
			if (typeof fn === "function") this.handlers.set(path, fn);
			return path;
		}
		input(id, value) {
			if (id === "__retry") return;
			const fn = this.handlers.get(id);
			if (fn === undefined) throw new Error(`No setting with id ${id}`);
			return fn(value);
		}
		// A value the plugin controls (`prop` defined) or, like an
		// uncontrolled React input, one the user's last input decides.
		ctl(path, prop, initial) {
			if (prop !== undefined) return prop;
			if (!this.local.has(path)) this.local.set(path, initial);
			return this.local.get(path);
		}
		callComponent(fn, props, path, ref) {
			this.seen.add(path);
			const prev = frame;
			frame = { root: this, path, hooks: this.hooks.get(path) ?? [], index: 0 };
			this.hooks.set(path, frame.hooks);
			try {
				return ref === undefined ? fn(props) : fn(props, ref);
			} finally {
				frame = prev;
			}
		}
		// One component that throws must not blank the panel: show what it
		// was given as children, or a note.
		guard(type, props, path, fn) {
			try {
				return fn();
			} catch (err) {
				try {
					L.reportError(err);
				} catch {}
				const name = type?.displayName ?? type?.name ?? "component";
				// The plugin's Settings itself threw: Luna's error boundary
				// shows "Settings crashed" with a Retry button.
				if (path === "r") return [{ kind: "crashed", error: String(err?.message ?? err) }];
				let kids = [];
				try {
					kids = this.renderChildren(props?.children, `${path}.err`);
				} catch {}
				if (kids.length) return kids;
				return [{ kind: "text", text: `(${name} could not be drawn: ${err?.message ?? err})` }];
			}
		}
		renderChildren(children, path) {
			const out = [];
			const list = Array.isArray(children) ? children : [children];
			list.forEach((c, i) => out.push(...this.renderNode(c, `${path}.${c?.key ?? i}`)));
			return this.tidy(out, path);
		}
		// Merge adjacent text, hand label text to the control it names,
		// and fold radio buttons of one group into a select.
		tidy(nodes, path) {
			const merged = [];
			for (const n of nodes) {
				const last = merged.at(-1);
				if (isText(n) && isText(last) && !n.title && !last.title && !n._b && !last._b && !n._lbl && !last._lbl) last.text = `${last.text}${n.text}`;
				else merged.push(n);
			}
			for (let i = 0; i < merged.length; i++) {
				const c = merged[i];
				if (!c.id || c.title || c.kind === "button") continue;
				const prev = merged[i - 1];
				const next = merged[i + 1];
				const lab = prev?._lbl && !prev._drop ? prev : next?._lbl && !next._drop ? next : null;
				if (lab) {
					c.title = lab.text.replace(/\s+/g, " ").trim();
					lab._drop = true;
				}
			}
			const kept = merged.filter((n) => !n._drop);
			merged.length = 0;
			merged.push(...kept);
			const out = [];
			for (let i = 0; i < merged.length; i++) {
				const n = merged[i];
				if (!n.radio) {
					out.push(n);
					continue;
				}
				const group = [n];
				while (merged[i + 1]?.radio && merged[i + 1].radio.name === n.radio.name) group.push(merged[++i]);
				if (group.length < 2) {
					out.push(n);
					continue;
				}
				const checked = group.find((g) => g.checked);
				const id = this.handler(`${group[0].id}.group`, (v) => {
					const pick = group.find((g) => sameJson(g.radio.value, v));
					return pick ? this.handlers.get(pick.id)?.(true) : undefined;
				});
				out.push({ kind: "select", id, title: group[0].groupTitle, disabled: group.every((g) => g.disabled) || undefined, value: checked ? checked.radio.value : null, options: group.map((g) => ({ value: g.radio.value, label: g.title || String(g.radio.value) })) });
			}
			return out;
		}
		block(kids) {
			if (isText(kids[0])) kids[0]._b = true;
			if (isText(kids.at(-1))) kids.at(-1)._b = true;
			return kids;
		}
		renderNode(node, path) {
			if (node === null || node === undefined || typeof node === "boolean") return [];
			if (typeof node === "string" || typeof node === "number") return [{ kind: "text", text: String(node) }];
			if (Array.isArray(node)) return this.renderChildren(node, path);
			if (typeof node?.then === "function") return [];
			if (!isValidElement(node)) return [];
			const { type, props } = node;
			if (type === FRAGMENT || type === STRICT || type === SUSPENSE || type === PROFILER) return this.renderChildren(props.children, path);
			if (typeof type === "string") return this.renderHost(type, props ?? {}, path);
			if (type && type.__luna) return this.guard(type, props, path, () => this.renderLuna(type.__luna, props ?? {}, path, type.displayName));
			if (type && type.$$typeof === PROVIDER) {
				const ctx = type._context;
				const prev = ctx._currentValue;
				ctx._currentValue = props.value;
				try {
					return this.renderChildren(props.children, path);
				} finally {
					ctx._currentValue = prev;
				}
			}
			if (type && type.$$typeof === CONTEXT && type._consumer) return this.renderNode(props.children?.(type._consumer._currentValue), `${path}.c`);
			if (type && type.$$typeof === MEMO) return this.renderNode(mkElement(type.type, node.key, node.ref, props), path);
			if (type && type.$$typeof === FORWARD) return this.guard(type, props, path, () => this.renderNode(this.callComponent(type.render, props, path, node.ref), `${path}.0`));
			if (type && type.$$typeof === LAZY) {
				if (type._resolved) return this.renderNode(mkElement(type._resolved, node.key, node.ref, props), path);
				if (!type._loading) {
					type._loading = true;
					Promise.resolve()
						.then(() => type.load?.() ?? type._payload?._result)
						.then(
							(m) => {
								type._resolved = m?.default ?? m ?? (() => null);
								this.invalidate();
							},
							() => (type._resolved = () => null),
						);
				}
				return [];
			}
			if (typeof type === "function") {
				if (type.prototype && type.prototype.isReactComponent) {
					return this.guard(type, props, path, () => {
						this.seen.add(path);
						let inst = this.instances.get(path);
						if (!inst) {
							inst = new type(props);
							inst.__root = this;
							this.instances.set(path, inst);
							inst.componentDidMount && this.pendingEffects.push(() => inst.componentDidMount());
						}
						const prevProps = inst.props;
						inst.props = props;
						if (typeof type.getDerivedStateFromProps === "function") inst.state = { ...inst.state, ...(type.getDerivedStateFromProps(props, inst.state) ?? {}) };
						if (prevProps !== props && inst.componentDidUpdate) this.pendingEffects.push(() => inst.componentDidUpdate(prevProps, inst.state));
						return this.renderNode(inst.render(), `${path}.0`);
					});
				}
				return this.guard(type, props, path, () => this.renderNode(this.callComponent(type, props, path), `${path}.0`));
			}
			// Anything else (a foreign wrapper object, a render-object): use
			// its render/type, or fall back to the children it was given.
			if (type && typeof type.render === "function") return this.guard(type, props, path, () => this.renderNode(this.callComponent(type.render, props, path, node.ref), `${path}.0`));
			if (type && type.type) return this.renderNode(mkElement(type.type, node.key, node.ref, props), path);
			return this.renderChildren(props?.children, path);
		}
		// ---- plain elements
		// An element with `position: fixed` leaves the page for the window: a
		// backdrop (a button that closes) or a card over it (colorama-lyrics'
		// colour picker). It is marked, so the host draws it over the window
		// and not in the list.
		renderHost(tag, props, path) {
			const out = this.renderHostFlow(tag, props, path);
			if (out.length === 0 || props.style?.position !== "fixed") return out;
			if (tag === "button" && out.length === 1 && out[0].kind === "button") {
				out[0].scrim = true;
				return out;
			}
			return [{ kind: "group", overlay: true, children: out }];
		}
		renderHostFlow(tag, props, path) {
			if (SKIP.has(tag) || props.hidden === true || props.style?.display === "none" || props["aria-hidden"] === true || props["aria-hidden"] === "true") return [];
			const disabled = props.disabled ? true : undefined;
			const aria = props["aria-label"] ?? props.title;
			switch (tag) {
				case "a": {
					if (props.href) return [{ kind: "link", text: textOnly(props.children) || String(props.href), href: String(props.href) }];
					if (typeof props.onClick === "function") return this.buttonNode(props, path);
					return this.renderChildren(props.children, path);
				}
				case "br":
					return [{ kind: "text", text: "\n", _b: true }];
				case "button":
					return this.buttonNode(props, path);
				case "input":
					return this.inputNode(props, path);
				case "textarea":
					return this.textNode(props, path, { multiline: true });
				case "select": {
					const { options, items } = optionsOf(props.children);
					const multiple = !!props.multiple;
					const value = this.ctl(path, props.value, props.defaultValue ?? (multiple ? [] : (options[0]?.value ?? null)));
					const id = this.handler(path, (v) => {
						this.local.set(path, v);
						const ev = mkEvent({ value: v, name: props.name, id: props.id, type: "select-one" });
						const i = options.findIndex((o) => sameJson(o.value, v));
						props.onChange?.(ev, items[i]);
						props.onInput?.(ev);
					});
					const node = { kind: "select", id, title: opt(aria), disabled, value, options, multiple: multiple || undefined };
					if (props.id) this.byId.set(props.id, node);
					return [node];
				}
				case "label": {
					const kids = this.renderChildren(props.children, path);
					for (const k of kids) if (isText(k)) k._lbl = true;
					if (props.htmlFor) for (const k of kids) if (isText(k)) this.labelFor.push({ id: props.htmlFor, node: k });
					return kids;
				}
				case "progress":
				case "meter": {
					const v = num(props.value);
					const max = num(props.max) ?? 1;
					return v === undefined ? [] : [{ kind: "text", text: `${Math.round((v / max) * 100)}%`, _b: true }];
				}
				case "details":
				case "summary":
					return this.block(this.renderChildren(props.children, path));
				default:
			}
			const role = props.role;
			if ((role === "switch" || role === "checkbox") && typeof props.onClick === "function" && props["aria-checked"] !== undefined) {
				const checked = props["aria-checked"] === true || props["aria-checked"] === "true";
				const id = this.handler(path, () => props.onClick(mkEvent({ checked: !checked })));
				const label = textOnly(props.children) || aria;
				return [{ kind: "switch", id, title: opt(label), disabled, checked }];
			}
			if (role === "button" && typeof props.onClick === "function") return this.buttonNode(props, path);
			let kids = this.renderChildren(props.children, path);
			if (HEADING.test(tag)) {
				const text = kids.filter(isText).map((k) => k.text).join(" ");
				return text ? [{ kind: "text", text, title: true, _b: true }, ...kids.filter((k) => !isText(k))] : kids;
			}
			if (tag === "li") {
				if (isText(kids[0])) kids[0].text = `• ${kids[0].text}`;
				else kids = [{ kind: "text", text: "•" }, ...kids];
			}
			if ((tag === "td" || tag === "th") && isText(kids.at(-1))) kids.at(-1).text += "  ";
			return BLOCK.has(tag) ? this.block(kids) : kids;
		}
		buttonNode(props, path, label0) {
			const kids = this.renderChildren(props.children, path);
			const text = kids.filter(isText).map((k) => k.text).join(" ").trim();
			const rest = kids.filter((k) => !isText(k));
			if (rest.some((k) => k.id)) return kids; // wraps real controls; not a button of its own
			const id = this.handler(path, () => props.onClick?.(mkEvent({ value: props.value, name: props.name, id: props.id })));
			const title = props["aria-label"] ?? props.title;
			// A swatch (colorama-lyrics' colour grid): no words, only a
			// background, which names it.
			const st = props.style && typeof props.style === "object" ? props.style : null;
			const bg = st ? st.backgroundColor ?? st.background : undefined;
			return [{ kind: "button", id, disabled: props.disabled || props.loading ? true : undefined, label: text || (typeof title === "string" ? title : undefined) || label0 || (typeof bg === "string" && bg.trim() ? bg.trim() : "Run") }];
		}
		// A field that records keys: it reacts to keydown and has no text
		// input of its own (quickExclusiveMode's keybind).
		isCapture(p) {
			if (typeof p.onKeyDown !== "function") return false;
			const html = p.slotProps?.htmlInput ?? p.inputProps ?? {};
			return !!(p.readOnly || html.readOnly || (typeof p.onChange !== "function" && typeof p.onInput !== "function"));
		}
		captureNode(p, path, base) {
			this.capProps.set(path, p);
			const id = this.handler(path, (k) => this.captureKeys(path, k));
			return [{ kind: "keys", id, ...base, value: String(p.value ?? p.defaultValue ?? "") }];
		}
		// Replay a key combination the app recorded as the browser would
		// send it: focus, modifier and key downs, ups, then blur once the
		// plugin's (often debounced) handler has stored it.
		captureKeys(path, k) {
			if (!k || typeof k.key !== "string") return;
			const target = { value: String(this.capProps.get(path)?.value ?? "") };
			const fire = (name, ev) => {
				const fn = this.capProps.get(path)?.[name];
				if (typeof fn === "function")
					try {
						fn(ev);
					} catch (err) {
						L.reportError(err);
					}
				if (!this.dead) this.render();
			};
			const state = { ctrlKey: false, shiftKey: false, altKey: false, metaKey: false };
			const kev = (type, key, code) => {
				const m = { ...state };
				return Object.assign(mkEvent(target), { type, key, code, ...m, repeat: false, location: 0, getModifierState: (n) => ({ Control: m.ctrlKey, Shift: m.shiftKey, Alt: m.altKey, Meta: m.metaKey })[n] ?? false });
			};
			fire("onFocus", Object.assign(mkEvent(target), { type: "focus" }));
			const held = [];
			for (const [flag, key, code] of [["ctrlKey", "Control", "ControlLeft"], ["shiftKey", "Shift", "ShiftLeft"], ["altKey", "Alt", "AltLeft"], ["metaKey", "Meta", "MetaLeft"]])
				if (k[flag]) {
					state[flag] = true;
					held.push([flag, key, code]);
					fire("onKeyDown", kev("keydown", key, code));
				}
			fire("onKeyDown", kev("keydown", k.key, String(k.code ?? "")));
			fire("onKeyUp", kev("keyup", k.key, String(k.code ?? "")));
			for (const [flag, key, code] of held.reverse()) {
				state[flag] = false;
				fire("onKeyUp", kev("keyup", key, code));
			}
			L.internalTimeout(() => {
				if (this.dead) return;
				fire("onBlur", Object.assign(mkEvent(target), { type: "blur" }));
				this.invalidate();
			}, 400);
		}
		textNode(props, path, extra = {}) {
			if (this.isCapture(props)) return this.captureNode(props, path, { title: opt(props["aria-label"] ?? props.title), disabled: props.disabled ? true : undefined });
			const type = String(props.type ?? "text").toLowerCase();
			const value = String(this.ctl(path, props.value, props.defaultValue ?? "") ?? "");
			const id = this.handler(path, (v) => {
				const s = String(v ?? "");
				this.local.set(path, s);
				const ev = mkEvent({ value: s, name: props.name, id: props.id, type: props.type ?? "text" });
				props.onChange?.(ev);
				props.onInput?.(ev);
				props.onBlur?.(ev);
			});
			const node = { kind: "text", id, title: opt(props["aria-label"] ?? props.title), disabled: props.disabled || props.readOnly ? true : undefined, value, secure: type === "password" || undefined, multiline: extra.multiline || undefined, placeholder: opt(props.placeholder) };
			if (props.id) this.byId.set(props.id, node);
			return [node];
		}
		inputNode(props, path) {
			const type = String(props.type ?? "text").toLowerCase();
			const disabled = props.disabled || props.readOnly ? true : undefined;
			const title = opt(props["aria-label"] ?? props.title);
			if (type === "hidden" || type === "file" || type === "image") return [];
			if (type === "checkbox" || type === "radio") {
				const checked = !!this.ctl(path, props.checked, !!props.defaultChecked);
				const id = this.handler(path, (v) => {
					const c = type === "radio" ? true : !!v;
					this.local.set(path, c);
					const ev = mkEvent({ checked: c, value: props.value ?? "on", name: props.name, id: props.id, type });
					props.onChange?.(ev, c);
					props.onClick?.(ev);
				});
				const node = { kind: "switch", id, title, disabled, checked };
				if (type === "radio") node.radio = { name: props.name ?? "", value: props.value ?? "on" };
				if (props.id) this.byId.set(props.id, node);
				return [node];
			}
			if (type === "number" || type === "range") {
				const value = num(this.ctl(path, num(props.value), num(props.defaultValue))) ?? num(props.min) ?? 0;
				const min = num(props.min);
				const max = num(props.max);
				const id = this.handler(path, (v) => {
					const n = Number(v);
					if (isNaN(n) || (min !== undefined && n < min) || (max !== undefined && n > max)) return;
					this.local.set(path, n);
					const ev = mkEvent({ value: String(n), valueAsNumber: n, name: props.name, id: props.id, type });
					props.onChange?.(ev);
					props.onInput?.(ev);
				});
				const node = { kind: "number", id, title, disabled, value, min, max, step: num(props.step), slider: type === "range" && min !== undefined && max !== undefined ? true : undefined };
				if (props.id) this.byId.set(props.id, node);
				return [node];
			}
			if (type === "button" || type === "submit" || type === "reset") {
				const id = this.handler(path, () => props.onClick?.(mkEvent({ value: props.value, name: props.name, id: props.id })));
				return [{ kind: "button", id, disabled, label: String(props.value ?? title ?? (type === "submit" ? "Submit" : type === "reset" ? "Reset" : "Run")) }];
			}
			return this.textNode(props, path);
		}
		// `name`: the component (`LunaNumberSetting`, MUI's `Button`), which
		// the page draws in that component's look.
		renderLuna(kind, p, path, name) {
			const title = p.title === undefined ? (typeof p.label === "string" ? p.label : undefined) : textOnly(p.title);
			const d = p.desc === undefined && p.helperText === undefined ? undefined : textOf(p.desc ?? p.helperText);
			const base = { title: title || undefined, desc: d?.text || undefined, links: d?.links?.length ? d.links : undefined, disabled: !!p.disabled || !!p.loading || undefined, mui: name || undefined, variant: typeof p.variant === "string" ? p.variant : undefined };
			switch (kind) {
				case "group":
				case "setting": {
					const children = this.renderChildren(p.children, path);
					if (kind === "setting" && !title && !d) return this.block(children);
					return [{ kind: "group", ...base, children }];
				}
				case "pass":
					return this.renderChildren(p.children, path);
				case "block":
					return this.block(this.renderChildren(p.children, path));
				case "none":
					return [];
				case "switch": {
					const checked = !!this.ctl(path, p.checked, p.defaultChecked ?? (typeof p.value === "boolean" ? p.value : false));
					const id = this.handler(path, (v) => {
						const c = !!v;
						this.local.set(path, c);
						const ev = mkEvent({ checked: c, value: c, name: p.name, id: p.id, type: "checkbox" });
						if (p.onChange) return p.onChange(ev, c);
						if (p.onClick) return p.onClick(ev);
					});
					return [{ kind: "switch", id, ...base, checked }];
				}
				case "formlabel": {
					const label = textOnly(p.label);
					const kids = p.control ? this.renderNode(p.control, `${path}.ctl`) : this.renderChildren(p.children, path);
					const c = kids.find((k) => k.id && !k.title);
					if (c && label) c.title = label;
					else if (label) kids.push({ kind: "text", text: label });
					return kids;
				}
				case "number": {
					const ip = p.inputProps ?? p.slotProps?.htmlInput ?? {};
					const min = num(p.min ?? ip.min);
					const max = num(p.max ?? ip.max);
					const value = num(this.ctl(path, num(p.value), num(p.defaultValue))) ?? 0;
					const id = this.handler(path, (v) => {
						const n = Number(v);
						if (isNaN(n) || (min !== undefined && n < min) || (max !== undefined && n > max)) return;
						this.local.set(path, n);
						if (p.onNumber) return p.onNumber(n);
						if (p.onChange) return p.onChange(mkEvent({ value: String(n), valueAsNumber: n, name: p.name, id: p.id, type: "number" }));
					});
					return [{ kind: "number", id, ...base, title: base.title ?? opt(p.placeholder), value, min, max, step: num(p.step ?? ip.step) }];
				}
				case "slider": {
					if (Array.isArray(p.value ?? p.defaultValue)) return [{ kind: "text", text: `${base.title ?? "Range"}: ${(p.value ?? p.defaultValue).join(" - ")}` }];
					const min = num(p.min) ?? 0;
					const max = num(p.max) ?? 100;
					const value = num(this.ctl(path, num(p.value), num(p.defaultValue))) ?? min;
					const id = this.handler(path, (v) => {
						const n = Number(v);
						if (isNaN(n)) return;
						this.local.set(path, n);
						const ev = mkEvent({ value: n, name: p.name });
						p.onChange?.(ev, n, 0);
						p.onChangeCommitted?.(ev, n);
					});
					return [{ kind: "number", id, ...base, title: base.title ?? opt(p["aria-label"]), value, min, max, step: num(p.step) ?? 1, slider: true }];
				}
				case "text":
				case "secure": {
					if (p.select) return this.renderLuna("select", { ...p, children: p.children }, path, name);
					if (this.isCapture(p)) return this.captureNode(p, path, base);
					const type = String(p.type ?? "text").toLowerCase();
					if (type === "number") return this.renderLuna("number", { ...p, onChange: p.onChange, inputProps: p.inputProps ?? p.slotProps?.htmlInput, value: p.value === undefined || p.value === "" ? undefined : Number(p.value) }, path, name);
					const value = String(this.ctl(path, p.value, p.defaultValue ?? "") ?? "");
					const id = this.handler(path, (v) => {
						const s = String(v ?? "");
						this.local.set(path, s);
						const ev = mkEvent({ value: s, name: p.name, id: p.id, type });
						p.onChange?.(ev);
						p.onBlur?.(ev);
					});
					return [{ kind: "text", id, ...base, value, secure: kind === "secure" || type === "password" || undefined, multiline: p.multiline || p.rows > 1 || p.minRows > 1 || undefined, placeholder: opt(p.placeholder) }];
				}
				case "select": {
					const { options, items } = optionsOf(p.children);
					const multiple = !!p.multiple;
					const value = this.ctl(path, p.value, p.defaultValue ?? (multiple ? [] : null));
					const id = this.handler(path, (v) => {
						const found = options.findIndex((o) => sameJson(o.value, v));
						const val = found >= 0 ? options[found].value : v;
						this.local.set(path, val);
						return p.onChange?.(mkEvent({ value: val, name: p.name, id: p.id, type: "select-one" }), items[found]);
					});
					return [{ kind: "select", id, ...base, value, options, multiple: multiple || undefined }];
				}
				case "button": {
					if (!p.onClick && p.href) return [{ kind: "link", text: textOnly(p.children) || title || String(p.href), href: String(p.href) }];
					const id = this.handler(path, () => p.onClick?.(mkEvent({ value: p.value, name: p.name, id: p.id })));
					const label = textOnly(p.children) || title || (typeof p.tooltip === "string" ? p.tooltip : undefined) || p["aria-label"] || p.defaultLabel || "Run";
					return [{ kind: "button", id, ...base, label }];
				}
				case "link":
					return [{ kind: "link", text: textOnly(p.children) || String(p.href ?? ""), href: String(p.href ?? "") }];
				case "title":
					return [{ kind: "text", text: title ?? "", title: true, desc: d?.text || undefined }];
				case "typo": {
					const v = String(p.variant ?? "body1");
					const kids = this.renderChildren(p.children, path);
					if (/^h[1-6]$/.test(v) || v === "subtitle1") for (const k of kids) if (isText(k)) k.title = true;
					return this.block(kids);
				}
				case "author": {
					const a = p.author;
					const name = typeof a === "string" ? a : a?.name;
					return name ? [{ kind: "text", text: `${p.prefix ?? "by "}${name}` }] : [];
				}
				case "item":
					return [];
				default:
					return this.renderChildren(p.children, path);
			}
		}
	}
	L.SettingsRoot = Root;

	// ---------------------------------------------------------------- DOM renderer
	// `createRoot(el).render(<X/>)`: the element tree becomes nodes of the
	// virtual `document` (host elements with props, style, class and on*
	// handlers; function, class, memo and forwardRef components with hooks,
	// fragments, context, text). Nodes are kept by tree path between renders,
	// so state, focus and listeners survive.
	const SVG_NS = "http://www.w3.org/2000/svg";
	const UNITLESS = new Set(["opacity", "zIndex", "flex", "flexGrow", "flexShrink", "fontWeight", "lineHeight", "order", "zoom", "aspectRatio", "scale"]);
	const SVG_KEEP = new Set(["viewBox", "preserveAspectRatio", "gradientUnits", "gradientTransform", "patternUnits", "clipPathUnits", "maskUnits", "markerWidth", "markerHeight", "refX", "refY", "textLength", "baseFrequency", "stdDeviation", "tableValues", "pathLength", "filterUnits", "primitiveUnits", "spreadMethod", "startOffset", "viewTarget"]);
	const kebab = (k) => k.replace(/[A-Z]/g, (m) => `-${m.toLowerCase()}`);
	const eventName = (prop, tag) => {
		let n = prop.slice(2);
		if (n.endsWith("Capture")) n = n.slice(0, -7);
		n = n.toLowerCase();
		if (n === "doubleclick") return "dblclick";
		if (n === "change" && (tag === "input" || tag === "textarea")) return "input";
		return n;
	};
	const toStyle = (el, style, prev) => {
		style = style && typeof style === "object" ? style : {};
		for (const k of Object.keys(prev)) if (!(k in style)) el.style.removeProperty(k.startsWith("--") ? k : kebab(k));
		for (const [k, v] of Object.entries(style)) {
			if (v === prev[k]) continue;
			const name = k.startsWith("--") ? k : kebab(k);
			if (v === null || v === undefined || v === false || v === "") el.style.removeProperty(name);
			else el.style.setProperty(name, typeof v === "number" && v !== 0 && !UNITLESS.has(k) && !k.startsWith("--") ? `${v}px` : String(v));
		}
		return { ...style };
	};
	class DomRoot extends Root {
		constructor(container) {
			super("dom", null);
			this.container = container;
			this.element = null;
			this.cache = new Map();
			this.refs = [];
		}
		setElement(el) {
			this.element = el;
			this.render();
		}
		render() {
			if (this.dead) return null;
			this.rendering = true;
			this.seen = new Set();
			this.used = new Set();
			this.refs = [];
			let nodes = [];
			try {
				nodes = this.build(this.element, "r", false);
			} catch (err) {
				L.reportError(err);
			} finally {
				this.rendering = false;
			}
			this.sync(this.container, nodes);
			for (const k of [...this.cache.keys()]) if (!this.used.has(k)) this.cache.delete(k);
			for (const [path, hooks] of this.hooks)
				if (!this.seen.has(path)) {
					for (const h of hooks) if (typeof h?.cleanup === "function") h.cleanup();
					this.hooks.delete(path);
				}
			for (const [ref, el] of this.refs) {
				if (typeof ref === "function") ref(el);
				else if (ref && typeof ref === "object") ref.current = el;
			}
			const effects = this.pendingEffects;
			this.pendingEffects = [];
			for (const e of effects) {
				try {
					e();
				} catch (err) {
					L.reportError(err);
				}
			}
			return null;
		}
		unmount() {
			super.unmount();
			this.sync(this.container, []);
			this.cache.clear();
		}
		sync(parent, nodes) {
			nodes.forEach((n, i) => {
				if (parent.childNodes[i] !== n) parent.insertBefore(n, parent.childNodes[i] ?? null);
			});
			while (parent.childNodes.length > nodes.length) parent.removeChild(parent.lastChild);
		}
		build(node, path, svg) {
			if (node === null || node === undefined || typeof node === "boolean") return [];
			if (typeof node === "string" || typeof node === "number") {
				this.used.add(path);
				let t = this.cache.get(path);
				if (!t || t.nodeType !== 3) {
					t = document.createTextNode("");
					this.cache.set(path, t);
				}
				if (t.data !== String(node)) t.data = String(node);
				return [t];
			}
			if (Array.isArray(node)) {
				const out = [];
				node.forEach((c, i) => out.push(...this.build(c, `${path}.${c?.key ?? i}`, svg)));
				return out;
			}
			if (typeof node?.then === "function" || !isValidElement(node)) return [];
			const { type, props } = node;
			if (type === FRAGMENT || type === STRICT || type === SUSPENSE || type === PROFILER) return this.build(props.children, path, svg);
			if (typeof type === "string") return [this.host(node, path, svg)];
			if (type && type.$$typeof === PROVIDER) {
				const ctx = type._context;
				const prev = ctx._currentValue;
				ctx._currentValue = props.value;
				try {
					return this.build(props.children, path, svg);
				} finally {
					ctx._currentValue = prev;
				}
			}
			if (type && type.$$typeof === CONTEXT && type._consumer) return this.build(props.children?.(type._consumer._currentValue), `${path}.c`, svg);
			if (type && type.$$typeof === MEMO) return this.build(mkElement(type.type, node.key, node.ref, props), path, svg);
			if (type && type.$$typeof === FORWARD) return this.build(this.callComponent(type.render, props, path, node.ref), `${path}.0`, svg);
			if (typeof type === "function") {
				if (type.prototype && type.prototype.isReactComponent) {
					this.seen.add(path);
					let inst = this.instances.get(path);
					if (!inst) {
						inst = new type(props);
						inst.__root = this;
						this.instances.set(path, inst);
						inst.componentDidMount && this.pendingEffects.push(() => inst.componentDidMount());
					}
					inst.props = props;
					return this.build(inst.render(), `${path}.0`, svg);
				}
				return this.build(this.callComponent(type, props, path), `${path}.0`, svg);
			}
			return [];
		}
		host(node, path, svgParent) {
			const { type, props } = node;
			const svg = svgParent || type === "svg";
			this.used.add(path);
			let el = this.cache.get(path);
			if (!el || el._lt !== type) {
				el = svg ? document.createElementNS(SVG_NS, type) : document.createElement(type);
				el._lt = type;
				el._lp = {};
				el._lh = {};
				el._lst = {};
				this.cache.set(path, el);
			}
			const prev = el._lp;
			for (const k of Object.keys(prev)) if (!(k in props) && k !== "children") this.unset(el, k);
			for (const [k, v] of Object.entries(props)) {
				if (k === "children" || k === "key" || k === "ref") continue;
				if (prev[k] === v && k !== "style") continue;
				this.set(el, k, v, svg, type);
			}
			el._lp = { ...props };
			if (props.dangerouslySetInnerHTML) {
				const html = String(props.dangerouslySetInnerHTML.__html ?? "");
				if (el._html !== html) {
					el._html = html;
					el.innerHTML = html;
				}
			} else {
				// A single child is its own node, not the element's: the same
				// tree path would hand back the parent from the cache (a `div`
				// whose only child is a `div` threw HierarchyRequestError).
				const kids = Array.isArray(props.children) ? props.children : [props.children];
				this.sync(el, this.build(kids, path, svg && type !== "foreignObject"));
			}
			if (node.ref) this.refs.push([node.ref, el]);
			return el;
		}
		unset(el, k) {
			if (k === "style") el._lst = toStyle(el, null, el._lst);
			else if (/^on[A-Z]/.test(k)) delete el._lh[eventName(k, el._lt)];
			else if (k === "className") el.removeAttribute("class");
			else if (k === "value") el.value = "";
			else if (k === "checked") el.checked = false;
			else el.removeAttribute(k === "htmlFor" ? "for" : k);
		}
		set(el, k, v, svg, tag) {
			if (k === "style") {
				el._lst = toStyle(el, v, el._lst);
				return;
			}
			if (/^on[A-Z]/.test(k)) {
				const name = eventName(k, tag);
				const capture = k.endsWith("Capture");
				if (typeof v === "function") {
					if (!(name in el._lh)) el.addEventListener(name, (e) => el._lh[name]?.(e), capture);
					el._lh[name] = v;
				} else delete el._lh[name];
				return;
			}
			if (k === "className" || k === "class") {
				if (v === null || v === undefined || v === false) el.removeAttribute("class");
				else el.setAttribute("class", String(v));
				return;
			}
			if (k === "value") {
				el.value = v === null || v === undefined ? "" : String(v);
				return;
			}
			if (k === "checked" || k === "disabled" || k === "selected") {
				el[k] = !!v;
				if (v) el.setAttribute(k, "");
				else el.removeAttribute(k);
				return;
			}
			if (k === "dangerouslySetInnerHTML" || k === "suppressContentEditableWarning" || k === "suppressHydrationWarning" || k === "defaultValue" || k === "defaultChecked") return;
			if (v === null || v === undefined || v === false) return void el.removeAttribute(k === "htmlFor" ? "for" : k);
			let name = k === "htmlFor" ? "for" : k;
			if (svg) {
				if (name === "xlinkHref") name = "xlink:href";
				else if (!SVG_KEEP.has(name)) name = kebab(name);
			} else if (name === "tabIndex") name = "tabindex";
			else if (name === "readOnly") name = "readonly";
			else if (name === "contentEditable") name = "contenteditable";
			el.setAttribute(name, v === true ? "" : String(v));
		}
	}
	L.DomRoot = DomRoot;

	const React = {
		createElement,
		cloneElement,
		isValidElement,
		Children,
		Component,
		PureComponent,
		Fragment: FRAGMENT,
		StrictMode: STRICT,
		Suspense: SUSPENSE,
		Profiler: PROFILER,
		memo,
		forwardRef,
		createContext,
		createRef,
		lazy,
		useState,
		useReducer,
		useEffect,
		useLayoutEffect: useEffect,
		useInsertionEffect: useEffect,
		useMemo,
		useCallback,
		useRef,
		useContext,
		useId,
		useSyncExternalStore,
		useTransition,
		useDeferredValue,
		useImperativeHandle,
		useDebugValue,
		useOptimistic,
		useActionState,
		useEffectEvent,
		use,
		startTransition: (fn) => fn(),
		version: "19.0.0-luna",
	};
	React.default = React;
	const jsxRuntime = { jsx, jsxs: jsx, jsxDEV: jsx, Fragment: FRAGMENT };
	jsxRuntime.default = jsxRuntime;
	const isNode = (c) => !!c && typeof c === "object" && typeof c.appendChild === "function";
	const createRoot = (container) => {
		// Without a DOM container (settings pages) there is nothing to draw.
		if (!isNode(container)) return { render() {}, unmount() {} };
		const root = new DomRoot(container);
		return {
			render: (element) => root.setElement(element),
			unmount: () => root.unmount(),
		};
	};
	const reactDomClient = {
		createRoot,
		hydrateRoot: (container, element) => {
			const root = createRoot(container);
			root.render(element);
			return root;
		},
	};
	reactDomClient.default = reactDomClient;
	const legacy = new WeakMap();
	const reactDom = {
		createPortal: (children) => children,
		flushSync: (fn) => fn?.(),
		render(element, container) {
			if (!isNode(container)) return;
			let root = legacy.get(container);
			if (!root) legacy.set(container, (root = createRoot(container)));
			root.render(element);
		},
		unmountComponentAtNode(container) {
			legacy.get(container)?.unmount();
			return legacy.delete(container);
		},
		...reactDomClient,
	};
	reactDom.default = reactDom;
	L.react = { React, jsxRuntime, reactDomClient, reactDom };
})();
