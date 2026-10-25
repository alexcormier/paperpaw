"""Checks that the hello reproducibility fixture stands on its own.

    python3 -m unittest discover -s tools -p "test_reproducible_plugin.py"
"""

import importlib.util
import json
import pathlib
import subprocess
import tempfile
import tomllib
import unittest

TOOLS = pathlib.Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location("reproducible_plugin", TOOLS / "reproducible-plugin.py")
reproducible = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(reproducible)


class Project(unittest.TestCase):
    def test_hello_is_isolated_from_other_examples(self):
        with tempfile.TemporaryDirectory() as tmp:
            top = pathlib.Path(tmp) / "hello"
            reproducible.project(top, "0.5.0-beta.8")
            manifest = tomllib.loads((top / "Cargo.toml").read_text(encoding="utf-8"))
            self.assertEqual(set(manifest["dependencies"]), {"paperpaw-sdk", "paperpaw-sample-hello"})
            self.assertEqual(manifest["dependencies"]["paperpaw-sdk"]["version"], "=0.5.0-beta.8")
            self.assertNotIn("path", manifest["dependencies"]["paperpaw-sdk"])
            for dependency in manifest["dependencies"].values():
                if "path" in dependency:
                    self.assertTrue((top / dependency["path"] / "Cargo.toml").is_file())
            sample = tomllib.loads((reproducible.SAMPLE / "Cargo.toml").read_text(encoding="utf-8"))
            self.assertEqual(manifest["profile"], sample["profile"])
            for name in ["src/main.rs", "rust-toolchain.toml", "paperpaw-plugin.x", ".cargo/config.toml"]:
                self.assertEqual((top / name).read_bytes(), (reproducible.SAMPLE / name).read_bytes())
            self.assertFalse((top / "examples").exists())
            for folder in ["src", "i18n", "how"]:
                source = reproducible.REPO / "samples" / "hello-plugin" / folder
                for item in source.rglob("*"):
                    if item.is_file():
                        self.assertEqual(item.read_bytes(), (top / "hello-plugin" / folder / item.relative_to(source)).read_bytes())

    def test_cargo_reads_the_isolated_project_without_the_repository(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            sdk = root / "sdk"
            (sdk / "src").mkdir(parents=True)
            (sdk / "src/lib.rs").write_text("#![no_std]\n", encoding="utf-8")
            (sdk / "Cargo.toml").write_text(
                '[package]\nname = "paperpaw-sdk"\nversion = "0.5.0-beta.8"\nedition = "2024"\n',
                encoding="utf-8",
            )
            top = root / "hello"
            patch = '\n[patch.crates-io]\npaperpaw-sdk = { path = "' + sdk.as_posix() + '" }\n'
            reproducible.project(top, "0.5.0-beta.8", patch)
            result = subprocess.run(
                ["cargo", "metadata", "--no-deps", "--offline", "--format-version", "1"],
                cwd=top, capture_output=True, text=True,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            metadata = json.loads(result.stdout)
            package = next(p for p in metadata["packages"] if p["name"] == "paperpaw-sample-hello-native")
            dependency = next(d for d in package["dependencies"] if d["name"] == "paperpaw-sdk")
            self.assertTrue(dependency["source"].startswith("registry+"))
            self.assertIsNone(dependency.get("path"))


if __name__ == "__main__":
    unittest.main()
