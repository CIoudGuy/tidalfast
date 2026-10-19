"""Tests for the release history in scripts/release.py: python -I scripts/test_release.py"""
import shutil
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import release  # noqa: E402


class History(unittest.TestCase):
    def setUp(self):
        self.dir = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.dir, ignore_errors=True)
        for version in ("7.9.0", "7.10.0", "7.10.5", "7.11.0", "8.0.0-beta.1", "8.0.0"):
            (self.dir / f"v{version}.md").write_text(f"Notes {version}\n", encoding="utf-8")
        self.meta = self.dir / "meta.xml"
        self.meta.write_text(
            '<releases>\n  <release version="7.11.0" date="2026-10-01">\n  </release>\n'
            '  <release version="7.10.5" date="2026-09-20">\n  </release>\n</releases>\n',
            encoding="utf-8",
        )

    def history(self, version, **kwargs):
        return release.release_history(version, self.dir, self.meta, **kwargs)

    def test_newest_first_up_to_the_release_with_dates(self):
        found = self.history("7.11.0")
        self.assertEqual([e["version"] for e in found], ["7.11.0", "7.10.5", "7.10.0", "7.9.0"])
        self.assertEqual(found[0], {"version": "7.11.0", "date": "2026-10-01", "notes": "Notes 7.11.0"})
        self.assertEqual(found[2]["date"], "")

    def test_prerelease_sorts_below_its_release(self):
        found = self.history("8.0.0")
        self.assertEqual([e["version"] for e in found][:2], ["8.0.0", "8.0.0-beta.1"])

    def test_limit(self):
        self.assertEqual(len(self.history("8.0.0", limit=2)), 2)

    def test_the_real_notes_include_the_release_itself(self):
        version = release.cargo_version()
        found = release.release_history(version)
        self.assertEqual(found[0]["version"], version)
        self.assertTrue(found[0]["notes"])
        self.assertTrue(len(found) > 1)


if __name__ == "__main__":
    unittest.main()
