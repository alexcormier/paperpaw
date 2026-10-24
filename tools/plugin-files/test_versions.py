"""Tests of versions.py: its rules, and the check in a repository made here.

    python3 -m unittest discover -s tools/plugin-files
"""

import contextlib
import io
import os
import pathlib
import subprocess
import sys
import tempfile
import unittest
import unittest.mock

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

    def test_a_plugin_s_own_tags(self):
        tags = ["v0.5.0-beta.2", "vocabulary-v1.0.1", "reading-goal-v1.0.1", "reading-v2.0.0",
                "vocabulary-v1.0.2-beta.1", "vocabulary-vx"]
        self.assertEqual(versions.own_tags("vocabulary", tags),
                         ["vocabulary-v1.0.1", "vocabulary-v1.0.2-beta.1"])
        self.assertEqual(versions.own_tags("reading", tags), ["reading-v2.0.0"])


class Repository(unittest.TestCase):
    """The check in a repository of one plugin: released with PaperPaw
    (`v0.5.0-beta.2`), then alone (`vocabulary-v1.0.1`, from a branch of
    the release merged back)."""

    def setUp(self):
        self.dir = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self.dir.name)
        self.env = {**os.environ, "GIT_AUTHOR_NAME": "t", "GIT_AUTHOR_EMAIL": "t@t",
                    "GIT_COMMITTER_NAME": "t", "GIT_COMMITTER_EMAIL": "t@t"}
        self.git("init", "-q", "-b", "main")
        self.write("tools/plugin-files/plugins.txt", "vocabulary reading\n")
        self.write("Cargo.toml", '[workspace.package]\nversion = "0.5.0-beta.2"\n')
        self.version("1.0.0")
        self.write("plugins/vocabulary/src/lib.rs", "// one\n")
        self.commit("release: 0.5.0-beta.2")
        self.git("tag", "v0.5.0-beta.2")
        # The fix alone, from the release's tag, merged back.
        self.git("switch", "-q", "-c", "fix", "v0.5.0-beta.2")
        self.write("plugins/vocabulary/src/lib.rs", "// two\n")
        self.version("1.0.1")
        self.commit("fix(vocabulary): Again waits")
        self.git("tag", "vocabulary-v1.0.1")
        self.git("switch", "-q", "main")
        self.git("merge", "-q", "--no-ff", "-m", "merge the fix", "fix")

    def tearDown(self):
        self.dir.cleanup()

    def git(self, *args):
        subprocess.run(["git", *args], cwd=self.root, env=self.env, check=True, capture_output=True)

    def write(self, path, text):
        path = self.root / path
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")

    def version(self, version):
        self.write("plugins/vocabulary/Cargo.toml", f'[package]\nname = "v"\nversion = "{version}"\n')

    def commit(self, message):
        self.git("add", "-A")
        self.git("commit", "-q", "-m", message)

    def check(self):
        """Whether the check passes in the repository."""
        here = self.root / "tools" / "plugin-files"
        with unittest.mock.patch.multiple(versions, ROOT=self.root, HERE=here), \
                contextlib.redirect_stdout(io.StringIO()):
            try:
                versions.main()
            except SystemExit:
                return False
        return True

    def test_the_fix_alone_merged_back_passes(self):
        self.assertTrue(self.check())

    def test_a_change_after_its_own_release_needs_a_newer_version(self):
        self.write("plugins/vocabulary/src/lib.rs", "// three\n")
        self.commit("fix(vocabulary): another")
        # 1.0.1 is newer than the PaperPaw release's 1.0.0, but not than
        # its own release's.
        self.assertFalse(self.check())
        self.version("1.0.2")
        self.commit("vocabulary 1.0.2")
        self.assertTrue(self.check())

    def test_a_change_no_reader_sees_after_its_own_release_keeps_it(self):
        self.write("plugins/vocabulary/src/lib.rs", "// three\n")
        self.commit("test(vocabulary): a comment\n\nPlugin-unchanged: vocabulary")
        self.assertTrue(self.check())

    def test_a_version_older_than_one_published_fails(self):
        # The fix released alone, not merged back yet.
        self.git("reset", "-q", "--hard", "HEAD^1")
        self.assertFalse(self.check())


if __name__ == "__main__":
    unittest.main()
