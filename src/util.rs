//! Small helpers shared across the application.

use std::borrow::Cow;

use crate::i18n::{Locale, gettext, ngettext, pgettext};

/// `3:45` for track lengths, `1:02:03` past an hour.
pub fn format_duration_ms(ms: u32) -> String {
    let total = ms / 1000;
    let hours = total / 3600;
    let minutes = (total / 60) % 60;
    let seconds = total % 60;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

/// `2 hr 13 min` for playlist totals, `45 min 12 sec` under an hour.
pub fn format_total_ms(locale: Locale, ms: u64) -> String {
    let total = ms / 1000;
    let hours = total / 3600;
    let minutes = (total / 60) % 60;
    let seconds = total % 60;
    if hours > 0 {
        // Translators: A length of time, abbreviated. Keep {hours} and {minutes}.
        gettext(locale, "{hours} hr {minutes} min")
            .replace("{hours}", &hours.to_string())
            .replace("{minutes}", &minutes.to_string())
    } else if minutes > 0 {
        // Translators: A length of time, abbreviated. Keep {minutes} and {seconds}.
        gettext(locale, "{minutes} min {seconds} sec")
            .replace("{minutes}", &minutes.to_string())
            .replace("{seconds}", &seconds.to_string())
    } else {
        // Translators: A length of time, abbreviated. Keep {seconds}.
        gettext(locale, "{seconds} sec").replace("{seconds}", &seconds.to_string())
    }
}

/// TIDAL's short counts: `950`, `12.3K`, `2M`, `1.4B`.
pub fn format_compact_count(count: u64) -> String {
    let (value, suffix) = match count {
        0..1_000 => return count.to_string(),
        1_000..1_000_000 => (count as f64 / 1e3, "K"),
        1_000_000..1_000_000_000 => (count as f64 / 1e6, "M"),
        _ => (count as f64 / 1e9, "B"),
    };
    let rounded = (value * 10.0).floor() / 10.0;
    if rounded.fract() == 0.0 || rounded >= 100.0 {
        format!("{}{suffix}", rounded.trunc() as u64)
    } else {
        format!("{rounded:.1}{suffix}")
    }
}

pub fn format_count(count: u64) -> String {
    let digits = count.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, character) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(character);
    }
    out
}

/// `Jan 5, 2024` from an ISO-8601 timestamp or a bare date.
pub fn format_date(locale: Locale, iso: &str) -> String {
    let date = iso.get(..10).unwrap_or(iso);
    let mut parts = date.split('-');
    let (Some(year), Some(month)) = (parts.next(), parts.next()) else {
        return iso.to_string();
    };
    let day = parts.next();
    let month_name = match month {
        "01" => pgettext(locale, "month", "Jan"),
        "02" => pgettext(locale, "month", "Feb"),
        "03" => pgettext(locale, "month", "Mar"),
        "04" => pgettext(locale, "month", "Apr"),
        "05" => pgettext(locale, "month", "May"),
        "06" => pgettext(locale, "month", "Jun"),
        "07" => pgettext(locale, "month", "Jul"),
        "08" => pgettext(locale, "month", "Aug"),
        "09" => pgettext(locale, "month", "Sep"),
        "10" => pgettext(locale, "month", "Oct"),
        "11" => pgettext(locale, "month", "Nov"),
        "12" => pgettext(locale, "month", "Dec"),
        _ => return iso.to_string(),
    };
    let dated = match day.and_then(|day| day.trim_start_matches('0').parse::<u8>().ok()) {
        Some(day) => {
            // Translators: A date. {month} is an abbreviated month name; reorder as your language writes dates.
            gettext(locale, "{month} {day}, {year}").replace("{day}", &day.to_string())
        }
        // Translators: A month and year. {month} is an abbreviated month name.
        None => gettext(locale, "{month} {year}").into_owned(),
    };
    dated
        .replace("{month}", &month_name)
        .replace("{year}", year)
}

