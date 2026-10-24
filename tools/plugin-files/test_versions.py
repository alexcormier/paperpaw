"""Tests of versions.py, without a repository.

    python3 -m unittest discover -s tools/plugin-files
"""

import pathlib
import sys
import unittest

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import versions  # noqa: E402

WORKSPACE = '[workspace.package]\nversion = "0.5.0-beta.1"\n'


class Versions(unittest.TestCase):
    def test_semver_order(self):
        order = ["0.5.0-beta.1", "0.5.0-beta.2", "0.5.0-beta.10", "0.5.0-rc.1", "0.5.0", "1.0.0",
                 "1.0.1-beta.1", "1.0.1", "1.10.0"]
        self.assertEqual(sorted(reversed(order), key=versions.semver), order)

    def test_a_crate_of_the_workspace_s_version_has_it(self):
        self.assertEqual(versions.version_in('[package]\nversion.workspace = true\n', WORKSPACE),
                         "0.5.0-beta.1")
        self.assertEqual(versions.version_in('[package]\nversion = "1.0.0"\n', WORKSPACE), "1.0.0")

    def test_a_change_needs_a_newer_version(self):
        fix = "fix(vocabulary): Again waits its ten minutes"
        self.assertIsNotNone(versions.problem("vocabulary", "1.0.0", "1.0.0", [fix]))
        self.assertIsNone(versions.problem("vocabulary", "1.0.0", "1.0.1", [fix]))
        self.assertIsNone(versions.problem("vocabulary", "1.0.0", "1.0.0", []))
        # Never older than the release's.
        self.assertIsNotNone(versions.problem("vocabulary", "1.0.1", "1.0.0", []))

    def test_a_change_no_reader_sees_keeps_it(self):
        quiet = "test(stats): one more case\n\nPlugin-unchanged: stats, widgets"
        everything = "chore: rustfmt\n\nplugin-unchanged: all"
        other = "docs(widgets): a comment\n\nPlugin-unchanged: widgets"
        self.assertIsNone(versions.problem("stats", "1.0.0", "1.0.0", [quiet, everything]))
        self.assertIsNotNone(versions.problem("stats", "1.0.0", "1.0.0", [quiet, other]))


if __name__ == "__main__":
    unittest.main()
