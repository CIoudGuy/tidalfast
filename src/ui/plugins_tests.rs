use super::*;
use crate::luna::{InstallOutcome, SelectOption};
use egui::accesskit::{Action as AkAction, ActionRequest, NodeId, Role, TreeId, TreeUpdate};
use serde_json::json;

fn tree() -> SettingsNode {
    SettingsNode::Group {
        meta: SettingsMeta {
            title: Some("Discord".into()),
            ..SettingsMeta::default()
        },
        children: vec![
            SettingsNode::Switch {
                id: "displayOnPause".into(),
                meta: SettingsMeta {
                    title: Some("Display on pause".into()),
                    desc: Some("Keep the <b>status</b> while paused".into()),
                    ..SettingsMeta::default()
                },
                checked: true,
            },
            SettingsNode::Number {
                id: "changeBy".into(),
                meta: SettingsMeta {
                    title: Some("Change by".into()),
                    ..SettingsMeta::default()
                },
                value: 2.0,
                min: Some(1.0),
                max: Some(50.0),
                step: None,
                slider: None,
            },
            SettingsNode::Text {
                id: "status".into(),
                meta: SettingsMeta {
                    title: Some("Status text".into()),
                    ..SettingsMeta::default()
                },
                value: "{track} by {artist}".into(),
                secure: false,
                multiline: None,
                placeholder: None,
            },
            SettingsNode::Select {
                id: "quality".into(),
                meta: SettingsMeta {
                    title: Some("Quality".into()),
                    ..SettingsMeta::default()
                },
                value: json!("HIGH"),
                options: vec![
                    SelectOption {
                        value: json!("HIGH"),
                        label: "High".into(),
                    },
                    SelectOption {
                        value: json!("LOSSLESS"),
                        label: "Lossless".into(),
                    },
                ],
                multiple: None,
            },
            SettingsNode::Button {
                id: "reset".into(),
                meta: SettingsMeta::default(),
                label: "Reset".into(),
            },
            SettingsNode::Label {
                text: "Hello".into(),
                title: Some(true),
                desc: None,
            },
        ],
    }
}

fn run(ctx: &egui::Context, events: Vec<egui::Event>, mut draw: impl FnMut(&mut Ui)) -> TreeUpdate {
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1000.0, 3000.0),
            )),
            events,
            ..Default::default()
        },
        |ui| {
            egui::CentralPanel::default().show(ui, |ui| draw(ui));
        },
    );
    output.textures_delta.clear();
    output.platform_output.accesskit_update.unwrap()
}

fn context() -> egui::Context {
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    theme::install(&ctx);
    ctx
}

fn frame(
    ctx: &egui::Context,
    node: &SettingsNode,
    events: Vec<egui::Event>,
) -> (TreeUpdate, Vec<(String, SettingValue)>) {
    let palette = Palette::dark();
    let mut changes = Vec::new();
    let mut links = Vec::new();
    let update = run(ctx, events, |ui| {
        settings_tree(
            ui,
            &palette,
            Locale::default(),
            "Test",
            node,
            &mut changes,
            &mut links,
        );
    });
    (update, changes)
}

/// One frame of the whole page on `tab`.
fn page_frame(
    ctx: &egui::Context,
    snapshot: &LunaSnapshot,
    page: &mut PageState,
    events: Vec<egui::Event>,
) -> (TreeUpdate, Vec<Do>) {
    page_frame_with(
        ctx,
        &Palette::dark(),
        snapshot,
        &Arc::new(Index::default()),
        page,
        events,
    )
}

/// The clock of the tests: far enough from 1970 for "recent" to mean something.
const NOW: i64 = 1_800_000_000;

fn page_frame_with(
    ctx: &egui::Context,
    palette: &Palette,
    snapshot: &LunaSnapshot,
    index: &Arc<Index>,
    page: &mut PageState,
    events: Vec<egui::Event>,
) -> (TreeUpdate, Vec<Do>) {
    let mut out = Vec::new();
    let update = run(ctx, events, |ui| {
        out = draw(ui, palette, Locale::default(), snapshot, index, NOW, page);
    });
    (update, out)
}

fn top(n: &egui::accesskit::Node) -> (f64, f64) {
    n.bounds().map_or((f64::MAX, f64::MAX), |b| (b.y0, b.x0))
}

/// The topmost (then leftmost) node with this label and role.
fn find_opt(tree: &TreeUpdate, label: &str, role: Role) -> Option<NodeId> {
    tree.nodes
        .iter()
        .filter(|(_, n)| n.label() == Some(label) && n.role() == role)
        .min_by(|a, b| top(&a.1).partial_cmp(&top(&b.1)).unwrap())
        .map(|(id, _)| *id)
}

/// Where the first node showing `text` sits (y, x).
fn pos_of(tree: &TreeUpdate, text: &str) -> (f64, f64) {
    tree.nodes
        .iter()
        .filter(|(_, n)| n.label().or(n.value()).is_some_and(|l| l.contains(text)))
        .map(|(_, n)| top(n))
        .min_by(|a, b| a.partial_cmp(b).unwrap())
        .unwrap_or_else(|| panic!("{text} not drawn"))
}

fn find(tree: &TreeUpdate, label: &str, role: Role) -> NodeId {
    find_opt(tree, label, role).unwrap_or_else(|| panic!("missing {label:?} ({role:?})"))
}

fn texts(tree: &TreeUpdate) -> Vec<String> {
    tree.nodes
        .iter()
        .filter_map(|(_, n)| {
            n.label()
                .map(str::to_string)
                .or_else(|| n.value().map(str::to_string))
        })
        .collect()
}

fn has_text(tree: &TreeUpdate, text: &str) -> bool {
    texts(tree).iter().any(|l| l.contains(text))
}

fn click(target: NodeId) -> egui::Event {
    egui::Event::AccessKitActionRequest(ActionRequest {
        target_tree: TreeId::ROOT,
        target_node: target,
        action: AkAction::Click,
        data: None,
    })
}

/// Click the control labelled `label` and return what the page did.
fn press(
    ctx: &egui::Context,
    snapshot: &LunaSnapshot,
    page: &mut PageState,
    label: &str,
    role: Role,
) -> Vec<Do> {
    let (update, _) = page_frame(ctx, snapshot, page, vec![]);
    let id = find(&update, label, role);
    page_frame(ctx, snapshot, page, vec![click(id)]).1
}

fn info(name: &str) -> PluginInfo {
    PluginInfo {
        name: name.into(),
        url: format!("https://example.com/{name}"),
        version: Some("1.2.0".into()),
        description: Some(format!("{name} <b>does</b> things")),
        author: Some("Someone".into()),
        author_url: None,
        author_avatar: None,
        homepage: Some("https://example.com/home".into()),
        hash: None,
        dev: None,
        update_available: false,
        latest_hash: None,
        tags: vec![],
        enabled: true,
        status: PluginStatus::Running,
        message: None,
        uses_dom: false,
        permissions: vec![],
        programs: vec![],
        account_writes: false,
        grants: vec![],
        settings: None,
        logs: vec![],
        live_reload: false,
        hide_settings: false,
    }
}

fn store_plugin(file: &str, installed: bool) -> StorePlugin {
    StorePlugin {
        file: file.into(),
        url: format!("https://store/{}", file.trim_end_matches(".mjs")),
        name: Some(file.trim_end_matches(".mjs").into()),
        version: Some("2.0".into()),
        description: Some(format!("{file} does things")),
        author: Some("Dev".into()),
        author_url: None,
        author_avatar: None,
        homepage: None,
        hash: None,
        tags: vec![],
        installed,
    }
}

fn store(url: &str, name: &str, plugins: Vec<StorePlugin>) -> StoreInfo {
    StoreInfo {
        url: url.into(),
        name: Some(name.into()),
        description: Some("A store".into()),
        plugins,
        loading: false,
        error: None,
        author: Some("Store Author".into()),
        author_url: None,
        author_avatar: None,
        homepage: None,
    }
}

#[test]
fn a_plugins_settings_tree_renders_natively_and_reports_changes() {
    let ctx = context();
    let node = tree();
    let (update, changes) = frame(&ctx, &node, vec![]);
    assert!(changes.is_empty());
    let switch = find(&update, "Display on pause", Role::CheckBox);
    find(&update, "Reset", Role::Button);
    for text in [
        "Discord",
        "Display on pause",
        "Keep the status while paused",
        "Change by",
        "Status text",
        "Quality",
        "Hello",
    ] {
        assert!(has_text(&update, text), "{text} not drawn");
    }
    let (_, changes) = frame(&ctx, &node, vec![click(switch)]);
    assert_eq!(
        changes,
        vec![("displayOnPause".to_string(), SettingValue::Bool(false))]
    );
    let (update, _) = frame(&ctx, &node, vec![]);
    let reset = find(&update, "Reset", Role::Button);
    let (_, changes) = frame(&ctx, &node, vec![click(reset)]);
    assert_eq!(changes, vec![("reset".to_string(), SettingValue::Click)]);
}

#[test]
fn a_key_capture_field_records_the_next_key_combination() {
    let ctx = context();
    let node = SettingsNode::KeyCapture {
        id: "r.0.1".into(),
        meta: SettingsMeta {
            title: Some("Keybind".into()),
            ..SettingsMeta::default()
        },
        value: "Click to set keybind".into(),
    };
    let (update, _) = frame(&ctx, &node, vec![]);
    let button = find(&update, "Keybind", Role::Button);
    let (_, changes) = frame(&ctx, &node, vec![click(button)]);
    assert!(changes.is_empty());
    let ctrl = egui::Modifiers {
        ctrl: true,
        command: !cfg!(target_os = "macos"),
        ..Default::default()
    };
    let key = egui::Event::Key {
        key: egui::Key::E,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: ctrl,
    };
    let (_, changes) = frame(&ctx, &node, vec![key]);
    assert_eq!(changes.len(), 1, "{changes:?}");
    let SettingValue::Json(v) = &changes[0].1 else {
        panic!("{changes:?}")
    };
    assert_eq!(v["key"], "e");
    assert_eq!(v["code"], "KeyE");
    assert_eq!(v["ctrlKey"], true);
    assert_eq!(v["shiftKey"], false);
    // Capture ended: the next key is not taken.
    let key = egui::Event::Key {
        key: egui::Key::F,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    };
    let (_, changes) = frame(&ctx, &node, vec![key]);
    assert!(changes.is_empty());
}

