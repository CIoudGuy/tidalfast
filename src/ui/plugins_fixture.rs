//! A made-up Luna state for the Plugins page: tests and off-screen demo shots
//! (`TIDALFAST_DEMO_LUNA_FIXTURE=1` with `--features demo`) draw the page
//! from it without a runtime or a network.

use super::*;
use crate::plugins::store_index::{CATALOG, CatalogTheme, PluginDates};

const DAY: i64 = 86400;

fn plugin(
    store: &str,
    name: &str,
    author: &str,
    version: &str,
    description: &str,
    installed: bool,
) -> StorePlugin {
    StorePlugin {
        file: format!("{name}.mjs"),
        url: format!("{store}/{}", name.replace(' ', ".")),
        name: Some(name.into()),
        version: Some(version.into()),
        description: Some(description.into()),
        author: Some(author.into()),
        author_url: Some(format!("https://github.com/{author}")),
        author_avatar: None,
        homepage: None,
        hash: Some(format!("h-{name}")),
        tags: vec![],
        installed,
    }
}

pub(super) fn installed(p: &StorePlugin, update: bool) -> PluginInfo {
    PluginInfo {
        name: p.name.clone().unwrap_or_default(),
        url: p.url.clone(),
        version: p.version.clone(),
        description: p.description.clone(),
        author: p.author.clone(),
        author_url: p.author_url.clone(),
        author_avatar: None,
        homepage: Some("https://example.com".into()),
        hash: p.hash.clone(),
        dev: None,
        update_available: update,
        latest_hash: None,
        tags: vec![],
        enabled: true,
        status: PluginStatus::Running,
        message: None,
        uses_dom: false,
        permissions: vec![],
        programs: vec![],
        account_writes: false,
        settings: None,
        logs: vec![],
        live_reload: false,
        hide_settings: true,
    }
}

fn store(url: &str, name: &str, author: &str, plugins: Vec<StorePlugin>) -> StoreInfo {
    StoreInfo {
        url: url.into(),
        name: Some(name.into()),
        description: Some(format!("Plugins by {author}")),
        plugins,
        loading: false,
        error: None,
        author: Some(author.into()),
        author_url: None,
        author_avatar: None,
        homepage: Some("https://example.com".into()),
    }
}

/// Colours for each catalogue theme: background, surface, accents, text.
fn colors_of(name: &str) -> Vec<[u8; 3]> {
    match name {
        "Catppuccin Mocha" => vec![
            [30, 30, 46],
            [24, 24, 37],
            [203, 166, 247],
            [148, 226, 213],
            [205, 214, 244],
        ],
        "Catppuccin Macchiato" => vec![
            [36, 39, 58],
            [30, 32, 48],
            [198, 160, 246],
            [139, 213, 202],
            [202, 211, 245],
        ],
        "Catppuccin Frappe" => vec![
            [48, 52, 70],
            [41, 44, 60],
            [202, 158, 230],
            [129, 200, 190],
            [198, 208, 245],
        ],
        "Catppuccin Mocha Blue" => vec![
            [30, 30, 46],
            [17, 17, 27],
            [137, 180, 250],
            [203, 166, 247],
            [205, 214, 244],
        ],
        "Lunallena" => vec![
            [10, 12, 26],
            [20, 24, 44],
            [214, 196, 255],
            [120, 150, 255],
            [236, 238, 250],
        ],
        "Cotton Candy" => vec![
            [2, 0, 14],
            [34, 18, 54],
            [255, 103, 235],
            [179, 207, 255],
            [253, 184, 255],
        ],
        "Cotton Candy Nights" => vec![
            [8, 4, 28],
            [26, 16, 58],
            [255, 140, 200],
            [140, 120, 255],
            [240, 220, 255],
        ],
        "Sugarcoat" => vec![
            [252, 244, 250],
            [244, 230, 242],
            [233, 96, 168],
            [96, 150, 233],
            [58, 34, 62],
        ],
        "Meteor Night" => vec![
            [3, 6, 18],
            [12, 21, 48],
            [192, 216, 255],
            [96, 130, 220],
            [255, 255, 255],
        ],
        _ => vec![],
    }
}

