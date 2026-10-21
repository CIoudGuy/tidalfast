//! Settings → Plugins: TidaLuna's own settings page, drawn natively. The
//! tabs, cards and wording follow Luna's `@luna/ui` SettingsPage
//! (Plugins, Plugin Store, Themes, Settings); Supporters is left out (it
//! lists TidaLuna's GitHub sponsors). The on/off switch lives in Settings;
//! this page is reached only while the runtime is enabled.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::{Arc, Mutex};

use egui::{
    Align, Color32, CornerRadius, Frame, Layout, Margin, RichText, Sense, Stroke, Ui, Vec2,
};

use crate::app::App;
use crate::i18n::{Locale, gettext};
use crate::luna::import::{self, ImportScan};
use crate::luna::{
    DevLog, LogLevel, Luna, LunaSnapshot, PluginInfo, PluginStatus, SETTINGS_RETRY, SettingValue,
    SettingsMeta, SettingsNode, StoreInfo, StorePlugin, ThemeInfo,
};
use crate::model::{Action, Page};
use crate::theme::{self, Icon, Palette};

use super::{scroll_anchor, skeleton, widgets};

#[cfg(any(test, feature = "demo"))]
#[path = "plugins_fixture.rs"]
mod fixture;
#[path = "plugins_store.rs"]
mod store;
use crate::plugins::store_index::{self, Index};
use store::{Sort, StoreView};

/// Luna's tabs, in Luna's order.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
enum Tab {
    #[default]
    Plugins,
    Store,
    Themes,
    Settings,
}

#[derive(Default)]
enum ImportState {
    #[default]
    Idle,
    Scanning(Receiver<Result<ImportScan, String>>),
    Ready {
        scan: ImportScan,
        picked: Vec<bool>,
        add_stores: bool,
    },
    Failed(String),
    Done(String),
}

/// Luna's "Install from URL" field: it acts on what is typed after half a
/// second without typing; its label shows the error or success.
#[derive(Default)]
struct InstallField {
    value: String,
    /// When the value last changed (egui time), until it is acted on.
    changed_at: Option<f64>,
    /// A plugin install waiting for its outcome: (url, outcomes seen).
    pending: Option<(String, u64)>,
    success: Option<(String, f64)>,
    error: Option<String>,
    /// A theme just added (or found installed) from the field: its identity
    /// ([`crate::luna::themes::identity`]), URL and when. Its row shows at
    /// once and grows in (or, already there, lights up).
    theme_added: Option<(String, String, f64)>,
    /// The theme just switched on (by identity), or `None` for all off, and
    /// when: shown so until the runtime agrees, so the switches never flicker.
    theme_on: Option<(Option<String>, f64)>,
}

#[derive(Clone, PartialEq, Debug)]
enum Confirm {
    Import(PathBuf),
    Reset,
    /// Put this plugin back to its defaults (by name).
    ResetPlugin(String),
}

/// The page's own state, kept in egui's memory (nothing to persist).
#[derive(Default)]
struct PageState {
    tab: Tab,
    search: String,
    install: InstallField,
    import: ImportState,
    looked: bool,
    /// Where each tab was scrolled to when the page left it, by [`Tab`].
    scrolls: [f32; 4],
    include_code: bool,
    import_path: String,
    confirm: Option<Confirm>,
    transfer_seen: u64,
    transfer: Option<Result<String, String>>,
    filter_draft: Option<String>,
    /// The developer log also lists debug lines.
    show_debug: bool,
    plugin_sort: Sort,
    theme_sort: Sort,
    /// What the Plugin Store tab lists, kept until its inputs change.
    view: Option<StoreView>,
    /// The snapshot last copied from the runtime, and its version.
    cached: Option<(u64, Arc<LunaSnapshot>)>,
}

type Shared = Arc<Mutex<PageState>>;

/// What the user did on the page, applied after drawing.
#[derive(Clone, Debug, PartialEq)]
enum Do {
    Install(String),
    InstallDev(String),
    Uninstall(String),
    SetEnabled(String, bool),
    Reload(String),
    /// Clear what the plugin stored and its permissions, and run it again.
    ResetPlugin(String),
    SetLiveReload(String, bool),
    SetHideSettings(String, bool),
    AddStore(String),
    RemoveStore(String),
    InstallTheme(String),
    /// Apply a theme: turn the listed others off, then enable it (or install
    /// it, when it is not installed yet).
    ApplyTheme(String, Vec<String>, bool),
    /// Update every listed installed plugin.
    UpdateAll(Vec<String>),
    SetThemeEnabled(String, bool),
    ReloadTheme(String),
    UninstallTheme(String),
    Setting(String, String, SettingValue),
    OpenUrl(String),
    Toast(String),
    SetDevLog(DevLog),
    Export(bool),
    Import(PathBuf),
    ResetAll,
    ImportPlugin(String, bool, Option<serde_json::Value>),
    /// Take back "Always allow" for a program: plugin name, program path.
    RevokeProgram(String, String),
    RevokeAccountWrites(String),
    /// Take back a permission given with Always (the TIDAL sign-in,
    /// system-wide shortcuts, a program's pipe, saving TIDAL audio).
    RevokePermission(String, crate::luna::Access),
}

const SUCCESS: Color32 = Color32::from_rgb(0x4c, 0xaf, 0x50);

/// The page's column: a little wider than Settings' 816, for the card grids.
const COLUMN_WIDTH: f32 = 960.0;

pub fn show(app: &mut App, ui: &mut Ui) {
    let full = ui.available_width();
    let width = full.min(COLUMN_WIDTH);
    let top_left = egui::pos2(ui.cursor().left() + (full - width) / 2.0, ui.cursor().top());
    ui.scope_builder(
        egui::UiBuilder::new().max_rect(egui::Rect::from_min_size(
            top_left,
            egui::vec2(width, f32::INFINITY),
        )),
        |ui| {
            ui.set_width(width);
            page(app, ui);
        },
    );
}

fn page(app: &mut App, ui: &mut Ui) {
    let palette = app.palette;
    let locale = app.locale;
    let t = |s: &'static str| gettext(locale, s).into_owned();
    // The artwork loader the cards' profile pictures come through; the cards
    // are drawn only while this page is open, so nothing is asked for
    // otherwise.
    ui.ctx().data_mut(|d| {
        d.insert_temp(store::art_id(), app.backend.art().clone());
    });
    ui.add_space(28.0);
    ui.horizontal(|ui| {
        if theme::icon_button(
            ui,
            Icon::ChevronLeft,
            20.0,
            palette.secondary,
            palette.text,
            &t("Settings"),
        )
        .clicked()
        {
            app.actions.push(Action::Open(Page::Settings));
        }
        // The page's title: plugins may rewrite it and add to it.
        let title = t("Plugins");
        let shown = super::plugin_dom::text(crate::luna::dom_anchors::keys::PAGE_TITLE, &title);
        let color = super::plugin_dom::color(crate::luna::dom_anchors::keys::PAGE_TITLE)
            .unwrap_or(palette.text);
        let response = theme::text(ui, shown.as_ref(), theme::bold(28.0), color);
        super::plugin_dom::page_title(ui, &title, response.rect, true);
    });
    ui.add_space(24.0);
    if !app.settings.plugins.luna_enabled && app.plugins.luna.is_none() {
        note(
            ui,
            &t("TidaLuna plugins are off. Turn them on in Settings."),
            palette.secondary,
        );
        return;
    }
    if let Some(error) = &app.plugins.luna_error {
        note(
            ui,
            &format!("{} {error}", t("The plugin runtime could not start:")),
            palette.danger,
        );
    }
    let Some(runtime) = app.plugins.luna.clone() else {
        return;
    };
    let shared: Shared = ui.ctx().data_mut(|d| {
        d.get_temp_mut_or_insert_with::<Shared>(
            egui::Id::new("luna-plugins-page"),
            Default::default,
        )
        .clone()
    });
    let Ok(mut page) = shared.lock() else {
        return;
    };
    // The runtime's snapshot is copied only when it changed.
    let version = runtime.snapshot_version();
    let snapshot = match &page.cached {
        Some((v, s)) if *v == version => s.clone(),
        _ => {
            let s = Arc::new(runtime.snapshot());
            page.cached = Some((version, s.clone()));
            s
        }
    };
    let dir = app.dirs.luna_dir();
    let now = store_index::now_secs();
    let index = store_index::snapshot(&dir);
    #[cfg(feature = "demo")]
    let (snapshot, index) = match fixture::demo(now) {
        Some((s, i)) => (Arc::new(s), Arc::new(i)),
        None => (snapshot, index),
    };
    if let Some(error) = &snapshot.runtime_error {
        note(ui, error, palette.danger);
    }
    if !page.looked {
        page.looked = true;
        // Demo shots: `TIDALFAST_DEMO_LUNA_TAB=store|themes|settings`.
        #[cfg(feature = "demo")]
        if let Ok(tab) = std::env::var("TIDALFAST_DEMO_LUNA_TAB") {
            page.tab = match tab.as_str() {
                "store" => Tab::Store,
                "themes" => Tab::Themes,
                "settings" => Tab::Settings,
                _ => Tab::Plugins,
            };
        }
        // Demo shots of adding a theme: `TIDALFAST_DEMO_LUNA_ADD=<url>@<s>`
        // types the URL into the field so it is acted on `s` seconds into
        // the run.
        #[cfg(feature = "demo")]
        if let Ok(add) = std::env::var("TIDALFAST_DEMO_LUNA_ADD")
            && let Some((url, at)) = add.rsplit_once('@')
            && let Ok(at) = at.parse::<f64>()
        {
            page.install.value = url.to_string();
            page.install.changed_at = Some(at - 0.5);
        }
        runtime.look_for_updates();
        if should_load_stores(&snapshot) {
            runtime.refresh_stores();
        }
    }
    // Dates and the theme catalogue: once the stores have listed their
    // plugins, in the background, at most every few hours.
    if store_index::due(now) && stores_listed(&snapshot) {
        let ctx = ui.ctx().clone();
        let stores = snapshot
            .stores
            .iter()
            .filter(|s| !s.plugins.is_empty())
            .map(|s| store_index::ListedStore {
                url: s.url.clone(),
                plugins: s
                    .plugins
                    .iter()
                    .map(|p| store_index::Listed {
                        url: p.url.clone(),
                        file: p.file.clone(),
                        hash: p.hash.clone(),
                    })
                    .collect(),
            })
            .collect();
        let wake = ctx.clone();
        // The index this frame drew was taken before the refresh began, so
        // one more pass shows it loading; every change after that wakes the
        // window itself.
        if store_index::refresh(&dir, stores, now, move || wake.request_repaint()) {
            ctx.request_repaint();
        }
    }
    let out = draw(ui, &palette, locale, &snapshot, &index, now, &mut page);
    drop(page);
    apply(app, &runtime, out);
}