#[test]
fn a_crashed_settings_panel_offers_retry() {
    let ctx = context();
    let node = SettingsNode::Group {
        meta: SettingsMeta::default(),
        children: vec![SettingsNode::Crashed {
            error: "boom".into(),
        }],
    };
    let (update, _) = frame(&ctx, &node, vec![]);
    assert!(has_text(&update, "Settings crashed: boom"));
    let retry = find(&update, "Retry", Role::Button);
    let (_, changes) = frame(&ctx, &node, vec![click(retry)]);
    assert_eq!(
        changes,
        vec![(SETTINGS_RETRY.to_string(), SettingValue::Click)]
    );
}

#[test]
fn the_tabs_are_lunas_in_lunas_order() {
    let ctx = context();
    let snapshot = LunaSnapshot::default();
    let mut page = PageState::default();
    let (update, _) = page_frame(&ctx, &snapshot, &mut page, vec![]);
    // The tabs sit in one row, left to right, each a paging tab.
    let mut tabs: Vec<(f64, String)> = update
        .nodes
        .iter()
        .filter(|(_, n)| n.role() != Role::Label)
        .filter_map(|(_, n)| Some((top(n).1, n.label()?.to_string())))
        .filter(|(_, l)| ["Plugins", "Plugin Store", "Themes", "Settings"].contains(&l.as_str()))
        .collect();
    tabs.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    let order: Vec<&str> = tabs.iter().map(|(_, l)| l.as_str()).collect();
    assert_eq!(order, ["Plugins", "Plugin Store", "Themes", "Settings"]);
    assert!(has_text(&update, "You have no plugins installed!"));
    let id = update
        .nodes
        .iter()
        .find(|(_, n)| n.label() == Some("Themes") && n.role() != Role::Label)
        .map(|(id, _)| *id)
        .unwrap();
    page_frame(&ctx, &snapshot, &mut page, vec![click(id)]);
    assert_eq!(page.tab, Tab::Themes);
}

#[test]
fn a_plugin_card_has_lunas_header_controls() {
    let ctx = context();
    let mut p = info("VolumeScroll");
    p.settings = Some(tree());
    let mut failed = info("Broken");
    failed.status = PluginStatus::Failed("boom on load".into());
    failed.enabled = false;
    let mut dev = info("Mine");
    dev.dev = Some("C:/dev/Mine".into());
    dev.live_reload = true;
    let snapshot = LunaSnapshot {
        plugins: vec![p, failed, dev],
        ..LunaSnapshot::default()
    };
    let mut page = PageState::default();
    let (update, _) = page_frame(&ctx, &snapshot, &mut page, vec![]);
    for text in [
        "VolumeScroll",
        "1.2.0",
        "Someone",
        "VolumeScroll does things",
        "boom on load",
        "1.2.0 [DEV]",
        "Display on pause",
    ] {
        assert!(
            has_text(&update, text),
            "{text} not drawn: {:?}",
            texts(&update)
        );
    }
    find(
        &update,
        "Enable live reloading (for development)",
        Role::Button,
    );
    find(&update, "Disable live reloading", Role::Button);
    find(&update, "Enable Broken", Role::CheckBox);
    assert_eq!(
        press(
            &ctx,
            &snapshot,
            &mut page,
            "Disable VolumeScroll",
            Role::CheckBox
        ),
        vec![Do::SetEnabled("VolumeScroll".into(), false)]
    );
    let reload = press(&ctx, &snapshot, &mut page, "Reload plugin", Role::Button);
    assert_eq!(reload, vec![Do::Reload("VolumeScroll".into())]);
    let hide = press(&ctx, &snapshot, &mut page, "Hide settings", Role::Button);
    assert_eq!(hide, vec![Do::SetHideSettings("VolumeScroll".into(), true)]);
    let gone = press(&ctx, &snapshot, &mut page, "Uninstall plugin", Role::Button);
    assert_eq!(gone, vec![Do::Uninstall("VolumeScroll".into())]);
    let live = press(
        &ctx,
        &snapshot,
        &mut page,
        "Enable live reloading (for development)",
        Role::Button,
    );
    assert_eq!(live, vec![Do::SetLiveReload("VolumeScroll".into(), true)]);
}

#[test]
fn hidden_settings_are_not_drawn() {
    let ctx = context();
    let mut p = info("VolumeScroll");
    p.settings = Some(tree());
    p.hide_settings = true;
    let snapshot = LunaSnapshot {
        plugins: vec![p],
        ..LunaSnapshot::default()
    };
    let mut page = PageState::default();
    let (update, _) = page_frame(&ctx, &snapshot, &mut page, vec![]);
    assert!(!has_text(&update, "Display on pause"));
    find(&update, "Show settings", Role::Button);
}

#[test]
fn the_store_tab_lists_stores_with_install_tiles() {
    let ctx = context();
    let mut loading = store("https://slow", "Slow", vec![]);
    loading.loading = true;
    let mut broken = store("https://broken", "Broken store", vec![]);
    broken.error = Some("HTTP 404".into());
    let snapshot = LunaSnapshot {
        plugins: vec![info("Lyrics")],
        stores: vec![
            store(
                "https://a",
                "Store A",
                vec![
                    store_plugin("Lyrics.mjs", true),
                    store_plugin("Volume.mjs", false),
                ],
            ),
            loading,
            broken,
        ],
        dev_store: Some(store(
            "http://127.0.0.1:3000",
            "Mine [DEV]",
            vec![store_plugin("Wip.mjs", false)],
        )),
        ..LunaSnapshot::default()
    };
    let mut page = PageState {
        tab: Tab::Store,
        ..PageState::default()
    };
    let (update, _) = page_frame(&ctx, &snapshot, &mut page, vec![]);
    for text in [
        "Install from URL",
        "Search plugins...",
        "Store A",
        "Store Author",
        "Loading store https://slow...",
        "HTTP 404",
        "Mine [DEV]",
        "2.0 [DEV]",
    ] {
        assert!(
            has_text(&update, text),
            "{text} not drawn: {:?}",
            texts(&update)
        );
    }
    // The dev store comes first.
    assert!(pos_of(&update, "Mine [DEV]") < pos_of(&update, "Store A"));
    assert_eq!(
        press(&ctx, &snapshot, &mut page, "Install Volume", Role::Button),
        vec![Do::Install("https://store/Volume".into())]
    );
    assert_eq!(
        press(&ctx, &snapshot, &mut page, "Uninstall Lyrics", Role::Button),
        vec![Do::Uninstall("Lyrics".into())]
    );
    // Search keeps the matching plugins; the stores stay listed.
    page.search = "volume".into();
    let (update, _) = page_frame(&ctx, &snapshot, &mut page, vec![]);
    assert!(find_opt(&update, "Install Volume", Role::Button).is_some());
    assert!(find_opt(&update, "Install Wip", Role::Button).is_none());
    assert!(has_text(&update, "Results"));
    assert!(has_text(&update, "Mine [DEV]"));
    page.search = "nothing like this".into();
    let (update, _) = page_frame(&ctx, &snapshot, &mut page, vec![]);
    assert!(has_text(&update, "No plugins match."));
}

#[test]
fn install_from_url_handles_stores_themes_plugins_and_errors() {
    let locale = Locale::default();
    let mut snapshot = LunaSnapshot {
        stores: vec![store("https://a", "A", vec![])],
        ..LunaSnapshot::default()
    };
    let mut field = InstallField::default();
    let mut out = Vec::new();

    submit_install(
        "https://a/store.json",
        &snapshot,
        &mut field,
        &mut out,
        locale,
        0.0,
    );
    assert_eq!(field.error.as_deref(), Some("Store already added"));
    submit_install(
        "https://b/store.json",
        &snapshot,
        &mut field,
        &mut out,
        locale,
        0.0,
    );
    assert_eq!(out[0], Do::AddStore("https://b".into()));
    assert_eq!(
        field.success.as_ref().map(|s| s.0.as_str()),
        Some("Added store https://b/store.json!")
    );

    out.clear();
    submit_install(
        "https://t/dark.css",
        &snapshot,
        &mut field,
        &mut out,
        locale,
        0.0,
    );
    assert_eq!(out[0], Do::InstallTheme("https://t/dark.css".into()));
    // The row shows at once, before the runtime lists it.
    assert_eq!(
        field.theme_added.as_ref().map(|(_, url, _)| url.as_str()),
        Some("https://t/dark.css")
    );
    snapshot.themes.push(ThemeInfo {
        url: "https://t/dark.css".into(),
        ..ThemeInfo::default()
    });
    // Added again, by the same or another link: the installed one is
    // applied, never a second copy.
    out.clear();
    submit_install(
        "https://t/dark.css?v=2",
        &snapshot,
        &mut field,
        &mut out,
        locale,
        0.0,
    );
    assert_eq!(
        out[0],
        Do::SetThemeEnabled("https://t/dark.css".into(), true)
    );
    assert_eq!(field.error, None);
    snapshot.themes.push(ThemeInfo {
        url: "https://raw.githubusercontent.com/o/r/HEAD/x.css".into(),
        ..ThemeInfo::default()
    });
    out.clear();
    submit_install(
        "https://github.com/O/R/blob/main/x.css",
        &snapshot,
        &mut field,
        &mut out,
        locale,
        0.0,
    );
    assert_eq!(
        out[0],
        Do::SetThemeEnabled(
            "https://raw.githubusercontent.com/o/r/HEAD/x.css".into(),
            true
        )
    );
    // A GitHub file page installs its raw file.
    out.clear();
    submit_install(
        "https://github.com/o/r/blob/main/y.css",
        &snapshot,
        &mut field,
        &mut out,
        locale,
        0.0,
    );
    assert_eq!(
        out[0],
        Do::InstallTheme("https://raw.githubusercontent.com/o/r/main/y.css".into())
    );

    submit_install("not a url", &snapshot, &mut field, &mut out, locale, 0.0);
    assert_eq!(field.error.as_deref(), Some("Invalid URL"));

    out.clear();
    field.value = "https://p/Plugin.mjs".into();
    submit_install(
        "https://p/Plugin.mjs",
        &snapshot,
        &mut field,
        &mut out,
        locale,
        0.0,
    );
    assert_eq!(out, vec![Do::Install("https://p/Plugin.mjs".into())]);
    assert_eq!(field.error, None);
    // Nothing yet.
    resolve_install(&mut field, &snapshot, &mut out, locale, 1.0);
    assert!(field.pending.is_some());
    snapshot.install_outcomes.push(InstallOutcome {
        seq: 1,
        url: "https://p/Plugin".into(),
        result: Ok("Plugin".into()),
    });
    resolve_install(&mut field, &snapshot, &mut out, locale, 1.0);
    assert!(field.pending.is_none());
    assert!(field.value.is_empty());
    assert_eq!(
        field.success.as_ref().map(|s| s.0.as_str()),
        Some("Loaded plugin Plugin!")
    );
    assert_eq!(out.last(), Some(&Do::Toast("Loaded plugin Plugin!".into())));

    // A failed install shows its error in the label.
    submit_install(
        "https://p/Other",
        &snapshot,
        &mut field,
        &mut out,
        locale,
        2.0,
    );
    snapshot.install_outcomes.push(InstallOutcome {
        seq: 2,
        url: "https://p/Other".into(),
        result: Err("HTTP 404".into()),
    });
    resolve_install(&mut field, &snapshot, &mut out, locale, 2.0);
    assert_eq!(field.error.as_deref(), Some("HTTP 404"));

    // Local paths and dev servers go to the dev install.
    out.clear();
    submit_install(
        "C:\\dev\\plugin\\dist",
        &snapshot,
        &mut field,
        &mut out,
        locale,
        3.0,
    );
    submit_install(
        "http://127.0.0.1:3000/Wip.mjs",
        &snapshot,
        &mut field,
        &mut out,
        locale,
        3.0,
    );
    assert_eq!(
        out,
        vec![
            Do::InstallDev("C:\\dev\\plugin\\dist".into()),
            Do::InstallDev("http://127.0.0.1:3000/Wip.mjs".into()),
        ]
    );
}

