//! The parts of TidaLuna's own settings page the runtime carries: live
//! reload, the local dev store, CSS themes, `@luna/dev` logging, and the
//! settings transfer (Luna's `ExportData`, version 1).

use std::path::Path;

use serde_json::{Value, json};

use super::super::storage::{self, PLUGIN_STORAGE};
use super::super::{DevLog, InstallOutcome, ThemeInfo, TransferOutcome};
use super::{Engine, Fetched, Inner, LogLevel, S, fetch_store_ex, get_json, normalize_plugin_url};

/// Luna's general key/value store (`storeUrls`, `themes`).
pub(super) const LUNA_STORAGE: &str = "@luna/storage";
const DEV_PLUGIN: &str = "@luna/dev";
/// Luna's core plugins; an export lists them but they are built in here.
const CORE_PLUGINS: &[&str] = &[
    "@luna/lib",
    "@luna/lib.native",
    "@luna/ui",
    "@luna/dev",
    "@luna/linux",
];
const MAX_CSS: usize = 4 * 1024 * 1024;
const MAX_IMPORT: u64 = 256 * 1024 * 1024;

/// Read what earlier runs saved: themes and `@luna/dev`'s switches.
pub(super) fn load_saved(inner: &S) {
    let mut i = inner.borrow_mut();
    if let Some(Value::Object(m)) = i.storage.get(LUNA_STORAGE, "themes") {
        i.themes = m;
        if super::super::themes::migrate(&mut i.themes) {
            i.save_themes();
        }
    }
    i.dev_log = read_dev_log(&mut i);
}

fn read_dev_log(i: &mut Inner) -> DevLog {
    let v = i
        .storage
        .get(PLUGIN_STORAGE, DEV_PLUGIN)
        .unwrap_or(Value::Null);
    DevLog {
        redux: v["logReduxEvents"].as_bool().unwrap_or(false),
        redux_filter: v["logInterceptsRegex"].as_str().unwrap_or("").to_string(),
        ipc_native: v["logIPCFromNative"].as_bool().unwrap_or(false),
        ipc_render: v["logIPCFromRender"].as_bool().unwrap_or(false),
    }
}

/// `{name, description, author}` from the JSON inside a theme's first
/// `/* */` comment (Neptune's theme manifest), when there is one. The
/// author may be a name or `{ name, url, avatarUrl }`.
pub(crate) fn theme_manifest(
    css: &str,
) -> (Option<String>, Option<String>, crate::luna::author::Author) {
    let parsed = css.find("/*").and_then(|a| {
        let rest = &css[a + 2..];
        rest.find("*/")
            .and_then(|b| serde_json::from_str::<Value>(rest[..b].trim()).ok())
    });
    let Some(m) = parsed.filter(Value::is_object) else {
        return Default::default();
    };
    let s = |v: &Value| v.as_str().map(str::to_string).filter(|s| !s.is_empty());
    let mut author = crate::luna::author::parse(&m["author"]);
    author.name = author.name.filter(|n| !n.is_empty());
    (s(&m["name"]), s(&m["description"]), author)
}

/// Milliseconds since 1970, for when a theme was switched on.
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// `YYYY-MM-DDTHH:MM:SS.mmmZ`, as JavaScript's `toISOString`.
fn iso_now() -> String {
    let d = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = d.as_secs() as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        rem / 3600,
        rem / 60 % 60,
        rem % 60,
        d.subsec_millis()
    )
}

/// The file name Luna gives an export (`tidaluna-settings-<date>.json`).
pub(crate) fn export_file_name() -> String {
    let t = iso_now();
    format!(
        "tidaluna-settings-{}.json",
        t[..19].replace([':', '.'], "-")
    )
}