/// `5 minutes ago` for recent ISO-8601 timestamps, otherwise the usual date.
///
/// Dates are shown relatively for their first 30 days, matching the playlist
/// table's compact, time-aware presentation. `now` is an argument so callers
/// can render against one instant and the boundary behaviour stays testable.
pub fn format_relative_date(locale: Locale, iso: &str, now: jiff::Timestamp) -> String {
    let Ok(added) = iso.parse::<jiff::Timestamp>() else {
        return format_date(locale, iso);
    };
    let seconds = added.duration_until(now).as_secs_f64().floor() as i64;
    if !(0..30 * 24 * 60 * 60).contains(&seconds) {
        return format_date(locale, iso);
    }

    let (count, text) = if seconds < 60 {
        let count = seconds;
        let text = ngettext(
            locale,
            // Translators: How long ago a song was added. Keep {count}.
            "{count} second ago",
            "{count} seconds ago",
            count as u32,
        );
        (count, text)
    } else if seconds < 60 * 60 {
        let count = seconds / 60;
        let text = ngettext(
            locale,
            // Translators: How long ago a song was added. Keep {count}.
            "{count} minute ago",
            "{count} minutes ago",
            count as u32,
        );
        (count, text)
    } else if seconds < 24 * 60 * 60 {
        let count = seconds / (60 * 60);
        let text = ngettext(
            locale,
            // Translators: How long ago a song was added. Keep {count}.
            "{count} hour ago",
            "{count} hours ago",
            count as u32,
        );
        (count, text)
    } else if seconds < 7 * 24 * 60 * 60 {
        let count = seconds / (24 * 60 * 60);
        // Translators: How long ago a song was added. Keep {count}.
        let text = ngettext(locale, "{count} day ago", "{count} days ago", count as u32);
        (count, text)
    } else {
        let count = seconds / (7 * 24 * 60 * 60);
        let text = ngettext(
            locale,
            // Translators: How long ago a song was added. Keep {count}.
            "{count} week ago",
            "{count} weeks ago",
            count as u32,
        );
        (count, text)
    };
    text.replace("{count}", &count.to_string())
}

/// How long until [`format_relative_date`] gives `iso` a different label,
/// or `None` once it is a plain date for good. A label in minutes changes
/// at the next whole minute, one in days at the next whole day, and so on,
/// so a page of old additions needs no frames at all.
pub fn relative_date_changes_in(iso: &str, now: jiff::Timestamp) -> Option<std::time::Duration> {
    const SECOND: i128 = 1000;
    const MINUTE: i128 = 60 * SECOND;
    const HOUR: i128 = 60 * MINUTE;
    const DAY: i128 = 24 * HOUR;
    const WEEK: i128 = 7 * DAY;
    const MONTH: i128 = 30 * DAY;
    let added = iso.parse::<jiff::Timestamp>().ok()?;
    let elapsed = added.duration_until(now).as_millis();
    let next = if elapsed < 0 {
        // Added in the future, by this clock: relative once that comes.
        0
    } else if elapsed >= MONTH {
        return None;
    } else {
        let unit = if elapsed < MINUTE {
            SECOND
        } else if elapsed < HOUR {
            MINUTE
        } else if elapsed < DAY {
            HOUR
        } else if elapsed < WEEK {
            DAY
        } else {
            WEEK
        };
        ((elapsed / unit + 1) * unit).min(MONTH)
    };
    // A little past the boundary, so the frame lands on the new label.
    let wait = next - elapsed + 20;
    Some(std::time::Duration::from_millis(u64::try_from(wait).ok()?))
}

/// Tears the id out of `tidal:track:123` and friends.
pub fn uri_id(uri: &str) -> Option<&str> {
    uri.rsplit(':').next().filter(|id| !id.is_empty())
}

pub fn uri_kind(uri: &str) -> Option<&str> {
    let mut parts = uri.split(':');
    parts.next()?;
    parts.next()
}