#[test]
fn the_themes_tab_lists_themes_with_their_manifest() {
    let ctx = context();
    let snapshot = LunaSnapshot {
        themes: vec![ThemeInfo {
            url: "https://t/clear.css".into(),
            enabled: true,
            loading: false,
            error: None,
            name: Some("Clear Coat".into()),
            description: Some("Glassy".into()),
            author: Some("Jelly".into()),
            author_url: None,
            author_avatar: None,
            preview: None,
        }],
        ..LunaSnapshot::default()
    };
    let mut page = PageState {
        tab: Tab::Themes,
        ..PageState::default()
    };
    let (update, _) = page_frame(&ctx, &snapshot, &mut page, vec![]);
    for text in ["Install from URL", "Clear Coat", "Glassy", "Jelly"] {
        assert!(has_text(&update, text), "{text} not drawn");
    }
    assert_eq!(
        press(
            &ctx,
            &snapshot,
            &mut page,
            "Disable Clear Coat",
            Role::CheckBox
        ),
        vec![Do::SetThemeEnabled("https://t/clear.css".into(), false)]
    );
    assert_eq!(
        press(&ctx, &snapshot, &mut page, "Reload theme", Role::Button),
        vec![Do::ReloadTheme("https://t/clear.css".into())]
    );
    assert_eq!(
        press(&ctx, &snapshot, &mut page, "Uninstall theme", Role::Button),
        vec![Do::UninstallTheme("https://t/clear.css".into())]
    );
}

#[test]
fn the_settings_tab_links_the_plugin_guide_and_lists_logs_with_levels_and_plugin_names() {
    use crate::luna::LogLine;
    let ctx = context();
    let line = |level, message: &str| LogLine {
        at: std::time::SystemTime::now(),
        level,
        message: message.into(),
    };
    let mut alpha = info("Alpha");
    alpha.logs = vec![
        line(LogLevel::Warn, "alpha is warned"),
        line(LogLevel::Debug, "alpha debug note"),
    ];
    let mut beta = info("Beta");
    beta.dev = Some("C:/dev/Beta".into());
    beta.logs = vec![line(LogLevel::Error, "beta failed at src/index.ts:3:9")];
    let snapshot = LunaSnapshot {
        plugins: vec![alpha, beta],
        ..LunaSnapshot::default()
    };
    let mut page = PageState {
        tab: Tab::Settings,
        ..PageState::default()
    };
    let (update, _) = page_frame(&ctx, &snapshot, &mut page, vec![]);
    for text in [
        "Plugin development",
        "WARN",
        "ERROR",
        "Alpha",
        "Beta",
        "alpha is warned",
        "beta failed at src/index.ts:3:9",
    ] {
        assert!(has_text(&update, text), "{text} not drawn");
    }
    assert!(
        !has_text(&update, "alpha debug note"),
        "debug lines wait for their switch"
    );
    assert_eq!(
        press(
            &ctx,
            &snapshot,
            &mut page,
            "Making plugins for Tidalfast",
            Role::Link
        ),
        vec![Do::OpenUrl(MAKING_PLUGINS_URL.into())]
    );
    press(
        &ctx,
        &snapshot,
        &mut page,
        "Show debug lines",
        Role::CheckBox,
    );
    let (update, _) = page_frame(&ctx, &snapshot, &mut page, vec![]);
    assert!(has_text(&update, "alpha debug note"));
    assert!(has_text(&update, "DEBUG"));
}

#[test]
fn the_settings_tab_exports_resets_and_switches_dev_logging() {
    let ctx = context();
    let snapshot = LunaSnapshot::default();
    let mut page = PageState {
        tab: Tab::Settings,
        ..PageState::default()
    };
    let (update, _) = page_frame(&ctx, &snapshot, &mut page, vec![]);
    for text in [
        "Thanks for using TidaLuna plugins!",
        crate::luna::LUNA_API_VERSION,
        "Settings Transfer",
        "Feature flags",
        "Luna core plugins",
        "Redux Logging",
        "IPC Logging",
        "Import from TIDAL",
    ] {
        assert!(has_text(&update, text), "{text} not drawn");
    }
    assert_eq!(
        press(&ctx, &snapshot, &mut page, "Export Settings", Role::Button),
        vec![Do::Export(false)]
    );
    press(
        &ctx,
        &snapshot,
        &mut page,
        "Include plugin source code",
        Role::CheckBox,
    );
    assert!(page.include_code);
    assert_eq!(
        press(&ctx, &snapshot, &mut page, "Native IPC", Role::CheckBox),
        vec![Do::SetDevLog(DevLog {
            ipc_native: true,
            ..DevLog::default()
        })]
    );
    // Reset asks first.
    assert!(press(&ctx, &snapshot, &mut page, "Reset", Role::Button).is_empty());
    assert_eq!(page.confirm, Some(Confirm::Reset));
    let (update, _) = page_frame(&ctx, &snapshot, &mut page, vec![]);
    let buttons: Vec<NodeId> = update
        .nodes
        .iter()
        .filter(|(_, n)| n.label() == Some("Reset") && n.role() == Role::Button)
        .map(|(id, _)| *id)
        .collect();
    let (_, out) = page_frame(&ctx, &snapshot, &mut page, vec![click(buttons[0])]);
    assert_eq!(out, vec![Do::ResetAll]);
    // Import asks too, naming the file.
    page.import_path = std::path::Path::new("x")
        .join("tidaluna-settings.json")
        .to_string_lossy()
        .into_owned();
    press(&ctx, &snapshot, &mut page, "Import Settings", Role::Button);
    let (update, _) = page_frame(&ctx, &snapshot, &mut page, vec![]);
    assert!(has_text(
        &update,
        "Import settings from tidaluna-settings.json?"
    ));
    assert_eq!(
        press(&ctx, &snapshot, &mut page, "Import", Role::Button),
        vec![Do::Import(
            PathBuf::from("x").join("tidaluna-settings.json")
        )]
    );
}

#[test]
fn first_open_triggers_a_store_load_only_when_nothing_is_cached() {
    let st = |plugins: Vec<StorePlugin>, loading, error: Option<&str>| {
        let mut s = store("A", "A", plugins);
        s.loading = loading;
        s.error = error.map(str::to_string);
        s
    };
    let snap = |stores| LunaSnapshot {
        stores,
        ..LunaSnapshot::default()
    };
    assert!(should_load_stores(&snap(vec![])));
    assert!(should_load_stores(&snap(vec![st(vec![], false, None)])));
    assert!(should_load_stores(&snap(vec![st(
        vec![],
        false,
        Some("offline")
    )])));
    assert!(!should_load_stores(&snap(vec![st(vec![], true, None)])));
    let cached = st(vec![store_plugin("A.mjs", false)], false, None);
    assert!(!should_load_stores(&snap(vec![cached])));
}

/// Download counts and dates are matched to the plugins the stores list, so
/// the refresh waits for a store to list some: on the frame the page opens
/// nothing is loading yet and no store has plugins (7.10.0 refreshed then,
/// found nothing to match, and showed no counts or dates).
#[test]
fn the_store_index_waits_for_listed_plugins() {
    let st = |plugins: Vec<StorePlugin>, loading| {
        let mut s = store("A", "A", plugins);
        s.loading = loading;
        s
    };
    let snap = |stores| LunaSnapshot {
        stores,
        ..LunaSnapshot::default()
    };
    assert!(!stores_listed(&snap(vec![])));
    assert!(!stores_listed(&snap(vec![st(vec![], false)])));
    assert!(!stores_listed(&snap(vec![st(vec![], true)])));
    let listed = st(vec![store_plugin("A.mjs", false)], false);
    assert!(!stores_listed(&snap(vec![
        listed.clone(),
        st(vec![], true)
    ])));
    assert!(stores_listed(&snap(vec![listed.clone()])));
    assert!(stores_listed(&snap(vec![listed, st(vec![], false)])));
}

#[test]
fn descriptions_lose_their_html() {
    assert_eq!(strip_html("<b>Bold</b> &amp; plain"), "Bold & plain");
    // AltPlay's setting: a `<` in plain text is text, not a tag that
    // swallows the rest of the description.
    assert_eq!(
        strip_html("a higher tier (lossy < lossless < hi-res) than TIDAL"),
        "a higher tier (lossy < lossless < hi-res) than TIDAL"
    );
    assert_eq!(strip_html("<br/>x<!-- c -->y"), "xy");
}

fn names(snapshot: &LunaSnapshot, at: &[(usize, usize)]) -> Vec<String> {
    at.iter()
        .map(|&(s, p)| snapshot.stores[s].plugins[p].name.clone().unwrap())
        .collect()
}

fn press_with(
    ctx: &egui::Context,
    palette: &Palette,
    snapshot: &LunaSnapshot,
    index: &Arc<Index>,
    page: &mut PageState,
    label: &str,
    role: Role,
) -> Vec<Do> {
    let (update, _) = page_frame_with(ctx, palette, snapshot, index, page, vec![]);
    let id = find(&update, label, role);
    page_frame_with(ctx, palette, snapshot, index, page, vec![click(id)]).1
}