/// The base of a colourway at `url`, from its GitHub repository's file list.
fn find_base(c: &reqwest::blocking::Client, url: &str, css: &str) -> Option<(String, String)> {
    let os = super::super::themes::this_os();
    super::super::themes::discover_base(
        url,
        css,
        os,
        &mut |repo_ref| {
            let (repo, reference) = repo_ref.split_once('@')?;
            let r = c
                .get(format!(
                    "https://api.github.com/repos/{repo}/git/trees/{reference}?recursive=1"
                ))
                .send()
                .ok()?;
            let v: Value = r.error_for_status().ok()?.json().ok()?;
            Some(
                v["tree"]
                    .as_array()?
                    .iter()
                    .filter(|e| e["type"] == "blob")
                    .filter_map(|e| e["path"].as_str().map(str::to_string))
                    .collect(),
            )
        },
        &mut |u| fetch_text(c, u).ok(),
    )
}

fn fetch_text(c: &reqwest::blocking::Client, url: &str) -> Result<String, String> {
    let r = c
        .get(super::super::themes::fetch_url(url))
        .send()
        .map_err(|e| e.to_string())?;
    if !r.status().is_success() {
        return Err(format!("{url}: HTTP {}", r.status()));
    }
    super::text_capped(r, MAX_CSS, "the theme")
}

impl Inner {
    pub(super) fn outcome(&mut self, url: String, result: Result<String, String>) {
        self.outcome_seq += 1;
        let seq = self.outcome_seq;
        self.install_outcomes
            .push(InstallOutcome { seq, url, result });
        let n = self.install_outcomes.len();
        if n > 10 {
            self.install_outcomes.drain(..n - 10);
        }
        self.dirty = true;
    }

    pub(super) fn finish_transfer(&mut self, result: Result<String, String>) {
        if let Err(e) = &result {
            self.log(None, LogLevel::Error, e.clone());
        }
        self.transfer_seq += 1;
        self.transfer = Some(TransferOutcome {
            seq: self.transfer_seq,
            result,
        });
        self.dirty = true;
    }

    pub(super) fn theme_infos(&self) -> Vec<ThemeInfo> {
        self.themes
            .iter()
            .map(|(url, t)| {
                let (name, description, author) =
                    t["css"].as_str().map(theme_manifest).unwrap_or_default();
                let (loading, error) = self.theme_state.get(url).cloned().unwrap_or_default();
                let preview = super::super::themes::applied_css(t).map(|css| {
                    use std::hash::{Hash, Hasher};
                    let mut h = std::collections::hash_map::DefaultHasher::new();
                    css.hash(&mut h);
                    let key = h.finish();
                    let mut cache = self.theme_previews.borrow_mut();
                    match cache.get(url) {
                        Some((k, p)) if *k == key => p.clone(),
                        _ => {
                            let p =
                                std::sync::Arc::new(crate::ui::theme_preview::resolve_both(&css));
                            cache.insert(url.clone(), (key, p.clone()));
                            p
                        }
                    }
                });
                ThemeInfo {
                    url: url.clone(),
                    enabled: t["enabled"].as_bool().unwrap_or(false),
                    loading,
                    error,
                    name,
                    description,
                    author: author.name,
                    author_url: author.url,
                    author_avatar: author.avatar,
                    preview,
                }
            })
            .collect()
    }

    fn save_themes(&mut self) {
        let v = Value::Object(self.themes.clone());
        if let Err(e) = self.storage.set(LUNA_STORAGE, "themes", v) {
            self.log(None, LogLevel::Error, format!("Could not save themes: {e}"));
        }
        self.dirty = true;
    }
}

impl Engine {
    /// After the saved plugins load: themes and `@luna/dev` logging.
    pub(super) fn start_extras(&self) {
        if self.inner.borrow().dev_log != DevLog::default() {
            self.apply_dev_log();
        }
        self.apply_saved_themes();
    }

    fn apply_saved_themes(&self) {
        let list: Vec<(String, Option<String>)> = self
            .inner
            .borrow()
            .themes
            .iter()
            .filter(|(_, t)| t["enabled"].as_bool() == Some(true))
            .map(|(u, t)| (u.clone(), super::super::themes::applied_css(t)))
            .collect();
        for (url, css) in list {
            match css {
                Some(css) => self.apply_theme(&url, Some(css)),
                None => self.fetch_theme(&url),
            }
        }
    }