/// The stores, installed plugins, themes and dates of an imaginary account,
/// as of `now`.
pub(super) fn sample(now: i64) -> (LunaSnapshot, Index) {
    let a = "https://store-a";
    let b = "https://store-b";
    let c = "https://store-c";
    let a_plugins = vec![
        plugin(
            a,
            "DiscordRPC",
            "Inrixia",
            "1.4.0",
            "Show off what you are listening to in your Discord status.",
            true,
        ),
        plugin(
            a,
            "NativeFullscreen",
            "Inrixia",
            "1.1.2",
            "Fullscreen from the keyboard with F11, like a normal app.",
            true,
        ),
        plugin(
            a,
            "Themer",
            "Inrixia",
            "0.9.0",
            "Edit the app's CSS live and keep your own themes.",
            false,
        ),
        plugin(
            a,
            "CoverTheme",
            "Inrixia",
            "1.0.3",
            "Tint the interface with the colours of the playing cover.",
            false,
        ),
        plugin(
            a,
            "RealMAX",
            "Inrixia",
            "2.0.1",
            "Prefer the highest quality version of a track when playing.",
            false,
        ),
        plugin(
            a,
            "PersistSettings",
            "Inrixia",
            "1.0.0",
            "Remember your settings across reinstalls.",
            false,
        ),
    ];
    let b_plugins = vec![
        plugin(
            b,
            "Radiant Lyrics",
            "meowarex",
            "3.2.0",
            "Lyrics with a glowing backdrop, a floating player bar and cover art everywhere.",
            false,
        ),
        plugin(
            b,
            "Element Hider",
            "meowarex",
            "1.3.1",
            "Hide any part of the interface you never use.",
            false,
        ),
        plugin(
            b,
            "Copy Lyrics",
            "meowarex",
            "1.0.2",
            "Copy the lyrics of the current song with one click.",
            false,
        ),
        plugin(
            b,
            "Total Play Time",
            "jxnxsdev",
            "1.0.1",
            "The time left in your queue, next to the player controls.",
            true,
        ),
    ];
    let c_plugins = vec![
        plugin(
            c,
            "Scroll To Playing",
            "squadgazzz",
            "1.2.0",
            "Highlight the playing row and jump back to it.",
            true,
        ),
        plugin(
            c,
            "Star Ratings",
            "FlazeIGuess",
            "0.4.0",
            "Interactive five-star ratings for the songs you love.",
            false,
        ),
        plugin(
            c,
            "No Remix",
            "CIoudGuy",
            "1.5.0",
            "Skip remixes, sped up versions and other edits automatically.",
            false,
        ),
        plugin(
            c,
            "QR Share",
            "jxnxsdev",
            "1.0.0",
            "Share a song with a QR code others can scan.",
            false,
        ),
    ];
    let mut dates = std::collections::HashMap::new();
    let mut give = |ps: &[StorePlugin], list: &[(i64, Option<i64>)], stars: u32| {
        for (p, (updated, added)) in ps.iter().zip(list.iter().cycle()) {
            dates.insert(
                p.url.clone(),
                PluginDates {
                    added: added.map(|d| now - d * DAY),
                    updated: Some(now - updated * DAY),
                    stars,
                },
            );
        }
    };
    give(
        &a_plugins,
        &[
            (2, None),
            (30, None),
            (1, Some(4)),
            (9, None),
            (75, None),
            (400, None),
        ],
        214,
    );
    give(
        &b_plugins,
        &[(1, Some(2)), (6, None), (20, None), (3, None)],
        29,
    );
    give(
        &c_plugins,
        &[(5, None), (12, Some(12)), (3, None), (200, None)],
        9,
    );
    let infos = vec![
        installed(&a_plugins[0], true),
        installed(&a_plugins[1], false),
        installed(&b_plugins[3], false),
        installed(&c_plugins[0], true),
    ];
    let themes = vec![
        ThemeInfo {
            url: CATALOG[3].url(),
            enabled: true,
            loading: false,
            error: None,
            name: Some("Catppuccin Mocha Blue".into()),
            description: Some("Catppuccin Mocha with a blue accent.".into()),
            author: Some("DavidHiFi".into()),
            author_url: None,
            author_avatar: None,
            preview: None,
        },
        ThemeInfo {
            url: CATALOG[8].url(),
            enabled: false,
            loading: false,
            error: None,
            name: Some("Meteor Night".into()),
            description: None,
            author: Some("masterjabberwook".into()),
            author_url: None,
            author_avatar: None,
            preview: None,
        },
    ];
    let snapshot = LunaSnapshot {
        plugins: infos,
        stores: vec![
            store(a, "Inrixia's Luna plugins", "Inrixia", a_plugins),
            store(b, "TidaLuna-Plugins", "meowarex", b_plugins),
            store(c, "Community plugins", "squadgazzz", c_plugins),
        ],
        themes,
        version: 1,
        ..LunaSnapshot::default()
    };
    // The catalogue: the day each was added and last changed.
    let ages = [
        (240, 3),
        (240, 3),
        (240, 3),
        (7, 7),
        (95, 40),
        (150, 5),
        (30, 30),
        (300, 20),
        (45, 40),
    ];
    let catalog = CATALOG
        .iter()
        .zip(ages)
        .enumerate()
        .map(|(i, (e, (added, updated)))| CatalogTheme {
            url: e.url(),
            name: e.name.into(),
            author: e.author.into(),
            author_url: None,
            author_avatar: None,
            description: e.description.into(),
            repo: e.repo.into(),
            colors: colors_of(e.name),
            preview: None,
            dark: e.name != "Sugarcoat",
            added: Some(now - added * DAY),
            updated: Some(now - updated * DAY),
            stars: [8, 8, 8, 3, 13, 5, 2, 1, 4][i % 9],
        })
        .collect();
    let index = Index {
        plugins: dates,
        themes: catalog,
        fetched_at: Some(now),
        loading: false,
    };
    (snapshot, index)
}