#[test]
fn the_store_has_a_new_shelf_then_all_with_sorting() {
    let (snapshot, index) = fixture::sample(NOW);
    let index = Arc::new(index);
    let view = store::view_for_tests(&snapshot, &index, "", store::Sort::Updated, NOW);
    // Newest additions first; no Updated shelf (the sort chip does that).
    assert_eq!(
        names(&snapshot, &view.new),
        ["Radiant Lyrics", "Themer", "Star Ratings"]
    );
    // All: recently updated first, the oldest last.
    let all = names(&snapshot, &view.all);
    assert_eq!(all.len(), 14);
    assert_eq!(all.first().map(String::as_str), Some("Radiant Lyrics"));
    assert_eq!(all.last().map(String::as_str), Some("PersistSettings"));
    let by_name = store::view_for_tests(&snapshot, &index, "", store::Sort::Name, NOW);
    assert_eq!(
        names(&snapshot, &by_name.all).first().map(String::as_str),
        Some("Copy Lyrics")
    );
    let popular = store::view_for_tests(&snapshot, &index, "", store::Sort::Popular, NOW);
    // The best-starred store first, the newest within it.
    assert_eq!(names(&snapshot, &popular.all)[0], "Themer");
    // A search lists matches only, in sort order, with no shelves.
    let found = store::view_for_tests(&snapshot, &index, "lyrics", store::Sort::Updated, NOW);
    assert_eq!(
        names(&snapshot, &found.all),
        ["Radiant Lyrics", "Copy Lyrics"]
    );
    assert!(found.new.is_empty());

    let ctx = context();
    let dark = Palette::dark();
    let mut page = PageState {
        tab: Tab::Store,
        ..PageState::default()
    };
    let (update, _) = page_frame_with(&ctx, &dark, &snapshot, &index, &mut page, vec![]);
    for text in ["New", "All plugins", "Stores"] {
        assert!(
            has_text(&update, text),
            "{text} missing: {:?}",
            texts(&update)
        );
    }
    assert!(pos_of(&update, "New") < pos_of(&update, "All plugins"));
    // The "Recently updated" chip does what an Updated shelf did.
    assert!(!texts(&update).iter().any(|t| t == "Updated"));
    assert!(has_text(&update, "Recently updated"));
    // Sorting by name from the chips.
    press_with(
        &ctx,
        &dark,
        &snapshot,
        &index,
        &mut page,
        "Name",
        Role::Button,
    );
    assert_eq!(page.plugin_sort, store::Sort::Name);
}

#[test]
fn installed_plugins_with_a_newer_build_show_an_update_badge_and_update_all() {
    let (snapshot, index) = fixture::sample(NOW);
    let index = Arc::new(index);
    let ctx = context();
    let dark = Palette::dark();
    let mut page = PageState {
        tab: Tab::Store,
        ..PageState::default()
    };
    let (update, _) = page_frame_with(&ctx, &dark, &snapshot, &index, &mut page, vec![]);
    assert!(has_text(&update, "2 updates available"));
    assert!(has_text(&update, "Update available"));
    let out = press_with(
        &ctx,
        &dark,
        &snapshot,
        &index,
        &mut page,
        "Update DiscordRPC",
        Role::Button,
    );
    assert_eq!(out, vec![Do::Install("https://store-a/DiscordRPC".into())]);
    let out = press_with(
        &ctx,
        &dark,
        &snapshot,
        &index,
        &mut page,
        "Update all",
        Role::Button,
    );
    assert_eq!(
        out,
        vec![Do::UpdateAll(vec![
            "https://store-a/DiscordRPC".into(),
            "https://store-c/Scroll.To.Playing".into()
        ])]
    );
    // The installed list carries the same bar.
    page.tab = Tab::Plugins;
    let (update, _) = page_frame_with(&ctx, &dark, &snapshot, &index, &mut page, vec![]);
    assert!(has_text(&update, "2 updates available"));
    // Without updates there is no bar.
    let mut calm = snapshot.clone();
    for p in &mut calm.plugins {
        p.update_available = false;
    }
    let (update, _) = page_frame_with(&ctx, &dark, &calm, &index, &mut page, vec![]);
    assert!(!has_text(&update, "updates available"));
}

#[test]
fn the_themes_tab_lists_community_themes_with_apply_and_active() {
    let (snapshot, index) = fixture::sample(NOW);
    let index = Arc::new(index);
    let ctx = context();
    let dark = Palette::dark();
    let mut page = PageState {
        tab: Tab::Themes,
        ..PageState::default()
    };
    let (update, _) = page_frame_with(&ctx, &dark, &snapshot, &index, &mut page, vec![]);
    for text in [
        "Applied: Catppuccin Mocha Blue",
        "Installed",
        "All themes",
        "Lunallena",
        "Acercandr0",
        "Cotton Candy Nights",
        "Sugarcoat",
        "Ctrl+Shift+T turns every theme off.",
    ] {
        assert!(
            has_text(&update, text),
            "{text} missing: {:?}",
            texts(&update)
        );
    }
    // Nine themes: one list, each theme once (no New and Updated shelves
    // repeating them).
    let cards = |label: &str| {
        update
            .nodes
            .iter()
            .filter(|(_, n)| n.label() == Some(label) && n.role() == Role::Button)
            .count()
    };
    assert_eq!(cards("Apply Lunallena"), 1);
    assert_eq!(cards("Apply Sugarcoat"), 1);
    let mocha = store_index::CATALOG[3].url();
    let out = press_with(
        &ctx,
        &dark,
        &snapshot,
        &index,
        &mut page,
        "Apply Lunallena",
        Role::Button,
    );
    assert_eq!(
        out,
        vec![Do::ApplyTheme(
            store_index::CATALOG[4].url(),
            vec![mocha.clone()],
            false
        )]
    );
    // An installed but switched-off theme is enabled, not installed again.
    let out = press_with(
        &ctx,
        &dark,
        &snapshot,
        &index,
        &mut page,
        "Apply Meteor Night",
        Role::Button,
    );
    assert_eq!(
        out,
        vec![Do::ApplyTheme(
            store_index::CATALOG[8].url(),
            vec![mocha.clone()],
            true
        )]
    );
    // It shows applied at once, and the other one off, before the runtime
    // has answered: one theme at a time.
    let (update, _) = page_frame_with(&ctx, &dark, &snapshot, &index, &mut page, vec![]);
    assert!(find_opt(&update, "Turn off Meteor Night", Role::Button).is_some());
    assert!(find_opt(&update, "Turn off Catppuccin Mocha Blue", Role::Button).is_none());
    assert!(has_text(&update, "Applied: Meteor Night"));
    // The runtime's word again (as when it never agreed).
    page.install.theme_on = None;
    let out = press_with(
        &ctx,
        &dark,
        &snapshot,
        &index,
        &mut page,
        "Turn off Catppuccin Mocha Blue",
        Role::Button,
    );
    assert_eq!(out, vec![Do::SetThemeEnabled(mocha, false)]);
    let out = press_with(
        &ctx,
        &dark,
        &snapshot,
        &index,
        &mut page,
        "Remove Meteor Night",
        Role::Button,
    );
    assert_eq!(out, vec![Do::UninstallTheme(store_index::CATALOG[8].url())]);
    // Nothing applies by default.
    let none = LunaSnapshot {
        themes: vec![],
        ..snapshot.clone()
    };
    let (update, _) = page_frame_with(&ctx, &dark, &none, &index, &mut page, vec![]);
    assert!(has_text(&update, "No theme is applied."));
    // The search narrows the catalogue.
    page.search = "cotton".into();
    let (update, _) = page_frame_with(&ctx, &dark, &none, &index, &mut page, vec![]);
    assert!(has_text(&update, "Cotton Candy"));
    assert!(!has_text(&update, "Lunallena"));
    assert!(has_text(&update, "Results"));
}

#[test]
fn every_tab_draws_in_both_themes() {
    let (snapshot, index) = fixture::sample(NOW);
    let index = Arc::new(index);
    let ctx = context();
    for palette in [Palette::dark(), Palette::light()] {
        for (tab, text) in [
            (Tab::Plugins, "Total Play Time"),
            (Tab::Store, "All plugins"),
            (Tab::Themes, "All themes"),
            (Tab::Settings, "Settings Transfer"),
        ] {
            let mut page = PageState {
                tab,
                ..PageState::default()
            };
            let (update, _) = page_frame_with(&ctx, &palette, &snapshot, &index, &mut page, vec![]);
            assert!(
                has_text(&update, text),
                "{tab:?} ({}) lacks {text}: {:?}",
                if palette.dark { "dark" } else { "light" },
                texts(&update)
            );
        }
    }
    // While the stores load, the Plugin Store keeps its layout with skeletons.
    let mut loading = LunaSnapshot::default();
    let mut slow = store("https://slow", "Slow", vec![]);
    slow.loading = true;
    loading.stores.push(slow);
    let mut page = PageState {
        tab: Tab::Store,
        ..PageState::default()
    };
    let empty = Arc::new(Index::default());
    let (update, _) = page_frame_with(&ctx, &Palette::light(), &loading, &empty, &mut page, vec![]);
    assert!(has_text(&update, "Loading store https://slow..."));
}

#[test]
fn ages_read_naturally() {
    let l = Locale::default();
    let day = 86400;
    assert_eq!(store::age_label(l, NOW, NOW), "Updated today");
    assert_eq!(store::age_label(l, NOW, NOW - day), "Updated yesterday");
    assert_eq!(
        store::age_label(l, NOW, NOW - 5 * day),
        "Updated 5 days ago"
    );
    assert_eq!(
        store::age_label(l, NOW, NOW - 21 * day),
        "Updated 3 weeks ago"
    );
    assert_eq!(
        store::age_label(l, NOW, NOW - 90 * day),
        "Updated 3 months ago"
    );
    assert_eq!(
        store::age_label(l, NOW, NOW - 800 * day),
        "Updated 2 years ago"
    );
}

/// Programs a plugin may run for good are listed in its Permissions, each
/// with a Revoke button that asks the runtime to take the permission back.
#[test]
fn permissions_list_the_allowed_programs_with_a_revoke_button() {
    let ctx = context();
    let palette = Palette::dark();
    let mut plugin = info("@vmohammad/mpv");
    plugin.permissions = vec![crate::luna::Permission::RunPrograms];
    plugin.programs = vec!["/usr/bin/mpv".into(), "/usr/bin/vlc".into()];
    let mut out = Vec::new();
    let draw = |ctx: &egui::Context, events: Vec<egui::Event>, out: &mut Vec<Do>| {
        run(ctx, events, |ui| {
            extras(ui, &palette, Locale::default(), &plugin, out)
        })
    };
    // The list is folded away until opened.
    let tree = draw(&ctx, vec![], &mut out);
    assert!(!has_text(&tree, "/usr/bin/mpv"));
    let header = [
        Role::Button,
        Role::DisclosureTriangle,
        Role::GenericContainer,
    ]
    .into_iter()
    .find_map(|role| find_opt(&tree, "Permissions", role))
    .expect("the Permissions header");
    draw(&ctx, vec![click(header)], &mut out);
    let tree = draw(&ctx, vec![], &mut out);
    assert!(has_text(
        &tree,
        "Runs programs on your computer (asks you first)"
    ));
    assert!(has_text(&tree, "/usr/bin/mpv") && has_text(&tree, "/usr/bin/vlc"));
    // Revoke the second one.
    let revokes: Vec<NodeId> = tree
        .nodes
        .iter()
        .filter(|(_, n)| n.label() == Some("Revoke") && n.role() == Role::Button)
        .map(|(id, _)| *id)
        .collect();
    assert_eq!(revokes.len(), 2);
    let second = *revokes
        .iter()
        .max_by(|a, b| {
            let y = |id: &NodeId| top(&tree.nodes.iter().find(|(i, _)| i == id).unwrap().1).0;
            y(a).partial_cmp(&y(b)).unwrap()
        })
        .unwrap();
    out.clear();
    draw(&ctx, vec![click(second)], &mut out);
    assert_eq!(
        out,
        vec![Do::RevokeProgram(
            "@vmohammad/mpv".into(),
            "/usr/bin/vlc".into()
        )]
    );
}