    // ------------------------------------------------------------ live reload

    pub(super) fn has_live(&self) -> bool {
        !self.inner.borrow().live.is_empty()
    }

    pub(super) fn set_live_reload(&self, name: &str, on: bool) {
        let mut i = self.inner.borrow_mut();
        let Some(dev) = i.record(name).map(|r| r.dev.is_some()) else {
            return;
        };
        match (dev, on) {
            (true, true) => i.live_off_dev.remove(name),
            (true, false) => i.live_off_dev.insert(name.to_string()),
            (false, true) => i.live.insert(name.to_string()),
            (false, false) => i.live.remove(name),
        };
        i.dirty = true;
    }

    /// Live reload: ask for each toggled plugin's `.json`.
    pub(super) fn poll_live(&self) {
        let list: Vec<(String, String)> = {
            let i = self.inner.borrow();
            i.records
                .iter()
                .filter(|r| {
                    r.enabled
                        && r.dev.is_none()
                        && i.live.contains(&r.name)
                        && !i.live_busy.contains(&r.name)
                })
                .map(|r| (r.name.clone(), r.url.clone()))
                .collect()
        };
        for (name, url) in list {
            self.inner.borrow_mut().live_busy.insert(name.clone());
            self.spawn_fetch(move |c| Fetched::LiveCheck {
                result: get_json(&c, &format!("{url}.json")),
                name,
                url,
            });
        }
    }

    pub(super) fn live_checked(&self, name: String, url: String, result: Result<Value, String>) {
        let changed = {
            let i = self.inner.borrow();
            let current = i.record(&name).and_then(|r| r.package["hash"].as_str());
            match &result {
                Ok(pkg) => {
                    i.live.contains(&name)
                        && pkg["hash"].as_str().is_some()
                        && pkg["hash"].as_str() != current
                }
                Err(_) => false,
            }
        };
        if changed {
            // Still busy until the new build is installed.
            self.install(&url, true);
        } else {
            self.inner.borrow_mut().live_busy.remove(&name);
        }
    }

    /// An install or update for `url` finished (either way).
    pub(super) fn release_live(&self, url: &str) {
        let mut i = self.inner.borrow_mut();
        if let Some(n) = i
            .records
            .iter()
            .find(|r| r.url == url)
            .map(|r| r.name.clone())
        {
            i.live_busy.remove(&n);
        }
    }

    // -------------------------------------------------------------- dev store

    pub(super) fn refresh_dev_store(&self) {
        let Some(url) = self.inner.borrow().cfg.dev_store.clone() else {
            return;
        };
        self.spawn_fetch(move |c| Fetched::DevStore(fetch_store_ex(&c, &url, true)));
    }

    // ----------------------------------------------------------------- themes

    fn apply_theme(&self, url: &str, css: Option<String>) {
        self.host_call("theme", json!({ "url": url, "css": css }));
    }

    pub(super) fn fetch_theme(&self, url: &str) {
        {
            let mut i = self.inner.borrow_mut();
            if !i.themes.contains_key(url) {
                return;
            }
            // A theme added over plain HTTP before HTTPS was required keeps
            // its saved CSS but is not fetched again.
            if !super::is_secure_source(url) {
                i.theme_state.insert(
                    url.to_string(),
                    (false, Some(super::insecure_source_error(url))),
                );
                i.dirty = true;
                return;
            }
            i.theme_state.insert(url.to_string(), (true, None));
            i.dirty = true;
        }
        let u = url.to_string();
        // A colourway's base, when it is known (the catalogue's, or one
        // found before), loads with it, so the two stay current together.
        // A theme not yet known to be a colourway may be one: its base is
        // looked for in its repository while it loads.
        let (known_base, has_base) = {
            let i = self.inner.borrow();
            let t = i.themes.get(url);
            (
                t.and_then(|t| t["base"].as_str())
                    .filter(|b| super::is_secure_source(b))
                    .map(str::to_string),
                t.is_some_and(|t| !t["baseCss"].is_null()),
            )
        };
        self.spawn_fetch(move |c| {
            let mut result = fetch_text(&c, &u);
            let base = match (&result, known_base) {
                (Err(_), _) => None,
                (Ok(_), Some(b)) => match fetch_text(&c, &b) {
                    Ok(css) => Some((b, css)),
                    // The saved copy stays; without one the colourway alone
                    // would show uncoloured, so the theme does not load.
                    Err(_) if has_base => None,
                    Err(e) => {
                        result = Err(format!("its base theme: {e}"));
                        None
                    }
                },
                (Ok(css), None) if !has_base => find_base(&c, &u, css),
                _ => None,
            };
            Fetched::Theme {
                result,
                url: u,
                base,
            }
        });
    }

