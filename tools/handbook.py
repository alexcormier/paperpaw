#!/usr/bin/env python3
"""The handbook of a release: what tools that write or check plugins for
it read (the workshop's AI handbook, its lookups and checks), so they need
no PaperPaw checkout of their own:

    python3 tools/handbook.py <registry.json> <version> <out.zip>

The zip holds, under `paperpaw-handbook-<version>/`:

- README.md: this list, the version, the format (`FORMAT`);
- registry.json: what the app offers data plugins (`paperpaw-emu
  --registry`): settings, widgets, actions, device profiles;
- checks.json: every finding paperpaw-dev's checks make, by its code
  (`visual.wrap`), each with its group (`visual`);
- docs/: the repository's docs (the plugin guide `docs/plugins.md`, the
  ADRs);
- samples/: the sample plugins, data and code;
- api/<crate>.txt: the public items of the SDK and the crates it passes
  on, as their `api.txt` lists them;
- src/<crate>/: the source of those and of every crate the SDK is built
  from, for the items' documentation.

A field or file is only added; FORMAT counts up where one changes.
"""

import json
import re
import sys
import zipfile
from pathlib import Path

FORMAT = 1
ROOT = Path(__file__).resolve().parent.parent
# The groups of paperpaw-dev's checks (its commands), the start of a code.
GROUPS = ("package", "lint", "visual", "reach", "explore", "perf", "flows",
          "gallery", "snapshots", "ui-kit", "data")
CODE = re.compile(r'"((?:%s)\.[a-z0-9-]+(?:\.[a-z0-9-]+)*)"' % "|".join(GROUPS))
# Below a crate's folder, what is not part of it.
SKIP = {"target", ".git", "node_modules", "out"}


def check_codes(dev: Path) -> list:
    """Every code a finding of paperpaw-dev can have, from its source."""
    codes = set()
    for path in dev.rglob("*.rs"):
        if "tests" in path.relative_to(dev).parts:
            continue
        codes.update(CODE.findall(path.read_text(encoding="utf-8")))
    return [{"code": code, "group": code.split(".")[0]} for code in sorted(codes)]


def files(folder: Path):
    """The files of `folder`, without builds and checkouts."""
    for path in sorted(folder.rglob("*")):
        if path.is_file() and not SKIP.intersection(path.relative_to(folder).parts):
            yield path


def crates_with_api() -> list:
    """The crates with a list of their public items: the SDK and those it
    passes on."""
    return sorted(p.parent for p in (ROOT / "crates").glob("*/api.txt"))


def sdk_crates() -> list:
    """The SDK and every PaperPaw crate it is built from (`Canvas`, `Rect`
    and `Font` are theirs), whose source documents what a plugin uses."""
    manifest = (ROOT / "crates" / "paperpaw-sdk" / "Cargo.toml").read_text(encoding="utf-8")
    names = set(re.findall(r"^(paperpaw-[a-z-]+)\s*[.=]", manifest, re.M)) | {"paperpaw-sdk"}
    return sorted(ROOT / "crates" / n for n in names if (ROOT / "crates" / n / "src").is_dir())


def main(registry: str, version: str, out: str) -> None:
    top = f"paperpaw-handbook-{version}"
    codes = check_codes(ROOT / "tools" / "dev" / "src")
    if len(codes) < 20:
        sys.exit(f"only {len(codes)} check codes found: has paperpaw-dev's source moved?")
    crates = crates_with_api()
    if not any(c.name == "paperpaw-sdk" for c in crates):
        sys.exit("no crates/paperpaw-sdk/api.txt")
    readme = f"""# PaperPaw {version}: the handbook

Format {FORMAT}. What tools that write or check plugins for PaperPaw
{version} read, made by its release (tools/handbook.py):

- registry.json: settings, widgets, actions, device profiles (`paperpaw-emu --registry`)
- checks.json: the codes of paperpaw-dev's findings ({len(codes)})
- docs/: the docs, the plugin guide `docs/plugins.md` first
- samples/: the sample plugins
- api/: the public items of {", ".join(c.name for c in crates)}
- src/: the source of those and of the crates the SDK is built from
"""
    with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED) as z:
        z.writestr(f"{top}/README.md", readme)
        z.write(registry, f"{top}/registry.json")
        z.writestr(f"{top}/checks.json", json.dumps({"format": FORMAT, "checks": codes}, indent=1) + "\n")
        for folder in ("docs", "samples"):
            for path in files(ROOT / folder):
                z.write(path, f"{top}/{path.relative_to(ROOT).as_posix()}")
        for crate in crates:
            z.write(crate / "api.txt", f"{top}/api/{crate.name}.txt")
        for crate in sorted(set(crates) | set(sdk_crates())):
            for path in files(crate / "src"):
                z.write(path, f"{top}/src/{crate.name}/{path.relative_to(crate / 'src').as_posix()}")
    print(f"{out}: {len(codes)} check codes, {len(crates)} crates")


if __name__ == "__main__":
    if len(sys.argv) != 4:
        sys.exit(__doc__.split("\n\n")[1])
    main(*sys.argv[1:])