/// The other permissions a plugin was allowed for good (the TIDAL sign-in,
/// system-wide shortcuts, a program's pipe, saving TIDAL audio) are listed
/// in its Permissions, each with a Revoke button for exactly that one.
#[test]
fn permissions_list_the_other_grants_with_a_revoke_button_each() {
    use crate::luna::Access;
    let ctx = context();
    let palette = Palette::dark();
    let mut plugin = info("@vmohammad/mpv");
    plugin.permissions = vec![crate::luna::Permission::ProgramPipes];
    let pipe = Access::Pipe {
        path: r"\\.\pipe\mpvserver-1".into(),
    };
    plugin.grants = vec![
        Access::Credentials,
        Access::GlobalShortcuts,
        pipe.clone(),
        Access::SaveAudio,
    ];
    let mut out = Vec::new();
    let draw = |ctx: &egui::Context, events: Vec<egui::Event>, out: &mut Vec<Do>| {
        run(ctx, events, |ui| {
            extras(ui, &palette, Locale::default(), &plugin, out)
        })
    };
    let tree = draw(&ctx, vec![], &mut out);
    let header = [
        Role::Button,
        Role::DisclosureTriangle,
        Role::GenericContainer,
    ]
    .into_iter()
    .find_map(|role| find_opt(&tree, "Permissions", role))
    .expect("the Permissions header");
    draw(&ctx, vec![click(header)], &mut out);
    let tree = draw(&ctx, vec![], &mut out);
    assert!(has_text(
        &tree,
        "Talks to another program over its own pipe or socket (asks you first)"
    ));
    for text in [
        "Uses your TIDAL sign-in",
        "Keyboard shortcuts across your whole computer",
        r"\\.\pipe\mpvserver-1",
        "Saves TIDAL songs as files",
    ] {
        assert!(has_text(&tree, text), "{text}");
    }
    let mut revokes: Vec<(f64, NodeId)> = tree
        .nodes
        .iter()
        .filter(|(_, n)| n.label() == Some("Revoke") && n.role() == Role::Button)
        .map(|(id, n)| (top(n).0, *id))
        .collect();
    revokes.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    assert_eq!(revokes.len(), 4);
    // The third row is the pipe.
    out.clear();
    draw(&ctx, vec![click(revokes[2].1)], &mut out);
    assert_eq!(
        out,
        vec![Do::RevokePermission("@vmohammad/mpv".into(), pipe)]
    );
}

fn listed(url: &str, name: &str, author: &str) -> StorePlugin {
    StorePlugin {
        file: format!("{name}.mjs"),
        url: url.to_string(),
        name: Some(name.to_string()),
        version: Some("1.0.0".into()),
        description: None,
        author: Some(author.to_string()),
        author_url: None,
        author_avatar: None,
        homepage: None,
        hash: Some("h".into()),
        tags: vec![],
        installed: false,
    }
}

fn store_of(url: &str, plugins: Vec<StorePlugin>) -> StoreInfo {
    StoreInfo {
        url: url.to_string(),
        name: None,
        description: None,
        plugins,
        loading: false,
        error: None,
        author: None,
        author_url: None,
        author_avatar: None,
        homepage: None,
    }
}

/// The same plugin listed by several stores, by different addresses, or by a
/// fork under the same name, shows once in every list, from the source the
/// user installed it from, else the author's own store.
#[test]
fn a_plugin_listed_by_several_stores_shows_once() {
    let a = "https://github.com/Inrixia/luna-plugins/releases/download/latest";
    let mirror = "http://github.com/someone/mirror/releases/download/v1/";
    let fork = "https://github.com/forker/luna-plugins/releases/download/latest";
    let own = "https://github.com/CIoudGuy/TidaLuna-Plugins/releases/download/latest";
    let snapshot = |installed: Option<&str>| {
        let mut stores = vec![
            // A mirror first, then the author's store, then a fork, with the
            // same plugin reached through other addresses too.
            store_of(
                mirror,
                vec![
                    listed(
                        &format!("{mirror}Radiant.Lyrics"),
                        "Radiant Lyrics",
                        "Inrixia",
                    ),
                    listed(&format!("{mirror}Solo"), "Solo", "CIoudGuy"),
                ],
            ),
            store_of(
                a,
                vec![
                    listed(&format!("{a}/Radiant.Lyrics"), "Radiant Lyrics", "Inrixia"),
                    listed(
                        &format!("{}/Radiant.Lyrics/", a.to_uppercase()),
                        "radiant lyrics",
                        "Inrixia",
                    ),
                ],
            ),
            store_of(
                fork,
                vec![listed(
                    &format!("{fork}/Radiant.Lyrics"),
                    "Radiant Lyrics",
                    "Inrixia",
                )],
            ),
            store_of(
                own,
                vec![listed(
                    "https://raw.githubusercontent.com/CIoudGuy/TidaLuna-Plugins/main/Solo.mjs",
                    "Solo",
                    "CIoudGuy",
                )],
            ),
        ];
        let mut snap = LunaSnapshot::default();
        if let Some(url) = installed {
            let mut p = info("Radiant Lyrics");
            p.url = url.to_string();
            snap.plugins.push(p);
            for s in &mut stores {
                for p in &mut s.plugins {
                    p.installed = p.name.as_deref() == Some("Radiant Lyrics");
                }
            }
        }
        snap.stores = stores;
        snap
    };
    let index = Index::default();
    for sort in [
        store::Sort::Updated,
        store::Sort::Name,
        store::Sort::Popular,
    ] {
        for query in ["", "lyrics", "solo"] {
            let snap = snapshot(None);
            let view = store::view_for_tests(&snap, &index, query, sort, NOW);
            let all = names(&snap, &view.all);
            let mut sorted: Vec<String> = all.iter().map(|n| n.to_lowercase()).collect();
            sorted.sort();
            sorted.dedup();
            assert_eq!(all.len(), sorted.len(), "{sort:?} {query:?}: {all:?}");
            assert_eq!(
                all.len(),
                if query.is_empty() { 2 } else { 1 },
                "{sort:?} {query:?}: {all:?}"
            );
        }
    }
    // The author's own store wins over the mirror and the fork (Solo: the
    // owner's store, though listed last, is the author's).
    let snap = snapshot(None);
    let view = store::view_for_tests(&snap, &index, "", store::Sort::Name, NOW);
    let url_of = |name: &str| {
        let &(s, p) = view
            .all
            .iter()
            .find(|&&(s, p)| snap.stores[s].plugins[p].name.as_deref() == Some(name))
            .unwrap();
        snap.stores[s].plugins[p].url.clone()
    };
    assert!(
        url_of("Radiant Lyrics").to_lowercase().contains("inrixia"),
        "{}",
        url_of("Radiant Lyrics")
    );
    assert!(url_of("Solo").contains("CIoudGuy"), "{}", url_of("Solo"));
    // What the user installed from wins over the author.
    let fork_url = format!("{fork}/Radiant.Lyrics");
    let snap = snapshot(Some(&fork_url));
    let view = store::view_for_tests(&snap, &index, "lyrics", store::Sort::Name, NOW);
    assert_eq!(view.all.len(), 1);
    let (s, p) = view.all[0];
    assert_eq!(snap.stores[s].plugins[p].url, fork_url);
    // The New shelf never repeats it either.
    let mut dates = std::collections::HashMap::new();
    for st in &snap.stores {
        for p in &st.plugins {
            dates.insert(
                p.url.clone(),
                crate::plugins::store_index::PluginDates {
                    added: Some(NOW - 86400),
                    updated: Some(NOW - 86400),
                    ..crate::plugins::store_index::PluginDates::default()
                },
            );
        }
    }
    let index = Index {
        plugins: dates,
        ..Index::default()
    };
    let view = store::view_for_tests(&snap, &index, "", store::Sort::Updated, NOW);
    for shelf in [&view.new, &view.all] {
        let shelf = names(&snap, shelf);
        assert!(
            shelf.iter().filter(|n| *n == "Radiant Lyrics").count() <= 1,
            "{shelf:?}"
        );
    }
}

const INRIXIA_STORE: &str = "https://github.com/Inrixia/TidaLuna-Plugins/releases/download/latest";

/// Clicking who made a plugin opens where they say they are, or else their
/// GitHub profile when the plugin is theirs: on the store card, on the
/// installed plugin and on the store's own header, as in TidaLuna.
#[test]
fn the_author_opens_their_github_profile() {
    let ctx = context();
    let url = "https://github.com/Foukapik/TidaLuna-Plugins/releases/download/latest";
    let mut listed = store_plugin("Lyrics.mjs", false);
    listed.url = format!("{url}/Lyrics");
    listed.author = Some("Foukapik".into());
    let mut installed = info("Volume");
    installed.url = format!("{url}/Volume");
    installed.author = Some("Foukapik".into());
    let snapshot = LunaSnapshot {
        stores: vec![store(url, "Store A", vec![listed])],
        plugins: vec![installed],
        ..LunaSnapshot::default()
    };
    let profile = vec![Do::OpenUrl("https://github.com/Foukapik".into())];
    for tab in [Tab::Store, Tab::Plugins] {
        let mut page = PageState {
            tab,
            ..PageState::default()
        };
        page_frame(&ctx, &snapshot, &mut page, vec![]);
        assert_eq!(
            press(&ctx, &snapshot, &mut page, "Foukapik", Role::Link),
            profile,
            "{tab:?}"
        );
    }
}

