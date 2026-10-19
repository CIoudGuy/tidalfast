// Test plugin in the shape esbuild gives real Luna plugins: externals come
// from luna.core.modules, .native code is registered through __ipcRenderer.
var core = luna?.core?.modules?.["@luna/core"];
var lib = luna?.core?.modules?.["@luna/lib"];
var ui = luna?.core?.modules?.["@luna/ui"];
var React = luna?.core?.modules?.react;
luna.core.LunaPlugin.getByName("@luna/lib")?.addDependant(luna.core.LunaPlugin.getByName("Fixture"));

const { trace, errSignal } = core.Tracer("[Fixture]");
export { errSignal, trace };
export const unloads = new Set();

const settings = await core.ReactiveStore.getPluginStorage("Fixture", { enabled: true, step: 5, name: "luna", mode: "a" });
const results = await core.ReactiveStore.getPluginStorage("Fixture.results", {});
results.version = __VERSION__;

const channel = await __ipcRenderer.invoke(
	"__Luna.registerNative",
	"Fixture/echo.native.ts",
	`"use strict";
var net = require("node:net"), crypto = require("crypto"), fs = require("fs");
var echo = (port, msg) => new Promise((res, rej) => {
	const s = net.createConnection({ host: "127.0.0.1", port });
	s.once("error", rej);
	s.once("connect", () => s.write(Buffer.from(msg)));
	s.on("data", (d) => { res(d.toString()); s.end(); });
});
var hashIt = (s) => crypto.createHash("md5").update(s).digest("hex");
var writeRead = (s) => {
	fs.writeFileSync("note.txt", s);
	let escaped = "none";
	try { fs.readFileSync("../../outside.txt"); escaped = "escaped"; } catch (e) { escaped = e.code; }
	return fs.readFileSync("note.txt", "utf8") + ":" + escaped;
};
module.exports = { echo, hashIt, writeRead };
`,
);
const echo = (...a) => __ipcRenderer.invoke(channel, "echo", ...a);
const hashIt = (...a) => __ipcRenderer.invoke(channel, "hashIt", ...a);
const writeRead = (...a) => __ipcRenderer.invoke(channel, "writeRead", ...a);

lib.redux.intercept("playbackControls/SET_PLAYBACK_STATE", unloads, (state) => {
	results.lastState = state;
	if (state === "PLAYING") lib.redux.actions["playbackControls/SET_VOLUME"]({ volume: settings.step * 2 });
});
lib.redux.intercept("playbackControls/SEEK", unloads, (t) => {
	results.seek = t;
});
lib.MediaItem.onMediaTransition(unloads, async (item) => {
	results.title = await item.title();
	results.artist = (await item.artist())?.name;
	results.cover = await item.coverUrl({ res: "320" });
	results.playing = lib.PlayState.playing;
});
lib.ContextMenu.onMediaItem(unloads, async ({ mediaCollection, contextMenu }) => {
	const n = await mediaCollection.count();
	const button = (globalThis.__fixtureButton ??= lib.ContextMenu.addButton(unloads));
	button.text = `Fixture: ${n} track(s)`;
	button.onClick(() => (results.menuClicked = n));
	await button.show(contextMenu);
});
unloads.add(() => {
	results.unloaded = (results.unloaded ?? 0) + 1;
	globalThis.__fixtureButton = undefined;
});

const port = core.findModuleProperty((k) => k === "echoPort")?.value;
if (port) echo(port, "ping").then((r) => (results.echo = r)).catch((e) => (results.echoErr = String(e)));
hashIt("abc").then((h) => (results.md5 = h));
writeRead("hello").then((r) => (results.file = r));
document.querySelector("div[class^='_bar']");

export const Settings = () => {
	const [enabled, setEnabled] = React.useState(settings.enabled);
	const [step, setStep] = React.useState(settings.step);
	const [name, setName] = React.useState(settings.name);
	const [mode, setMode] = React.useState(settings.mode);
	React.useEffect(() => {
		errSignal._ = name === "" ? "Name not set." : undefined;
	}, [name]);
	return React.createElement(
		ui.LunaSettings,
		null,
		React.createElement(ui.LunaSwitchSetting, { title: "Enabled", desc: "Turn it on", checked: enabled, onChange: (_, c) => setEnabled((settings.enabled = c)) }),
		React.createElement(ui.LunaNumberSetting, { title: "Step", desc: React.createElement(React.Fragment, null, "See ", React.createElement(ui.LunaLink, { href: "https://example.com" }, "docs")), value: step, min: 0, max: 100, onNumber: (n) => setStep((settings.step = n)) }),
		React.createElement(ui.LunaTextSetting, { title: "Name", value: name, onChange: (e) => setName((settings.name = e.target.value)) }),
		React.createElement(
			ui.LunaSelectSetting,
			{ title: "Mode", value: mode, onChange: (e) => setMode((settings.mode = e.target.value)) },
			React.createElement(ui.LunaSelectItem, { value: "a" }, "Mode A"),
			React.createElement(ui.LunaSelectItem, { value: "b" }, "Mode B"),
		),
		React.createElement(ui.LunaButtonSetting, { title: "Reset", onClick: () => (results.clicked = true) }, "Reset now"),
	);
};
