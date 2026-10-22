#!/usr/bin/env python3
"""The plugin library's entries for PaperPaw's own plugins the store offers
(plugins.txt), from the files build.sh made for every reader of a release:

    python3 tools/plugin-files/catalogue.py <dist> <version> <base-url> [--date YYYY-MM-DD]

<dist> holds <id>-<version>-<device>.ppn for each reader built. Writes into
<dist>:

- <id>-<version>.zip: the plugin's folder (its plugin.ini; the code comes
  as each reader's file), as readers install it;
- catalogue/<id>.toml: its entry in the plugin library, as the website's
  catalogue reads it (`catalogue/<id>.toml`, ADR 0023), with this
  version's builds, each
  file named by its address under <base-url> (the release's downloads)
  and its SHA-256;
- SHA256SUMS of the ZIPs and the plugin files.

Name and summary come from each plugin's translations
(`<id>.plugin.name`, `<id>.plugin.description`, English and German), the
author's key from the files' signatures (all must have the same), the
fingerprint (`abi`) from each file's header. Nothing is invented: a
plugin without a file, a name or a signature fails.
"""

import argparse
import datetime
import hashlib
import pathlib
import struct
import sys
import zipfile

HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parent.parent
DEVICES = ["x4", "x4-pro", "x4-classic"]
REPOSITORY = "https://github.com/hthoene/paperpaw"
SIGNATURE_LEN = 112


def fail(message):
    print(f"catalogue.py: {message}", file=sys.stderr)
    sys.exit(1)


def plugins():
    """(id, group) of plugins.txt."""
    out = []
    for line in (HERE / "plugins.txt").read_text(encoding="utf-8").splitlines():
        line = line.split("#", 1)[0].split()
        if line:
            out.append((line[0], line[1]))
    return out


def message(plugin, language, key):
    """A message of the plugin's translations in `language`."""
    for path in sorted((ROOT / "plugins" / plugin / "i18n").glob(f"*{language}.ini")):
        for line in path.read_text(encoding="utf-8").splitlines():
            name, sep, value = line.partition("=")
            if sep and name.strip() == key:
                return value.strip()
    return None


def ppn(path):
    """The fingerprint (16 hex digits) and the author's key (64 hex digits)
    of a signed plugin file."""
    data = path.read_bytes()
    if data[:4] != b"PPNP":
        fail(f"{path}: not a plugin file")
    fingerprint = struct.unpack_from("<Q", data, 8)[0]
    trailer = data[-SIGNATURE_LEN:]
    if len(data) < 80 + SIGNATURE_LEN or trailer[:4] != b"PPNS":
        fail(f"{path}: not signed")
    return f"{fingerprint:016x}", trailer[12:44].hex()


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def toml(text):
    return '"' + text.replace("\\", "\\\\").replace('"', '\\"') + '"'


def folder_zip(path, plugin_ini):
    """The folder's ZIP, the same bytes for the same contents."""
    with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as z:
        info = zipfile.ZipInfo("plugin.ini", date_time=(1980, 1, 1, 0, 0, 0))
        info.external_attr = 0o644 << 16
        z.writestr(info, plugin_ini)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("dist", type=pathlib.Path)
    parser.add_argument("version")
    parser.add_argument("base_url")
    parser.add_argument("--date", default=datetime.date.today().isoformat())
    args = parser.parse_args()
    dist, version, base = args.dist, args.version, args.base_url.rstrip("/")
    # The plugins' own version: three numbers, as their manifests say it
    # (`package_version!`) and devices read it; the files keep the
    # release's name (a release candidate's too).
    plugin_version = version.split("-")[0]
    sdk = ".".join(plugin_version.split(".")[:2])
    (dist / "catalogue").mkdir(parents=True, exist_ok=True)
    keys = set()
    sums = []
    for plugin, group in plugins():
        name = message(plugin, "en", f"{plugin}.plugin.name")
        summary = message(plugin, "en", f"{plugin}.plugin.description")
        if not name or not summary:
            fail(f"{plugin}: no {plugin}.plugin.name or .description in English")
        if len(summary) > 160:
            fail(f"{plugin}: its description is longer than a summary may be (160)")
        builds = []
        for device in DEVICES:
            path = dist / f"{plugin}-{version}-{device}.ppn"
            if path.exists():
                abi, key = ppn(path)
                keys.add(key)
                digest = sha256(path)
                sums.append((digest, path.name))
                builds.append((device, abi, f"{base}/{path.name}", digest))
        if not builds:
            fail(f"{plugin}: no file built for any reader")
        plugin_ini = (
            "[plugin]\n"
            f"id = {plugin}\n"
            f"name = {name}\n"
            f"description = {summary}\n"
            f"version = {plugin_version}\n"
        )
        folder = dist / f"{plugin}-{version}.zip"
        folder_zip(folder, plugin_ini)
        sums.append((sha256(folder), folder.name))
        lines = [
            f"# PaperPaw's own plugin, built for PaperPaw {version} by its release",
            "# (tools/plugin-files). Newer versions are added above older ones.",
            f"id = {toml(plugin)}",
            f"name = {toml(name)}",
            f"summary = {toml(summary)}",
            'kind = "code"',
            'author = "PaperPaw"',
            "author_key = AUTHOR_KEY",
            'licence = "Apache-2.0"',
            f"source = {toml(REPOSITORY + '/tree/main/plugins/' + plugin)}",
            # The crate a firmware lists when it has the plugin built in
            # (ADR 0022): updates find it here as a file.
            f"crate = {toml('paperpaw-plugin-' + plugin)}",
            f"group = {toml(group)}",
            "devices = [" + ", ".join(toml(b[0]) for b in builds) + "]",
        ]
        de_name = message(plugin, "de", f"{plugin}.plugin.name")
        de_summary = message(plugin, "de", f"{plugin}.plugin.description")
        if de_name and de_summary and len(de_summary) <= 160:
            lines += ["", "[i18n.de]", f"name = {toml(de_name)}", f"summary = {toml(de_summary)}"]
        lines += [
            "",
            "[[versions]]",
            f"version = {toml(plugin_version)}",
            f"sdk = {toml(sdk)}",
            f"date = {toml(args.date)}",
            f"url = {toml(base + '/' + folder.name)}",
            f"sha256 = {toml(sha256(folder))}",
            "",
            "[versions.source]",
            f"repository = {toml(REPOSITORY)}",
            f"revision = {toml('v' + version)}",
            'path = "tools/plugin-files"',
            f"bin = {toml(plugin)}",
        ]
        for device, abi, url, digest in builds:
            lines += [
                "",
                "[[versions.builds]]",
                f"device = {toml(device)}",
                f"abi = {toml(abi)}",
                f"url = {toml(url)}",
                f"sha256 = {toml(digest)}",
            ]
        (dist / "catalogue" / f"{plugin}.toml").write_text("\n".join(lines) + "\n", encoding="utf-8")
    if len(keys) != 1:
        fail(f"the files are signed by {len(keys)} keys, not one")
    key = keys.pop()
    for entry in (dist / "catalogue").glob("*.toml"):
        entry.write_text(entry.read_text(encoding="utf-8").replace("AUTHOR_KEY", toml(key)), encoding="utf-8")
    (dist / "SHA256SUMS").write_text("".join(f"{d}  {n}\n" for d, n in sorted(sums, key=lambda s: s[1])), encoding="utf-8")
    print(f"{len(plugins())} plugins, signed by {key}")


if __name__ == "__main__":
    main()