/// CoverTheme is Nick Oates's, in Inrixia's store: its author is not
/// Inrixia, so the name leads nowhere (it opened Inrixia's profile). With a
/// link in its package, the name leads there.
#[test]
fn an_author_other_than_the_stores_owner_is_not_linked_to_the_owner() {
    let ctx = context();
    let mut listed = store_plugin("CoverTheme.mjs", false);
    listed.url = format!("{INRIXIA_STORE}/CoverTheme");
    listed.author = Some("Nick Oates".into());
    let mut installed = info("CoverTheme");
    installed.url = format!("{INRIXIA_STORE}/CoverTheme");
    installed.author = Some("Nick Oates".into());
    let mut snapshot = LunaSnapshot {
        stores: vec![store(INRIXIA_STORE, "Inrixia", vec![listed])],
        plugins: vec![installed],
        ..LunaSnapshot::default()
    };
    for tab in [Tab::Store, Tab::Plugins] {
        let mut page = PageState {
            tab,
            ..PageState::default()
        };
        let (update, _) = page_frame(&ctx, &snapshot, &mut page, vec![]);
        assert!(
            has_text(&update, "Nick Oates"),
            "{tab:?}: the name is shown"
        );
        assert_eq!(
            find_opt(&update, "Nick Oates", Role::Link),
            None,
            "{tab:?}: plain text"
        );
    }
    let nick = "https://github.com/NickOates";
    snapshot.stores[0].plugins[0].author_url = Some(nick.into());
    snapshot.plugins[0].author_url = Some(nick.into());
    for tab in [Tab::Store, Tab::Plugins] {
        let mut page = PageState {
            tab,
            ..PageState::default()
        };
        assert_eq!(
            press(&ctx, &snapshot, &mut page, "Nick Oates", Role::Link),
            vec![Do::OpenUrl(nick.into())],
            "{tab:?}"
        );
    }
}

/// A theme's author on its catalogue card and on the installed theme is a
/// link like a plugin's: to the account that hosts it when that is the
/// author's name, to the link the theme gives, and nowhere else.
#[test]
fn theme_authors_are_links() {
    let ctx = context();
    let mocha = "https://raw.githubusercontent.com/DavidHiFi/MochaTidal/HEAD/MochaTidal.css";
    let cover = "https://raw.githubusercontent.com/Inrixia/TidaLuna-Plugins/HEAD/cover.css";
    let theme = |url: &str, name: &str, author: &str| store_index::CatalogTheme {
        url: url.into(),
        name: name.into(),
        author: author.into(),
        updated: Some(NOW - 86400),
        stars: 5,
        ..store_index::CatalogTheme::default()
    };
    let mut index = Index {
        themes: vec![
            theme(mocha, "Mocha Blue", "DavidHiFi"),
            theme(cover, "Cover", "Nick Oates"),
        ],
        ..Index::default()
    };
    let index_of = |index: &Index| Arc::new(index.clone());
    let snapshot = LunaSnapshot::default();
    let shown = |index: &Index| {
        let mut page = PageState {
            tab: Tab::Themes,
            ..PageState::default()
        };
        page_frame_with(
            &ctx,
            &Palette::dark(),
            &snapshot,
            &index_of(index),
            &mut page,
            vec![],
        )
        .0
    };
    let update = shown(&index);
    assert_eq!(find_opt(&update, "Nick Oates", Role::Link), None);
    let id = find(&update, "DavidHiFi", Role::Link);
    let mut page = PageState {
        tab: Tab::Themes,
        ..PageState::default()
    };
    let clicked = page_frame_with(
        &ctx,
        &Palette::dark(),
        &snapshot,
        &index_of(&index),
        &mut page,
        vec![click(id)],
    )
    .1;
    assert_eq!(
        clicked,
        vec![Do::OpenUrl("https://github.com/DavidHiFi".into())]
    );

    // The theme's own link wins, and is the one a click opens.
    index.themes[1].author_url = Some("https://github.com/NickOates".into());
    let update = shown(&index);
    let id = find(&update, "Nick Oates", Role::Link);
    let clicked = page_frame_with(
        &ctx,
        &Palette::dark(),
        &snapshot,
        &index_of(&index),
        &mut page,
        vec![click(id)],
    )
    .1;
    assert_eq!(
        clicked,
        vec![Do::OpenUrl("https://github.com/NickOates".into())]
    );

    // An installed theme: its CSS names Nick Oates, hosted in Inrixia's repo.
    let snapshot = LunaSnapshot {
        themes: vec![ThemeInfo {
            url: cover.into(),
            author: Some("Nick Oates".into()),
            ..ThemeInfo::default()
        }],
        ..LunaSnapshot::default()
    };
    let mut page = PageState {
        tab: Tab::Themes,
        ..PageState::default()
    };
    let none = Index::default();
    let update = page_frame_with(
        &ctx,
        &Palette::dark(),
        &snapshot,
        &Arc::new(none),
        &mut page,
        vec![],
    )
    .0;
    assert!(has_text(&update, "Nick Oates"));
    assert_eq!(find_opt(&update, "Nick Oates", Role::Link), None);
}

/// Which link and which picture an author gets.
#[test]
fn an_author_gets_their_own_link_and_picture() {
    let source = format!("{INRIXIA_STORE}/CoverTheme");
    let none = AuthorRef::default();
    // Not the owner, nothing given: plain text, the letter.
    assert_eq!(author_ref(Some("Nick Oates"), None, None, &source), none);
    assert_eq!(
        author_ref(None, Some("https://github.com/x"), None, &source),
        none
    );
    // The owner by another spelling: their profile and picture.
    let owner = AuthorRef {
        link: Some("https://github.com/Inrixia".into()),
        avatar: Some("https://github.com/Inrixia.png?size=80".into()),
    };
    for name in ["Inrixia", "inrixia", "@Inrixia", "Inr ixia"] {
        assert_eq!(author_ref(Some(name), None, None, &source), owner, "{name}");
    }
    let dashed = "https://raw.githubusercontent.com/aya-zzz-zzz/cottoncandy/HEAD/c.css";
    assert_eq!(
        author_ref(Some("Aya_ZZZ zzz"), None, None, dashed)
            .link
            .as_deref(),
        Some("https://github.com/aya-zzz-zzz")
    );
    // The link the package gives, and the picture of the account it names.
    let given = author_ref(
        Some("Nick Oates"),
        Some("https://github.com/NickOates"),
        None,
        &source,
    );
    assert_eq!(given.link.as_deref(), Some("https://github.com/NickOates"));
    assert_eq!(
        given.avatar.as_deref(),
        Some("https://github.com/NickOates.png?size=80")
    );
    // The picture the package gives wins.
    let pic = "https://avatars.githubusercontent.com/u/1?v=4";
    let both = author_ref(
        Some("Nick Oates"),
        Some("https://github.com/NickOates"),
        Some(pic),
        &source,
    );
    assert_eq!(both.avatar.as_deref(), Some(pic));
    // A link elsewhere is kept, but names no GitHub account to take a picture of.
    let site = author_ref(
        Some("Nick Oates"),
        Some("https://nick.example/"),
        None,
        &source,
    );
    assert_eq!(site.link.as_deref(), Some("https://nick.example/"));
    assert_eq!(site.avatar, None);
    // A development server belongs to no account.
    assert_eq!(
        author_ref(Some("Inrixia"), None, None, "http://127.0.0.1:3000/x"),
        none
    );
}

/// A package's `author` is a name or `{ name, url, avatarUrl }`; only an
/// `https` link is kept, and a picture only from GitHub.
#[test]
fn a_package_author_is_read_with_its_link_and_picture() {
    use crate::luna::author;
    let kisakay = author::parse(&json!({
        "name": "Kisakay",
        "url": "https://github.com/Kisakay",
        "avatarUrl": "https://github.com/Kisakay.png",
    }));
    assert_eq!(kisakay.name.as_deref(), Some("Kisakay"));
    assert_eq!(kisakay.url.as_deref(), Some("https://github.com/Kisakay"));
    assert_eq!(
        kisakay.avatar.as_deref(),
        Some("https://github.com/Kisakay.png?size=80")
    );
    assert_eq!(
        author::parse(&json!("Someone")).name.as_deref(),
        Some("Someone")
    );
    let odd = author::parse(&json!({
        "name": "X",
        "url": "http://example.com",
        "avatarUrl": "https://tracker.example/pixel.png",
    }));
    assert_eq!((odd.url, odd.avatar), (None, None));
    for bad in [
        "javascript:alert(1)",
        "file:///etc/passwd",
        "https://u:p@github.com/x",
        "not a url",
    ] {
        assert_eq!(author::https_url(bad), None, "{bad}");
    }
    // A size already asked for is left alone; GitHub's own `?v=4` is no size.
    assert_eq!(
        author::avatar_url("https://avatars.githubusercontent.com/u/1?v=4&s=200").as_deref(),
        Some("https://avatars.githubusercontent.com/u/1?v=4&s=200")
    );
    assert_eq!(
        author::avatar_url("https://avatars.githubusercontent.com/u/1?v=4").as_deref(),
        Some("https://avatars.githubusercontent.com/u/1?v=4&s=80")
    );
    assert_eq!(
        author::avatar_url("https://avatars.githubusercontent.com/u/1").as_deref(),
        Some("https://avatars.githubusercontent.com/u/1?s=80")
    );
    assert_eq!(
        author::github_login("https://github.com/Kisakay").as_deref(),
        Some("Kisakay")
    );
    assert_eq!(
        author::github_login("https://github.com/Kisakay/repo"),
        None
    );
    assert_eq!(author::github_login("https://example.com/Kisakay"), None);
}

/// A description cut short on a store card shows in full in a tooltip when
/// the pointer rests on it.
#[test]
fn a_cut_description_shows_in_full_on_hover() {
    let ctx = context();
    ctx.all_styles_mut(|s| s.interaction.tooltip_delay = 0.0);
    let long = "Adds a button to the bottom bar to go to the lyrics tab, \
                with options for where it sits, how it looks, which key opens it, \
                and what happens when no lyrics are found for the playing song.";
    let mut plugin = store_plugin("Lyrics.mjs", false);
    plugin.description = Some(long.into());
    let snapshot = LunaSnapshot {
        stores: vec![store("https://a", "Store A", vec![plugin])],
        ..LunaSnapshot::default()
    };
    let mut page = PageState {
        tab: Tab::Store,
        ..PageState::default()
    };
    let shown_whole = |tree: &TreeUpdate| {
        tree.nodes
            .iter()
            .any(|(_, n)| n.label().or(n.value()) == Some(long))
    };
    let (update, _) = page_frame(&ctx, &snapshot, &mut page, vec![]);
    assert!(!shown_whole(&update), "cut on the card");
    let card = update
        .nodes
        .iter()
        .find(|(_, n)| {
            n.label()
                .or(n.value())
                .is_some_and(|l| l.starts_with("Lyrics,"))
        })
        .and_then(|(_, n)| n.bounds())
        .unwrap_or_else(|| panic!("the card: {:?}", texts(&update)));
    let over = egui::pos2(card.x0 as f32 + 30.0, card.y0 as f32 + 76.0);
    let mut seen = false;
    for _ in 0..4 {
        let (update, _) = page_frame(
            &ctx,
            &snapshot,
            &mut page,
            vec![egui::Event::PointerMoved(over)],
        );
        seen |= shown_whole(&update);
    }
    assert!(seen, "the whole description on hover");
}