/// The whole page below the title; returns what the user did.
fn draw(
    ui: &mut Ui,
    palette: &Palette,
    locale: Locale,
    snapshot: &LunaSnapshot,
    index: &Arc<Index>,
    now: i64,
    page: &mut PageState,
) -> Vec<Do> {
    let mut out = Vec::new();
    scroll_anchor::begin(ui);
    store::paging_tabs(ui, palette, locale, page);
    ui.add_space(32.0);
    match page.tab {
        Tab::Plugins => plugins_tab(
            ui,
            palette,
            locale,
            snapshot,
            index,
            now,
            &mut page.confirm,
            &mut out,
        ),
        Tab::Store => store::store_tab(ui, palette, locale, snapshot, index, now, page, &mut out),
        Tab::Themes => store::themes_tab(ui, palette, locale, snapshot, index, now, page, &mut out),
        Tab::Settings => settings_tab(ui, palette, locale, snapshot, page, &mut out),
    }
    // What is looked at stays put when the content above it grows or shrinks.
    scroll_anchor::settle(ui);
    out
}

fn apply(app: &mut App, runtime: &Luna, out: Vec<Do>) {
    for d in out {
        // A plugin that goes away takes the titles it wrote with it.
        if matches!(
            d,
            Do::Uninstall(_) | Do::SetEnabled(_, false) | Do::Reload(_) | Do::ResetPlugin(_)
        ) {
            app.plugins.retitles.clear();
        }
        match d {
            Do::Install(url) => runtime.install(&url),
            Do::InstallDev(source) => runtime.install_dev(&source),
            Do::Uninstall(name) => runtime.uninstall(&name),
            Do::SetEnabled(name, on) => runtime.set_enabled(&name, on),
            Do::Reload(name) => runtime.reload(&name),
            Do::ResetPlugin(name) => runtime.reset_plugin(&name),
            Do::SetLiveReload(name, on) => runtime.set_live_reload(&name, on),
            Do::SetHideSettings(name, hide) => runtime.set_hide_settings(&name, hide),
            Do::RevokeProgram(plugin, program) => runtime.revoke_program(&plugin, &program),
            Do::RevokeAccountWrites(plugin) => runtime.revoke_account_writes(&plugin),
            Do::RevokePermission(plugin, access) => runtime.revoke_permission(&plugin, &access),
            Do::AddStore(url) => runtime.add_store(&url),
            Do::RemoveStore(url) => runtime.remove_store(&url),
            Do::InstallTheme(url) => runtime.install_theme(&url),
            Do::ApplyTheme(url, others, installed) => {
                for other in others {
                    runtime.set_theme_enabled(&other, false);
                }
                // Installing enables; an installed one is switched on.
                if installed {
                    runtime.set_theme_enabled(&url, true);
                } else {
                    runtime.install_theme(&url);
                }
            }
            Do::UpdateAll(urls) => {
                for url in urls {
                    runtime.install(&url);
                }
            }
            Do::SetThemeEnabled(url, on) => runtime.set_theme_enabled(&url, on),
            Do::ReloadTheme(url) => runtime.reload_theme(&url),
            Do::UninstallTheme(url) => runtime.uninstall_theme(&url),
            Do::Setting(plugin, id, value) => runtime.settings_input(&plugin, &id, value),
            Do::OpenUrl(url) => {
                if url.starts_with("https://") || url.starts_with("http://") {
                    app.actions.push(Action::OpenUrl(url));
                }
            }
            Do::Toast(text) => app.toast(text),
            Do::SetDevLog(log) => runtime.set_dev_log(log),
            Do::Export(include_code) => runtime.export_settings(
                export_dir().join(crate::luna::export_file_name()),
                include_code,
            ),
            Do::Import(path) => runtime.import_settings(path),
            Do::ResetAll => runtime.reset_all(),
            Do::ImportPlugin(url, enabled, storage) => {
                runtime.import_plugin(&url, enabled, storage)
            }
        }
    }
}

/// Where exports go: the user's Downloads folder (no file dialog).
fn export_dir() -> PathBuf {
    directories::UserDirs::new()
        .and_then(|u| {
            u.download_dir()
                .map(Path::to_path_buf)
                .or_else(|| Some(u.home_dir().to_path_buf()))
        })
        .unwrap_or_else(std::env::temp_dir)
}

fn note(ui: &mut Ui, text: &str, color: Color32) {
    ui.horizontal_wrapped(|ui| {
        ui.add_space(16.0);
        ui.label(RichText::new(text).font(theme::regular(12.5)).color(color));
    });
}

/// The guide for plugin developers (the project's documentation page).
const MAKING_PLUGINS_URL: &str =
    "https://github.com/CIoudGuy/tidalfast/blob/main/docs/_guide/making-plugins.md";

fn caption(ui: &mut Ui, text: &str, color: Color32) {
    ui.label(RichText::new(text).font(theme::regular(12.5)).color(color));
}

/// Whether the stores have listed their plugins, so the dates and download
/// counts can be matched to them. On the frame the page opens the stores have
/// not started loading yet: a refresh then would find no plugins, and the
/// next one waits half an hour.
fn stores_listed(snapshot: &LunaSnapshot) -> bool {
    !snapshot.stores.iter().any(|s| s.loading)
        && snapshot.stores.iter().any(|s| !s.plugins.is_empty())
}

/// Whether opening the page must fetch the stores: nothing cached yet (or a
/// store that failed) and no fetch already running. Cached lists show at once
/// and are not refetched.
fn should_load_stores(snapshot: &LunaSnapshot) -> bool {
    !snapshot.stores.iter().any(|s| s.loading)
        && (snapshot.stores.is_empty()
            || snapshot
                .stores
                .iter()
                .all(|s| s.plugins.is_empty() || s.error.is_some()))
}

fn norm_store(url: &str) -> String {
    let url = url.trim();
    url.strip_suffix("/store.json")
        .unwrap_or(url)
        .trim_end_matches('/')
        .to_string()
}

/// Plugin descriptions may hold simple HTML.
fn strip_html(text: &str) -> String {
    let mut out = String::new();
    let mut tag = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            // A tag opens with a name, `/` or `!`; a lone `<` in plain
            // text ("lossy < lossless") is text, as in a browser.
            '<' if !tag
                && chars
                    .peek()
                    .is_some_and(|n| n.is_ascii_alphabetic() || matches!(n, '/' | '!')) =>
            {
                tag = true
            }
            '>' if tag => tag = false,
            c if !tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
}

// ------------------------------------------------------------ shared pieces

/// Luna's card: `rgba(0,0,0,0.1)`, radius 3, red glow on a load error.
fn card(ui: &mut Ui, palette: &Palette, error: bool, add: impl FnOnce(&mut Ui)) {
    let mut frame = Frame::new()
        .fill(palette.surface)
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin {
            left: 16,
            right: 16,
            top: 8,
            bottom: 12,
        });
    if error {
        frame = frame
            .stroke(Stroke::new(1.0, palette.danger.gamma_multiply(0.8)))
            .shadow(egui::Shadow {
                offset: [0, 0],
                blur: 10,
                spread: 0,
                color: palette.danger.gamma_multiply(0.7),
            });
    }
    frame.show(ui, |ui| {
        ui.set_width(ui.available_width());
        add(ui);
    });
}

/// Luna's red error pill.
fn error_pill(ui: &mut Ui, text: &str) {
    let short: String = text
        .lines()
        .next()
        .unwrap_or("")
        .chars()
        .take(160)
        .collect();
    let r = Frame::new()
        .fill(Color32::from_rgba_unmultiplied(255, 0, 0, 128))
        .corner_radius(CornerRadius::same(4))
        .inner_margin(Margin::symmetric(8, 4))
        .show(ui, |ui| {
            ui.label(
                RichText::new(short)
                    .font(theme::medium(12.0))
                    .color(Color32::WHITE),
            );
        })
        .response;
    if text.len() > 160 || text.contains('\n') {
        r.on_hover_text(text);
    }
}

/// The side of the author's picture beside their name on a card header.
const HEADER_AVATAR: f32 = 18.0;

struct Header<'a> {
    locale: Locale,
    name: &'a str,
    version: Option<String>,
    link: Option<&'a str>,
    error: Option<&'a str>,
    author: Option<&'a str>,
    /// Where the author's name leads, and their picture.
    author_ref: AuthorRef,
    desc: Option<String>,
    /// A limit of this store plugin in Tidalfast (the pill and its reason).
    compat: Option<crate::luna::CompatNote>,
}

/// The `<owner>/<repo>` a plugin's or theme's release or file comes from on
/// GitHub (`github.com/<owner>/<repo>/...` or
/// `raw.githubusercontent.com/<owner>/<repo>/...`). None for a development
/// server or another host.
fn github_repo(url: &str) -> Option<(&str, &str)> {
    let rest = url
        .strip_prefix("https://github.com/")
        .or_else(|| url.strip_prefix("https://raw.githubusercontent.com/"))?;
    let mut parts = rest.split('/');
    let owner = parts.next().filter(|o| !o.is_empty())?;
    let repo = parts.next().filter(|r| !r.is_empty())?;
    Some((owner, repo))
}

/// Where an author's name leads and which picture stands for them.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct AuthorRef {
    pub(super) link: Option<String>,
    pub(super) avatar: Option<String>,
}

/// Where an author's name leads and their picture. `name`, `url` and
/// `avatar` are what the package, manifest or catalogue says; `source` is
/// the release, file or store URL it came from.
///
/// The link is the one the author gave. Without one, the profile of the
/// GitHub account `source` is under, but only when the author's name is that
/// account's: a store belongs to its owner, the plugins and themes in it do
/// not necessarily (Nick Oates's theme in Inrixia's store is not Inrixia's).
/// Otherwise the name is plain text. The picture is the one the author gave,
/// else the GitHub account's they are known to be.
pub(super) fn author_ref(
    name: Option<&str>,
    url: Option<&str>,
    avatar: Option<&str>,
    source: &str,
) -> AuthorRef {
    use crate::luna::author;
    let Some(name) = name else {
        return AuthorRef::default();
    };
    let owner = github_repo(source)
        .map(|(owner, _)| owner)
        .filter(|owner| author::is_login(name, owner));
    let link = url
        .map(str::to_string)
        .or_else(|| owner.map(|owner| format!("https://github.com/{owner}")));
    let login = url
        .and_then(author::github_login)
        .or_else(|| owner.filter(|o| author::valid_login(o)).map(str::to_string));
    let avatar = avatar
        .map(str::to_string)
        .or_else(|| login.as_deref().map(author::github_avatar));
    AuthorRef { link, avatar }
}

