"""Tests of catalogue.py, without a build: plugin files made up here.

    python3 -m unittest discover -s tools/plugin-files
"""

import contextlib
import io
import pathlib
import struct
import sys
import tempfile
import unittest
import unittest.mock
import zipfile

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import catalogue  # noqa: E402

KEY = bytes(range(32))


def signed(fingerprint):
    """A plugin file as catalogue.py reads it: the header with its
    fingerprint, and a signature's trailer with the author's key."""
    header = b"PPNP" + bytes(4) + struct.pack("<Q", fingerprint)
    header += bytes(80 - len(header))
    trailer = b"PPNS" + bytes(8) + KEY
    return header + trailer + bytes(catalogue.SIGNATURE_LEN - len(trailer))


class HowItWorks(unittest.TestCase):
    def test_every_plugin_of_the_store_has_one_in_english(self):
        for plugin, _ in catalogue.plugins():
            files = dict(catalogue.how_files(plugin))
            self.assertIn("how/en.txt", files, plugin)
            self.assertTrue(all("\r" not in text for text in files.values()), plugin)

    def test_only_languages_are_taken_with_lf(self):
        with tempfile.TemporaryDirectory() as root:
            how = pathlib.Path(root, "demo", "how")
            how.mkdir(parents=True)
            how.joinpath("en.txt").write_bytes(b"First.\r\n\r\nSecond.\r\n")
            how.joinpath("de.txt").write_text("Erstens.\n", encoding="utf-8")
            how.joinpath("README.txt").write_text("Not a language.\n", encoding="utf-8")
            how.joinpath("en.md").write_text("Not a text file.\n", encoding="utf-8")
            files = catalogue.how_files("demo", pathlib.Path(root))
        self.assertEqual(files, [("how/de.txt", "Erstens.\n"), ("how/en.txt", "First.\n\nSecond.\n")])

    def test_the_store_s_folders_carry_it(self):
        with tempfile.TemporaryDirectory() as dist:
            dist = pathlib.Path(dist)
            for plugin, _ in catalogue.plugins():
                dist.joinpath(f"{plugin}-0.4.0-x4.ppn").write_bytes(signed(0x85073A61323717FF))
            argv = ["catalogue.py", str(dist), "0.4.0", "https://example.invalid/r", "--date", "2026-10-23"]
            with unittest.mock.patch.object(sys, "argv", argv), contextlib.redirect_stdout(io.StringIO()):
                catalogue.main()
            first = dist.joinpath("battery-0.4.0.zip").read_bytes()
            with zipfile.ZipFile(dist / "battery-0.4.0.zip") as z:
                names = z.namelist()
                english = z.read("how/en.txt").decode("utf-8")
            self.assertEqual(names, ["plugin.ini", "how/de.txt", "how/en.txt"])
            self.assertEqual(english, catalogue.ROOT.joinpath("plugins/battery/how/en.txt").read_text(encoding="utf-8"))
            # The same bytes again: the release's checksums hold.
            with unittest.mock.patch.object(sys, "argv", argv), contextlib.redirect_stdout(io.StringIO()):
                catalogue.main()
            self.assertEqual(dist.joinpath("battery-0.4.0.zip").read_bytes(), first)
            sums = dist.joinpath("SHA256SUMS").read_text(encoding="utf-8")
            self.assertIn("battery-0.4.0.zip", sums)


if __name__ == "__main__":
    unittest.main()