/// A card's date and star count beside its Install button: one line
/// when they fit, else one per line, never cut to "129K …" (7.10.1).
#[test]
fn card_facts_go_on_two_lines_rather_than_being_cut() {
    let facts = vec!["Updated 6 months ago".to_string(), "1.2K stars".to_string()];
    let wide = store::fact_lines(&facts, |_| true);
    assert_eq!(wide, ["Updated 6 months ago  ·  1.2K stars"]);
    let narrow = store::fact_lines(&facts, |text| text.len() <= 20);
    assert_eq!(narrow, facts, "each fact whole, on its own line");
    let one = store::fact_lines(&facts[..1], |_| false);
    assert_eq!(one, ["Updated 6 months ago"]);
}

#[test]
fn counts_and_dates_read_compactly_in_the_users_language() {
    let en = Locale::English;
    assert_eq!(store::count_text(en, 12_400), "12.4K");
    assert_eq!(store::count_text(Locale::German, 12_400), "12,4K");
    assert_eq!(store::count_text(Locale::French, 950), "950");
    assert_eq!(store::stars_label(en, 1_250).as_deref(), Some("1.2K stars"));
    assert_eq!(store::stars_label(en, 0), None);
}
/// Adding a theme from the field, frame by frame: its row shows in the
/// very first frame (named from its URL, its preview shimmering), grows in
/// without the list below jumping, and stays put while the runtime lists
/// it, loads it and gives it its own name: nothing flickers away.
#[test]
fn an_added_theme_grows_in_and_fills_in_without_a_jump() {
    let ctx = context();
    let dark = Palette::dark();
    let index = Arc::new(Index::default());
    let mut snapshot = LunaSnapshot::default();
    let mut page = PageState {
        tab: Tab::Themes,
        ..PageState::default()
    };
    let url = "https://example.com/themes/rose-pine.css";
    let frame_at = |t: f64, snapshot: &LunaSnapshot, page: &mut PageState| {
        let mut out = Vec::new();
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000.0, 3000.0),
                )),
                time: Some(t),
                predicted_dt: 1.0 / 60.0,
                ..Default::default()
            },
            |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    out = draw(ui, &dark, Locale::default(), snapshot, &index, NOW, page);
                });
            },
        );
        output.textures_delta.clear();
        (output.platform_output.accesskit_update.unwrap(), out)
    };
    frame_at(0.0, &snapshot, &mut page);
    page.install.value = url.into();
    page.install.changed_at = Some(-1.0);
    let (update, out) = frame_at(1.0 / 60.0, &snapshot, &mut page);
    assert!(out.contains(&Do::InstallTheme(url.into())), "{out:?}");
    let mut t = 2.0 / 60.0;
    let (update2, _) = frame_at(t, &snapshot, &mut page);
    assert!(
        has_text(&update2, "Rose Pine"),
        "the row shows at once: {:?}",
        texts(&update2)
    );
    let _ = update;
    let mut ys = Vec::new();
    for _ in 0..40 {
        t += 1.0 / 60.0;
        let (u, _) = frame_at(t, &snapshot, &mut page);
        ys.push(pos_of(&u, "All themes").0);
    }
    for w in ys.windows(2) {
        assert!(w[1] >= w[0] - 0.5, "the list below never moves up: {ys:?}");
        assert!(w[1] - w[0] < 40.0, "it grows, never jumps: {ys:?}");
    }
    let settled = *ys.last().unwrap();
    assert!(ys[0] < settled - 10.0, "it grows over frames: {ys:?}");
    // The runtime lists it, loading, then loaded with its own name.
    snapshot.themes.push(ThemeInfo {
        url: url.into(),
        enabled: true,
        loading: true,
        ..ThemeInfo::default()
    });
    snapshot.version += 1;
    for _ in 0..5 {
        t += 1.0 / 60.0;
        let (u, _) = frame_at(t, &snapshot, &mut page);
        assert!(has_text(&u, "Rose Pine"));
        assert!(
            (pos_of(&u, "All themes").0 - settled).abs() < 1.0,
            "no jump"
        );
    }
    snapshot.themes[0].loading = false;
    snapshot.themes[0].name = Some("Rose Pine Dawn".into());
    snapshot.version += 1;
    for _ in 0..5 {
        t += 1.0 / 60.0;
        let (u, _) = frame_at(t, &snapshot, &mut page);
        assert!(has_text(&u, "Rose Pine Dawn"));
        assert!(
            (pos_of(&u, "All themes").0 - settled).abs() < 1.0,
            "no jump"
        );
        assert_eq!(
            u.nodes
                .iter()
                .filter(|(_, n)| n.label() == Some("Uninstall theme"))
                .count(),
            1,
            "one row"
        );
    }
}

/// Every card and installed row has a "Rate" button that opens the GitHub
/// repository the plugin or theme is published from, to star it; none for
/// a development server or another host.
#[test]
fn rate_opens_the_repository_on_github() {
    assert_eq!(
        repo_link("https://github.com/Foukapik/TidaLuna-Plugins/releases/download/latest/x"),
        Some("https://github.com/Foukapik/TidaLuna-Plugins".into())
    );
    assert_eq!(
        repo_link("https://raw.githubusercontent.com/pernydev/tidal-catppuccin/HEAD/mocha.css"),
        Some("https://github.com/pernydev/tidal-catppuccin".into())
    );
    assert_eq!(repo_link("https://github.com/owner"), None);
    assert_eq!(repo_link("http://127.0.0.1:3000/x"), None);
    assert_eq!(repo_link("https://example.com/a/b"), None);

    let ctx = context();
    let url = "https://github.com/Foukapik/TidaLuna-Plugins/releases/download/latest";
    let repo = vec![Do::OpenUrl(
        "https://github.com/Foukapik/TidaLuna-Plugins".into(),
    )];
    let mut listed = store_plugin("Lyrics.mjs", false);
    listed.url = format!("{url}/Lyrics");
    let mut installed = info("Volume");
    installed.url = format!("{url}/Volume");
    let snapshot = LunaSnapshot {
        stores: vec![store(url, "Store A", vec![listed])],
        plugins: vec![installed],
        ..LunaSnapshot::default()
    };
    for tab in [Tab::Store, Tab::Plugins] {
        let mut page = PageState {
            tab,
            ..PageState::default()
        };
        page_frame(&ctx, &snapshot, &mut page, vec![]);
        assert_eq!(
            press(&ctx, &snapshot, &mut page, "Rate", Role::Button),
            repo,
            "{tab:?}"
        );
    }

    // A development server has no repository to star.
    let mut dev = store_plugin("Local.mjs", false);
    dev.url = "http://127.0.0.1:3000/Local".into();
    let snapshot = LunaSnapshot {
        stores: vec![store("http://127.0.0.1:3000", "Dev", vec![dev])],
        ..LunaSnapshot::default()
    };
    let mut page = PageState {
        tab: Tab::Store,
        ..PageState::default()
    };
    let (update, _) = page_frame(&ctx, &snapshot, &mut page, vec![]);
    assert!(find_opt(&update, "Rate", Role::Button).is_none());
}

/// The cards show the repository's stars, never download counts, and the
/// Themes tab has no Updated shelf however long the catalogue is.
#[test]
fn cards_show_stars_not_downloads_and_themes_have_no_updated_shelf() {
    let (snapshot, mut index) = fixture::sample(NOW);
    for i in 0..14 {
        index.themes.push(store_index::CatalogTheme {
            url: format!("https://raw.githubusercontent.com/o/t{i}/HEAD/t{i}.css"),
            name: format!("Extra {i}"),
            updated: Some(NOW - 86400),
            stars: 5,
            ..store_index::CatalogTheme::default()
        });
    }
    let index = Arc::new(index);
    let ctx = context();
    let dark = Palette::dark();
    for tab in [Tab::Store, Tab::Themes] {
        let mut page = PageState {
            tab,
            ..PageState::default()
        };
        let (update, _) = page_frame_with(&ctx, &dark, &snapshot, &index, &mut page, vec![]);
        let said = texts(&update);
        assert!(
            said.iter().any(|t| t.contains(" stars")),
            "{tab:?}: {said:?}"
        );
        assert!(
            !said.iter().any(|t| t.to_lowercase().contains("download")),
            "{tab:?}: {said:?}"
        );
        assert!(!said.iter().any(|t| t == "Updated"), "{tab:?}: {said:?}");
    }
}

/// A plugin on the New shelf is also in All plugins: its two cards must not
/// share widget ids (egui's id-clash warning in debug builds, and the second
/// card's author link and hover would answer for the first).
#[test]
fn a_plugin_on_the_new_shelf_and_in_all_has_distinct_widget_ids() {
    let ctx = context();
    let url = "https://github.com/Foukapik/TidaLuna-Plugins/releases/download/latest";
    let mut listed = store_plugin("Lyrics.mjs", false);
    listed.url = format!("{url}/Lyrics");
    listed.author = Some("Foukapik".into());
    let snapshot = LunaSnapshot {
        stores: vec![store(url, "Store A", vec![listed.clone()])],
        ..LunaSnapshot::default()
    };
    let index = Arc::new(Index {
        plugins: std::collections::HashMap::from([(
            listed.url.clone(),
            crate::plugins::store_index::PluginDates {
                added: Some(NOW - 86400),
                updated: Some(NOW - 86400),
                ..crate::plugins::store_index::PluginDates::default()
            },
        )]),
        ..Index::default()
    });
    let mut page = PageState {
        tab: Tab::Store,
        ..PageState::default()
    };
    let (update, _) = page_frame_with(&ctx, &Palette::dark(), &snapshot, &index, &mut page, vec![]);
    let ids: Vec<_> = update
        .nodes
        .iter()
        .filter(|(_, n)| n.label() == Some("Foukapik") && n.role() == Role::Link)
        .map(|(id, _)| *id)
        .collect();
    assert_eq!(
        ids.len(),
        2,
        "on the New shelf and in All: {:?}",
        texts(&update)
    );
    assert_ne!(ids[0], ids[1], "the two cards share an id");
}