/// TIDAL's radio seeded by a song, playlist, album, or artist.
pub fn station_uri(seed: &str) -> Option<String> {
    let kind = uri_kind(seed)?;
    let id = uri_id(seed)?;
    (seed == format!("tidal:{kind}:{id}")
        && matches!(kind, "track" | "playlist" | "album" | "artist"))
    .then(|| format!("tidal:station:{kind}:{id}"))
}

/// The song, playlist, album, or artist a radio station is seeded by.
pub fn station_seed(station: &str) -> Option<String> {
    let seed = format!("tidal:{}", station.strip_prefix("tidal:station:")?);
    station_uri(&seed).is_some().then_some(seed)
}

/// The TIDAL web page for a track, album, artist, playlist, or mix URI.
pub fn open_tidal_url(uri: &str) -> Option<String> {
    let kind = uri_kind(uri)?;
    let id = uri_id(uri)?;
    (uri == format!("tidal:{kind}:{id}")
        && matches!(
            kind,
            "track" | "album" | "artist" | "playlist" | "mix" | "video"
        ))
    .then(|| format!("https://tidal.com/browse/{kind}/{id}"))
}

/// The link Copy link copies for a track, album, artist, playlist, mix or
/// video: `https://tidal.com/<kind>/<id>`. Tracks, albums and artists get
/// TIDAL's "share with anyone" page (`…/<id>/u`), which offers TIDAL and the
/// other services instead of opening TIDAL; playlists, mixes and videos
/// have no such page.
pub fn share_url(uri: &str) -> Option<String> {
    let kind = uri_kind(uri)?;
    let id = uri_id(uri)?;
    if uri != format!("tidal:{kind}:{id}") {
        return None;
    }
    match kind {
        "track" | "album" | "artist" => Some(format!("https://tidal.com/{kind}/{id}/u")),
        "playlist" | "mix" | "video" => Some(format!("https://tidal.com/{kind}/{id}")),
        _ => None,
    }
}

/// The one-time link a playlist owner copies from "Invite as collaborator":
/// the playlist's link with the invite's code.
pub fn invite_url(playlist_id: &str, code: &str) -> String {
    format!("https://tidal.com/playlist/{playlist_id}/invite?code={code}")
}

/// The HTML that embeds TIDAL's player for a track, album, playlist, mix
/// or video, as TIDAL's "Copy embed code" gives it. Artists have none.
/// Only the video's size is confirmed (500 by 328); the other heights are
/// marked unverified in `docs/_reference/tidal-api.md`.
pub fn embed_code(uri: &str) -> Option<String> {
    let kind = uri_kind(uri)?;
    let id = uri_id(uri)?;
    if uri != format!("tidal:{kind}:{id}") {
        return None;
    }
    let (path, height) = match kind {
        "track" => ("tracks", 96),
        "album" => ("albums", 400),
        "playlist" => ("playlists", 400),
        "mix" => ("mixes", 400),
        "video" => ("videos", 328),
        _ => return None,
    };
    Some(format!(
        "<iframe src=\"https://embed.tidal.com/{path}/{id}\" width=\"500\" height=\"{height}\" allow=\"encrypted-media; fullscreen; clipboard-write https://embed.tidal.com; web-share\" sandbox=\"allow-same-origin allow-scripts allow-forms allow-popups allow-popups-to-escape-sandbox\" style=\"color-scheme: light dark\" title=\"TIDAL Embed Player\" />"
    ))
}

/// The menu-bar shape for macOS: the wave and arrow alone. macOS template
/// images use only the alpha channel and paint the shape themselves, black
/// in a light menu bar and white in a dark one.
pub fn tray_template_rgba(size: usize) -> Vec<u8> {
    render_mark(size, true)
}