    pub(super) fn install_theme(&self, url: &str) {
        if !(url.starts_with("https://") || url.starts_with("http://")) {
            self.inner
                .borrow_mut()
                .log(None, LogLevel::Error, format!("Not a theme URL: {url}"));
            return;
        }
        if !super::is_secure_source(url) {
            self.inner
                .borrow_mut()
                .log(None, LogLevel::Error, super::insecure_source_error(url));
            return;
        }
        // The same theme by another link (a GitHub page, jsDelivr, `main`
        // for `HEAD`) is the one installed: it is switched on instead.
        let existing =
            super::super::themes::find_installed(&self.inner.borrow().themes, url).cloned();
        if let Some(existing) = existing {
            self.set_theme_enabled(&existing, true);
            return;
        }
        let url = &super::super::themes::canonical_url(url);
        {
            let mut i = self.inner.borrow_mut();
            let mut entry = json!({ "enabled": true, "enabledAt": now_ms() });
            // A catalogue colourway names its base: it is fetched with it.
            if let Some(base) = crate::plugins::store_index::catalog_base(url) {
                entry["base"] = json!(base);
            }
            i.themes.insert(url.to_string(), entry);
            i.save_themes();
            i.log(None, LogLevel::Info, format!("Added theme {url}"));
        }
        // The new theme replaces the one applied.
        self.others_off(url);
        self.fetch_theme(url);
    }

    pub(super) fn theme_fetched(
        &self,
        url: String,
        result: Result<String, String>,
        found_base: Option<(String, String)>,
    ) {
        let apply = {
            let mut i = self.inner.borrow_mut();
            // A colourway applies over its base, and is one theme to the
            // user: the base installed already, else the one found in the
            // repository, kept inside this theme's own entry. A base named
            // already (the catalogue's, or found before) is taken as fetched.
            let (named, has_base) = i.themes.get(&url).map_or((false, false), |t| {
                (t["base"].is_string(), !t["baseCss"].is_null())
            });
            let base = match &result {
                Ok(_) if named => found_base,
                Ok(css) if !has_base => {
                    super::super::themes::installed_base(&i.themes, &url, css).or(found_base)
                }
                _ => None,
            };
            if let Some((base_url, _)) = base.as_ref().filter(|_| !has_base) {
                i.log(
                    None,
                    LogLevel::Info,
                    format!("Theme {url} is a colourway of {base_url}: applied over it"),
                );
            }
            let Some(t) = i.themes.get_mut(&url) else {
                i.theme_state.remove(&url);
                return;
            };
            match result {
                Ok(css) => {
                    t["css"] = json!(css);
                    if let Some((base_url, base_css)) = base {
                        t["base"] = json!(base_url);
                        t["baseCss"] = json!(base_css);
                    }
                    let applied = super::super::themes::applied_css(t);
                    let on = t["enabled"].as_bool() == Some(true);
                    i.theme_state.remove(&url);
                    // The same CSS installed by another URL: one theme (a
                    // colourway over another base is another theme).
                    let twin = i
                        .themes
                        .iter()
                        .find(|(u, t)| {
                            **u != url && super::super::themes::applied_css(t) == applied
                        })
                        .map(|(u, _)| u.clone());
                    if let Some(twin) = twin {
                        i.themes.remove(&url);
                        i.save_themes();
                        drop(i);
                        if on {
                            self.set_theme_enabled(&twin, true);
                        }
                        return;
                    }
                    i.save_themes();
                    on.then_some(applied).flatten()
                }
                Err(e) => {
                    i.log(
                        None,
                        LogLevel::Error,
                        format!("Failed to load theme {url}: {e}"),
                    );
                    i.theme_state.insert(url.clone(), (false, Some(e)));
                    None
                }
            }
        };
        if let Some(css) = apply {
            self.apply_theme(&url, Some(css));
        }
    }