/// A plugin's empty text field shows as a box on its card. Its default
/// fill was the card's own colour, with no outline, so AltPlay's empty
/// Jellyfin sign-in fields could not be seen at all. Empty, it shows its
/// title inside, as TidaLuna's does.
#[test]
fn an_empty_text_setting_shows_its_box_on_the_card() {
    for palette in [Palette::dark(), Palette::light()] {
        let ctx = context();
        theme::apply(&ctx, &palette);
        let node = SettingsNode::Text {
            id: "r.0.4".into(),
            meta: SettingsMeta {
                title: Some("Username".into()),
                ..SettingsMeta::default()
            },
            value: String::new(),
            secure: false,
            multiline: None,
            placeholder: None,
        };
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000.0, 400.0),
                )),
                ..Default::default()
            },
            |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    // On a plugin's card, as on the Plugins page.
                    Frame::new().fill(palette.surface).show(ui, |ui| {
                        settings_tree(
                            ui,
                            &palette,
                            Locale::default(),
                            "AltPlay",
                            &node,
                            &mut Vec::new(),
                            &mut Vec::new(),
                        );
                    });
                });
            },
        );
        output.textures_delta.clear();
        let field = output
            .platform_output
            .accesskit_update
            .unwrap()
            .nodes
            .iter()
            .find(|(_, n)| n.role() == Role::TextInput)
            .and_then(|(_, n)| n.bounds())
            .expect("a text field");
        let center = egui::pos2(
            ((field.x0 + field.x1) / 2.0) as f32,
            ((field.y0 + field.y1) / 2.0) as f32,
        );
        let boxes: Vec<Color32> = output
            .shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::Shape::Rect(r) if r.rect.contains(center) && r.rect.height() < 60.0 => {
                    Some(r.fill)
                }
                _ => None,
            })
            .collect();
        assert!(
            boxes
                .iter()
                .any(|fill| fill.a() > 0 && *fill != palette.surface),
            "dark {}: field boxes {boxes:?} on a {:?} card",
            palette.dark,
            palette.surface
        );
        // TidaLuna's empty field shows its title inside (MUI's label).
        let field_rect = egui::Rect::from_min_max(
            egui::pos2(field.x0 as f32, field.y0 as f32),
            egui::pos2(field.x1 as f32, field.y1 as f32),
        );
        assert!(
            output.shapes.iter().any(|c| matches!(
                &c.shape,
                egui::Shape::Text(t) if t.galley.text() == "Username"
                    && field_rect.expand(4.0).contains(t.pos)
            )),
            "the title inside the empty field"
        );
    }
}

/// Every row of a plugin's settings has its line and spacing above it:
/// a text field, number or choice right after a switch had neither
/// (AltPlay's "Jellyfin server URL" sat tight under its last switch).
#[test]
fn every_setting_row_gets_its_spacing() {
    let ctx = context();
    let meta = |title: &str| SettingsMeta {
        title: Some(title.into()),
        ..SettingsMeta::default()
    };
    let switch = |id: &str, title: &str| SettingsNode::Switch {
        id: id.into(),
        meta: meta(title),
        checked: false,
    };
    let node = SettingsNode::Group {
        meta: SettingsMeta::default(),
        children: vec![
            switch("a", "First switch"),
            switch("b", "Second switch"),
            SettingsNode::Text {
                id: "c".into(),
                meta: meta("Server"),
                value: String::new(),
                secure: false,
                multiline: None,
                placeholder: None,
            },
            SettingsNode::Number {
                id: "d".into(),
                meta: meta("Port"),
                value: 1.0,
                min: None,
                max: None,
                step: None,
                slider: None,
            },
            switch("e", "Third switch"),
        ],
    };
    frame(&ctx, &node, vec![]);
    let (update, _) = frame(&ctx, &node, vec![]);
    let y: Vec<f64> = [
        "First switch",
        "Second switch",
        "Server",
        "Port",
        "Third switch",
    ]
    .iter()
    .map(|t| pos_of(&update, t).0)
    .collect();
    let gaps: Vec<f64> = y.windows(2).map(|w| w[1] - w[0]).collect();
    // A row's title, then the row's padding, its line and the next
    // row's padding (16 + 1 + 16) before the next title.
    assert!(
        gaps.iter().all(|gap| *gap > 16.0 + 1.0 + 16.0 + 14.0),
        "row spacing {gaps:?}"
    );
}

/// How soon the window asks to be drawn again after one frame of the page,
/// `None` when it waits for something to happen.
fn repaint_after(index: &Arc<Index>, tab: Tab) -> Option<std::time::Duration> {
    let (snapshot, _) = fixture::sample(NOW);
    let ctx = context();
    let dark = Palette::dark();
    let mut page = PageState {
        tab,
        ..PageState::default()
    };
    // The first frames settle the layout; the one after is the page at rest.
    let mut delay = std::time::Duration::ZERO;
    for _ in 0..4 {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000.0, 3000.0),
                )),
                ..Default::default()
            },
            |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    draw(
                        ui,
                        &dark,
                        Locale::default(),
                        &snapshot,
                        index,
                        NOW,
                        &mut page,
                    );
                });
            },
        );
        output.textures_delta.clear();
        delay = output.viewport_output[&egui::ViewportId::ROOT].repaint_delay;
    }
    (delay < std::time::Duration::from_secs(60)).then_some(delay)
}

/// With no catalogue to show and none on its way (offline, or GitHub's
/// hourly limit spent), the Themes tab asked for a pass every 500 ms for as
/// long as it was open. Only a catalogue being fetched is waited for.
#[test]
fn the_themes_tab_without_a_catalogue_idles_unless_one_is_loading() {
    let none = Arc::new(Index::default());
    assert_eq!(repaint_after(&none, Tab::Themes), None);
    let loading = Arc::new(Index {
        loading: true,
        ..Index::default()
    });
    assert!(repaint_after(&loading, Tab::Themes).is_some());
    let (_, sample) = fixture::sample(NOW);
    assert_eq!(repaint_after(&Arc::new(sample), Tab::Themes), None);
}

/// A `position: fixed` card in a plugin's settings (colorama-lyrics' colour
/// picker) is drawn at the window's centre, as in a browser, and its
/// controls work; it is not a row of the list.
#[test]
fn a_fixed_card_in_a_plugins_settings_is_drawn_over_the_window_centre() {
    let ctx = context();
    let node = SettingsNode::Group {
        meta: SettingsMeta::default(),
        children: vec![
            SettingsNode::Button {
                id: "scrim".into(),
                meta: SettingsMeta {
                    scrim: Some(true),
                    ..SettingsMeta::default()
                },
                label: "Close picker".into(),
            },
            SettingsNode::Group {
                meta: SettingsMeta {
                    overlay: Some(true),
                    ..SettingsMeta::default()
                },
                children: vec![SettingsNode::Button {
                    id: "red".into(),
                    meta: SettingsMeta::default(),
                    label: "Red".into(),
                }],
            },
        ],
    };
    frame(&ctx, &node, vec![]);
    let (update, _) = frame(&ctx, &node, vec![]);
    let red = find(&update, "Red", Role::Button);
    let b = update
        .nodes
        .iter()
        .find(|(id, _)| *id == red)
        .and_then(|(_, n)| n.bounds())
        .unwrap();
    let (cx, cy) = ((b.x0 + b.x1) / 2.0, (b.y0 + b.y1) / 2.0);
    assert!(
        (cx - 500.0).abs() < 200.0 && (cy - 1500.0).abs() < 300.0,
        "the card sits at ({cx}, {cy}), not over the window's centre"
    );
    let (_, changes) = frame(&ctx, &node, vec![click(red)]);
    assert_eq!(changes, vec![("red".to_string(), SettingValue::Click)]);
}

#[test]
fn a_store_card_shows_the_compat_pill_only_for_the_listed_plugin() {
    let mut snapshot = LunaSnapshot::default();
    fixture::add_compat_stores(&mut snapshot);
    let index = Arc::new(Index::default());
    let ctx = context();
    for palette in [Palette::dark(), Palette::light()] {
        let mut page = PageState {
            tab: Tab::Store,
            ..PageState::default()
        };
        let (update, _) = page_frame_with(&ctx, &palette, &snapshot, &index, &mut page, vec![]);
        let all = texts(&update);
        let said = |label: &str| all.iter().filter(|t| t.starts_with(label)).count();
        // discord-utils (needs a permission) and lyrics-everywhere (partly):
        // one pill each, with its reason. The same name from another store,
        // and a plugin that works, have none.
        assert_eq!(said("Needs your permission: "), 1, "{all:?}");
        assert_eq!(said("Not supported: "), 0, "{all:?}");
        assert_eq!(said("Partly supported: "), 1, "{all:?}");
        assert!(has_text(&update, "separate lyrics window does not"));
        assert!(has_text(&update, "sends your TIDAL sign-in"));
        assert!(has_text(&update, "@jxnxsdev/miniplayer"));
        assert!(!all.iter().any(|t| t.contains("mini player is a separate")));
    }
}

#[test]
fn an_installed_plugin_row_shows_the_pill_and_its_reason() {
    let mut snapshot = LunaSnapshot::default();
    fixture::add_compat_stores(&mut snapshot);
    let listed = snapshot.stores[0].plugins[0].clone();
    let works = snapshot.stores[0].plugins[2].clone();
    snapshot.plugins = vec![
        fixture::installed(&listed, false),
        fixture::installed(&works, false),
    ];
    let ctx = context();
    let mut page = PageState::default();
    let (update, _) = page_frame(&ctx, &snapshot, &mut page, vec![]);
    let all = texts(&update);
    assert_eq!(
        all.iter()
            .filter(|t| t.starts_with("Needs your permission: "))
            .count(),
        1,
        "{all:?}"
    );
    // The reason is also written out in the card.
    assert!(
        all.iter()
            .any(|t| t.starts_with("It sends your TIDAL sign-in"))
    );
    assert!(!all.iter().any(|t| t.starts_with("Partly supported")));
}

/// A plugin's large download (an AI model) shows how far it got, with the
/// file's name.
#[test]
fn a_large_download_shows_how_far_it_got() {
    let d = crate::luna::downloads::Progress {
        plugin: "@visiuun/3d-cover".into(),
        url: "https://huggingface.co/onnx-community/depth-anything-v2-base/resolve/main/onnx/model.onnx?download=true".into(),
        received: 45 << 20,
        total: Some(102 << 20),
    };
    assert_eq!(
        download_line(Locale::English, &d),
        "Downloading 45 of 102 MB (model.onnx)"
    );
    let unknown = crate::luna::downloads::Progress { total: None, ..d };
    assert_eq!(
        download_line(Locale::English, &unknown),
        "Downloading 45 MB (model.onnx)"
    );
}