/// The GitHub page of the repository a plugin or theme is published from,
/// where it can be starred. None for a development server or another host.
pub(super) fn repo_link(url: &str) -> Option<String> {
    github_repo(url).map(|(owner, repo)| format!("https://github.com/{owner}/{repo}"))
}

/// Luna's `LunaPluginHeader`: name (a link when there is one) + version,
/// the controls, an error pill, the author on the right, the description
/// below.
fn header(
    ui: &mut Ui,
    palette: &Palette,
    h: &Header<'_>,
    out: &mut Vec<Do>,
    controls: impl FnOnce(&mut Ui, &mut Vec<Do>),
) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        match h.link.filter(|l| l.starts_with("http")) {
            Some(link) => {
                if theme::link(ui, h.name, theme::semibold(16.0), palette.text).clicked() {
                    out.push(Do::OpenUrl(link.to_string()));
                }
            }
            None => {
                theme::text(ui, h.name, theme::semibold(16.0), palette.text);
            }
        }
        if let Some(v) = h.version.as_deref().filter(|v| !v.trim().is_empty()) {
            ui.label(
                RichText::new(v.trim())
                    .font(theme::regular(12.0))
                    .color(palette.secondary),
            );
        }
        if let Some(note) = h.compat {
            let (rect, _) = ui.allocate_exact_size(
                Vec2::new(store::compat_pill_width(ui, h.locale, note), 17.0),
                Sense::hover(),
            );
            store::compat_pill(
                ui,
                palette,
                h.locale,
                rect.left_top(),
                note,
                egui::Id::new(("luna-header-compat", h.name)),
            );
        }
        controls(ui, out);
        if let Some(e) = h.error {
            error_pill(ui, e);
        }
        if let Some(a) = h.author {
            let picture = h.author_ref.avatar.as_deref();
            let w = ui
                .painter()
                .layout_no_wrap(a.to_string(), theme::regular(12.5), palette.secondary)
                .size()
                .x
                + picture.map_or(0.0, |_| HEADER_AVATAR + ui.spacing().item_spacing.x);
            // The author leads to their GitHub profile, as in TidaLuna.
            let author = |ui: &mut Ui, out: &mut Vec<Do>, reversed: bool| {
                let picture = |ui: &mut Ui| {
                    if let Some(url) = picture {
                        let (rect, _) =
                            ui.allocate_exact_size(Vec2::splat(HEADER_AVATAR), Sense::hover());
                        store::avatar_picture(ui, palette, rect, a, Some(url));
                    }
                };
                if !reversed {
                    picture(ui);
                }
                match &h.author_ref.link {
                    Some(link) => {
                        if theme::link(ui, a, theme::regular(12.5), palette.secondary)
                            .on_hover_text(link)
                            .clicked()
                        {
                            out.push(Do::OpenUrl(link.clone()));
                        }
                    }
                    None => caption(ui, a, palette.secondary),
                }
                if reversed {
                    picture(ui);
                }
            };
            if ui.max_rect().right() - ui.cursor().min.x > w + 12.0 {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    author(ui, out, true)
                });
            } else {
                // Too narrow beside the name: it goes on the next line.
                ui.end_row();
                author(ui, out, false);
            }
        }
    });
    if let Some(d) = h.desc.as_deref().filter(|d| !d.is_empty()) {
        ui.label(
            RichText::new(d)
                .font(theme::medium(13.0))
                .color(palette.text),
        );
    }
    // The reason, in the card's details.
    if let Some(note) = h.compat {
        caption(ui, &note.reason_in(h.locale), palette.secondary);
    }
}

/// An icon control with Luna's tooltip as its label; `None` when disabled.
fn icon_btn(
    ui: &mut Ui,
    palette: &Palette,
    icon: Icon,
    label: &str,
    enabled: bool,
    color: Color32,
) -> bool {
    ui.add_enabled_ui(enabled, |ui| {
        theme::icon_button(ui, icon, 18.0, color, palette.text, label).clicked()
    })
    .inner
}

/// Luna's `SpinningButton`: a refresh icon that spins while busy.
fn spinning_btn(ui: &mut Ui, palette: &Palette, label: &str, spin: bool, enabled: bool) -> bool {
    if spin {
        let r = theme::spinner(ui, 18.0, palette.secondary);
        r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, false, label));
        return false;
    }
    icon_btn(
        ui,
        palette,
        Icon::Refresh,
        label,
        enabled,
        palette.secondary,
    )
}

/// Luna's `LunaSwitch` with its "Enable X" / "Disable X" tooltip.
fn switch_with_tip(ui: &mut Ui, palette: &Palette, tip: &str, on: &mut bool) -> bool {
    widgets::switch(ui, palette, tip, on)
        .on_hover_text(tip)
        .changed()
}

// ------------------------------------------------------------ Install from URL

/// What typing `input` into "Install from URL" does (Luna's `loadPlugin`).
fn submit_install(
    input: &str,
    snapshot: &LunaSnapshot,
    field: &mut InstallField,
    out: &mut Vec<Do>,
    locale: Locale,
    now: f64,
) {
    let t = |s: &'static str| gettext(locale, s).into_owned();
    let v = input.trim();
    if v.is_empty() {
        return;
    }
    let seen = snapshot.install_outcomes.last().map_or(0, |o| o.seq);
    let ok = |field: &mut InstallField, out: &mut Vec<Do>, msg: String| {
        field.value.clear();
        field.error = None;
        field.success = Some((msg.clone(), now));
        out.push(Do::Toast(msg));
    };
    let is_url =
        (v.starts_with("https://") || v.starts_with("http://")) && url::Url::parse(v).is_ok();
    if is_url && v.ends_with("/store.json") {
        let url = norm_store(v);
        if snapshot.stores.iter().any(|s| s.url == url) {
            field.error = Some(t("Store already added"));
            return;
        }
        out.push(Do::AddStore(url));
        ok(field, out, t("Added store {url}!").replace("{url}", v));
        return;
    }
    if is_url
        && v.to_ascii_lowercase()
            .split('?')
            .next()
            .is_some_and(|p| p.ends_with(".css"))
    {
        use crate::luna::themes;
        let id = themes::identity(v);
        // Added before, by this or another link: that theme is applied.
        if let Some(th) = snapshot
            .themes
            .iter()
            .find(|th| themes::identity(&th.url) == id)
        {
            out.push(Do::SetThemeEnabled(th.url.clone(), true));
            let name = th
                .name
                .clone()
                .unwrap_or_else(|| themes::name_from_url(&th.url).0);
            field.theme_added = Some((id, th.url.clone(), now));
            ok(field, out, t("Applied: {name}").replace("{name}", &name));
            return;
        }
        let url = themes::canonical_url(v);
        out.push(Do::InstallTheme(url.clone()));
        let name = themes::name_from_url(&url).0;
        field.theme_added = Some((id, url, now));
        ok(field, out, t("Added theme {url}!").replace("{url}", &name));
        return;
    }
    if crate::luna::is_dev_source(v) {
        out.push(Do::InstallDev(v.to_string()));
        field.error = None;
        field.pending = Some((crate::luna::normalize_plugin_url(v.trim_matches('"')), seen));
        return;
    }
    if !is_url {
        field.error = Some(t("Invalid URL"));
        return;
    }
    out.push(Do::Install(v.to_string()));
    field.error = None;
    field.pending = Some((crate::luna::normalize_plugin_url(v), seen));
}

/// A pending plugin install finished: "Loaded plugin X!" or its error.
fn resolve_install(
    field: &mut InstallField,
    snapshot: &LunaSnapshot,
    out: &mut Vec<Do>,
    locale: Locale,
    now: f64,
) {
    let Some((url, seen)) = field.pending.clone() else {
        return;
    };
    let Some(o) = snapshot
        .install_outcomes
        .iter()
        .rev()
        .find(|o| o.seq > seen && o.url == url)
    else {
        return;
    };
    field.pending = None;
    match &o.result {
        Ok(name) => {
            let msg = gettext(locale, "Loaded plugin {name}!").replace("{name}", name);
            field.value.clear();
            field.error = None;
            field.success = Some((msg.clone(), now));
            out.push(Do::Toast(msg));
        }
        Err(e) => {
            field.error = Some(e.clone());
            field.success = None;
        }
    }
}

fn install_field(
    ui: &mut Ui,
    palette: &Palette,
    locale: Locale,
    snapshot: &LunaSnapshot,
    field: &mut InstallField,
    width: f32,
    out: &mut Vec<Do>,
) {
    let t = |s: &'static str| gettext(locale, s).into_owned();
    let now = ui.input(|i| i.time);
    resolve_install(field, snapshot, out, locale, now);
    if let Some((_, at)) = &field.success
        && now - at > 2.0
    {
        field.success = None;
    }
    let (label, color) = match (&field.error, &field.success) {
        (Some(e), _) => (e.clone(), palette.danger),
        (None, Some((s, _))) => (s.clone(), SUCCESS),
        _ => (t("Install from URL"), palette.secondary),
    };
    ui.vertical(|ui| {
        ui.set_width(width);
        theme::text(ui, &label, theme::medium(13.0), color);
        ui.add_space(6.0);
        // The same field the other pages use: a surface box, 8-point corners.
        let r = Frame::new()
            .fill(palette.surface)
            .corner_radius(CornerRadius::same(8))
            .inner_margin(Margin::symmetric(12, 10))
            .show(ui, |ui| {
                widgets::text_edit(
                    ui,
                    locale,
                    egui::TextEdit::singleline(&mut field.value)
                        .hint_text(RichText::new(t("Install from URL")).color(palette.dim))
                        .font(theme::regular(14.0))
                        .frame(egui::Frame::NONE)
                        .desired_width(ui.available_width()),
                )
            })
            .inner;
        r.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, t("Install from URL"))
        });
        if r.changed() {
            if field.value.is_empty() {
                field.error = None;
            }
            field.changed_at = Some(now);
        }
    });
    if let Some(at) = field.changed_at {
        let wait = 0.5 - (now - at);
        if wait <= 0.0 {
            field.changed_at = None;
            let value = field.value.clone();
            submit_install(&value, snapshot, field, out, locale, now);
        } else {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_secs_f64(wait));
        }
    }
    if field.success.is_some() || field.pending.is_some() {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(250));
    }
}

