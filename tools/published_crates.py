#!/usr/bin/env python3
"""List plugin crates that may be reused from crates.io during a publish check.

A published version may be excluded only when its packaged sources still
match. Cargo's generated files are ignored; CRLF and LF compare equally.
Inherited PaperPaw library requirements may follow the same release's beta
number, but their features and other dependency options must still match.
Network errors stop the check: only a registry 404 means a crate is new.
"""

import concurrent.futures
import hashlib
import io
import json
import pathlib
import re
import subprocess
import sys
import tarfile
import tomllib
import urllib.error
import urllib.request

ROOT = pathlib.Path(__file__).resolve().parent.parent
GENERATED = {"Cargo.lock", "Cargo.toml", ".cargo_vcs_info.json"}
KINDS = ("dependencies", "build-dependencies", "dev-dependencies")


def fetch(url):
    request = urllib.request.Request(url, headers={"User-Agent": "PaperPaw-publish-check"})
    with urllib.request.urlopen(request, timeout=60) as response:
        return response.read()


def published_version(name, version):
    try:
        data = fetch(f"https://index.crates.io/{name[:2]}/{name[2:4]}/{name}")
    except urllib.error.HTTPError as error:
        if error.code == 404:
            return None
        raise
    return next((item for line in data.splitlines()
                 if (item := json.loads(line))["vers"] == version), None)


def archive_files(name, version, checksum):
    data = fetch(f"https://static.crates.io/crates/{name}/{name}-{version}.crate")
    if hashlib.sha256(data).hexdigest() != checksum:
        raise ValueError(f"{name} {version}: downloaded package checksum differs from the index")
    with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as archive:
        return {item.name.split("/", 1)[1]: archive.extractfile(item).read()
                for item in archive.getmembers() if item.isfile()}


def normalized(data):
    # Binary payloads must stay byte for byte, even if they contain CRLF.
    if b"\0" in data:
        return data
    try:
        return data.decode("utf-8").replace("\r\n", "\n")
    except UnicodeDecodeError:
        return data


def dependencies(manifest, workspace):
    """The dependency options Cargo publishes, with workspace values resolved."""
    result = {}
    sections = [("", manifest), *manifest.get("target", {}).items()]
    for target, section in sections:
        for kind in KINDS:
            for name, spec in section.get(kind, {}).items():
                spec = {"version": spec} if isinstance(spec, str) else dict(spec)
                if spec.pop("workspace", False):
                    inherited = workspace["dependencies"][name]
                    inherited = {"version": inherited} if isinstance(inherited, str) else dict(inherited)
                    features = inherited.get("features", []) + spec.get("features", [])
                    spec = inherited | spec | {"features": features}
                # Cargo omits development dependencies with a path but no version.
                if kind == "dev-dependencies" and "path" in spec and "version" not in spec:
                    continue
                spec.pop("path", None)
                spec.setdefault("optional", False)
                spec.setdefault("default-features", True)
                spec["features"] = sorted(set(spec.get("features", [])))
                result[target, kind, name] = spec
    return result


def dependency_changes(current, published, workspace):
    before = dependencies(published, {})
    now = dependencies(current, workspace)
    for key, spec in now.items():
        inherited = workspace.get("dependencies", {}).get(key[2], {})
        if isinstance(inherited, str):
            inherited = {"version": inherited}
        version = spec.get("version", "")
        previous = before.get(key, {}).get("version", "")
        # Only the beta number follows the release. Operators and the
        # release's x.y.z must match, as must every other option.
        release = workspace.get("package", {}).get("version")
        if (inherited.get("path", "").startswith("crates/") and version == release
                and re.fullmatch(r"\d+\.\d+\.\d+-[0-9A-Za-z.-]+", previous)
                and version.split("-", 1)[0] == previous.split("-", 1)[0]):
            spec["version"] = previous
    return now != before


def changed_files(folder, names, published, workspace):
    names = {name.replace("\\", "/") for name in names} - GENERATED
    before = set(published) - GENERATED
    changes = sorted(names ^ before)
    for name in sorted(names & before):
        path = folder / ("Cargo.toml" if name == "Cargo.toml.orig" else name)
        if normalized(path.read_bytes()) != normalized(published[name]):
            changes.append(name)
    current = tomllib.loads((folder / "Cargo.toml").read_text(encoding="utf-8"))
    generated = tomllib.loads(published["Cargo.toml"].decode("utf-8"))
    if dependency_changes(current, generated, workspace):
        changes.append("Cargo.toml (resolved dependencies)")
    return changes


def main():
    workspace = tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))["workspace"]
    plugins = []
    for path in sorted((ROOT / "plugins").glob("*/Cargo.toml")):
        package = tomllib.loads(path.read_text(encoding="utf-8"))["package"]
        if package.get("publish") is not False:
            version = package["version"]
            if isinstance(version, dict):
                version = workspace["package"]["version"]
            plugins.append((path, package["name"], version))
    with concurrent.futures.ThreadPoolExecutor(max_workers=8) as pool:
        entries = list(pool.map(lambda p: published_version(p[1], p[2]), plugins))
    excluded, problems = [], []
    for (path, name, version), entry in zip(plugins, entries):
        if entry is None:
            continue
        if entry["yanked"]:
            problems.append(f"{name} {version} is yanked; choose a new version")
            continue
        published = archive_files(name, version, entry["cksum"])
        listed = subprocess.run(
            ["cargo", "package", "--list", "--allow-dirty", "--no-verify", "-p", name],
            cwd=ROOT, check=True, capture_output=True, text=True,
        ).stdout.splitlines()
        changes = changed_files(path.parent, listed, published, workspace)
        if changes:
            problems.append(f"{path.relative_to(ROOT).as_posix()}: {name} {version} differs "
                            f"from crates.io ({', '.join(changes[:5])}); raise its version")
        else:
            excluded.append(name)
    if problems:
        sys.exit("\n".join(problems))
    print("\n".join(excluded))


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        sys.exit(f"published_crates.py: {error}")