/// The mark rasterised to pixels: the window icon, the trays and the logo
/// drawn in the app (`theme::logo`) all use this one picture.
///
/// It is the Tidalfast mark of `packaging/icons/tidalfast.svg` at every size:
/// a white wave that curls into an arrow on a black rounded square.
pub fn app_icon_rgba(size: usize) -> Vec<u8> {
    render_mark(size, false)
}

/// The mark's glyph alone, white on transparent, for the logo drawn in the
/// app: `theme::logo` tints it with the text colour, so it follows Dark,
/// Light and a theme's palette instead of sitting on a black square.
pub fn logo_glyph_rgba(size: usize) -> Vec<u8> {
    let mut rgba = render_mark(size, true);
    for pixel in rgba.as_chunks_mut::<4>().0 {
        pixel[..3].fill(255);
    }
    rgba
}

/// The mark's drawing, read from the same SVG the packages ship: a black
/// rounded square, then the white glyph as one path of absolute `M`, `L`,
/// `C` and `Z` commands on a 128-unit square.
const MARK_SVG: &str = include_str!("../packaging/icons/tidalfast.svg");

/// The glyph's outline from `MARK_SVG`.
fn mark_glyph() -> Option<tiny_skia::Path> {
    let data = MARK_SVG.split("<path").nth(1)?.split(" d=\"").nth(1)?;
    let data = data.split('"').next()?;
    let mut builder = tiny_skia::PathBuilder::new();
    let mut command = ' ';
    let mut numbers: Vec<f32> = Vec::new();
    let mut flush = |command: char, numbers: &mut Vec<f32>| {
        let n = numbers.as_slice();
        match (command, n.len()) {
            ('M', 2) => builder.move_to(n[0], n[1]),
            ('L', 2) => builder.line_to(n[0], n[1]),
            ('C', 6) => builder.cubic_to(n[0], n[1], n[2], n[3], n[4], n[5]),
            ('Z', 0) => builder.close(),
            _ => {}
        }
        numbers.clear();
    };
    let mut token = String::new();
    for character in data.chars().chain(std::iter::once(' ')) {
        if character.is_ascii_alphabetic() || character == ' ' {
            if let Ok(number) = token.parse::<f32>() {
                numbers.push(number);
            }
            token.clear();
            if character.is_ascii_alphabetic() {
                flush(command, &mut numbers);
                command = character;
            }
        } else {
            token.push(character);
        }
    }
    flush(command, &mut numbers);
    builder.finish()
}

/// The mark at `size` pixels, as straight-alpha RGBA. `glyph_only` draws
/// the glyph alone, larger and in black, for the menu-bar template.
///
/// Up to 32 pixels the glyph grows,
/// so the wave and its arrow stay readable in a title bar or a tray.
fn render_mark(size: usize, glyph_only: bool) -> Vec<u8> {
    let Some(mut pixmap) = tiny_skia::Pixmap::new(size as u32, size as u32) else {
        return Vec::new();
    };
    let small = size <= 32;
    let scale = size as f32 / 128.0;
    let mut paint = tiny_skia::Paint {
        anti_alias: true,
        ..tiny_skia::Paint::default()
    };
    if !glyph_only {
        let (inset, radius) = (0.0_f32, 29.0_f32);
        if let Some(rect) =
            tiny_skia::Rect::from_xywh(inset, inset, 128.0 - 2.0 * inset, 128.0 - 2.0 * inset)
        {
            let mut builder = tiny_skia::PathBuilder::new();
            let (left, top, right, bottom) = (rect.left(), rect.top(), rect.right(), rect.bottom());
            // Quarter circles by cubic curves: 0.5523 is the circle constant.
            let k = radius * (1.0 - 0.552_284_7);
            builder.move_to(left + radius, top);
            builder.line_to(right - radius, top);
            builder.cubic_to(right - k, top, right, top + k, right, top + radius);
            builder.line_to(right, bottom - radius);
            builder.cubic_to(right, bottom - k, right - k, bottom, right - radius, bottom);
            builder.line_to(left + radius, bottom);
            builder.cubic_to(left + k, bottom, left, bottom - k, left, bottom - radius);
            builder.line_to(left, top + radius);
            builder.cubic_to(left, top + k, left + k, top, left + radius, top);
            builder.close();
            if let Some(path) = builder.finish() {
                paint.set_color_rgba8(0, 0, 0, 255);
                pixmap.fill_path(
                    &path,
                    &paint,
                    tiny_skia::FillRule::Winding,
                    tiny_skia::Transform::from_scale(scale, scale),
                    None,
                );
            }
        }
    }
    if let Some(glyph) = mark_glyph() {
        let grow = if glyph_only {
            1.18
        } else if small {
            1.08
        } else {
            1.0
        };
        let transform = tiny_skia::Transform::from_scale(scale, scale)
            .pre_translate(64.0, 64.0)
            .pre_scale(grow, grow)
            .pre_translate(-64.0, -64.0);
        if glyph_only {
            paint.set_color_rgba8(0, 0, 0, 255);
        } else {
            paint.set_color_rgba8(255, 255, 255, 255);
        }
        pixmap.fill_path(
            &glyph,
            &paint,
            tiny_skia::FillRule::Winding,
            transform,
            None,
        );
    }
    pixmap
        .pixels()
        .iter()
        .flat_map(|pixel| {
            let color = pixel.demultiply();
            [color.red(), color.green(), color.blue(), color.alpha()]
        })
        .collect()
}