// ------------------------------------------------------------ Plugins tab

#[allow(clippy::too_many_arguments)]
fn plugins_tab(
    ui: &mut Ui,
    palette: &Palette,
    locale: Locale,
    snapshot: &LunaSnapshot,
    index: &Index,
    now: i64,
    confirm: &mut Option<Confirm>,
    out: &mut Vec<Do>,
) {
    if snapshot.plugins.is_empty() {
        ui.add_space(24.0);
        widgets::empty_state(
            ui,
            palette,
            Icon::LayoutGrid,
            &gettext(locale, "You have no plugins installed!"),
            &store::find_hint(locale),
        );
        return;
    }
    store::updates_bar(ui, palette, locale, snapshot, out);
    for info in &snapshot.plugins {
        let updated = index.dates(&info.url).updated;
        let (top, done) = (ui.cursor().top(), out.len());
        plugin_card(
            ui,
            palette,
            locale,
            info,
            updated.map(|then| (now, then)),
            confirm,
            out,
        );
        scroll_anchor::block(
            ui,
            ("plugin", &info.url),
            top,
            ui.cursor().top(),
            out.len() > done,
        );
        ui.add_space(16.0);
    }
}

fn is_dev(info: &PluginInfo) -> bool {
    info.dev.is_some() || info.url.starts_with("http://127.0.0.1")
}

/// The red pill's text: why it failed, or the plugin's own error signal.
fn load_error(info: &PluginInfo) -> Option<&str> {
    match &info.status {
        PluginStatus::Failed(e) => Some(e.as_str()),
        _ => info.message.as_deref(),
    }
}

/// Luna's `LunaPluginSettings` card.
fn plugin_card(
    ui: &mut Ui,
    palette: &Palette,
    locale: Locale,
    info: &PluginInfo,
    updated: Option<(i64, i64)>,
    confirm: &mut Option<Confirm>,
    out: &mut Vec<Do>,
) {
    let t = |s: &'static str| gettext(locale, s).into_owned();
    let loading = matches!(
        info.status,
        PluginStatus::Loading | PluginStatus::Installing
    );
    let error = load_error(info);
    let version = if is_dev(info) {
        Some(format!(
            "{} [DEV]",
            info.version.clone().unwrap_or_default()
        ))
    } else {
        info.version.clone()
    };
    let has_settings = info.settings.is_some();
    card(ui, palette, error.is_some(), |ui| {
        let h = Header {
            locale,
            name: &info.name,
            version,
            link: info.homepage.as_deref(),
            error,
            author: info.author.as_deref(),
            author_ref: author_ref(
                info.author.as_deref(),
                info.author_url.as_deref(),
                info.author_avatar.as_deref(),
                &info.url,
            ),
            desc: info.description.as_deref().map(strip_html),
            compat: crate::luna::compat_note(&info.url, Some(&info.name)),
        };
        header(ui, palette, &h, out, |ui, out| {
            let mut on = info.enabled;
            let tip = if info.enabled {
                t("Disable {name}")
            } else {
                t("Enable {name}")
            }
            .replace("{name}", &info.name);
            if switch_with_tip(ui, palette, &tip, &mut on) {
                out.push(Do::SetEnabled(info.name.clone(), on));
            }
            let disabled = !info.enabled || loading;
            if spinning_btn(ui, palette, &t("Reload plugin"), loading, !disabled) {
                out.push(Do::Reload(info.name.clone()));
            }
            let (icon, tip, color) = if info.live_reload {
                (Icon::Radio, t("Disable live reloading"), SUCCESS)
            } else {
                (
                    Icon::WifiOff,
                    t("Enable live reloading (for development)"),
                    palette.danger,
                )
            };
            if icon_btn(ui, palette, icon, &tip, !disabled, color) {
                out.push(Do::SetLiveReload(info.name.clone(), !info.live_reload));
            }
            let tip = if info.hide_settings {
                t("Show settings")
            } else {
                t("Hide settings")
            };
            if icon_btn(
                ui,
                palette,
                Icon::Settings,
                &tip,
                info.enabled && has_settings,
                palette.secondary,
            ) {
                out.push(Do::SetHideSettings(info.name.clone(), !info.hide_settings));
            }
            if icon_btn(
                ui,
                palette,
                Icon::Trash,
                &t("Uninstall plugin"),
                true,
                palette.secondary,
            ) {
                out.push(Do::Uninstall(info.name.clone()));
            }
            if let Some(link) = repo_link(&info.url) {
                store::rate_button(ui, palette, locale, false, &link, out);
            }
        });
        // When the build was published, as on the store's cards.
        if let Some((now, then)) = updated {
            caption(ui, &store::age_label(locale, now, then), palette.dim);
        }
        extras(ui, palette, locale, info, out);
        if let Some(tree) = &info.settings
            && !info.hide_settings
        {
            ui.add_space(4.0);
            let mut changes = Vec::new();
            let mut links = Vec::new();
            settings_tree(
                ui,
                palette,
                locale,
                &info.name,
                tree,
                &mut changes,
                &mut links,
            );
            for (id, value) in changes {
                out.push(Do::Setting(info.name.clone(), id, value));
            }
            out.extend(links.into_iter().map(Do::OpenUrl));
        }
        reset_row(ui, palette, locale, info, confirm, out);
    });
}

/// "Reset to defaults" at the bottom of a plugin's card (TidaLuna only has
/// one reset for everything): clears what the plugin stored and its
/// permissions after asking, and runs it again. Shown while the plugin has
/// stored anything and its settings are not folded away.
fn reset_row(
    ui: &mut Ui,
    palette: &Palette,
    locale: Locale,
    info: &PluginInfo,
    confirm: &mut Option<Confirm>,
    out: &mut Vec<Do>,
) {
    let t = |s: &'static str| gettext(locale, s).into_owned();
    let asking = matches!(confirm, Some(Confirm::ResetPlugin(n)) if *n == info.name);
    if !asking && (!info.has_stored || (info.hide_settings && info.settings.is_some())) {
        return;
    }
    ui.add_space(8.0);
    if asking {
        caption(
            ui,
            &t("Reset {name} to its defaults? Its settings and saved data are cleared. Downloaded files stay.")
                .replace("{name}", &info.name),
            palette.text,
        );
        ui.horizontal(|ui| {
            let name = t("Reset {name}").replace("{name}", &info.name);
            if theme::pill_button_named(ui, palette, &t("Reset"), &name, true).clicked() {
                out.push(Do::ResetPlugin(info.name.clone()));
                *confirm = None;
            }
            if theme::soft_button(ui, palette, None, &t("Cancel"), false).clicked() {
                *confirm = None;
            }
        });
    } else if theme::soft_button(ui, palette, None, &t("Reset to defaults"), false).clicked() {
        *confirm = Some(Confirm::ResetPlugin(info.name.clone()));
    }
}

/// "Downloading 45 of 102 MB (model.onnx)" for a plugin's large download.
fn download_line(locale: Locale, d: &crate::luna::downloads::Progress) -> String {
    let mb = |b: u64| format!("{:.0}", b as f64 / f64::from(1u32 << 20));
    let file = d
        .url
        .split(['?', '#'])
        .next()
        .and_then(|u| u.rsplit('/').next())
        .filter(|f| !f.is_empty())
        .unwrap_or(&d.url);
    let line = match d.total {
        Some(total) => gettext(locale, "Downloading {got} of {total} MB ({file})")
            .replace("{total}", &mb(total)),
        None => gettext(locale, "Downloading {got} MB ({file})").into_owned(),
    };
    line.replace("{got}", &mb(d.received))
        .replace("{file}", file)
}

/// Tidalfast's additions, kept small: update, permissions, logs.
fn extras(ui: &mut Ui, palette: &Palette, locale: Locale, info: &PluginInfo, out: &mut Vec<Do>) {
    let t = |s: &'static str| gettext(locale, s).into_owned();
    if info.update_available && info.dev.is_none() {
        store::update_row(ui, palette, locale, info, out);
    }
    // A large download under way (an AI model): how far it got. It stops
    // when the plugin does.
    for d in crate::luna::downloads::active()
        .iter()
        .filter(|d| d.plugin == info.name)
    {
        caption(ui, &download_line(locale, d), palette.secondary);
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(250));
    }
    if info.permissions.is_empty() && info.logs.is_empty() {
        return;
    }
    ui.horizontal_wrapped(|ui| {
        if !info.permissions.is_empty() {
            egui::CollapsingHeader::new(
                RichText::new(t("Permissions"))
                    .font(theme::regular(12.0))
                    .color(palette.secondary),
            )
            .id_salt(("luna-perms", &info.name))
            .show(ui, |ui| {
                for permission in &info.permissions {
                    caption(
                        ui,
                        &format!("• {}", permission.describe_in(locale)),
                        palette.secondary,
                    );
                }
                // The other permissions allowed for good, each revocable.
                for access in &info.grants {
                    let (what, hover) = grant_text(locale, access);
                    ui.horizontal(|ui| {
                        ui.add_space(14.0);
                        let font = match access {
                            crate::luna::Access::Pipe { .. } => egui::FontId::monospace(11.5),
                            _ => theme::regular(11.5),
                        };
                        ui.label(RichText::new(what).font(font).color(palette.dim));
                        if theme::soft_button(ui, palette, None, &t("Revoke"), false)
                            .on_hover_text(hover)
                            .clicked()
                        {
                            out.push(Do::RevokePermission(info.name.clone(), access.clone()));
                        }
                    });
                }
                // Programs allowed for good, each one revocable.
                for program in &info.programs {
                    ui.horizontal(|ui| {
                        ui.add_space(14.0);
                        ui.label(
                            RichText::new(program)
                                .font(egui::FontId::monospace(11.5))
                                .color(palette.dim),
                        );
                        if theme::soft_button(ui, palette, None, &t("Revoke"), false)
                            .on_hover_text(t("Ask again before this plugin runs this program"))
                            .clicked()
                        {
                            out.push(Do::RevokeProgram(info.name.clone(), program.clone()));
                        }
                    });
                }
                // The permission to change the account, given for good.
                if info.account_writes {
                    ui.horizontal(|ui| {
                        ui.add_space(14.0);
                        ui.label(
                            RichText::new(t("Changes your playlists and favourites"))
                                .font(theme::regular(11.5))
                                .color(palette.dim),
                        );
                        if theme::soft_button(ui, palette, None, &t("Revoke"), false)
                            .on_hover_text(t("Ask again before this plugin changes your account"))
                            .clicked()
                        {
                            out.push(Do::RevokeAccountWrites(info.name.clone()));
                        }
                    });
                }
            });
        }
        if !info.logs.is_empty() {
            egui::CollapsingHeader::new(
                RichText::new(format!("{} ({})", t("Logs"), info.logs.len()))
                    .font(theme::regular(12.0))
                    .color(palette.secondary),
            )
            .id_salt(("luna-logs", &info.name))
            .show(ui, |ui| log_lines(ui, palette, &info.logs, None));
        }
    });
}

