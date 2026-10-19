// @luna/ui: the settings components plugins build their Settings from. Each
// is a marker the settings renderer (react.js) turns into a native node.
"use strict";
(() => {
	const L = globalThis.__lunaInternal;
	const { React } = L.react;
	const mark = (name, kind) => {
		const C = (props) => React.createElement(C, props);
		Object.defineProperty(C, "name", { value: name });
		C.__luna = kind;
		C.displayName = name;
		return C;
	};
	// A marker that also supplies default props (a label for icon buttons).
	const markWith = (name, kind, defaults) => {
		const C = mark(name, kind);
		C.defaultProps = defaults;
		return C;
	};
	// TidaLuna's MUI theme (`lunaMuiTheme`, the result of MUI's `createTheme`
	// with Luna's palette, typography and component overrides), built here
	// with MUI's defaults, so a plugin that bundles MUI and hands the theme to
	// its own `ThemeProvider` finds every field it reads (palette, typography,
	// spacing, breakpoints, transitions, shadows, z-index, mixins).
	const buildLunaTheme = () => {
		const grey = { 50: "#fafafa", 100: "#f5f5f5", 200: "#eeeeee", 300: "#e0e0e0", 400: "#bdbdbd", 500: "#9e9e9e", 600: "#757575", 700: "#616161", 800: "#424242", 900: "#212121", A100: "#f5f5f5", A200: "#eeeeee", A400: "#bdbdbd", A700: "#616161" };
		const rgb = (h) => (h.length === 4 ? [1, 2, 3].map((i) => parseInt(h[i] + h[i], 16)) : [1, 3, 5].map((i) => parseInt(h.slice(i, i + 2), 16)));
		const hex = (c) => `#${c.map((v) => Math.round(v).toString(16).padStart(2, "0")).join("")}`;
		const darken = (h, k) => hex(rgb(h).map((v) => v * (1 - k)));
		const lighten = (h, k) => hex(rgb(h).map((v) => v + (255 - v) * k));
		const luminance = (h) => {
			const [r, g, b] = rgb(h).map((v) => {
				v /= 255;
				return v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4;
			});
			return 0.2126 * r + 0.7152 * g + 0.0722 * b;
		};
		const contrast = (a, b) => {
			const [x, y] = [luminance(a), luminance(b)].sort((p, q) => q - p);
			return (x + 0.05) / (y + 0.05);
		};
		const text = { primary: grey.A200, secondary: "#a7a7a9", disabled: "rgba(0, 0, 0, 0.38)" };
		const getContrastText = (bg) => (contrast(bg, "#ffffff") >= 3 ? "#fff" : "rgba(0, 0, 0, 0.87)");
		const augmentColor = ({ color: c, mainShade = 500, lightShade = 300, darkShade = 700 }) => {
			const b = { ...c };
			if (!b.main && b[mainShade]) b.main = b[mainShade];
			b.light ??= b[lightShade] ?? lighten(b.main, 0.2);
			b.dark ??= b[darkShade] ?? darken(b.main, 0.3);
			b.contrastText ??= getContrastText(b.main);
			return b;
		};
		const color = (c) => augmentColor({ color: typeof c === "string" ? { main: c } : c });
		const palette = {
			mode: "light",
			common: { black: "#000", white: "#fff" },
			primary: color(grey.A200),
			secondary: color("#31d8ff"),
			error: color({ main: "#d32f2f", light: "#ef5350", dark: "#c62828" }),
			warning: color({ main: "#ed6c02", light: "#ff9800", dark: "#e65100" }),
			info: color({ main: "#0288d1", light: "#03a9f4", dark: "#01579b" }),
			success: color({ main: "#2e7d32", light: "#4caf50", dark: "#1b5e20" }),
			grey,
			contrastThreshold: 3,
			tonalOffset: 0.2,
			text,
			divider: "rgba(0, 0, 0, 0.12)",
			background: { paper: "#fff", default: "#fff" },
			action: { active: "rgba(0, 0, 0, 0.54)", hover: "rgba(0, 0, 0, 0.04)", hoverOpacity: 0.04, selected: "rgba(0, 0, 0, 0.08)", selectedOpacity: 0.08, disabled: "rgba(0, 0, 0, 0.26)", disabledBackground: "rgba(0, 0, 0, 0.12)", disabledOpacity: 0.38, focus: "rgba(0, 0, 0, 0.12)", focusOpacity: 0.12, activatedOpacity: 0.12 },
			getContrastText,
			augmentColor,
		};
		const family = '"Roboto", "Helvetica", "Arial", sans-serif';
		const face = (fontWeight, size, lineHeight, letterSpacing, extra) => ({ fontFamily: family, fontWeight, fontSize: `${size}rem`, lineHeight, letterSpacing, ...extra });
		const h6 = face(500, 1.25, 1.6, "0.0075em");
		const typography = {
			htmlFontSize: 16,
			pxToRem: (px) => `${px / 16}rem`,
			fontFamily: family,
			fontSize: 14,
			fontWeightLight: 300,
			fontWeightRegular: 400,
			fontWeightMedium: 500,
			fontWeightBold: 700,
			h1: face(300, 6, 1.167, "-0.01562em"),
			h2: face(300, 3.75, 1.2, "-0.00833em"),
			h3: face(400, 3, 1.167, "0em"),
			h4: face(400, 2.125, 1.235, "0.00735em"),
			h5: face(400, 1.5, 1.334, "0em"),
			h6,
			subtitle1: face(400, 1, 1.75, "0.00938em"),
			subtitle2: { ...face(500, 0.875, 1.57, "0.00714em"), color: grey.A400 },
			body1: face(400, 1, 1.5, "0.00938em"),
			body2: face(400, 0.875, 1.43, "0.01071em"),
			button: face(500, 0.875, 1.75, "0.02857em", { textTransform: "uppercase" }),
			caption: face(400, 0.75, 1.66, "0.03333em"),
			overline: face(400, 0.75, 2.66, "0.08333em", { textTransform: "uppercase" }),
			inherit: { fontFamily: "inherit", fontWeight: "inherit", fontSize: "inherit", lineHeight: "inherit", letterSpacing: "inherit" },
			h7: { ...h6, fontSize: "1.2rem" },
			h8: { ...h6, fontSize: "1.075rem" },
			h9: { ...h6, fontSize: "1.05rem" },
		};
		const spacing = (...args) => (args.length === 0 ? "8px" : args.map((n) => (typeof n === "number" ? `${8 * n}px` : n)).join(" "));
		spacing.mui = true;
		const values = { xs: 0, sm: 600, md: 900, lg: 1200, xl: 1536 };
		const val = (k) => (typeof k === "number" ? k : values[k]);
		const keys = Object.keys(values);
		const breakpoints = {
			keys,
			values,
			unit: "px",
			step: 5,
			up: (k) => `@media (min-width:${val(k)}px)`,
			down: (k) => `@media (max-width:${val(k) - 0.05}px)`,
			between: (a, b) => `@media (min-width:${val(a)}px) and (max-width:${val(b) - 0.05}px)`,
			only: (k) => {
				const i = keys.indexOf(k);
				return i === keys.length - 1 ? `@media (min-width:${val(k)}px)` : `@media (min-width:${val(k)}px) and (max-width:${val(keys[i + 1]) - 0.05}px)`;
			},
			not: (k) => {
				const i = keys.indexOf(k);
				return i === 0 ? `@media (min-width:${val(keys[1])}px)` : `@media (max-width:${val(k) - 0.05}px), (min-width:${val(keys[i + 1] ?? k)}px)`;
			},
		};
		const shadow = (a, b, c) => `0px ${a}px ${b}px ${c < 0 ? c : 0}px rgba(0,0,0,0.2),0px ${a}px ${a * 2}px 0px rgba(0,0,0,0.14),0px 1px ${a + 2}px 0px rgba(0,0,0,0.12)`;
		const shadows = ["none", ...Array.from({ length: 24 }, (_, i) => shadow(Math.ceil((i + 1) / 2), i + 2, -1))];
		const duration = { shortest: 150, shorter: 200, short: 250, standard: 300, complex: 375, enteringScreen: 225, leavingScreen: 195 };
		const easing = { easeInOut: "cubic-bezier(0.4, 0, 0.2, 1)", easeOut: "cubic-bezier(0.0, 0, 0.2, 1)", easeIn: "cubic-bezier(0.4, 0, 1, 1)", sharp: "cubic-bezier(0.4, 0, 0.6, 1)" };
		const ms = (v) => (typeof v === "string" ? v : `${Math.round(v)}ms`);
		const transitions = {
			easing,
			duration,
			create: (props = ["all"], options = {}) => (Array.isArray(props) ? props : [props]).map((p) => `${p} ${typeof options.duration === "string" ? options.duration : ms(options.duration ?? duration.standard)} ${options.easing ?? easing.easeInOut} ${typeof options.delay === "string" ? options.delay : ms(options.delay ?? 0)}`).join(","),
			getAutoHeightDuration: (h) => {
				if (!h) return 0;
				const c = h / 36;
				return Math.min(Math.round((4 + 15 * c ** 0.25 + c / 5) * 10), 3000);
			},
		};
		const mixins = {
			toolbar: { minHeight: 56, "@media (min-width:0px)": { "@media (orientation: landscape)": { minHeight: 48 } }, "@media (min-width:600px)": { minHeight: 64 } },
			gutters: (styles = {}) => ({ paddingLeft: spacing(2), paddingRight: spacing(2), ...styles, [breakpoints.up("sm")]: { paddingLeft: spacing(3), paddingRight: spacing(3), ...styles[breakpoints.up("sm")] } }),
		};
		const theme = {
			breakpoints,
			direction: "ltr",
			components: {},
			palette,
			spacing,
			shape: { borderRadius: 4 },
			mixins,
			shadows,
			typography,
			transitions,
			zIndex: { mobileStepper: 1000, fab: 1050, speedDial: 1050, appBar: 1100, drawer: 1200, modal: 1300, snackbar: 1400, tooltip: 1500 },
			applyStyles: (mode, styles) => (mode === theme.palette.mode ? styles : {}),
			unstable_sxConfig: {},
			unstable_sx: () => ({}),
			vars: undefined,
		};
		// Luna's overrides, as the settings page sets them.
		const tab = (t) => ({
			minHeight: 0,
			transition: t.transitions.create("color", { duration: t.transitions.duration.short }),
			"&.Mui-selected": { color: t.palette.text.primary, "& .MuiTab-icon": { color: t.palette.secondary.main, transition: t.transitions.create("color", { duration: t.transitions.duration.short }), zIndex: 1 } },
			"&:hover:not(.Mui-selected)": { color: t.palette.text.primary },
		});
		theme.components = {
			MuiTabs: { styleOverrides: { indicator: ({ theme: t }) => ({ display: "flex", justifyContent: "center", backgroundColor: t.palette.primary.main }) } },
			MuiTab: { styleOverrides: { root: ({ theme: t }) => tab(t) } },
			MuiSwitch: {
				styleOverrides: {
					root: { padding: 7 },
					switchBase: { "& + .MuiSwitch-track": { opacity: 0.5 }, "&.Mui-checked": { "& + .MuiSwitch-track": { opacity: 1 } } },
					track: { opacity: 1, borderRadius: 12 },
				},
			},
			MuiSelect: { styleOverrides: { select: { backgroundColor: "rgba(0, 0, 0, 0.20)" } } },
			MuiMenu: { styleOverrides: { paper: { backgroundColor: grey[900] } } },
			MuiPaper: { styleOverrides: { root: { backgroundColor: grey[900] } } },
		};
		return theme;
	};
	const ui = {
		LunaSettings: mark("LunaSettings", "group"),
		LunaStack: mark("LunaStack", "group"),
		LunaSetting: mark("LunaSetting", "setting"),
		LunaSwitchSetting: mark("LunaSwitchSetting", "switch"),
		LunaSwitch: mark("LunaSwitch", "switch"),
		LunaNumberSetting: mark("LunaNumberSetting", "number"),
		LunaNumber: mark("LunaNumber", "number"),
		LunaTextSetting: mark("LunaTextSetting", "text"),
		LunaSecureTextSetting: mark("LunaSecureTextSetting", "secure"),
		LunaSecureText: mark("LunaSecureText", "secure"),
		LunaSelectSetting: mark("LunaSelectSetting", "select"),
		LunaSelectItem: mark("LunaSelectItem", "item"),
		LunaButtonSetting: mark("LunaButtonSetting", "button"),
		LunaButton: mark("LunaButton", "button"),
		SpinningButton: markWith("SpinningButton", "button", { defaultLabel: "Refresh" }),
		LunaTrashButton: markWith("LunaTrashButton", "button", { defaultLabel: "Delete" }),
		LunaLink: mark("LunaLink", "link"),
		LunaTitle: mark("LunaTitle", "title"),
		LunaAuthor: mark("LunaAuthor", "author"),
		LunaAuthorDisplay: mark("LunaAuthorDisplay", "author"),
		lunaMuiTheme: buildLunaTheme(),
		// The sx LunaSettings applies to its stack (plugins reuse it on
		// their own MUI boxes; the settings renderer ignores styling).
		settingsSx: { borderRadius: 2, backgroundColor: "rgba(0, 0, 0, 0.20)", boxShadow: 5, padding: 2, paddingBottom: 1 },
		unloads: new Set(),
		// material-ui-confirm's `confirm(options)`: a dialog over the app that
		// resolves { confirmed, reason } (Esc and the cancel button are "cancel").
		confirm: async (o = {}) => {
			const text = (n) => (n == null || typeof n === "boolean" ? "" : Array.isArray(n) ? n.map(text).join("") : typeof n === "object" ? text(n.props?.children) : String(n));
			const buttons = [text(o.confirmationText ?? "Ok")];
			if (!o.hideCancelButton) buttons.unshift(text(o.cancellationText ?? "Cancel"));
			const { response } = await L.askUser({ title: text(o.title), message: text(o.description), buttons, defaultId: buttons.length - 1, cancelId: 0 });
			const confirmed = response === buttons.length - 1;
			return { confirmed, reason: confirmed ? "confirm" : "cancel" };
		},
		// A page of a plugin, opened as a route of TIDAL's router: `open()` pushes
		// `/not-found?<name>` (as Luna's does) and the page's root element joins
		// `main` while the app shows that route; any other route takes it out.
		Page: class Page {
			static openPage = undefined;
			static pages = {};
			static hooked = false;
			static register(name, unloads, component) {
				Page.hook();
				return (Page.pages[name] ??= new Page(name, unloads, component));
			}
			// Once, for every plugin's pages: a route change shows or removes a page.
			// (Not a plugin's interceptor: it must outlive whichever plugin registered first.)
			static hook() {
				if (Page.hooked) return;
				Page.hooked = true;
				const onNavigated = (payload) => {
					const pageName = String(payload?.search ?? "").slice(1);
					if (pageName === Page.openPage?.name) return;
					Page.openPage?.removeFromDOM();
					if (Object.prototype.hasOwnProperty.call(Page.pages, pageName)) {
						const page = Page.pages[pageName];
						L.internalTimeout(() => page.addtoDOM(), 0);
					}
				};
				(L.interceptors["router/NAVIGATED"] ??= new Set()).add(onNavigated);
			}
			constructor(name, unloads, component) {
				this.name = name;
				this.unloads = unloads;
				this.component = component;
				this.rootId = `luna-page-${name}`;
				this.root = document.getElementById(this.rootId) ?? document.createElement("div");
				this.root.id = this.rootId;
				this.reactRoot = undefined;
				unloads?.add(() => {
					if (Page.openPage === this) this.removeFromDOM();
					this.root.remove();
					delete Page.pages[name];
				});
				// Styles for the area the page fills (Luna sets them on its container).
				this.pageStyles = new Proxy(
					{},
					{
						set: (t, k, v) => {
							t[k] = v;
							if (this.root.parentElement && typeof k === "string") this.root.parentElement.style[k] = v;
							return true;
						},
					},
				);
			}
			removeFromDOM() {
				const parent = this.root.parentElement;
				if (parent) for (const k of Object.keys(this.pageStyles)) parent.style[k] = "";
				this.root.remove();
				if (Page.openPage === this) Page.openPage = undefined;
			}
			addtoDOM() {
				const main = document.getElementById("main");
				if (!main) return;
				main.prepend(this.root);
				Object.assign(main.style, this.pageStyles);
				Page.openPage = this;
				this.render();
			}
			// Creates the React root on first use and draws the component.
			render() {
				if (this.component === undefined || this.reactRoot !== undefined) return;
				this.reactRoot = L.react.reactDomClient.createRoot(this.root);
				this.unloads?.add(() => this.reactRoot?.unmount());
				this.reactRoot.render(this.component);
			}
			open() {
				L.reduxStore.dispatch({ type: "router/PUSH", payload: { pathname: "/not-found", search: this.name, replace: true } });
			}
		},
	};
	L.ui = ui;

	// The MUI pieces plugins commonly build settings from, for plugins that
	// reach for `luna.core.modules["@mui/material"]`. Unknown names draw
	// their children, so a missing component never empties the panel.
	const known = {
		Stack: "block", Box: "block", Paper: "block", Card: "block", CardContent: "block", Container: "block", Grid: "block", Grid2: "block",
		FormControl: "block", FormGroup: "block", List: "block", ListItem: "block", ListItemText: "block", Alert: "block", AlertTitle: "block", Accordion: "block",
		AccordionDetails: "block", AccordionSummary: "block", Collapse: "pass", Fade: "pass", Tooltip: "pass", ThemeProvider: "pass", CssBaseline: "none",
		Typography: "typo", InputLabel: "typo", FormLabel: "typo", FormHelperText: "typo", Chip: "typo", ListItemButton: "button",
		Button: "button", IconButton: "button", Fab: "button", ToggleButton: "button", LoadingButton: "button",
		Switch: "switch", Checkbox: "switch", FormControlLabel: "formlabel", Slider: "slider",
		TextField: "text", OutlinedInput: "text", Input: "text", InputBase: "text", FilledInput: "text", Select: "select", NativeSelect: "select", MenuItem: "item",
		Divider: "none", ListSubheader: "none", Link: "link", CircularProgress: "none", LinearProgress: "none", Skeleton: "none", Avatar: "none", Icon: "none",
	};
	const mui = {};
	for (const [name, kind] of Object.entries(known)) mui[name] = mark(name, kind);
	const muiModule = new Proxy(mui, {
		get(target, key) {
			if (key in target || typeof key !== "string" || !/^[A-Z]/.test(key)) return target[key];
			return (target[key] = mark(key, "block"));
		},
	});
	L.mui = muiModule;
})();
