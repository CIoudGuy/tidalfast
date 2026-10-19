//! Source maps for dev plugins: a stack frame that points into the built
//! `.mjs` (`plugin:Name@3:1:2345`) is shown as the TypeScript file and line
//! it was built from (`plugins/Example/src/index.ts:8:13`), so an error in
//! the log leads to the source.
//!
//! Only the version 3 map esbuild writes next to a plugin is read
//! (`sources`, `mappings`); `sourcesContent` and `names` are not needed.

use std::cell::OnceCell;

/// One mapped position: generated column to an original position.
#[derive(Clone, Copy, Debug)]
struct Segment {
    col: u32,
    source: u32,
    line: u32,
    src_col: u32,
}

#[derive(Debug)]
pub(super) struct SourceMap {
    sources: Vec<String>,
    /// Per generated line (0-based), segments in column order.
    lines: Vec<Vec<Segment>>,
}

/// The largest map read. esbuild's maps hold the sources too, so a plugin
/// with many dependencies can reach a few MB.
pub(super) const MAX_MAP: usize = 32 * 1024 * 1024;

fn vlq(bytes: &mut impl Iterator<Item = u8>) -> Option<i64> {
    let (mut value, mut shift) = (0i64, 0u32);
    loop {
        let b = bytes.next()?;
        let digit = match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        } as i64;
        value += (digit & 31) << shift;
        if digit & 32 == 0 {
            break;
        }
        shift += 5;
        if shift > 30 {
            return None;
        }
    }
    Some(if value & 1 == 1 {
        -(value >> 1)
    } else {
        value >> 1
    })
}

impl SourceMap {
    pub(super) fn parse(text: &str) -> Result<SourceMap, String> {
        let v: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
        if v["version"] != 3 || v.get("sections").is_some() {
            return Err("only version 3 source maps are read".into());
        }
        let sources: Vec<String> = v["sources"]
            .as_array()
            .ok_or("the source map has no sources")?
            .iter()
            .map(|s| s.as_str().unwrap_or_default().to_string())
            .collect();
        let mappings = v["mappings"]
            .as_str()
            .ok_or("the source map has no mappings")?;
        let (mut source, mut line, mut src_col) = (0i64, 0i64, 0i64);
        let mut lines = Vec::new();
        for gen_line in mappings.split(';') {
            let mut col = 0i64;
            let mut segments = Vec::new();
            for seg in gen_line.split(',').filter(|s| !s.is_empty()) {
                let mut bytes = seg.bytes();
                col += vlq(&mut bytes).ok_or("bad source map mappings")?;
                // One field: a generated position with no source.
                let Some(ds) = vlq(&mut bytes) else { continue };
                let dl = vlq(&mut bytes).ok_or("bad source map mappings")?;
                let dc = vlq(&mut bytes).ok_or("bad source map mappings")?;
                source += ds;
                line += dl;
                src_col += dc;
                if col < 0 || source < 0 || line < 0 || src_col < 0 {
                    return Err("bad source map mappings".into());
                }
                segments.push(Segment {
                    col: col as u32,
                    source: source as u32,
                    line: line as u32,
                    src_col: src_col as u32,
                });
            }
            lines.push(segments);
        }
        Ok(SourceMap { sources, lines })
    }

    pub(super) fn source_count(&self) -> usize {
        self.sources
            .iter()
            .filter(|s| !s.contains(':') || s.starts_with("file:"))
            .count()
    }

    /// The original position of generated `line:col` (both 1-based, as the
    /// engine counts): `(file, line, column)`, also 1-based.
    pub(super) fn lookup(&self, line: u32, col: u32) -> Option<(&str, u32, u32)> {
        let segments = self.lines.get(line.checked_sub(1)? as usize)?;
        let col = col.checked_sub(1)?;
        let at = segments.partition_point(|s| s.col <= col);
        let s = segments.get(at.checked_sub(1)?)?;
        let file = self.sources.get(s.source as usize)?;
        Some((file, s.line + 1, s.src_col + 1))
    }