/// A permission allowed for good, as the plugin's Permissions list it, and
/// what Revoke does.
fn grant_text(locale: Locale, access: &crate::luna::Access) -> (String, String) {
    use crate::luna::Access;
    let t = |s: &'static str| gettext(locale, s).into_owned();
    match access {
        Access::Credentials => (
            t("Uses your TIDAL sign-in"),
            t(
                "Ask again before this plugin gets your TIDAL sign-in. A sign-in it already has stays valid until TIDAL expires it.",
            ),
        ),
        Access::GlobalShortcuts => (
            t("Keyboard shortcuts across your whole computer"),
            t(
                "Ask again before this plugin takes shortcuts across your whole computer; its shortcuts let go now",
            ),
        ),
        Access::Pipe { path } => (
            path.clone(),
            t(
                "Ask again before this plugin talks to the program behind this pipe; its connections close now",
            ),
        ),
        Access::SaveAudio => (
            t("Saves TIDAL songs as files"),
            t("Ask again before this plugin saves TIDAL songs; downloads it has running stop now"),
        ),
    }
}

fn log_lines(ui: &mut Ui, palette: &Palette, lines: &[crate::luna::LogLine], plugin: Option<&str>) {
    for line in lines.iter().rev().take(80) {
        log_row(ui, palette, line, plugin);
    }
}

/// The level as it is written in the log (not translated, as in a console).
fn level_name(level: LogLevel) -> &'static str {
    match level {
        LogLevel::Error => "ERROR",
        LogLevel::Warn => "WARN",
        LogLevel::Info => "INFO",
        LogLevel::Debug => "DEBUG",
    }
}

/// `14:03:09`, local time.
fn log_time(at: std::time::SystemTime) -> String {
    jiff::Timestamp::try_from(at)
        .map(|t| {
            t.to_zoned(jiff::tz::TimeZone::system())
                .strftime("%H:%M:%S")
                .to_string()
        })
        .unwrap_or_default()
}

/// One log line: time, level, the plugin when the list mixes several, then
/// the message (a stack trace keeps its lines).
fn log_row(ui: &mut Ui, palette: &Palette, line: &crate::luna::LogLine, plugin: Option<&str>) {
    let color = match line.level {
        LogLevel::Error => palette.danger,
        LogLevel::Warn => palette.warning,
        LogLevel::Info => palette.text,
        LogLevel::Debug => palette.dim,
    };
    // One wrapped label, so a long line or a stack trace wraps inside the
    // column.
    let mut job = egui::text::LayoutJob::default();
    let mut part = |text: String, font: egui::FontId, color: Color32| {
        job.append(
            &text,
            0.0,
            egui::TextFormat {
                font_id: font,
                color,
                ..Default::default()
            },
        );
    };
    part(
        format!("{}  ", log_time(line.at)),
        egui::FontId::monospace(11.0),
        palette.dim,
    );
    part(
        format!("{:<5}  ", level_name(line.level)),
        egui::FontId::monospace(11.0),
        color,
    );
    if let Some(plugin) = plugin {
        part(
            format!("{plugin}  "),
            theme::semibold(11.5),
            palette.secondary,
        );
    }
    part(line.message.clone(), theme::regular(11.5), color);
    ui.add(egui::Label::new(job).wrap());
}

/// Every plugin's log lines (and the runtime's) in one list, newest first,
/// each with the plugin that wrote it: what a developer watches while the
/// plugin reloads.
fn developer_log(
    ui: &mut Ui,
    palette: &Palette,
    locale: Locale,
    snapshot: &LunaSnapshot,
    page: &mut PageState,
) {
    let t = |s: &'static str| gettext(locale, s).into_owned();
    let mut rows: Vec<(&crate::luna::LogLine, &str)> = snapshot
        .plugins
        .iter()
        .flat_map(|p| p.logs.iter().map(move |l| (l, p.name.as_str())))
        .chain(snapshot.runtime_log.iter().map(|l| (l, "Tidalfast")))
        .filter(|(l, _)| page.show_debug || l.level != LogLevel::Debug)
        .collect();
    rows.sort_by_key(|(l, _)| std::cmp::Reverse(l.at));
    let mut on = page.show_debug;
    if widgets::switch(ui, palette, &t("Show debug lines"), &mut on).changed() {
        page.show_debug = on;
    }
    caption(
        ui,
        &t("Show debug lines: the runtime's own notes (intercepts, ignored actions)."),
        palette.secondary,
    );
    ui.add_space(6.0);
    if rows.is_empty() {
        caption(ui, &t("Nothing logged yet."), palette.dim);
        return;
    }
    egui::CollapsingHeader::new(
        RichText::new(format!("{} ({})", t("Plugin logs"), rows.len()))
            .font(theme::regular(12.0))
            .color(palette.secondary),
    )
    .id_salt("luna-developer-log")
    .default_open(true)
    .show(ui, |ui| {
        for (line, plugin) in rows.into_iter().take(200) {
            log_row(ui, palette, line, Some(plugin));
        }
    });
}

// ------------------------------------------------------------ Settings tab

/// Luna's `LunaSettings` block: a title, a description, then contents.
fn block(ui: &mut Ui, palette: &Palette, title: &str, desc: &str, add: impl FnOnce(&mut Ui)) {
    theme::text(ui, title, theme::semibold(18.0), palette.text);
    if !desc.is_empty() {
        caption(ui, desc, palette.secondary);
    }
    ui.add_space(8.0);
    add(ui);
    ui.add_space(32.0);
}

fn settings_tab(
    ui: &mut Ui,
    palette: &Palette,
    locale: Locale,
    snapshot: &LunaSnapshot,
    page: &mut PageState,
    out: &mut Vec<Do>,
) {
    let t = |s: &'static str| gettext(locale, s).into_owned();
    if let Some(tr) = &snapshot.transfer
        && tr.seq > page.transfer_seen
    {
        page.transfer_seen = tr.seq;
        page.transfer = Some(tr.result.clone());
        if let Ok(msg) = &tr.result {
            out.push(Do::Toast(msg.clone()));
        }
    }
    // A settings file dropped on the window.
    let dropped: Vec<PathBuf> = ui.ctx().input(|i| {
        i.raw
            .dropped_files
            .iter()
            .map(|f| f.path().to_path_buf())
            .filter(|p| !p.as_os_str().is_empty())
            .filter(|p| {
                p.extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| e.eq_ignore_ascii_case("json"))
            })
            .collect()
    });
    if let Some(p) = dropped.into_iter().next() {
        page.import_path = p.to_string_lossy().into_owned();
        page.confirm = Some(Confirm::Import(p));
    }

    // Version (Luna's LunaVersionInfo, honest about what this is).
    card(ui, palette, false, |ui| {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(56.0, 56.0), Sense::hover());
            theme::logo(ui, rect.center(), 56.0, palette.text);
            ui.add_space(16.0);
            ui.vertical(|ui| {
                theme::text(
                    ui,
                    t("Thanks for using TidaLuna plugins!"),
                    theme::semibold(20.0),
                    palette.text,
                );
                caption(
                    ui,
                    &t("Tidalfast runs TidaLuna plugins in its own Luna-compatible runtime."),
                    palette.secondary,
                );
                ui.horizontal_wrapped(|ui| {
                    caption(ui, &t("Luna API:"), palette.text);
                    caption(ui, crate::luna::LUNA_API_VERSION, palette.accent);
                    caption(ui, &t("Tidalfast:"), palette.text);
                    caption(ui, env!("CARGO_PKG_VERSION"), palette.accent);
                });
            });
        });
        ui.add_space(8.0);
        if page.confirm == Some(Confirm::Reset) {
            caption(
                ui,
                &t(
                    "Remove every Luna plugin, plugin setting and theme? Store URLs go back to TidaLuna's list.",
                ),
                palette.text,
            );
            ui.horizontal(|ui| {
                if theme::pill_button(ui, palette, &t("Reset"), true).clicked() {
                    out.push(Do::ResetAll);
                    page.confirm = None;
                }
                if theme::soft_button(ui, palette, None, &t("Cancel"), false).clicked() {
                    page.confirm = None;
                }
            });
        } else if theme::soft_button(ui, palette, Some(Icon::Trash), &t("Reset"), false).clicked() {
            page.confirm = Some(Confirm::Reset);
        }
    });
    ui.add_space(32.0);

    block(
        ui,
        palette,
        &t("Settings Transfer"),
        &t(
            "Exports installed plugins, plugin settings, themes and store URLs. Import clears existing settings and restores them.",
        ),
        |ui| {
            ui.horizontal_wrapped(|ui| {
                if theme::soft_button(
                    ui,
                    palette,
                    Some(Icon::Download),
                    &t("Export Settings"),
                    false,
                )
                .clicked()
                {
                    out.push(Do::Export(page.include_code));
                }
                let path = page.import_path.trim().trim_matches('"').to_string();
                if ui
                    .add_enabled_ui(!path.is_empty(), |ui| {
                        theme::soft_button(
                            ui,
                            palette,
                            Some(Icon::Share),
                            &t("Import Settings"),
                            false,
                        )
                    })
                    .inner
                    .clicked()
                {
                    page.confirm = Some(Confirm::Import(PathBuf::from(path)));
                }
                let r = ui.add(
                    egui::TextEdit::singleline(&mut page.import_path)
                        .hint_text(t("Settings file to import (or drop a .json on the window)"))
                        .desired_width((ui.available_width() - 8.0).clamp(160.0, 420.0)),
                );
                r.widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::WidgetType::TextEdit,
                        true,
                        t("Settings file to import"),
                    )
                });
            });
            if let Some(Confirm::Import(path)) = page.confirm.clone() {
                let file = path
                    .file_name()
                    .map(|f| f.to_string_lossy().into_owned())
                    .unwrap_or_default();
                caption(
                    ui,
                    &t("Import settings from {file}? Existing settings will be cleared and replaced.").replace("{file}", &file),
                    palette.text,
                );
                ui.horizontal(|ui| {
                    if theme::pill_button(ui, palette, &t("Import"), true).clicked() {
                        out.push(Do::Import(path.clone()));
                        page.confirm = None;
                    }
                    if theme::soft_button(ui, palette, None, &t("Cancel"), false).clicked() {
                        page.confirm = None;
                    }
                });
            }
            match &page.transfer {
                Some(Ok(msg)) => caption(ui, msg, SUCCESS),
                Some(Err(e)) => caption(ui, e, palette.danger),
                None => caption(ui, &t("Exports go to your Downloads folder."), palette.dim),
            }
            let mut include = page.include_code;
            widgets::setting_row(
                ui,
                palette,
                &t("Include plugin source code"),
                &t(
                    "Including plugin source code will increase the size of the exported file. This is only useful for exporting dev or unreleased plugins.",
                ),
                |ui| {
                    if widgets::switch(ui, palette, &t("Include plugin source code"), &mut include)
                        .changed()
                    {
                        page.include_code = include;
                    }
                },
            );
        },
    );

    block(
        ui,
        palette,
        &t("Import from TIDAL"),
        &t(
            "Bring your plugins and their settings over from an existing TIDAL with TidaLuna on this computer. TIDAL's files are only read, from a temporary copy.",
        ),
        |ui| import_section(ui, palette, locale, snapshot, page, out),
    );

    block(
        ui,
        palette,
        &t("Feature flags"),
        &t("TIDAL's feature flags belong to TIDAL's own app; Tidalfast has none to toggle."),
        |_| {},
    );

    block(
        ui,
        palette,
        &t("Plugin development"),
        &t(
            "Build plugins with TidaLuna's template, install them from a folder or the dev server (http://127.0.0.1:3000), and watch what they log here.",
        ),
        |ui| {
            if theme::link(
                ui,
                t("Making plugins for Tidalfast"),
                theme::regular(13.0),
                palette.text,
            )
            .clicked()
            {
                out.push(Do::OpenUrl(MAKING_PLUGINS_URL.to_string()));
            }
            ui.add_space(8.0);
            developer_log(ui, palette, locale, snapshot, page);
        },
    );

    block(
        ui,
        palette,
        &t("Luna core plugins"),
        &t("Plugins providing core luna functionality"),
        |ui| dev_card(ui, palette, locale, snapshot, page, out),
    );

    if !snapshot.runtime_log.is_empty() {
        egui::CollapsingHeader::new(t("Runtime log"))
            .id_salt("luna-runtime-log")
            .show(ui, |ui| log_lines(ui, palette, &snapshot.runtime_log, None));
    }
}

