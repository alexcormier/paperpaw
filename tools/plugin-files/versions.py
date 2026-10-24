#!/usr/bin/env python3
"""Whether each of PaperPaw's own plugins the store offers (plugins.txt)
that changed since the last release has a newer version of its own:
devices update a plugin only to a newer version, so a fix under the
same one would never reach them.

    python3 tools/plugin-files/versions.py [<since>]

<since> is the release's tag compared with, the newest `v<version>` tag
before HEAD where not given (CI runs it with the history and the tags).
A plugin's files are `plugins/<id>/` and its binary here
(`src/bin/<id>.rs`). A change no reader sees (a comment, a test, a
rename inside) keeps the version when each commit with it says so in a
trailer of its message:

    Plugin-unchanged: <id>, <id>      (or `all`)

The release rebuilds such a plugin anyway, for its firmware, at the
version it has.
"""

import pathlib
import re
import subprocess
import sys
import tomllib

HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parent.parent
TRAILER = re.compile(r"^Plugin-unchanged:\s*(.+)$", re.IGNORECASE | re.MULTILINE)


def git(*args):
    return subprocess.run(["git", *args], cwd=ROOT, check=True, capture_output=True,
                          text=True, encoding="utf-8").stdout


def plugins():
    """The ids of plugins.txt."""
    out = []
    for line in (HERE / "plugins.txt").read_text(encoding="utf-8").splitlines():
        line = line.split("#", 1)[0].split()
        if line:
            out.append(line[0])
    return out


def semver(version):
    """A key that orders versions as SemVer does (build metadata left out)."""
    version = version.split("+", 1)[0]
    core, _, pre = version.partition("-")
    numbers = tuple(int(n) for n in core.split("."))
    if not pre:
        return numbers, (1,)
    ids = tuple((0, int(i), "") if i.isdigit() else (1, 0, i) for i in pre.split("."))
    return numbers, (0, ids)


def version_in(manifest, workspace):
    """The [package] version of `manifest` (Cargo.toml's text), the
    workspace's (its text) where it takes that one."""
    version = tomllib.loads(manifest)["package"].get("version")
    if isinstance(version, dict):
        return tomllib.loads(workspace)["workspace"]["package"]["version"]
    return version


def unchanged(message, plugin):
    """Whether the commit `message` says it changed nothing a reader of
    `plugin` sees."""
    for names in TRAILER.findall(message):
        ids = {name.strip() for name in names.split(",")}
        if plugin in ids or "all" in ids:
            return True
    return False


def problem(plugin, before, now, changes):
    """What is wrong with `plugin` at version `now`, `before` at the
    release, `changes` the messages of the commits that changed it since;
    None where nothing is."""
    if semver(now) < semver(before):
        return f"its version {now} is older than the release's {before}"
    seen = [m for m in changes if not unchanged(m, plugin)]
    if seen and semver(now) == semver(before):
        first = seen[0].splitlines()[0] if seen[0] else ""
        return (
            f"changed since the release ({len(seen)} commits, e.g. \"{first}\"), its version "
            f"{now} did not: raise it in plugins/{plugin}/Cargo.toml (and the Cargo.lock files "
            f"that list it), or say `Plugin-unchanged: {plugin}` in each commit no reader sees"
        )
    return None


def main():
    since = sys.argv[1] if len(sys.argv) > 1 else git(
        "describe", "--tags", "--abbrev=0", "--match", "v[0-9]*", "HEAD").strip()
    workspace_then = git("show", f"{since}:Cargo.toml")
    workspace_now = (ROOT / "Cargo.toml").read_text(encoding="utf-8")
    failed = False
    for plugin in plugins():
        manifest = f"plugins/{plugin}/Cargo.toml"
        try:
            then = git("show", f"{since}:{manifest}")
        except subprocess.CalledProcessError:
            continue  # new since the release
        before = version_in(then, workspace_then)
        now = version_in((ROOT / manifest).read_text(encoding="utf-8"), workspace_now)
        log = git("log", "--no-merges", "--format=%B%x00", f"{since}..HEAD", "--",
                  f"plugins/{plugin}", f"tools/plugin-files/src/bin/{plugin}.rs")
        changes = [m.strip() for m in log.split("\0") if m.strip()]
        found = problem(plugin, before, now, changes)
        if found:
            print(f"::error file={manifest}::{plugin}: {found}")
            failed = True
    if failed:
        sys.exit(1)
    print(f"every plugin that changed since {since} has a newer version")


if __name__ == "__main__":
    main()
