//! The "Not supported" / "Partly supported" labels (`COMPAT`): each row hits
//! the store entry it names and nothing else.

use super::*;

/// The entries of the ten stores that hold a plugin Tidalfast labels, as
/// their `store.json` and `<file>.json` listed them on 2026-10-16 (one entry,
/// which is not labelled, is left out):
/// (store repository, file, package name).
const STORE_INDEX: &[(&str, &str, &str)] = &[
    ("Inrixia/luna-plugins", "Avatar.mjs", "Avatar"),
    ("Inrixia/luna-plugins", "CoverTheme.mjs", "CoverTheme"),
    (
        "Inrixia/luna-plugins",
        "DesktopConnect.mjs",
        "DesktopConnect",
    ),
    ("Inrixia/luna-plugins", "DiscordRPC.mjs", "DiscordRPC"),
    ("Inrixia/luna-plugins", "LastFM.mjs", "LastFM"),
    ("Inrixia/luna-plugins", "ListenBrainz.mjs", "ListenBrainz"),
    (
        "Inrixia/luna-plugins",
        "NativeFullscreen.mjs",
        "NativeFullscreen",
    ),
    ("Inrixia/luna-plugins", "NoBuffer.mjs", "NoBuffer"),
    (
        "Inrixia/luna-plugins",
        "PersistSettings.mjs",
        "PersistSettings",
    ),
    ("Inrixia/luna-plugins", "RealMAX.mjs", "RealMAX"),
    ("Inrixia/luna-plugins", "Shazam.mjs", "Shazam"),
    ("Inrixia/luna-plugins", "SmallWindow.mjs", "SmallWindow"),
    (
        "Inrixia/luna-plugins",
        "Song Downloader.mjs",
        "Song Downloader",
    ),
    ("Inrixia/luna-plugins", "Themer.mjs", "Themer"),
    ("Inrixia/luna-plugins", "Tidal Tags.mjs", "Tidal Tags"),
    ("Inrixia/luna-plugins", "VolumeScroll.mjs", "VolumeScroll"),
    (
        "jxnxsdev/luna-plugins",
        "jxnxsdev.audiovisualiser.mjs",
        "@jxnxsdev/audiovisualiser",
    ),
    (
        "jxnxsdev/luna-plugins",
        "jxnxsdev.cover-colored-ui.mjs",
        "@jxnxsdev/cover-colored-ui",
    ),
    (
        "jxnxsdev/luna-plugins",
        "jxnxsdev.custom-fonts.mjs",
        "@jxnxsdev/custom-fonts",
    ),
    (
        "jxnxsdev/luna-plugins",
        "jxnxsdev.discord-utils.mjs",
        "@jxnxsdev/discord-utils",
    ),
    (
        "jxnxsdev/luna-plugins",
        "jxnxsdev.lyrics-everywhere.mjs",
        "@jxnxsdev/lyrics-everywhere",
    ),
    (
        "jxnxsdev/luna-plugins",
        "jxnxsdev.miniplayer.mjs",
        "@jxnxsdev/miniplayer",
    ),
    (
        "jxnxsdev/luna-plugins",
        "jxnxsdev.qr-share.mjs",
        "@jxnxsdev/qr-share",
    ),
    (
        "jxnxsdev/luna-plugins",
        "jxnxsdev.spinny-cover.mjs",
        "@jxnxsdev/spinny-cover",
    ),
    (
        "jxnxsdev/luna-plugins",
        "jxnxsdev.streamer-tools.mjs",
        "@jxnxsdev/streamer-tools",
    ),
    (
        "jxnxsdev/luna-plugins",
        "jxnxsdev.total-play-time.mjs",
        "@jxnxsdev/total-play-time",
    ),
    (
        "jxnxsdev/luna-plugins",
        "jxnxsdev.utils.mjs",
        "@jxnxsdev/utils",
    ),
    (
        "jxnxsdev/luna-plugins",
        "jxnxsdev.uwu-ifier.mjs",
        "@jxnxsdev/uwu-ifier",
    ),
    (
        "Aztup/luna-plugins",
        "DeviceDisconnectFix.mjs",
        "DeviceDisconnectFix",
    ),
    ("Aztup/luna-plugins", "MediaKeysFixer.mjs", "MediaKeysFixer"),
    (
        "vMohammad24/luna-plugins",
        "vmohammad.api.mjs",
        "@vmohammad/api",
    ),
    (
        "vMohammad24/luna-plugins",
        "vmohammad.betterFullscreen.mjs",
        "@vmohammad/betterFullscreen",
    ),
    (
        "vMohammad24/luna-plugins",
        "vmohammad.mpv.mjs",
        "@vmohammad/mpv",
    ),
    (
        "vMohammad24/luna-plugins",
        "vmohammad.noTrack.mjs",
        "@vmohammad/noTrack",
    ),
    (
        "vMohammad24/luna-plugins",
        "vmohammad.QoL.mjs",
        "@vmohammad/QoL",
    ),
    (
        "vMohammad24/luna-plugins",
        "vmohammad.quickExclusiveMode.mjs",
        "@vmohammad/quickExclusiveMode",
    ),
    (
        "squadgazzz/luna-plugins",
        "squadgazzz.clear-favorites.mjs",
        "@squadgazzz/clear-favorites",
    ),
    (
        "squadgazzz/luna-plugins",
        "squadgazzz.filtered-queue-fix.mjs",
        "@squadgazzz/filtered-queue-fix",
    ),
    (
        "squadgazzz/luna-plugins",
        "squadgazzz.playlist-indicator.mjs",
        "@squadgazzz/playlist-indicator",
    ),
    (
        "squadgazzz/luna-plugins",
        "squadgazzz.playlist-tools.mjs",
        "@squadgazzz/playlist-tools",
    ),
    (
        "squadgazzz/luna-plugins",
        "squadgazzz.scroll-to-playing.mjs",
        "@squadgazzz/scroll-to-playing",
    ),
    (
        "squadgazzz/luna-plugins",
        "squadgazzz.track-radio.mjs",
        "@squadgazzz/track-radio",
    ),
    ("FireWall-code/TidaLuna-Plugins", "TidalAPI.mjs", "TidalAPI"),
    (
        "FireWall-code/TidaLuna-Plugins",
        "TidalLyricsTranslate.mjs",
        "TidalLyricsTranslate",
    ),
    (
        "FireWall-code/TidaLuna-Plugins",
        "TidalMediaControls.mjs",
        "TidalMediaControls",
    ),
    ("ZakME/ScrobbleFix", "ScrobbleFix.mjs", "ScrobbleFix"),
    (
        "np3ir/songdownloader-store",
        "SongDownloader ElVigilante.mjs",
        "SongDownloader ElVigilante",
    ),
    (
        "Kisakay/luna-plugins",
        "DownloadManager.mjs",
        "DownloadManager",
    ),
    // visiuun's store, as listed on 2026-10-20.
    (
        "visiuun/tidaluna-plugins",
        "visiuun.3d-cover.mjs",
        "@visiuun/3d-cover",
    ),
    (
        "visiuun/tidaluna-plugins",
        "visiuun.quick-queue.mjs",
        "@visiuun/quick-queue",
    ),
    (
        "visiuun/tidaluna-plugins",
        "visiuun.tidal-genres.mjs",
        "@visiuun/tidal-genres",
    ),
];