/// `@luna/dev`'s settings: Redux and IPC logging.
fn dev_card(
    ui: &mut Ui,
    palette: &Palette,
    locale: Locale,
    snapshot: &LunaSnapshot,
    page: &mut PageState,
    out: &mut Vec<Do>,
) {
    let t = |s: &'static str| gettext(locale, s).into_owned();
    let log = &snapshot.dev_log;
    card(ui, palette, false, |ui| {
        let h = Header {
            locale,
            name: "@luna/dev",
            version: None,
            link: None,
            error: None,
            author: Some("Inrixia"),
            author_ref: author_ref(
                Some("Inrixia"),
                None,
                None,
                "https://github.com/Inrixia/TidaLuna",
            ),
            desc: None,
            compat: None,
        };
        header(ui, palette, &h, out, |_, _| {});
        theme::text(ui, t("Redux Logging"), theme::semibold(15.0), palette.text);
        let mut next = log.clone();
        widgets::setting_row(
            ui,
            palette,
            &t("Render redux events"),
            &t("Log Redux dispatch events to the runtime log"),
            |ui| {
                let draft = page
                    .filter_draft
                    .get_or_insert_with(|| log.redux_filter.clone());
                let r = ui.add(
                    egui::TextEdit::singleline(draft)
                        .hint_text(".*")
                        .desired_width(160.0),
                );
                r.widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::WidgetType::TextEdit,
                        true,
                        t("Event regex filter"),
                    )
                });
                if r.lost_focus() && *draft != log.redux_filter {
                    next.redux_filter = draft.clone();
                }
                if !r.has_focus() && !r.lost_focus() {
                    page.filter_draft = None;
                }
                let mut on = log.redux;
                if widgets::switch(ui, palette, &t("Render redux events"), &mut on).changed() {
                    next.redux = on;
                }
            },
        );
        ui.add_space(8.0);
        theme::text(ui, t("IPC Logging"), theme::semibold(15.0), palette.text);
        caption(
            ui,
            &t("Logs IPC events between Render & Native to the runtime log"),
            palette.secondary,
        );
        let mut native = log.ipc_native;
        widgets::setting_row(
            ui,
            palette,
            &t("Native IPC"),
            &t("Log Native > Render IPC events"),
            |ui| {
                if widgets::switch(ui, palette, &t("Native IPC"), &mut native).changed() {
                    next.ipc_native = native;
                }
            },
        );
        let mut render = log.ipc_render;
        widgets::setting_row(
            ui,
            palette,
            &t("Render IPC"),
            &t("Log Render > Native IPC events"),
            |ui| {
                if widgets::switch(ui, palette, &t("Render IPC"), &mut render).changed() {
                    next.ipc_render = render;
                }
            },
        );
        if next != *log {
            out.push(Do::SetDevLog(next));
        }
    });
}

fn import_section(
    ui: &mut Ui,
    palette: &Palette,
    locale: Locale,
    snapshot: &LunaSnapshot,
    page: &mut PageState,
    out: &mut Vec<Do>,
) {
    let t = |s: &'static str| gettext(locale, s).into_owned();
    if let ImportState::Scanning(rx) = &page.import {
        match rx.try_recv() {
            Ok(Ok(scan)) => {
                let picked = scan
                    .plugins
                    .iter()
                    // Dev installs load from a server on that computer: off by default.
                    .map(|p| !p.dev && !snapshot.plugins.iter().any(|i| i.name == p.name))
                    .collect();
                page.import = ImportState::Ready {
                    scan,
                    picked,
                    add_stores: true,
                };
            }
            Ok(Err(e)) => page.import = ImportState::Failed(e),
            Err(TryRecvError::Empty) => {
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(120));
            }
            Err(TryRecvError::Disconnected) => {
                page.import = ImportState::Failed(t("The scan stopped unexpectedly."))
            }
        }
    }
    let scanning = matches!(page.import, ImportState::Scanning(_));
    ui.horizontal(|ui| {
        if ui
            .add_enabled_ui(!scanning, |ui| {
                theme::soft_button(
                    ui,
                    palette,
                    Some(Icon::Search),
                    &t("Scan TIDAL install"),
                    false,
                )
            })
            .inner
            .clicked()
        {
            let (tx, rx) = std::sync::mpsc::channel();
            let ctx = ui.ctx().clone();
            let spawned = std::thread::Builder::new()
                .name("luna-import-scan".into())
                .spawn(move || {
                    let _ = tx.send(import::scan_default());
                    ctx.request_repaint();
                });
            page.import = match spawned {
                Ok(_) => ImportState::Scanning(rx),
                Err(e) => ImportState::Failed(e.to_string()),
            };
        }
        if scanning {
            caption(ui, &t("Reading…"), palette.dim);
        }
    });
    let mut finished = None;
    match &mut page.import {
        ImportState::Failed(e) => caption(ui, e, palette.danger),
        ImportState::Done(msg) => caption(ui, msg, palette.secondary),
        ImportState::Ready {
            scan,
            picked,
            add_stores,
        } => {
            if scan.plugins.is_empty() {
                caption(ui, &t("No installed plugins found."), palette.secondary);
            }
            for (plugin, pick) in scan.plugins.iter().zip(picked.iter_mut()) {
                let already = snapshot.plugins.iter().any(|i| i.name == plugin.name);
                let mut desc = vec![if plugin.enabled {
                    t("Enabled")
                } else {
                    t("Off")
                }];
                if let Some(v) = &plugin.version {
                    desc.insert(0, v.clone());
                }
                if plugin.dev {
                    desc.push(t("Dev install (loads from a local dev server)"));
                }
                if plugin.storage.is_some() {
                    desc.push(t("Has saved settings"));
                }
                if already {
                    desc.push(t("Already installed"));
                }
                widgets::setting_row(ui, palette, &plugin.name, &desc.join("  ·  "), |ui| {
                    ui.add_enabled_ui(!already, |ui| {
                        widgets::switch(ui, palette, &plugin.name, pick);
                    });
                });
            }
            let missing: Vec<&String> = scan
                .stores
                .iter()
                .filter(|u| !snapshot.stores.iter().any(|s| s.url == norm_store(u)))
                .collect();
            let new_themes: Vec<&import::ImportTheme> = scan
                .themes
                .iter()
                .filter(|th| !snapshot.themes.iter().any(|x| x.url == th.url))
                .collect();
            if !missing.is_empty() || !new_themes.is_empty() {
                widgets::setting_row(
                    ui,
                    palette,
                    &format!(
                        "{} ({}, {})",
                        t("Add the store URLs and themes"),
                        missing.len(),
                        new_themes.len()
                    ),
                    "",
                    |ui| {
                        widgets::switch(ui, palette, "stores", add_stores);
                    },
                );
            }
            let n = picked.iter().filter(|p| **p).count();
            if ui
                .add_enabled_ui(
                    n > 0 || (*add_stores && (!missing.is_empty() || !new_themes.is_empty())),
                    |ui| {
                        theme::pill_button(
                            ui,
                            palette,
                            &format!("{} ({n})", t("Import selected")),
                            true,
                        )
                    },
                )
                .inner
                .clicked()
            {
                if *add_stores {
                    out.extend(missing.iter().map(|u| Do::AddStore((*u).clone())));
                    for th in &new_themes {
                        out.push(Do::InstallTheme(th.url.clone()));
                        if !th.enabled {
                            out.push(Do::SetThemeEnabled(th.url.clone(), false));
                        }
                    }
                }
                for (plugin, pick) in scan.plugins.iter().zip(picked.iter()) {
                    if *pick {
                        out.push(Do::ImportPlugin(
                            plugin.url.clone(),
                            plugin.enabled,
                            plugin.storage.clone(),
                        ));
                    }
                }
                finished = Some(format!("{} {n}", t("Importing plugins:")));
            }
        }
        ImportState::Idle | ImportState::Scanning(_) => {}
    }
    if let Some(msg) = finished {
        page.import = ImportState::Done(msg);
    }
}

// ------------------------------------------------------------ plugin settings