    /// `text` with every `plugin:<name>@<generation>:L:C` position read as
    /// the source file position it was built from. Positions the map does
    /// not cover stay as they are.
    pub(super) fn remap(&self, text: &str, plugin: &str) -> String {
        let prefix = format!("plugin:{}@", urlencoding::encode(plugin));
        let mut out = String::with_capacity(text.len());
        let mut rest = text;
        while let Some(at) = rest.find(&prefix) {
            out.push_str(&rest[..at]);
            let tail = &rest[at + prefix.len()..];
            match frame_position(tail) {
                Some((len, l, c)) => {
                    match self.lookup(l, c) {
                        Some((file, sl, sc)) => {
                            out.push_str(&format!("{}:{sl}:{sc}", display_path(file)))
                        }
                        None => out.push_str(&rest[at..at + prefix.len() + len]),
                    }
                    rest = &tail[len..];
                }
                None => {
                    out.push_str(&prefix);
                    rest = tail;
                }
            }
        }
        out.push_str(rest);
        out
    }
}

/// `<generation>:<line>:<col>` at the start of `s`: its length and the
/// line and column.
fn frame_position(s: &str) -> Option<(usize, u32, u32)> {
    let num = |s: &str| {
        let n = s.bytes().take_while(u8::is_ascii_digit).count();
        (n > 0 && n < 10).then(|| (n, s[..n].parse::<u32>().unwrap_or(0)))
    };
    let (g, _) = num(s)?;
    let s2 = s[g..].strip_prefix(':')?;
    let (l, line) = num(s2)?;
    let s3 = s2[l..].strip_prefix(':')?;
    let (c, col) = num(s3)?;
    Some((g + 1 + l + 1 + c, line, col))
}

/// A source path as a developer reads it: no leading `../`.
fn display_path(file: &str) -> &str {
    let mut f = file;
    while let Some(rest) = f.strip_prefix("../").or_else(|| f.strip_prefix("./")) {
        f = rest;
    }
    f
}

/// A dev plugin's source map, parsed when an error first needs it.
pub(super) struct DevMap {
    text: String,
    parsed: OnceCell<Option<SourceMap>>,
}

impl DevMap {
    pub(super) fn new(text: String) -> DevMap {
        DevMap {
            text,
            parsed: OnceCell::new(),
        }
    }

    pub(super) fn get(&self) -> Option<&SourceMap> {
        self.parsed
            .get_or_init(|| SourceMap::parse(&self.text).ok())
            .as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vlq_values() {
        let mut b = "A".bytes();
        assert_eq!(vlq(&mut b), Some(0));
        let mut b = "C".bytes();
        assert_eq!(vlq(&mut b), Some(1));
        let mut b = "D".bytes();
        assert_eq!(vlq(&mut b), Some(-1));
        let mut b = "gB".bytes();
        assert_eq!(vlq(&mut b), Some(16));
    }

    #[test]
    fn positions_map_to_the_source_and_unmapped_ones_stay() {
        // Generated line 1: col 0 -> a.ts 1:1, col 10 -> a.ts 3:5;
        // line 2: col 0 -> b.ts 7:1.
        let map = SourceMap::parse(
            r#"{"version":3,"sources":["../src/a.ts","../src/b.ts"],"mappings":"AAAA,UAEI;ACIJ"}"#,
        )
        .unwrap();
        assert_eq!(map.lookup(1, 1), Some(("../src/a.ts", 1, 1)));
        assert_eq!(map.lookup(1, 11), Some(("../src/a.ts", 3, 5)));
        assert_eq!(map.lookup(1, 40), Some(("../src/a.ts", 3, 5)));
        assert_eq!(map.lookup(2, 5), Some(("../src/b.ts", 7, 1)));
        assert_eq!(map.lookup(3, 1), None);
        let text = "Error: x\n    at f (plugin:My%20Plugin@2:1:12)\n    at g (plugin:Other@1:1:1)\n    at h (plugin:My%20Plugin@2:9:9)\n";
        let out = map.remap(text, "My Plugin");
        assert_eq!(
            out,
            "Error: x\n    at f (src/a.ts:3:5)\n    at g (plugin:Other@1:1:1)\n    at h (plugin:My%20Plugin@2:9:9)\n"
        );
    }

    #[test]
    fn bad_maps_are_refused() {
        assert!(SourceMap::parse("not json").is_err());
        assert!(SourceMap::parse(r#"{"version":2,"sources":[],"mappings":""}"#).is_err());
        assert!(SourceMap::parse(r#"{"version":3,"sources":[],"mappings":"!!"}"#).is_err());
    }
}