    pub(super) fn set_theme_enabled(&self, url: &str, on: bool) {
        let css = {
            let mut i = self.inner.borrow_mut();
            let Some(t) = i.themes.get_mut(url) else {
                return;
            };
            t["enabled"] = json!(on);
            if on {
                t["enabledAt"] = json!(now_ms());
            }
            let css = super::super::themes::applied_css(t);
            i.save_themes();
            css
        };
        if on {
            self.others_off(url);
        }
        match (on, css) {
            (true, Some(css)) => self.apply_theme(url, Some(css)),
            (true, None) => self.fetch_theme(url),
            (false, _) => self.apply_theme(url, None),
        }
    }

    /// Every theme but `url` off: one theme applies at a time.
    fn others_off(&self, url: &str) {
        let others: Vec<String> = {
            let mut i = self.inner.borrow_mut();
            let others: Vec<String> = i
                .themes
                .iter()
                .filter(|(u, t)| *u != url && t["enabled"].as_bool() == Some(true))
                .map(|(u, _)| u.clone())
                .collect();
            for u in &others {
                if let Some(t) = i.themes.get_mut(u) {
                    t["enabled"] = json!(false);
                }
            }
            if !others.is_empty() {
                i.save_themes();
            }
            others
        };
        for u in others {
            self.apply_theme(&u, None);
        }
    }

    pub(super) fn uninstall_theme(&self, url: &str) {
        {
            let mut i = self.inner.borrow_mut();
            if i.themes.remove(url).is_none() {
                return;
            }
            i.theme_state.remove(url);
            i.save_themes();
        }
        self.apply_theme(url, None);
    }

    /// Luna's emergency shortcut: every enabled theme off.
    pub(super) fn disable_themes(&self) {
        let on: Vec<String> = self
            .inner
            .borrow()
            .themes
            .iter()
            .filter(|(_, t)| t["enabled"].as_bool() == Some(true))
            .map(|(u, _)| u.clone())
            .collect();
        if on.is_empty() {
            return;
        }
        for url in &on {
            self.set_theme_enabled(url, false);
        }
        self.inner.borrow_mut().log(
            None,
            LogLevel::Info,
            format!("[Themes] Emergency disable: disabled {} theme(s)", on.len()),
        );
    }

    // -------------------------------------------------------------- @luna/dev

    fn apply_dev_log(&self) {
        let d = self.inner.borrow().dev_log.clone();
        let filter = if d.redux_filter.is_empty() {
            ".*".to_string()
        } else {
            d.redux_filter.clone()
        };
        self.host_call(
            "dev_log",
            json!({
                "redux": d.redux.then_some(filter),
                "native": d.ipc_native,
                "render": d.ipc_render,
            }),
        );
    }

    pub(super) fn set_dev_log(&self, log: DevLog) {
        {
            let mut i = self.inner.borrow_mut();
            if i.dev_log == log {
                return;
            }
            let mut v = i
                .storage
                .get(PLUGIN_STORAGE, DEV_PLUGIN)
                .unwrap_or(json!({}));
            if !v.is_object() {
                v = json!({});
            }
            v["logReduxEvents"] = json!(log.redux);
            v["logInterceptsRegex"] = json!(log.redux_filter);
            v["logIPCFromNative"] = json!(log.ipc_native);
            v["logIPCFromRender"] = json!(log.ipc_render);
            if let Err(e) = i.storage.set(PLUGIN_STORAGE, DEV_PLUGIN, v) {
                i.log(
                    None,
                    LogLevel::Error,
                    format!("Could not save @luna/dev: {e}"),
                );
            }
            i.dev_log = log;
            i.dirty = true;
        }
        self.apply_dev_log();
    }

