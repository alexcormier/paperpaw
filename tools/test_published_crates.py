"""The publish check must never replace changed local sources with an old crate."""

import copy
import hashlib
import io
import pathlib
import tarfile
import tempfile
import tomllib
import unittest
import urllib.error
from unittest import mock

import published_crates as guard

MANIFEST = b'''[package]
name = "paperpaw-plugin-example"
version = "1.0.0"
[dependencies]
paperpaw-sdk.workspace = true
'''
PUBLISHED = b'''[package]
name = "paperpaw-plugin-example"
version = "1.0.0"
[dependencies.paperpaw-sdk]
version = "0.5.0-beta.2"
'''
WORKSPACE = {
    "package": {"version": "0.5.0-beta.8"},
    "dependencies": {
        "paperpaw-sdk": {"path": "crates/paperpaw-sdk", "version": "0.5.0-beta.8"},
    },
}


class PackageSources(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.folder = pathlib.Path(self.temp.name)
        (self.folder / "Cargo.toml").write_bytes(MANIFEST)
        (self.folder / "src").mkdir()
        (self.folder / "src/lib.rs").write_bytes(b"pub fn title() {}\n")
        self.files = {
            "Cargo.toml": PUBLISHED,
            "Cargo.toml.orig": MANIFEST,
            "src/lib.rs": b"pub fn title() {}\n",
            "Cargo.lock": b"generated old lock",
            ".cargo_vcs_info.json": b"generated old commit",
        }
        self.names = list(self.files)

    def changes(self):
        return guard.changed_files(self.folder, self.names, self.files, WORKSPACE)

    def test_unchanged_source_can_reuse_published_beta_dependency(self):
        self.assertEqual(self.changes(), [])

    def test_changed_code_requires_new_version(self):
        (self.folder / "src/lib.rs").write_bytes(b"pub fn title() -> u8 { 1 }\n")
        self.assertEqual(self.changes(), ["src/lib.rs"])

    def test_added_and_removed_files_are_not_silently_skipped(self):
        self.names.remove("src/lib.rs")
        self.names.append("src/new.rs")
        (self.folder / "src/new.rs").write_bytes(b"pub fn new() {}\n")
        self.assertEqual(self.changes(), ["src/lib.rs", "src/new.rs"])

    def test_crlf_and_windows_package_paths_match_linux_archive(self):
        (self.folder / "Cargo.toml").write_bytes(MANIFEST.replace(b"\n", b"\r\n"))
        (self.folder / "src/lib.rs").write_bytes(b"pub fn title() {}\r\n")
        self.names = [name.replace("/", "\\") for name in self.names]
        self.assertEqual(self.changes(), [])

    def test_binary_payloads_are_not_line_normalized(self):
        (self.folder / "data.bin").write_bytes(b"\xff\r\n")
        self.files["data.bin"] = b"\xff\n"
        self.names.append("data.bin")
        self.assertEqual(self.changes(), ["data.bin"])
        self.assertNotEqual(guard.normalized(b"data\0\r\n"), guard.normalized(b"data\0\n"))

    def test_explicit_manifest_dependency_change_requires_new_version(self):
        (self.folder / "Cargo.toml").write_bytes(
            MANIFEST.replace(b"paperpaw-sdk.workspace = true", b'paperpaw-sdk = { workspace = true, features = ["kernel"] }')
        )
        self.assertIn("Cargo.toml.orig", self.changes())
        self.assertIn("Cargo.toml (resolved dependencies)", self.changes())


class Dependencies(unittest.TestCase):
    def setUp(self):
        self.current = tomllib.loads(MANIFEST.decode())
        self.published = tomllib.loads(PUBLISHED.decode())
        self.workspace = copy.deepcopy(WORKSPACE)

    def changes(self):
        return guard.dependency_changes(self.current, self.published, self.workspace)

    def test_inherited_feature_change_is_detected(self):
        self.workspace["dependencies"]["paperpaw-sdk"]["features"] = ["kernel"]
        self.assertTrue(self.changes())

    def test_inherited_default_features_change_is_detected(self):
        self.workspace["dependencies"]["paperpaw-sdk"]["default-features"] = False
        self.assertTrue(self.changes())

    def test_version_operator_is_not_ignored(self):
        self.published["dependencies"]["paperpaw-sdk"]["version"] = "=0.5.0-beta.2"
        self.assertTrue(self.changes())

    def test_new_release_family_is_not_ignored(self):
        self.published["dependencies"]["paperpaw-sdk"]["version"] = "0.4.0-beta.2"
        self.assertTrue(self.changes())

    def test_target_dependencies_are_compared(self):
        self.current["target"] = {"cfg(windows)": {"dependencies": {"other": "1.0"}}}
        self.assertTrue(self.changes())

    def test_inherited_string_requirements_are_supported(self):
        self.workspace["dependencies"]["other"] = "1.0"
        self.current["dependencies"]["other"] = {"workspace": True}
        self.published["dependencies"]["other"] = {"version": "1.0"}
        self.assertFalse(self.changes())

    def test_path_only_dev_dependencies_are_not_published(self):
        self.current["dev-dependencies"] = {"paperpaw-app": {"path": "../../crates/paperpaw-app"}}
        self.assertFalse(self.changes())

    def test_git_only_dev_dependencies_are_not_published(self):
        self.current["dev-dependencies"] = {
            "fatfs": {"git": "https://example.invalid/fatfs", "rev": "revision",
                      "default-features": False, "features": ["alloc", "std"]},
        }
        self.assertFalse(self.changes())

    def test_versioned_git_dev_dependencies_are_still_compared(self):
        dependency = {"git": "https://example.invalid/fatfs", "rev": "old", "version": "0.4"}
        self.current["dev-dependencies"] = {"fatfs": dependency | {"rev": "new"}}
        self.published["dev-dependencies"] = {"fatfs": dependency}
        self.assertTrue(self.changes())

    def test_git_normal_and_build_dependencies_are_not_omitted(self):
        for kind in ("dependencies", "build-dependencies"):
            with self.subTest(kind=kind):
                manifest = {kind: {"fatfs": {"git": "https://example.invalid/fatfs", "rev": "revision"}}}
                self.assertIn(("", kind, "fatfs"), guard.dependencies(manifest, {}))


class RegistryFailures(unittest.TestCase):
    def test_yanked_version_is_never_reused(self):
        with tempfile.TemporaryDirectory() as temp:
            root = pathlib.Path(temp)
            (root / "Cargo.toml").write_text('[workspace.package]\nversion = "0.5.0-beta.8"\n')
            folder = root / "plugins/example"
            folder.mkdir(parents=True)
            (folder / "Cargo.toml").write_bytes(MANIFEST)
            with (mock.patch.object(guard, "ROOT", root),
                  mock.patch.object(guard, "published_version", return_value={"yanked": True}),
                  mock.patch.object(guard, "archive_files") as download):
                with self.assertRaisesRegex(SystemExit, "is yanked; choose a new version"):
                    guard.main()
                download.assert_not_called()

    @mock.patch.object(guard, "fetch")
    def test_only_404_means_a_crate_is_new(self, fetch):
        fetch.side_effect = urllib.error.HTTPError("url", 404, "new crate", {}, None)
        self.assertIsNone(guard.published_version("paperpaw-example", "1.0.0"))
        fetch.side_effect = urllib.error.HTTPError("url", 503, "unavailable", {}, None)
        with self.assertRaises(urllib.error.HTTPError):
            guard.published_version("paperpaw-example", "1.0.0")

    @mock.patch.object(guard, "fetch")
    def test_network_failure_does_not_skip_a_crate(self, fetch):
        fetch.side_effect = urllib.error.URLError("connection failed")
        with self.assertRaises(urllib.error.URLError):
            guard.published_version("paperpaw-example", "1.0.0")

    @mock.patch.object(guard, "fetch", return_value=b"broken download")
    def test_package_must_match_registry_checksum(self, fetch):
        with self.assertRaisesRegex(ValueError, "checksum differs"):
            guard.archive_files("paperpaw-example", "1.0.0", "0" * 64)

    @mock.patch.object(guard, "fetch")
    def test_archive_is_read_without_extracting_paths(self, fetch):
        data = io.BytesIO()
        with tarfile.open(fileobj=data, mode="w:gz") as archive:
            entry = tarfile.TarInfo("paperpaw-example-1.0.0/src/lib.rs")
            content = b"pub fn example() {}\n"
            entry.size = len(content)
            archive.addfile(entry, io.BytesIO(content))
        fetch.return_value = data.getvalue()
        checksum = hashlib.sha256(data.getvalue()).hexdigest()
        self.assertEqual(guard.archive_files("paperpaw-example", "1.0.0", checksum),
                         {"src/lib.rs": content})


if __name__ == "__main__":
    unittest.main()