pub fn greeting(locale: Locale) -> Cow<'static, str> {
    match local_hour() {
        5..=11 => gettext(locale, "Good morning"),
        12..=17 => gettext(locale, "Good afternoon"),
        _ => gettext(locale, "Good evening"),
    }
}

fn local_hour() -> u8 {
    jiff::Zoned::now().hour() as u8
}

/// Strips the HTML TIDAL embeds in playlist descriptions.
pub fn strip_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_tag = false;
    for character in text.chars() {
        match character {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(character),
            _ => {}
        }
    }
    out.replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#x27;", "'")
        .replace("&#39;", "'")
        .replace("&#x2F;", "/")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
}

/// The temporary files a replacement (fastframe-file) left in `dir`, which
/// a finished or failed one never does.
#[cfg(test)]
pub(crate) fn temporary_files(dir: &std::path::Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(|entry| entry.ok())
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .filter(|name| name.ends_with(".tmp"))
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_counts_keep_one_decimal() {
        assert_eq!(format_compact_count(950), "950");
        assert_eq!(format_compact_count(19_118), "19.1K");
        assert_eq!(format_compact_count(1_951_000), "1.9M");
        assert_eq!(format_compact_count(2_000_000), "2M");
    }

    #[test]
    fn share_links_follow_tidals_forms() {
        for (uri, url) in [
            ("tidal:track:1", "https://tidal.com/track/1/u"),
            ("tidal:album:2", "https://tidal.com/album/2/u"),
            ("tidal:artist:3", "https://tidal.com/artist/3/u"),
            ("tidal:mix:00ab", "https://tidal.com/mix/00ab"),
            ("tidal:video:441699021", "https://tidal.com/video/441699021"),
            ("tidal:playlist:u-1", "https://tidal.com/playlist/u-1"),
        ] {
            assert_eq!(share_url(uri).as_deref(), Some(url));
        }
        assert_eq!(share_url("tidal:station:track:1"), None);
        // A shared page's link opens the item again.
        assert_eq!(
            crate::link::parse("https://tidal.com/album/2/u").as_deref(),
            Some("tidal:album:2")
        );
        assert_eq!(
            invite_url("p-1", "c-2"),
            "https://tidal.com/playlist/p-1/invite?code=c-2"
        );
    }

    #[test]
    fn the_video_embed_snippet_is_tidals() {
        assert_eq!(
            embed_code("tidal:video:441699021").unwrap(),
            "<iframe src=\"https://embed.tidal.com/videos/441699021\" width=\"500\" height=\"328\" allow=\"encrypted-media; fullscreen; clipboard-write https://embed.tidal.com; web-share\" sandbox=\"allow-same-origin allow-scripts allow-forms allow-popups allow-popups-to-escape-sandbox\" style=\"color-scheme: light dark\" title=\"TIDAL Embed Player\" />"
        );
    }

    #[test]
    fn embed_code_points_at_tidals_embed_player() {
        let track = embed_code("tidal:track:123").unwrap();
        assert!(track.contains("https://embed.tidal.com/tracks/123\""));
        assert!(
            embed_code("tidal:mix:00ab")
                .unwrap()
                .contains("embed.tidal.com/mixes/00ab")
        );
        assert_eq!(embed_code("tidal:artist:1"), None);
        assert_eq!(embed_code("tidal:track:1:extra"), None);
    }

    fn pixel(rgba: &[u8], size: usize, x: usize, y: usize) -> [u8; 4] {
        let index = (y * size + x) * 4;
        [
            rgba[index],
            rgba[index + 1],
            rgba[index + 2],
            rgba[index + 3],
        ]
    }

    /// The icon is a white wave and arrow on a black rounded square, and the
    /// tray template keeps the glyph alone.
    #[test]
    fn the_icon_is_a_white_wave_arrow_on_a_black_square() {
        // #given the icon at a dock size and at a tray size
        let (large, small) = (app_icon_rgba(128), app_icon_rgba(32));

        // #then the square is black and opaque inside, with the wave on it
        assert_eq!(pixel(&large, 128, 20, 20), [0, 0, 0, 255]);
        assert_eq!(pixel(&large, 128, 104, 110), [0, 0, 0, 255]);
        assert!(pixel(&large, 128, 40, 66)[0] > 240, "the wave's body");
        assert!(pixel(&small, 32, 16, 4)[3] == 255 && pixel(&small, 32, 4, 16)[0] == 0);

        // #and the corners stay clear
        assert_eq!(pixel(&large, 128, 1, 1)[3], 0);
        assert_eq!(pixel(&small, 32, 0, 0)[3], 0);

        // #and the menu-bar template is the glyph alone
        let template = tray_template_rgba(128);
        assert_eq!(pixel(&template, 128, 20, 20)[3], 0);
        assert_eq!(pixel(&template, 128, 40, 66), [0, 0, 0, 255]);
    }

    /// The logo drawn in the app has no square behind it: the glyph is white
    /// so the text colour can tint it, and everything else is clear.
    #[test]
    fn the_in_app_logo_is_the_glyph_alone() {
        let logo = logo_glyph_rgba(128);
        assert_eq!(pixel(&logo, 128, 20, 20)[3], 0, "no square behind it");
        assert_eq!(pixel(&logo, 128, 104, 110)[3], 0);
        assert_eq!(pixel(&logo, 128, 40, 66), [255, 255, 255, 255]);
    }

    #[test]
    fn radio_stations_map_to_their_seeds_and_back() {
        for kind in ["track", "playlist", "album", "artist"] {
            let seed = format!("tidal:{kind}:77646170");
            let station = station_uri(&seed).expect("a station");
            assert_eq!(station, format!("tidal:station:{kind}:77646170"));
            assert_eq!(station_seed(&station).as_deref(), Some(seed.as_str()));
        }
        for seed in [
            "tidal:mix:abc",
            "tidal:collection",
            "tidal:track:",
            "other:track:abc",
            "track:abc",
        ] {
            assert_eq!(station_uri(seed), None, "{seed}");
        }
        assert_eq!(station_seed("tidal:playlist:abc"), None);
    }

    #[test]
    fn durations() {
        assert_eq!(format_duration_ms(225_000), "3:45");
        assert_eq!(format_duration_ms(3_723_000), "1:02:03");
        assert_eq!(format_total_ms(Locale::English, 7_980_000), "2 hr 13 min");
        assert_eq!(format_total_ms(Locale::English, 2_712_000), "45 min 12 sec");
    }

    #[test]
    fn counts_and_dates() {
        assert_eq!(format_count(1_234_567), "1,234,567");
        assert_eq!(format_count(12), "12");
        assert_eq!(
            format_date(Locale::English, "2024-01-05T10:00:00Z"),
            "Jan 5, 2024"
        );
        assert_eq!(format_date(Locale::English, "2024-03"), "Mar 2024");
        assert_eq!(format_date(Locale::English, "2024"), "2024");
    }

    #[test]
    fn recent_dates_are_relative_for_the_first_month() {
        let now: jiff::Timestamp = "2026-08-31T12:00:00Z".parse().unwrap();
        assert_eq!(
            format_relative_date(Locale::English, "2026-08-31T11:59:30Z", now),
            "30 seconds ago"
        );
        assert_eq!(
            format_relative_date(Locale::English, "2026-08-31T11:59:00Z", now),
            "1 minute ago"
        );
        assert_eq!(
            format_relative_date(Locale::English, "2026-08-31T11:00:00Z", now),
            "1 hour ago"
        );
        assert_eq!(
            format_relative_date(Locale::English, "2026-08-30T12:00:00Z", now),
            "1 day ago"
        );
        assert_eq!(
            format_relative_date(Locale::English, "2026-08-17T12:00:00Z", now),
            "2 weeks ago"
        );
        assert_eq!(
            format_relative_date(Locale::English, "2026-08-01T12:00:00Z", now),
            "Aug 1, 2026"
        );
    }

    #[test]
    fn dates_and_lengths_follow_the_interface_language() {
        let now: jiff::Timestamp = "2026-08-31T12:00:00Z".parse().unwrap();
        assert_eq!(format_date(Locale::Spanish, "2024-01-05"), "5 ene 2024");
        assert_eq!(format_date(Locale::Spanish, "2024-09"), "sept 2024");
        assert_eq!(format_total_ms(Locale::Spanish, 7_980_000), "2 h 13 min");
        for (added, expected) in [
            ("2026-08-31T11:59:59Z", "hace 1 segundo"),
            ("2026-08-31T11:58:00Z", "hace 2 minutos"),
            ("2026-08-30T12:00:00Z", "hace 1 día"),
            ("2026-08-17T12:00:00Z", "hace 2 semanas"),
        ] {
            assert_eq!(format_relative_date(Locale::Spanish, added, now), expected);
        }
        // Each language orders the date its own way.
        assert_eq!(format_date(Locale::Swedish, "2024-01-05"), "5 jan. 2024");
        assert_eq!(format_date(Locale::English, "2024-01-05"), "Jan 5, 2024");
        assert_eq!(
            format_relative_date(Locale::Swedish, "2026-08-30T12:00:00Z", now),
            "för 1 dag sedan"
        );
        assert_eq!(format_date(Locale::Turkish, "2024-01-05"), "5 Oca 2024");
        assert_eq!(format_date(Locale::Turkish, "2024-09"), "Eyl 2024");
        assert_eq!(format_total_ms(Locale::Turkish, 7_980_000), "2 sa 13 dk");
        for (added, expected) in [
            ("2026-08-31T11:59:59Z", "1 saniye önce"),
            ("2026-08-31T11:58:00Z", "2 dakika önce"),
            ("2026-08-30T12:00:00Z", "1 gün önce"),
            ("2026-08-17T12:00:00Z", "2 hafta önce"),
        ] {
            assert_eq!(format_relative_date(Locale::Turkish, added, now), expected);
        }
        assert_eq!(
            format_date(Locale::Ukrainian, "2024-01-05"),
            "5 січ. 2024 р."
        );
        assert_eq!(format_date(Locale::Ukrainian, "2024-09"), "вер. 2024 р.");
        assert_eq!(format_total_ms(Locale::Ukrainian, 7_980_000), "2 год 13 хв");
        for (added, expected) in [
            ("2026-08-31T11:59:59Z", "1 секунду тому"),
            ("2026-08-31T11:58:00Z", "2 хвилини тому"),
            ("2026-08-31T07:00:00Z", "5 годин тому"),
            ("2026-08-30T12:00:00Z", "1 день тому"),
            ("2026-08-17T12:00:00Z", "2 тижні тому"),
        ] {
            assert_eq!(
                format_relative_date(Locale::Ukrainian, added, now),
                expected
            );
        }
    }

    #[test]
    fn relative_dates_ask_for_a_frame_only_when_the_label_changes() {
        let now: jiff::Timestamp = "2026-08-31T12:00:00Z".parse().unwrap();
        let wait = |iso| relative_date_changes_in(iso, now).map(|wait| wait.as_millis());
        assert_eq!(wait("2026-08-31T11:59:30Z"), Some(1_020), "seconds tick");
        assert_eq!(
            wait("2026-08-31T11:55:00Z"),
            Some(60_020),
            "the next minute"
        );
        assert_eq!(wait("2026-08-31T09:30:00Z"), Some(30 * 60_000 + 20));
        assert_eq!(wait("2026-08-29T12:00:00Z"), Some(24 * 3_600_000 + 20));
        // Four weeks and a day: the next change is the plain date at 30 days.
        assert_eq!(wait("2026-08-02T12:00:00Z"), Some(24 * 3_600_000 + 20));
        assert_eq!(wait("2026-07-01T12:00:00Z"), None, "a plain date stays");
        assert_eq!(wait("not-a-date"), None);
    }

    #[test]
    fn relative_dates_fall_back_for_future_and_invalid_timestamps() {
        let now: jiff::Timestamp = "2026-08-31T12:00:00Z".parse().unwrap();
        assert_eq!(
            format_relative_date(Locale::English, "2026-09-01T12:00:00Z", now),
            "Sep 1, 2026"
        );
        assert_eq!(
            format_relative_date(Locale::English, "not-a-date", now),
            "not-a-date"
        );
    }

    #[test]
    fn uris() {
        assert_eq!(uri_id("tidal:track:abc"), Some("abc"));
        assert_eq!(uri_kind("tidal:playlist:x"), Some("playlist"));
        assert_eq!(
            open_tidal_url("tidal:album:z").as_deref(),
            Some("https://tidal.com/browse/album/z")
        );
        assert_eq!(
            open_tidal_url("tidal:mix:0123abc").as_deref(),
            Some("https://tidal.com/browse/mix/0123abc")
        );
        assert_eq!(open_tidal_url("tidal:collection"), None);
    }

    #[test]
    fn html_is_stripped() {
        assert_eq!(
            strip_html("Hi <a href=\"x\">there</a> &amp; you"),
            "Hi there & you"
        );
        assert_eq!(strip_html("ONE&#x2F;TWO&#x2F;THREE"), "ONE/TWO/THREE");
    }
}

/// A size the short way: "820 KB", "48 MB", "1.2 GB".
pub fn format_bytes(bytes: u64) -> String {
    const KB: f64 = 1000.0;
    let value = bytes as f64;
    if value >= KB * KB * KB {
        format!("{:.1} GB", value / (KB * KB * KB))
    } else if value >= KB * KB {
        format!("{:.0} MB", value / (KB * KB))
    } else {
        format!("{:.0} KB", (value / KB).ceil())
    }
}

#[cfg(test)]
mod format_bytes_tests {
    #[test]
    fn sizes_read_the_short_way() {
        assert_eq!(super::format_bytes(0), "0 KB");
        assert_eq!(super::format_bytes(48_200_000), "48 MB");
        assert_eq!(super::format_bytes(1_230_000_000), "1.2 GB");
    }
}