    // ------------------------------------------------------- settings transfer

    /// Luna's `SettingsTransfer.dump`: `@luna/pluginStorage`, `@luna/plugins`
    /// and `@luna/storage`, with no feature flags (Tidalfast has none).
    pub(super) fn export_data(&self, include_code: bool) -> Value {
        let mut i = self.inner.borrow_mut();
        let mut plugin_storage = serde_json::Map::new();
        for k in i.storage.keys(PLUGIN_STORAGE) {
            if let Some(v) = i.storage.get(PLUGIN_STORAGE, &k) {
                plugin_storage.insert(k, v);
            }
        }
        let mut plugins = serde_json::Map::new();
        for r in &i.records {
            let mut package = r.package.clone();
            if !package.is_object() {
                package = json!({ "name": r.name });
            }
            if include_code && let Ok(code) = std::fs::read_to_string(i.storage.code_path(&r.name))
            {
                package["code"] = json!(code);
            }
            plugins.insert(
                r.name.clone(),
                json!({
                    "url": r.url,
                    "package": package,
                    "enabled": r.enabled,
                    "installed": true,
                    "liveReload": false,
                    "hideSettings": r.hide_settings,
                }),
            );
        }
        let mut luna = serde_json::Map::new();
        for k in i.storage.keys(LUNA_STORAGE) {
            if let Some(v) = i.storage.get(LUNA_STORAGE, &k) {
                luna.insert(k, v);
            }
        }
        luna.insert("storeUrls".into(), json!(i.stores));
        luna.insert("themes".into(), Value::Object(i.themes.clone()));
        json!({
            "version": 1,
            "timestamp": iso_now(),
            "stores": {
                PLUGIN_STORAGE: plugin_storage,
                "@luna/plugins": plugins,
                LUNA_STORAGE: luna,
            },
            "featureFlags": null,
        })
    }

    pub(super) fn export_settings(
        &self,
        path: &Path,
        include_code: bool,
    ) -> Result<String, String> {
        let data = self.export_data(include_code);
        let bytes = serde_json::to_vec(&data).map_err(|e| e.to_string())?;
        storage::write_atomic(path, &bytes)
            .map_err(|e| format!("Failed to export settings: {e}"))?;
        Ok(format!("Settings exported to {}", path.display()))
    }

    /// Uninstall every plugin and forget every setting and theme.
    fn clear_everything(&self) {
        let names: Vec<String> = self
            .inner
            .borrow()
            .records
            .iter()
            .map(|r| r.name.clone())
            .collect();
        for n in names {
            self.remove_plugin(&n);
        }
        let themes: Vec<String> = self.inner.borrow().themes.keys().cloned().collect();
        for url in themes {
            self.apply_theme(&url, None);
        }
        let mut i = self.inner.borrow_mut();
        i.themes.clear();
        i.theme_state.clear();
        for store in [PLUGIN_STORAGE, LUNA_STORAGE] {
            for k in i.storage.keys(store) {
                let _ = i.storage.delete(store, &k);
            }
        }
        i.pending_import.clear();
        i.pending_hide.clear();
        i.dirty = true;
    }

