//! The author of a plugin, store or theme as its package or manifest gives
//! it: a plain name, or `{ name, url, avatarUrl }` as TidaLuna packages write
//! it. Only `https` links are kept, and a picture only from GitHub.

use serde_json::Value;

/// An author as a package or manifest names them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Author {
    pub name: Option<String>,
    /// Where the author's name leads (`https` only).
    pub url: Option<String>,
    /// The author's profile picture (GitHub's only).
    pub avatar: Option<String>,
}

/// The `author` of a package: a string, or an object with `name`, `url` and
/// `avatarUrl`.
pub fn parse(v: &Value) -> Author {
    match v {
        Value::String(s) => Author {
            name: Some(s.clone()),
            ..Author::default()
        },
        Value::Object(o) => {
            let text = |k: &str| o.get(k).and_then(Value::as_str);
            Author {
                name: text("name").map(str::to_string),
                url: text("url").and_then(https_url),
                avatar: text("avatarUrl").and_then(avatar_url),
            }
        }
        _ => Author::default(),
    }
}

/// `s` as a link the app may open: `https`, a host, no credentials.
pub fn https_url(s: &str) -> Option<String> {
    let u = url::Url::parse(s.trim()).ok()?;
    (u.scheme() == "https" && u.host_str().is_some() && u.username().is_empty())
        .then(|| u.to_string())
}

/// The size, in pixels, a profile picture is asked for: well above the
/// largest place it is drawn (40 points, on a screen of up to 2x).
const AVATAR_PX: &str = "80";

/// A profile picture link, kept only when it is on GitHub (`github.com` or
/// `avatars.githubusercontent.com`), so a package cannot make the app
/// contact another host. A link without a size (`s` or `size`) is asked for
/// a small one, whatever else its query holds.
pub fn avatar_url(s: &str) -> Option<String> {
    let mut u = url::Url::parse(&https_url(s)?).ok()?;
    let key = match u.host_str()? {
        "github.com" => "size",
        "avatars.githubusercontent.com" => "s",
        _ => return None,
    };
    // GitHub's own links end in `?v=4`, which is not a size: they would be
    // fetched at 460 pixels (280 KB for some) to draw 40 points.
    if !u.query_pairs().any(|(k, _)| k == key) {
        u.query_pairs_mut().append_pair(key, AVATAR_PX);
    }
    Some(u.to_string())
}

/// The picture of a GitHub account by its login.
pub fn github_avatar(login: &str) -> String {
    format!("https://github.com/{login}.png?size={AVATAR_PX}")
}

/// The GitHub login a profile link names (`https://github.com/<login>`).
pub fn github_login(url: &str) -> Option<String> {
    let u = url::Url::parse(url).ok()?;
    if u.scheme() != "https" || u.host_str() != Some("github.com") {
        return None;
    }
    let mut parts = u.path_segments()?;
    let login = parts.next().filter(|l| valid_login(l))?;
    parts.all(str::is_empty).then(|| login.to_string())
}

/// GitHub logins are letters, digits and hyphens.
pub fn valid_login(login: &str) -> bool {
    !login.is_empty()
        && login.len() <= 39
        && login.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// Whether an author's name is the GitHub account `login`: equal ignoring
/// case, spaces, `-`, `_` and `@`.
pub fn is_login(name: &str, login: &str) -> bool {
    let fold = |s: &str| {
        s.chars()
            .filter(|c| !matches!(c, ' ' | '-' | '_' | '@'))
            .flat_map(char::to_lowercase)
            .collect::<String>()
    };
    let name = fold(name);
    !name.is_empty() && name == fold(login)
}