fn meta_text(meta: &SettingsMeta) -> (String, String) {
    (
        meta.title.clone().unwrap_or_default(),
        meta.desc.as_deref().map(strip_html).unwrap_or_default(),
    )
}

/// The key combination of a key press, as a browser's `KeyboardEvent`
/// reports it: `{key, code, ctrlKey, shiftKey, altKey, metaKey}`.
pub(crate) fn key_combo(key: egui::Key, modifiers: egui::Modifiers) -> serde_json::Value {
    let (k, code) = super::plugin_dom::dom_key(key, modifiers.shift);
    serde_json::json!({
        "key": k,
        "code": code,
        "ctrlKey": modifiers.ctrl || (modifiers.command && !modifiers.mac_cmd),
        "shiftKey": modifiers.shift,
        "altKey": modifiers.alt,
        "metaKey": modifiers.mac_cmd,
    })
}

/// Draws a Luna plugin's settings natively. What the user changes is
/// pushed to `changes` (for [`crate::luna::Luna::settings_input`]); links
/// clicked go to `links`. Text is sent when its field loses focus or Enter
/// is pressed, as Luna's own text settings do.
pub(crate) fn settings_tree(
    ui: &mut Ui,
    palette: &Palette,
    locale: Locale,
    plugin: &str,
    node: &SettingsNode,
    changes: &mut Vec<(String, SettingValue)>,
    links: &mut Vec<String>,
) {
    if luna_look(ui, palette, locale, plugin, node, changes, links) {
        return;
    }
    match node {
        // A `position: fixed` card (colorama-lyrics' colour picker) goes
        // where a browser puts it, over the window's centre.
        SettingsNode::Group { meta, children } if meta.overlay == Some(true) => {
            let id = egui::Id::new(("luna-settings-overlay", plugin, children.len()));
            egui::Area::new(id)
                .order(egui::Order::Foreground)
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .show(ui.ctx(), |ui| {
                    Frame::new()
                        .fill(palette.panel)
                        .stroke(egui::Stroke::new(1.0, palette.outline))
                        .corner_radius(CornerRadius::same(12))
                        .inner_margin(Margin::same(16))
                        .show(ui, |ui| {
                            ui.set_max_width(360.0);
                            // A card taller than the window scrolls.
                            let room = (ui.ctx().content_rect().height() - 96.0).max(120.0);
                            egui::ScrollArea::vertical()
                                .max_height(room)
                                .show(ui, |ui| {
                                    for child in children {
                                        settings_tree(
                                            ui, palette, locale, plugin, child, changes, links,
                                        );
                                    }
                                });
                        });
                });
        }
        // The backdrop of such a card: dims the window, and a click on it
        // does what the plugin's backdrop does (close the card).
        SettingsNode::Button { id, meta, .. } if meta.scrim == Some(true) => {
            let screen = ui.ctx().content_rect();
            egui::Area::new(egui::Id::new(("luna-settings-scrim", plugin, id.as_str())))
                .order(egui::Order::Foreground)
                .fixed_pos(screen.min)
                .show(ui.ctx(), |ui| {
                    let r = ui.allocate_rect(screen, Sense::click());
                    ui.painter()
                        .rect_filled(screen, 0.0, Color32::from_black_alpha(150));
                    if r.clicked() {
                        changes.push((id.clone(), SettingValue::Click));
                    }
                });
        }
        SettingsNode::Group { meta, children } => {
            let (title, desc) = meta_text(meta);
            if !title.is_empty() {
                ui.add_space(4.0);
                theme::text(ui, &title, theme::semibold(14.0), palette.text);
            }
            if !desc.is_empty() {
                ui.label(
                    RichText::new(desc)
                        .font(theme::regular(12.0))
                        .color(palette.secondary),
                );
            }
            for child in children {
                settings_tree(ui, palette, locale, plugin, child, changes, links);
            }
        }
        SettingsNode::Switch { id, meta, checked } => {
            let (title, desc) = meta_text(meta);
            let mut on = *checked;
            ui.add_enabled_ui(meta.disabled != Some(true), |ui| {
                widgets::setting_row(ui, palette, &title, &desc, |ui| {
                    if widgets::switch(ui, palette, &title, &mut on).changed() {
                        changes.push((id.clone(), SettingValue::Bool(on)));
                    }
                });
            });
        }
        SettingsNode::Number {
            id,
            meta,
            value,
            min,
            max,
            step,
            slider,
        } => {
            let (title, desc) = meta_text(meta);
            let mut v = *value;
            // In the same `Ui` as the switches and buttons, so its row
            // counts among theirs and gets its line and spacing.
            ui.add_enabled_ui(meta.disabled != Some(true), |ui| {
                widgets::setting_row(ui, palette, &title, &desc, |ui| {
                    if let (Some(true), Some(lo), Some(hi)) = (slider, min, max) {
                        let mut s = egui::Slider::new(&mut v, *lo..=*hi);
                        if let Some(step) = step {
                            s = s.step_by(*step);
                        }
                        let r = ui.add_enabled(meta.disabled != Some(true), s);
                        if r.changed() {
                            changes.push((id.clone(), SettingValue::Number(v)));
                        }
                        return;
                    }
                    let mut drag = egui::DragValue::new(&mut v)
                        .range(min.unwrap_or(f64::MIN)..=max.unwrap_or(f64::MAX));
                    if let Some(step) = step {
                        drag = drag.speed(*step);
                    }
                    let r = ui.add(drag);
                    r.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::WidgetType::DragValue, true, &title)
                    });
                    if r.changed() {
                        changes.push((id.clone(), SettingValue::Number(v)));
                    }
                });
            });
        }
        SettingsNode::Text {
            id,
            meta,
            value,
            secure,
            placeholder,
            ..
        } => {
            let (title, desc) = meta_text(meta);
            let draft_id = egui::Id::new(("luna-setting-draft", plugin, id));
            let mut draft: String = ui
                .data(|d| d.get_temp(draft_id))
                .unwrap_or_else(|| value.clone());
            // In the same `Ui` as the switches and buttons, so its row
            // counts among theirs and gets its line and spacing.
            ui.add_enabled_ui(meta.disabled != Some(true), |ui| {
                widgets::setting_row(ui, palette, &title, &desc, |ui| {
                    // A box of its own, darker than the card it sits in: the
                    // field's default fill is the card's own colour with no
                    // outline, so an empty field (AltPlay's Jellyfin sign-in)
                    // could not be seen.
                    let r = Frame::new()
                        .fill(palette.window)
                        .corner_radius(CornerRadius::same(6))
                        .inner_margin(Margin::symmetric(10, 6))
                        .show(ui, |ui| {
                            widgets::text_edit(
                                ui,
                                locale,
                                egui::TextEdit::singleline(&mut draft)
                                    .password(*secure)
                                    // TidaLuna's field shows its title inside
                                    // while empty (MUI's label).
                                    .hint_text(
                                        RichText::new(
                                            placeholder.clone().unwrap_or_else(|| title.clone()),
                                        )
                                        .color(palette.dim),
                                    )
                                    .font(theme::regular(13.0))
                                    .frame(egui::Frame::NONE)
                                    .desired_width(200.0),
                            )
                        })
                        .inner;
                    ui.ctx()
                        .accesskit_node_builder(r.id, |node| node.set_label(title.as_str()));
                    let commit = r.lost_focus();
                    if commit && draft != *value {
                        changes.push((id.clone(), SettingValue::Text(draft.clone())));
                    }
                    if r.has_focus() || (commit && draft != *value) {
                        ui.data_mut(|d| d.insert_temp(draft_id, draft.clone()));
                    } else {
                        ui.data_mut(|d| d.remove_temp::<String>(draft_id));
                    }
                });
            });
        }
        SettingsNode::KeyCapture { id, meta, value } => {
            let (title, desc) = meta_text(meta);
            let state = egui::Id::new(("luna-key-capture", plugin, id));
            let mut capturing: bool = ui.data(|d| d.get_temp(state)).unwrap_or(false);
            ui.add_enabled_ui(meta.disabled != Some(true), |ui| {
                widgets::setting_row(ui, palette, &title, &desc, |ui| {
                    let label = if capturing {
                        gettext(locale, "Press keys…").into_owned()
                    } else if value.trim().is_empty() {
                        gettext(locale, "Click, then press keys").into_owned()
                    } else {
                        value.clone()
                    };
                    let r = theme::soft_button(ui, palette, None, &label, capturing);
                    r.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &title)
                    });
                    if r.clicked() {
                        capturing = !capturing;
                    }
                    if capturing {
                        let pressed = ui.input_mut(|i| {
                            let mut found = None;
                            i.events.retain(|e| match e {
                                egui::Event::Key {
                                    key,
                                    pressed: true,
                                    modifiers,
                                    ..
                                } if found.is_none() => {
                                    found = Some((*key, *modifiers));
                                    false
                                }
                                egui::Event::Key { .. } | egui::Event::Text(_) => false,
                                _ => true,
                            });
                            found
                        });
                        match pressed {
                            Some((egui::Key::Escape, m)) if m.is_none() => capturing = false,
                            Some((key, modifiers)) => {
                                changes.push((
                                    id.clone(),
                                    SettingValue::Json(key_combo(key, modifiers)),
                                ));
                                capturing = false;
                            }
                            None => {}
                        }
                    }
                });
            });
            ui.data_mut(|d| d.insert_temp(state, capturing));
        }
        SettingsNode::Crashed { error } => {
            ui.label(
                RichText::new(
                    gettext(locale, "Settings crashed: {error}").replace("{error}", error),
                )
                .font(theme::regular(13.0))
                .color(palette.danger),
            );
            if theme::soft_button(ui, palette, None, &gettext(locale, "Retry"), false).clicked() {
                changes.push((SETTINGS_RETRY.to_string(), SettingValue::Click));
            }
        }
        SettingsNode::Select {
            id,
            meta,
            value,
            options,
            ..
        } => {
            let (title, desc) = meta_text(meta);
            // In the same `Ui` as the switches and buttons, so its row
            // counts among theirs and gets its line and spacing.
            ui.add_enabled_ui(meta.disabled != Some(true), |ui| {
                widgets::setting_row(ui, palette, &title, &desc, |ui| {
                    let shown = options
                        .iter()
                        .find(|o| o.value == *value)
                        .map(|o| o.label.clone())
                        .unwrap_or_default();
                    egui::ComboBox::from_id_salt(("luna-select", plugin, id))
                        .selected_text(shown)
                        .show_ui(ui, |ui| {
                            for option in options {
                                if ui
                                    .selectable_label(option.value == *value, &option.label)
                                    .clicked()
                                {
                                    changes.push((
                                        id.clone(),
                                        SettingValue::Json(option.value.clone()),
                                    ));
                                }
                            }
                        });
                });
            });
        }
        SettingsNode::Button { id, meta, label } => {
            let (title, desc) = meta_text(meta);
            ui.add_enabled_ui(meta.disabled != Some(true), |ui| {
                widgets::setting_row(ui, palette, &title, &desc, |ui| {
                    if theme::soft_button(ui, palette, None, label, false).clicked() {
                        changes.push((id.clone(), SettingValue::Click));
                    }
                });
            });
        }
        SettingsNode::Link { text, href } => {
            if theme::link(ui, text, theme::regular(13.0), palette.accent).clicked()
                && (href.starts_with("https://") || href.starts_with("http://"))
            {
                links.push(href.clone());
            }
        }
        SettingsNode::Label { text, title, desc } => {
            let font = if *title == Some(true) {
                theme::semibold(14.0)
            } else {
                theme::regular(13.0)
            };
            ui.label(
                RichText::new(strip_html(text))
                    .font(font)
                    .color(palette.text),
            );
            if let Some(desc) = desc {
                ui.label(
                    RichText::new(strip_html(desc))
                        .font(theme::regular(12.0))
                        .color(palette.secondary),
                );
            }
        }
    }
}