    /// Luna's `SettingsTransfer.restore`: clear, then take everything from
    /// the export. Plugins whose code is in the file install from it.
    pub(super) fn import_settings(&self, path: &Path) -> Result<String, String> {
        let invalid = || "Invalid settings file format".to_string();
        let size = std::fs::metadata(path)
            .map_err(|e| format!("Failed to import settings: {e}"))?
            .len();
        if size > MAX_IMPORT {
            return Err(invalid());
        }
        let bytes = std::fs::read(path).map_err(|e| format!("Failed to import settings: {e}"))?;
        let data: Value = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
        if data["version"].as_u64() != Some(1) || !data["stores"].is_object() {
            return Err(invalid());
        }
        let stores = &data["stores"];
        self.clear_everything();
        {
            let mut i = self.inner.borrow_mut();
            if let Some(m) = stores[PLUGIN_STORAGE].as_object() {
                for (k, v) in m {
                    let _ = i.storage.set(PLUGIN_STORAGE, k, v.clone());
                }
            }
            if let Some(m) = stores[LUNA_STORAGE].as_object() {
                for (k, v) in m {
                    match k.as_str() {
                        "storeUrls" => {
                            let mut urls: Vec<String> = Vec::new();
                            for u in v.as_array().into_iter().flatten().filter_map(Value::as_str) {
                                let u = super::normalize_store_url(u);
                                if !urls.contains(&u) {
                                    urls.push(u);
                                }
                            }
                            if i.cfg.default_stores {
                                super::super::ensure_pinned_stores(&mut urls);
                            }
                            i.stores = urls;
                            i.store_info.clear();
                            i.save_stores();
                        }
                        "themes" => {
                            if let Some(t) = v.as_object() {
                                i.themes = t.clone();
                                super::super::themes::migrate(&mut i.themes);
                            }
                        }
                        _ => {
                            let _ = i.storage.set(LUNA_STORAGE, k, v.clone());
                        }
                    }
                }
            }
            i.save_themes();
            i.dev_log = read_dev_log(&mut i);
        }
        self.apply_dev_log();
        self.apply_saved_themes();
        self.refresh_stores();
        let mut count = 0;
        for (name, rec) in stores["@luna/plugins"].as_object().into_iter().flatten() {
            if CORE_PLUGINS.contains(&name.as_str()) || rec["installed"].as_bool() == Some(false) {
                continue;
            }
            let Some(url) = rec["url"].as_str().filter(|u| !u.is_empty()) else {
                continue;
            };
            let enabled = rec["enabled"].as_bool().unwrap_or(true);
            let hide = rec["hideSettings"].as_bool().unwrap_or(false);
            let local = !(url.starts_with("http://") || url.starts_with("https://"));
            let url = normalize_plugin_url(url);
            count += 1;
            let code = rec["package"]["code"]
                .as_str()
                .filter(|c| !c.trim().is_empty())
                .map(str::to_string);
            if let Some(code) = code {
                let mut pkg = rec["package"].clone();
                if let Some(o) = pkg.as_object_mut() {
                    o.remove("code");
                }
                if pkg["name"].as_str().is_none() {
                    pkg["name"] = json!(name);
                }
                {
                    let mut i = self.inner.borrow_mut();
                    i.pending_import.insert(url.clone(), (enabled, None));
                    i.pending_hide.insert(url.clone(), hide);
                }
                let dev = local.then(|| url.clone());
                self.installed_ex(url, false, dev, Ok((pkg, code)));
            } else if local {
                self.install_dev(&url);
            } else {
                {
                    let mut i = self.inner.borrow_mut();
                    i.pending_import.insert(url.clone(), (enabled, None));
                    i.pending_hide.insert(url.clone(), hide);
                }
                self.install(&url, false);
            }
        }
        let file = path
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_default();
        Ok(format!("Imported {count} plugins from {file}"))
    }

    /// Remove every plugin, plugin setting and theme; stores go back to
    /// TidaLuna's list.
    pub(super) fn reset_all(&self) {
        self.clear_everything();
        {
            let mut i = self.inner.borrow_mut();
            i.stores = if i.cfg.default_stores {
                let mut stores: Vec<String> = super::super::DEFAULT_STORES
                    .iter()
                    .map(|s| s.to_string())
                    .collect();
                super::super::ensure_pinned_stores(&mut stores);
                super::super::record_default_stores(&i.cfg.data_dir);
                stores
            } else {
                Vec::new()
            };
            i.store_info.clear();
            i.save_stores();
            i.save_themes();
            i.dev_log = DevLog::default();
            let _ = std::fs::remove_dir_all(i.cfg.data_dir.join("data"));
            i.log(
                None,
                LogLevel::Info,
                "Reset: every Luna plugin, setting and theme removed".into(),
            );
        }
        self.apply_dev_log();
    }
}