/// jxnxsdev's store, with a plugin that is not supported
/// (`@jxnxsdev/discord-utils`), one that is partly supported
/// (`@jxnxsdev/lyrics-everywhere`), one that works, and a copy of
/// `@jxnxsdev/miniplayer` published from some other repository, which is
/// not labelled. Added to a sample for the labels' tests and shots
/// (`TIDALFAST_DEMO_LUNA_COMPAT=1`).
pub(super) fn add_compat_stores(snapshot: &mut LunaSnapshot) {
    let jx = "https://github.com/jxnxsdev/luna-plugins/releases/download/latest";
    let other = "https://github.com/someone-else/luna-plugins/releases/download/latest";
    snapshot.stores.push(store(
        jx,
        "jxnxsdev's Luna plugins",
        "jxnxsdev",
        vec![
            plugin(
                jx,
                "@jxnxsdev/discord-utils",
                "jxnxsdev",
                "1.0.0",
                "Control TIDAL from Discord.",
                false,
            ),
            plugin(
                jx,
                "@jxnxsdev/lyrics-everywhere",
                "jxnxsdev",
                "1.2.0",
                "Shows the current lyric line over the player bar.",
                false,
            ),
            plugin(
                jx,
                "@jxnxsdev/spinny-cover",
                "jxnxsdev",
                "1.0.0",
                "A blurred, spinning cover behind the panels.",
                false,
            ),
        ],
    ));
    snapshot.stores.push(store(
        other,
        "Someone else's plugins",
        "someone-else",
        vec![plugin(
            other,
            "@jxnxsdev/miniplayer",
            "someone-else",
            "1.0.0",
            "A different plugin that happens to share a name.",
            false,
        )],
    ));
    snapshot.version += 1;
}

/// Demo builds: the made-up state in place of the runtime's own.
#[cfg(feature = "demo")]
pub(super) fn demo(now: i64) -> Option<(LunaSnapshot, Index)> {
    std::env::var_os("TIDALFAST_DEMO_LUNA_FIXTURE")?;
    let (mut snapshot, mut index) = sample(now);
    // `TIDALFAST_DEMO_LUNA_COMPAT=1`: only the stores with "Not supported"
    // and "Partly supported" plugins, newly added so that the Store tab
    // shows them first, and an installed copy of a not supported one.
    if std::env::var_os("TIDALFAST_DEMO_LUNA_COMPAT").is_some() {
        snapshot.stores.clear();
        snapshot.plugins.clear();
        add_compat_stores(&mut snapshot);
        for p in snapshot.stores.iter().flat_map(|s| &s.plugins) {
            index.plugins.insert(
                p.url.clone(),
                PluginDates {
                    added: Some(now - 2 * DAY),
                    updated: Some(now - DAY),
                    stars: 3,
                },
            );
        }
        if let Some(p) = snapshot.stores[0].plugins.first().cloned() {
            snapshot.plugins.push(installed(&p, false));
        }
    }
    // `TIDALFAST_DEMO_LUNA_LOGS=1`: a plugin under development with a few
    // log lines, for the Settings tab's developer log.
    if std::env::var_os("TIDALFAST_DEMO_LUNA_LOGS").is_some()
        && let Some(p) = snapshot.plugins.first_mut()
    {
        use crate::luna::{LogLevel, LogLine};
        let at = std::time::SystemTime::now();
        let line = |level, message: &str| LogLine {
            at,
            level,
            message: message.into(),
        };
        p.dev = Some("http://127.0.0.1:3000/luna.example".into());
        p.logs = vec![
            line(
                LogLevel::Info,
                "Source map loaded (2 source files): errors show positions in your sources",
            ),
            line(
                LogLevel::Info,
                "[ExamplePlugin] Hello Ada from the Example plugin!",
            ),
            line(
                LogLevel::Warn,
                "Redux action some/UNKNOWN_ACTION is not an action type TIDAL has, so it did nothing",
            ),
            line(
                LogLevel::Error,
                "Uncaught Error: Example plugin crashed on purpose
    at L (plugins/Example/src/explode.ts:2:44)
    at <anonymous> (plugins/Example/src/index.ts:24:12)",
            ),
        ];
    }
    Some((snapshot, index))
}