/// The option a MUI `Select` shows for `value`, and its words: none for an
/// empty value (`""` or `null`), as MUI shows nothing then even when an
/// item has that value (custom-fonts' "None" is an empty field in TidaLuna).
pub(crate) fn shown_option<'a>(
    value: &serde_json::Value,
    options: &'a [crate::luna::SelectOption],
) -> (Option<usize>, &'a str) {
    let chosen = options.iter().position(|o| o.value == *value);
    let empty = value.is_null() || value.as_str() == Some("");
    let shown = match chosen {
        Some(i) if !empty => options[i].label.as_str(),
        _ => "",
    };
    (chosen, shown)
}

/// A node `@luna/ui` or MUI made, drawn in that component's look (see
/// [`super::luna_settings`]): `LunaSettings`' box, switches, number
/// steppers, text fields, selects and buttons. `false` for anything else,
/// which keeps the app's widgets.
fn luna_look(
    ui: &mut Ui,
    palette: &Palette,
    locale: Locale,
    plugin: &str,
    node: &SettingsNode,
    changes: &mut Vec<(String, SettingValue)>,
    links: &mut Vec<String>,
) -> bool {
    use super::luna_settings::{self as look, FieldLabel, Place};
    let mui = look::Mui::new(palette);
    let named = |meta: &SettingsMeta, names: &[&str]| {
        meta.mui.as_deref().is_some_and(|n| names.contains(&n))
    };
    let enabled = |meta: &SettingsMeta| meta.disabled != Some(true);
    match node {
        SettingsNode::Group { meta, children } if look::is_settings_box(meta) => {
            let (title, desc) = meta_text(meta);
            if !title.is_empty() || !desc.is_empty() {
                look::title_block(ui, &mui, &title, &desc, ui.available_width());
            }
            look::settings_box(ui, &mui, |ui| {
                for child in children {
                    settings_tree(ui, palette, locale, plugin, child, changes, links);
                }
            });
            true
        }
        SettingsNode::Switch { id, meta, checked }
            if named(meta, &["LunaSwitchSetting", "LunaSwitch", "Switch"]) =>
        {
            let (title, desc) = meta_text(meta);
            let mut on = *checked;
            ui.add_enabled_ui(enabled(meta), |ui| {
                look::row(
                    ui,
                    &mui,
                    &title,
                    &desc,
                    Place::After { width: 58.0 },
                    |ui, _| {
                        if look::switch(ui, &mui, &title, &mut on).changed() {
                            changes.push((id.clone(), SettingValue::Bool(on)));
                        }
                    },
                );
            });
            true
        }
        SettingsNode::Number {
            id,
            meta,
            value,
            min,
            max,
            slider,
            ..
        } if slider != &Some(true) && named(meta, &["LunaNumberSetting", "LunaNumber"]) => {
            let (title, desc) = meta_text(meta);
            let key = egui::Id::new(("luna-number", plugin, id));
            ui.add_enabled_ui(enabled(meta), |ui| {
                look::row(
                    ui,
                    &mui,
                    &title,
                    &desc,
                    Place::After { width: 128.0 },
                    |ui, _| {
                        if let Some(v) = look::number(ui, &mui, key, &title, *value, *min, *max) {
                            changes.push((id.clone(), SettingValue::Number(v)));
                        }
                    },
                );
            });
            true
        }
        SettingsNode::Text {
            id,
            meta,
            value,
            secure,
            placeholder,
            ..
        } if named(
            meta,
            &[
                "LunaTextSetting",
                "LunaSecureTextSetting",
                "LunaSecureText",
                "TextField",
                "OutlinedInput",
            ],
        ) =>
        {
            let (title, desc) = meta_text(meta);
            let name = meta.mui.as_deref().unwrap_or_default();
            let key = egui::Id::new(("luna-text", plugin, id));
            let draft_id = egui::Id::new(("luna-setting-draft", plugin, id));
            let mut draft: String = ui
                .data(|d| d.get_temp(draft_id))
                .unwrap_or_else(|| value.clone());
            let hint = placeholder.clone().unwrap_or_default();
            ui.add_enabled_ui(enabled(meta), |ui| {
                let mut field = |ui: &mut Ui, width: f32| {
                    let label = match name {
                        // `LunaTextSetting` and MUI's TextField: the title is
                        // the field's label.
                        "LunaTextSetting" | "TextField" => FieldLabel::Floating(&title),
                        // `LunaSecureTextSetting`: the title as placeholder.
                        "LunaSecureTextSetting" => FieldLabel::Placeholder(&title),
                        _ => FieldLabel::Placeholder(&hint),
                    };
                    let r = look::text_field(ui, &mui, key, &mut draft, label, *secure, width);
                    ui.ctx()
                        .accesskit_node_builder(r.id, |node| node.set_label(title.as_str()));
                    let commit = r.lost_focus();
                    if commit && draft != *value {
                        changes.push((id.clone(), SettingValue::Text(draft.clone())));
                    }
                    if r.has_focus() || (commit && draft != *value) {
                        ui.data_mut(|d| d.insert_temp(draft_id, draft.clone()));
                    } else {
                        ui.data_mut(|d| d.remove_temp::<String>(draft_id));
                    }
                };
                if name.ends_with("Setting") {
                    look::row(
                        ui,
                        &mui,
                        &title,
                        &desc,
                        Place::Fill {
                            basis: 220.0,
                            height: 37.0,
                        },
                        |ui, w| field(ui, w),
                    );
                } else {
                    // A field on its own, as wide as its box.
                    field(ui, ui.available_width());
                }
            });
            true
        }
        SettingsNode::Select {
            id,
            meta,
            value,
            options,
            ..
        } if named(meta, &["LunaSelectSetting", "Select", "TextField"]) => {
            let (title, desc) = meta_text(meta);
            let key = egui::Id::new(("luna-select", plugin, id));
            let labels: Vec<&str> = options.iter().map(|o| o.label.as_str()).collect();
            let (chosen, shown) = shown_option(value, options);
            ui.add_enabled_ui(enabled(meta), |ui| {
                let mut field = |ui: &mut Ui, width: f32| {
                    if let Some(i) = look::select(
                        ui,
                        &mui,
                        key,
                        &title,
                        look::Choices {
                            options: &labels,
                            chosen,
                            shown,
                        },
                        width,
                    ) {
                        changes.push((id.clone(), SettingValue::Json(options[i].value.clone())));
                    }
                };
                if named(meta, &["LunaSelectSetting"]) {
                    // The select's own width: its words and arrow.
                    let basis = 14.0 + 32.0 + 16.0;
                    look::row(
                        ui,
                        &mui,
                        &title,
                        &desc,
                        Place::Fill {
                            basis,
                            height: 53.0,
                        },
                        |ui, w| field(ui, w),
                    );
                } else {
                    field(ui, ui.available_width());
                }
            });
            true
        }
        SettingsNode::Button { id, meta, label }
            if named(meta, &["LunaButtonSetting", "LunaButton", "Button"]) =>
        {
            let (title, desc) = meta_text(meta);
            let variant = meta.variant.clone().unwrap_or_else(|| {
                if named(meta, &["Button"]) {
                    "text".into()
                } else {
                    "contained".into()
                }
            });
            ui.add_enabled_ui(enabled(meta), |ui| {
                if named(meta, &["LunaButtonSetting"]) {
                    let width = look::button_width(ui, label, &variant);
                    look::row(ui, &mui, &title, &desc, Place::Right { width }, |ui, _| {
                        if look::button(ui, &mui, label, &variant, Some(40.0)).clicked() {
                            changes.push((id.clone(), SettingValue::Click));
                        }
                    });
                } else if look::button(ui, &mui, label, &variant, None).clicked() {
                    changes.push((id.clone(), SettingValue::Click));
                }
            });
            true
        }
        _ => false,
    }
}

#[cfg(test)]
#[path = "plugins_tests.rs"]
mod tests;

/// A code editor a Luna plugin opened in a window of its own (Themer's
/// Monaco CSS editor), drawn as a window inside the app. Every edit goes
/// back to the plugin, so a theme changes as it is typed.
pub fn editor_window(app: &mut App, ctx: &egui::Context) {
    let plugins = &mut app.plugins;
    let Some(editor) = plugins.editor.as_mut() else {
        return;
    };
    let mut open = true;
    let mut changed = false;
    egui::Window::new(editor.title.clone())
        .id(egui::Id::new(("luna-editor", editor.window)))
        .open(&mut open)
        .default_size([640.0, 480.0])
        .resizable(true)
        .collapsible(false)
        .show(ctx, |ui| {
            egui::ScrollArea::both().auto_shrink(false).show(ui, |ui| {
                let response = ui.add(
                    egui::TextEdit::multiline(&mut editor.text)
                        .code_editor()
                        .desired_width(f32::INFINITY)
                        .desired_rows(24)
                        .lock_focus(true),
                );
                changed = response.changed();
            });
        });
    let window = editor.window;
    let text = editor.text.clone();
    if let Some(luna) = &plugins.luna {
        if changed {
            luna.editor_input(window, Some(text));
        }
        if !open {
            luna.editor_input(window, None);
        }
    }
    if !open {
        plugins.editor = None;
    }
}
