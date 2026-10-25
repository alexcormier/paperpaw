#!/usr/bin/env python3
"""PaperPaw's own data plugins the store offers (data.toml): their list,
and building their folders for a release:

    python3 tools/plugin-files/data.py <dir>

runs each builder once (`python3 <build> <dir>`), which writes the
folder `<dir>/<id>` of each of its plugins, and checks that every plugin
of data.toml is there with the version data.toml gives it.
"""

import pathlib
import subprocess
import sys
import tomllib

HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parent.parent


def plugins(path=HERE / "data.toml"):
    """The data plugins of data.toml: id to their fields."""
    return tomllib.loads(path.read_text(encoding="utf-8"))


def plugin_ini(plugin, fields):
    """The `plugin.ini` of `plugin`, as its builder writes it."""
    return (
        "[plugin]\n"
        f"id = {plugin}\n"
        f"name = {fields['name']}\n"
        f"description = {fields['description']}\n"
        f"version = {fields['version']}\n"
    )


def catalogs(plugin, fields):
    """Its translations (`i18n/<lang>.ini`, text): its name and description
    in English and German, for the plugin manager and the store."""
    out = []
    for lang, name, words in [
        ("en", fields["name"], fields["description"]),
        ("de", fields["de"]["name"], fields["de"]["description"]),
    ]:
        out.append((
            f"i18n/{lang}.ini",
            f"[meta]\nlanguage = {lang}\n\n[messages]\n"
            f"{plugin}.plugin.name = {name}\n{plugin}.plugin.description = {words}\n",
        ))
    return out


def main():
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    out = pathlib.Path(sys.argv[1])
    out.mkdir(parents=True, exist_ok=True)
    listed = plugins()
    for build in sorted({fields["build"] for fields in listed.values()}):
        subprocess.run([sys.executable, str(ROOT / build), str(out)], cwd=ROOT, check=True)
    for plugin, fields in listed.items():
        ini = out / plugin / "plugin.ini"
        if not ini.exists():
            sys.exit(f"data.py: {fields['build']} did not write {plugin}")
        if f"version = {fields['version']}\n" not in ini.read_text(encoding="utf-8"):
            sys.exit(f"data.py: {plugin} is not at version {fields['version']}")
    print(f"{len(listed)} data plugins in {out}")


if __name__ == "__main__":
    main()
