//! Every version the Luna runtime follows or reports to plugins, in one place.
//!
//! Upgrading to a new TidaLuna: run `python scripts/luna-api-diff.py` (it
//! compares upstream's exports with `src/luna/api-surface.json`), close the
//! gaps it lists in the shim, then raise [`LUNA_API`] here. The JS shim reads
//! these values from `__lunaVersions` (set before `prelude.js` runs); nothing
//! else in `src/luna/js/` spells a version out.

use serde_json::json;

/// The TidaLuna release whose plugin API the shim follows (upstream's
/// `package.json` version). Shown on the Settings tab and given to plugins as
/// `@luna/lib.native`'s `pkg.version`.
pub const LUNA_API: &str = "1.18.0-beta";

/// What `process.versions` reports: the Node and Electron TidaLuna 1.17 ran
/// on inside TIDAL 2.43. Plugins check these for feature support.
pub const NODE: &str = "20.15.0";
pub const V8: &str = "12.4";
pub const UV: &str = "1.48.0";
pub const NODE_MODULES: &str = "115";
pub const ELECTRON: &str = "31.0.0";
pub const CHROME: &str = "126.0.0";

/// `electron.app.getVersion()`: TIDAL's own app version. Tidalfast is not
/// TIDAL, so it reports none.
pub const TIDAL_APP: &str = "0.0.0";

/// The object the shim reads as `globalThis.__lunaVersions`.
pub(crate) fn js_object() -> String {
    json!({
        "luna": LUNA_API,
        "node": NODE,
        "v8": V8,
        "uv": UV,
        "modules": NODE_MODULES,
        "electron": ELECTRON,
        "chrome": CHROME,
        "tidal": TIDAL_APP,
    })
    .to_string()
}
