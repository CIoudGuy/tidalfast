import importlib.util
from pathlib import Path
import sys
import unittest

sys.dont_write_bytecode = True


def load(name):
    spec = importlib.util.spec_from_file_location(name.replace("-", "_"), Path(__file__).with_name(f"{name}.py"))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


sources = load("check-dependency-sources")
evidence = load("check-review-evidence")

LOCK = """
version = 4

[[package]]
name = "serde"
version = "1.0.0"
source = "registry+https://github.com/rust-lang/crates.io-index"

[[package]]
name = "examplecrate-core"
version = "0.8.0"
source = "{source}"
"""
OWN_LOCK_SOURCE = "git+https://github.com/crmne/examplecrate?rev=23fc42c#23fc42cff37848e51f0d8eaefad1a93941b71e59"


def flagged(found):
    return [path for path, _message in found]


class DependencySourcesTest(unittest.TestCase):
    def test_crates_io_and_crmne_repositories_pass(self):
        manifest = """
[dependencies]
serde = "1"
fastframe-text = { git = "https://github.com/crmne/fastframe", tag = "v0.5.1" }

[target.'cfg(windows)'.dependencies.fastframe-tray]
git = "https://github.com/crmne/fastframe.git"

[patch.crates-io]
examplecrate-core = { git = "https://github.com/crmne/examplecrate", rev = "23fc42c" }
"""
        found = sources.problems({"Cargo.toml": manifest, "Cargo.lock": LOCK.format(source=OWN_LOCK_SOURCE)})
        self.assertEqual(found, [])

    def test_a_contributor_fork_fails_in_the_manifest_and_the_lockfile(self):
        manifest = '[patch.crates-io]\nexamplecrate-core = { git = "https://github.com/dennisgr7/examplecrate", rev = "abc" }\n'
        lock = LOCK.format(source="git+https://github.com/dennisgr7/examplecrate?rev=abc#abc")
        found = sources.problems({"Cargo.toml": manifest, "Cargo.lock": lock})
        self.assertEqual(flagged(found), ["Cargo.lock", "Cargo.toml"])
        self.assertIn("dennisgr7/examplecrate", found[1][1])

    def test_lookalike_owners_and_paths_out_of_crmne_fail(self):
        for url in (
            "https://github.com/crmne-fork/examplecrate",
            "https://github.com/crmne/../dennisgr7/examplecrate",
            "https://github.com/crmne/%2e%2e/dennisgr7/examplecrate",
            "https://gitlab.com/crmne/examplecrate",
            "http://github.com/crmne/examplecrate",
            "ssh://git@github.com/crmne/examplecrate",
            "https://github.com/crmne",
        ):
            found = sources.problems({"Cargo.toml": f'[dependencies]\nexamplecrate-core = {{ git = "{url}" }}\n'})
            self.assertEqual(flagged(found), ["Cargo.toml"], url)

    def test_patches_from_a_path_or_another_registry_fail(self):
        manifest = """
[patch.crates-io]
egui = { path = "../egui/crates/egui" }
winit = { version = "0.30", registry = "mirror" }

[replace]
"rustls:0.23.0" = { path = "vendor/rustls" }
"""
        found = sources.problems({"Cargo.toml": manifest})
        self.assertEqual(len(found), 3)

    def test_other_registries_and_source_replacement_fail(self):
        lock = LOCK.format(source="registry+https://example.com/index")
        config = '[source.crates-io]\nreplace-with = "mirror"\n\n[source.mirror]\nregistry = "https://example.com/index"\n'
        found = sources.problems({"Cargo.lock": lock, ".cargo/config.toml": config})
        self.assertEqual(flagged(found), [".cargo/config.toml", "Cargo.lock"])


def asset_files(**extra):
    files = {
        "docs/_config.yml": b"logo: /assets/images/logo.svg\nimage: /screenshot.png\n",
        "docs/_guide/winamp.md": b"![The mini player](/assets/images/winamp.png)\n",
        "README.md": b'<img src="docs/assets/images/milkdrop-poster.jpg">\n',
        "docs/assets/images/logo.svg": b"<svg/>",
        "docs/assets/images/winamp.png": b"\x89PNG\0",
        "docs/assets/images/milkdrop-poster.jpg": b"\xff\xd8\0",
        "docs/screenshot.png": b"\x89PNG\0",
        "src/app.rs": b"// Before the queue moves, the cursor stays put.\n",
        "packaging/release-notes/v1.0.0/icon.png": b"\x89PNG\0",
    }
    files.update(extra)
    return files


class ReviewEvidenceTest(unittest.TestCase):
    def test_files_the_documentation_uses_pass(self):
        self.assertEqual(evidence.problems(asset_files()), [])

    def test_a_review_folder_fails_even_with_its_own_page(self):
        found = evidence.problems(
            asset_files(
                **{
                    "docs/assets/sharp-covers/README.md": b"![](after-dark-narrow.png)\n",
                    "docs/assets/sharp-covers/review.html": b'<img src="after-dark-narrow.png">',
                    "docs/assets/sharp-covers/after-dark-narrow.png": b"\x89PNG\0",
                    "docs/assets/prerelease-review/dark.png": b"\x89PNG\0",
                }
            )
        )
        self.assertEqual(
            flagged(found),
            [
                "docs/assets/prerelease-review/dark.png",
                "docs/assets/sharp-covers/README.md",
                "docs/assets/sharp-covers/after-dark-narrow.png",
                "docs/assets/sharp-covers/review.html",
            ],
        )

    def test_code_and_ci_naming_an_asset_do_not_count_as_documentation(self):
        found = evidence.problems(
            asset_files(
                **{
                    "docs/assets/cover-flight/t0016.jpg": b"\xff\xd8\0",
                    "src/demo.rs": b'const SHOT: &str = "docs/assets/cover-flight/t0016.jpg";\n',
                    ".github/scripts/test.py": b'"docs/assets/cover-flight/t0016.jpg"\n',
                }
            )
        )
        self.assertEqual(flagged(found), ["docs/assets/cover-flight/t0016.jpg"])

    def test_a_capture_dumped_beside_the_site_fails(self):
        found = evidence.problems(asset_files(**{"docs/captures/home-dark.png": b"\x89PNG\0"}))
        self.assertEqual(flagged(found), ["docs/captures/home-dark.png"])

    def test_review_named_media_fails_anywhere(self):
        names = [
            "review/index.html",
            "screenshots/before-dark-narrow.png",
            "assets/compare.html",
            ".github/comparison-light.webp",
            "packaging/release-notes/v1.0.0/queue-after.jpg",
            "src/testdata/after_t0016.mp4",
        ]
        found = evidence.problems(asset_files(**{name: b"\0" for name in names}))
        self.assertEqual(flagged(found), sorted(names))

    def test_review_words_inside_other_words_pass(self):
        names = ["assets/icons/crafter.svg", "docs/assets/images/afterglow.png", "src/reviewer.html"]
        files = asset_files(**{name: b"\0" for name in names})
        files["docs/_guide/glow.md"] = b"![Afterglow](/assets/images/afterglow.png)\n"
        self.assertEqual(evidence.problems(files), [])


if __name__ == "__main__":
    unittest.main()