/// The install URL a store entry has (spaces in a file name become dots).
fn store_url(repo: &str, file: &str) -> String {
    let file = file.trim_end_matches(".mjs").replace(' ', ".");
    format!("https://github.com/{repo}/releases/download/latest/{file}")
}

/// The labels hit exactly the store entries they name and no other entry of
/// those stores: 16 of the 51 (one entry that only needs an outside account is left out of the list).
#[test]
fn labels_match_their_store_entries_and_nothing_else() {
    let mut got: Vec<&str> = STORE_INDEX
        .iter()
        .filter(|(repo, file, name)| compat_note(&store_url(repo, file), Some(name)).is_some())
        .map(|(_, _, name)| *name)
        .collect();
    let mut want: Vec<&str> = COMPAT.iter().map(|r| r.name).collect();
    got.sort_unstable();
    want.sort_unstable();
    assert_eq!(got, want);
    assert_eq!(got.len(), 16);
    // Every row targets an entry of the index, in the repository it names.
    for row in COMPAT {
        assert!(
            STORE_INDEX
                .iter()
                .any(|(repo, _, name)| repo.eq_ignore_ascii_case(row.repo) && *name == row.name),
            "{} is not in {}",
            row.name,
            row.repo
        );
    }
}

#[test]
fn labels_are_keyed_by_store_and_package_name() {
    let jx =
        "https://github.com/jxnxsdev/luna-plugins/releases/download/latest/jxnxsdev.miniplayer";
    let note = compat_note(jx, Some("@jxnxsdev/miniplayer")).unwrap();
    assert_eq!(note.level, Compat::PartlySupported);
    // Any case in the address, and the dev channel of the same repository.
    assert!(
        compat_note(
            "https://GitHub.com/JXNXSDEV/luna-plugins/releases/download/dev/x",
            Some("@jxnxsdev/miniplayer")
        )
        .is_some()
    );
    // The same package name from another store, another package from the
    // same store, or no name: nothing.
    let other = "https://github.com/someone/luna-plugins/releases/download/latest/miniplayer";
    assert!(compat_note(other, Some("@jxnxsdev/miniplayer")).is_none());
    assert!(compat_note(jx, Some("@jxnxsdev/spinny-cover")).is_none());
    assert!(compat_note(jx, None).is_none());
    // A development copy on this computer is never labelled.
    assert!(
        compat_note(
            "http://127.0.0.1:3000/miniplayer",
            Some("@jxnxsdev/miniplayer")
        )
        .is_none()
    );
    // Partly supported: miniplayer and lyrics-everywhere (their windows
    // open and work, without see-through edges), Avatar (no email for Gravatar) and
    // Themer (no Monaco window). 3d-cover's Base model and its fade and
    // spinner work, so it carries no label.
    let partly: Vec<&str> = COMPAT
        .iter()
        .filter(|r| r.note.level == Compat::PartlySupported)
        .map(|r| r.name)
        .collect();
    assert_eq!(
        partly,
        [
            "@jxnxsdev/miniplayer",
            "@jxnxsdev/lyrics-everywhere",
            "Avatar",
            "Themer"
        ]
    );
    assert!(
        compat_note(
            &store_url("visiuun/tidaluna-plugins", "visiuun.3d-cover.mjs"),
            Some("@visiuun/3d-cover")
        )
        .is_none()
    );
}

