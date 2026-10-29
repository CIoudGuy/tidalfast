//! Round 47: what the parity checker's comparisons found. Proved on the
//! demo's pages, with made-up themes (their own values, TIDAL's classes and
//! anchors only).

use std::sync::Arc;

use serde_json::json;

use super::*;

/// A sheet is read by jumping to the next quote, bracket or stop instead
/// of a character at a time (a theme's sheet runs to hundreds of kB). What
/// it reads stays what it was: comments go wherever they are outside a
/// string, a quoted `;`, `{`, `}` or `/*` and an escaped quote are text, a
/// bracketed `;` or `,` belongs to its value or selector, and nested and
/// grouping rules read as before.
#[test]
fn a_sheet_reads_the_same_through_quotes_brackets_and_comments() {
    let css = r#"/* a comment with "quotes" and { braces } */
.a::before { content: "/* not a comment */"; color: red; }
.b[title="x;y{z}"], .b2 { content: 'it\'s; {fine}'; background: url("a(b).png"); }
@media (min-width: 10px) { .c { color: blue; /* inner; } */ } .d:is(.e, .f) { margin: 0 } }
.g { background: url(data:image/svg+xml;utf8,<svg></svg>); color: rgb(1 2 3 / 50%) !important }
.h { padding: 1px; & .i { color: green } .j & { color: lime } }
@keyframes k { from { opacity: 0 } to { opacity: 1 } }
@import url("x;y.css");
.k { content: "\""; width: calc((1px + 2px) * 3) }
;;  .l{color:#abc}  /* unclosed"#;
    let files: Files = Arc::default();
    files.lock().unwrap().insert(
        "/Reader.json".into(),
        json!({ "name": "Reader", "version": "1.0.0", "hash": "r1" })
            .to_string()
            .into_bytes(),
    );
    files.lock().unwrap().insert(
        "/Reader.mjs".into(),
        format!(
            r#"export const unloads = new Set();
const sh = new CSSStyleSheet();
sh.replaceSync({css});
const out = [];
const walk = (rs) => {{ for (const r of rs) {{ out.push([r.type, r.selectorText ?? null, r.cssText]); if (r.cssRules) walk(r.cssRules); }} }};
walk(sh.cssRules);
const d = document.createElement("div");
d.style.cssText = "color: red; content: 'a;b'; background: url(x;y) ; --v: (1;2)";
out.push([...d.style].map((k) => [k, d.style.getPropertyValue(k)]));
console.log("READ " + JSON.stringify(out));
"#,
            css = serde_json::to_string(css).unwrap()
        )
        .into_bytes(),
    );
    let base = serve(files);
    let dir = temp_dir("round47-reader");
    let luna = Luna::start(config(&dir), Arc::new(MockHost::default())).unwrap();
    luna.install(&format!("{base}/Reader.mjs"));
    let s = wait_until(&luna, 30, |s| {
        plugin(s, "Reader").is_some_and(|p| p.logs.iter().any(|l| l.message.starts_with("READ ")))
    });
    luna.shutdown();
    let line = plugin(&s, "Reader")
        .unwrap()
        .logs
        .iter()
        .find(|l| l.message.starts_with("READ "))
        .unwrap()
        .message
        .clone();
    let got: serde_json::Value = serde_json::from_str(&line[5..]).unwrap();
    let want = json!([
        [
            1,
            ".a::before",
            ".a::before { content: \"/* not a comment */\"; color: red; }"
        ],
        [
            1,
            ".b[title=\"x;y{z}\"], .b2",
            ".b[title=\"x;y{z}\"], .b2 { content: 'it\\'s; {fine}'; background: url(\"a(b).png\"); }"
        ],
        [
            4,
            null,
            "@media (min-width: 10px) { .c { color: blue; } .d:is(.e, .f) { margin: 0; } }"
        ],
        [1, ".c", ".c { color: blue; }"],
        [1, ".d:is(.e, .f)", ".d:is(.e, .f) { margin: 0; }"],
        [
            1,
            ".g",
            ".g { background: url(data:image/svg+xml;utf8,<svg></svg>); color: rgba(1, 2, 3, 0.5) !important; }"
        ],
        [1, ".h", ".h { padding: 1px; }"],
        [1, ".h .i", ".h .i { color: green; }"],
        [1, ".j .h", ".j .h { color: lime; }"],
        [
            7,
            null,
            "@keyframes k {  from { opacity: 0 } to { opacity: 1 }  }"
        ],
        [3, null, "@import url(\"x;y.css\");"],
        [
            1,
            ".k",
            ".k { content: \"\\\"\"; width: calc((1px + 2px) * 3); }"
        ],
        [1, ".l", ".l { color: rgb(170, 187, 204); }"],
        [
            ["color", "red"],
            ["content", "'a;b'"],
            ["background", "url(x;y)"],
            ["--v", "(1;2)"]
        ]
    ]);
    assert_eq!(got, want);
}
