//! Compiles translations and embeds Windows resources.

/// Embeds `packaging/release-notes/v*.md` for the What's new page.
fn embed_release_notes() {
    use std::fmt::Write as _;
    println!("cargo:rerun-if-changed=packaging/release-notes");
    let mut files: Vec<(String, String)> = std::fs::read_dir("packaging/release-notes")
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_str()?.to_string();
            let version = name.strip_prefix('v')?.strip_suffix(".md")?.to_string();
            Some((version, name))
        })
        .collect();
    files.sort();
    let mut code = String::from("pub const RELEASE_NOTES: &[(&str, &str)] = &[\n");
    for (version, name) in &files {
        // Found from the crate being compiled, so a build folder reused
        // after its first checkout was deleted still finds the notes.
        let _ = writeln!(
            code,
            "    ({version:?}, include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), {:?}))),",
            format!("/packaging/release-notes/{name}")
        );
    }
    code.push_str("];\n");
    // The release dates come from the Flatpak metainfo, which lists every
    // release once with its date.
    let metainfo = "packaging/flatpak/rocks.tidalfast.Tidalfast.metainfo.xml";
    println!("cargo:rerun-if-changed={metainfo}");
    code.push_str("pub const RELEASE_DATES: &[(&str, &str)] = &[\n");
    for line in std::fs::read_to_string(metainfo)
        .unwrap_or_default()
        .lines()
    {
        let attribute = |name: &str| {
            let rest = line.split(&format!("{name}=\"")).nth(1)?;
            Some(rest.split('"').next()?.to_string())
        };
        if line.trim_start().starts_with("<release ")
            && let (Some(version), Some(date)) = (attribute("version"), attribute("date"))
        {
            let _ = writeln!(code, "    ({version:?}, {date:?}),");
        }
    }
    code.push_str("];\n");
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    std::fs::write(out.join("release_notes.rs"), code).expect("write release_notes.rs");
}

fn main() {
    embed_release_notes();
    fastframe_i18n::build::compile_catalogs("assets/i18n");
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rerun-if-changed=packaging/windows/tidalfast.ico");
        let mut resource = winresource::WindowsResource::new();
        resource
            .set_icon("packaging/windows/tidalfast.ico")
            .set("ProductName", "Tidalfast")
            .set("FileDescription", "A fast native TIDAL client");
        if let Err(error) = resource.compile() {
            println!("cargo:warning=Windows resources not embedded: {error}");
        }
    }
}