/// Every reason is one plain sentence with no dash used as punctuation, and
/// the plugins the runtime refuses are in the table as not supported.
#[test]
fn reasons_are_plain_and_cover_the_refused_plugins() {
    for row in COMPAT {
        let why = row.note.reason;
        assert!(why.ends_with('.'), "{why}");
        assert!(!why.contains(['\u{2014}', '\u{2013}']), "{why}");
        assert!(!why.trim_end_matches('.').contains(". "), "{why}");
        assert!(why.len() < 140, "{why}");
    }
    for (name, store) in UNSUPPORTED_STORES {
        let row = COMPAT
            .iter()
            .find(|r| r.name == *name && store.contains(&r.repo.to_ascii_lowercase()))
            .unwrap_or_else(|| panic!("{name} is refused but not labelled"));
        assert_eq!(row.note.level, Compat::NotSupported);
    }
    let mut seen = std::collections::HashSet::new();
    assert!(
        COMPAT
            .iter()
            .all(|r| seen.insert((r.repo.to_ascii_lowercase(), r.name)))
    );
}

/// Each labelled store is one Tidalfast lists by default or the sweep checks.
#[test]
fn labels_name_stores_tidalfast_lists() {
    let listed: Vec<String> = DEFAULT_STORES
        .iter()
        .chain(sweep::EXTRA_STORES)
        .map(|s| format!("{}/", s.to_ascii_lowercase()))
        .collect();
    for row in COMPAT {
        let marker = format!("/{}/", row.repo.to_ascii_lowercase());
        assert!(
            listed.iter().any(|s| s.contains(&marker)),
            "{} is not a listed store",
            row.repo
        );
    }
}

/// The pill's label and every row's reason are translated: each reason has
/// an entry in `reason_in`, and a German reader gets German, not the English
/// key. English stays English.
#[test]
fn labels_and_reasons_are_translated() {
    use crate::i18n::Locale;
    assert_eq!(Compat::NotSupported.label(Locale::English), "Not supported");
    assert_eq!(
        Compat::NotSupported.label(Locale::German),
        "Nicht unterstützt"
    );
    assert_eq!(
        Compat::PartlySupported.label(Locale::German),
        "Teilweise unterstützt"
    );
    assert_eq!(
        Compat::NeedsPermission.label(Locale::English),
        "Needs your permission"
    );
    for row in COMPAT {
        assert_eq!(row.note.reason_in(Locale::English), row.note.reason);
        // The reasons written when these plugins became permissions go into
        // the template and the catalogs together in the next translation
        // pass (tests/localization.rs keeps them in step); until then they
        // show in English.
        if NEW_REASONS.contains(&row.name) {
            continue;
        }
        let german = row.note.reason_in(Locale::German);
        assert_ne!(german, row.note.reason, "{} has no translation", row.name);
    }
}

/// Rows whose reason is newer than the translations.
const NEW_REASONS: &[&str] = &[
    "Avatar",
    "Themer",
    "DownloadManager",
    "Song Downloader",
    "SongDownloader ElVigilante",
    "@jxnxsdev/discord-utils",
    "MediaKeysFixer",
    "@vmohammad/mpv",
    "@jxnxsdev/audiovisualiser",
    "@jxnxsdev/miniplayer",
    "@jxnxsdev/lyrics-everywhere",
];

/// The plugins that run once the user gives them a permission are labelled
/// so, and the ones Tidalfast still refuses stay "Not supported".
#[test]
fn plugins_behind_a_permission_say_so() {
    let level = |name: &str| COMPAT.iter().find(|r| r.name == name).map(|r| r.note.level);
    for name in [
        "Song Downloader",
        "SongDownloader ElVigilante",
        "@jxnxsdev/discord-utils",
        "MediaKeysFixer",
        "@vmohammad/mpv",
        "DownloadManager",
        "@jxnxsdev/audiovisualiser",
    ] {
        assert_eq!(level(name), Some(Compat::NeedsPermission), "{name}");
    }
    for name in [
        "DesktopConnect",
        "TidalMediaControls",
        "ScrobbleFix",
        "@squadgazzz/playlist-indicator",
    ] {
        assert_eq!(level(name), Some(Compat::NotSupported), "{name}");
    }
    // Its window opens since plugin windows came (round 1), without its
    // icons yet.
    assert_eq!(level("@jxnxsdev/miniplayer"), Some(Compat::PartlySupported));
}
