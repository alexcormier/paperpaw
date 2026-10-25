#!/usr/bin/env python3
"""Builds the sample code plugin (samples/hello-native) for the X4 as an
author's project would be: in a folder of its own, its plugin crate in it,
the SDK and the crates below it from a registry (here a vendored folder
that stands in for crates.io, keeping their identity), twice in two
folders of different depth, and checks that both files are the same:

    python3 tools/reproducible-plugin.py <out>

Writes <out>/hello-x4.ppn (unsigned) and prints its SHA-256. Run on Linux
and on Windows, the two files are the same too (CI compares them):
nothing of the computer goes into a plugin. Paths go in as an author's
build writes them (the plugin library's rebuild does the same): the
project as /plugin, the registry's crates as Cargo's home has them, each
folder with Rust files on its own, so that Windows' `\\` goes too.

Needs the X4's Rust (the sample's rust-toolchain.toml, with
riscv32imc-unknown-none-elf) and network for the few crates of others.
"""

import hashlib
import json
import os
import pathlib
import re
import shutil
import subprocess
import sys
import tarfile
import tomllib

REPO = pathlib.Path(__file__).resolve().parent.parent
SAMPLE = REPO / "samples" / "hello-native"
REGISTRY = "/cargo/registry/src/index.crates.io-1949cf8c6b5b557f"
TARGET = "riscv32imc-unknown-none-elf"


def run(args, cwd, **kw):
    env = dict(os.environ, CARGO_INCREMENTAL="0")
    env.pop("RUSTC_WRAPPER", None)
    env.pop("RUSTFLAGS", None)
    subprocess.run(args, cwd=cwd, check=True, env=env, **kw)


def posix(path):
    return pathlib.Path(path).as_posix()


def version():
    for line in (REPO / "Cargo.toml").read_text(encoding="utf-8").splitlines():
        if line.startswith("version = "):
            return line.split('"')[1]
    sys.exit("reproducible-plugin.py: no version in Cargo.toml")


def packages():
    """The SDK and the PaperPaw crates it is built from, as `cargo package`
    makes them."""
    tree = subprocess.run(
        ["cargo", "tree", "-p", "paperpaw-sdk", "-e", "normal", "--prefix", "none", "-f", "{p}"],
        cwd=REPO, check=True, capture_output=True, text=True,
    ).stdout
    names = sorted({line.split()[0] for line in tree.splitlines() if line.startswith("paperpaw-")})
    folder = REPO / "target" / "package"
    for old in folder.glob("*.crate"):
        old.unlink()
    run(["cargo", "package", "-q", "--no-verify", "--allow-dirty"] + [a for n in names for a in ("-p", n)], REPO)
    return sorted(folder.glob("*.crate"))


def project(at, ver, extra_toml=""):
    """The hello binary alone as an author's project at `at`."""
    shutil.rmtree(at, ignore_errors=True)
    at.mkdir(parents=True)
    for item in ["src", "plugin", "rust-toolchain.toml", "paperpaw-plugin.x"]:
        src = SAMPLE / item
        if src.is_dir():
            shutil.copytree(src, at / item)
        elif src.exists():
            shutil.copy(src, at / item)
    toml = (SAMPLE / "Cargo.toml").read_text(encoding="utf-8")
    # Other examples in hello-native have their own sample dependencies.
    # Only the hello binary is copied here; keep its two dependencies,
    # with the SDK from the registry, and the sample's build settings.
    deps = tomllib.loads(toml)["dependencies"]
    sdk = dict(deps["paperpaw-sdk"], version=f"={ver}")
    sdk.pop("path", None)
    hello = dict(deps["paperpaw-sample-hello"], path="hello-plugin")
    entries = "".join(
        f'{name} = {{ ' + ", ".join(f"{k} = {json.dumps(v)}" for k, v in fields.items()) + " }\n"
        for name, fields in [("paperpaw-sdk", sdk), ("paperpaw-sample-hello", hello)]
    )
    toml = re.sub(r"(?ms)^\[dependencies\]\n.*?(?=^\[|\Z)", "[dependencies]\n" + entries + "\n", toml)
    (at / "Cargo.toml").write_text(toml + extra_toml, encoding="utf-8")
    lib = at / "hello-plugin"
    for item in ["src", "i18n", "how"]:
        if (REPO / "samples" / "hello-plugin" / item).is_dir():
            shutil.copytree(REPO / "samples" / "hello-plugin" / item, lib / item)
    (lib / "Cargo.toml").write_text(
        '[package]\nname = "paperpaw-sample-hello"\nversion = "0.1.0"\nedition = "2024"\n\n'
        f'[dependencies]\npaperpaw-sdk = "={ver}"\n',
        encoding="utf-8",
    )
    (at / ".cargo").mkdir()
    shutil.copy(SAMPLE / ".cargo" / "config.toml", at / ".cargo" / "config.toml")


def remaps(top, vendor):
    """The paths as written into the build: broad ones first (the last that
    matches wins), then every folder with Rust files on its own."""
    lines = [f"--remap-path-prefix={posix(top)}=/plugin", f"--remap-path-prefix={posix(vendor)}={REGISTRY}"]
    for root, prefix in [(top, "/plugin"), (vendor, REGISTRY)]:
        folders = sorted({p.parent for p in root.rglob("*.rs") if "target" not in p.relative_to(root).parts})
        for folder in folders:
            rel = folder.relative_to(root).as_posix()
            if rel != ".":
                lines.append(f"--remap-path-prefix={posix(folder)}={prefix}/{rel}/")
    return "\n".join(lines) + "\n"


def main():
    if len(sys.argv) != 2:
        sys.exit(__doc__.split("\n\n")[1])
    out = pathlib.Path(sys.argv[1]).resolve()
    shutil.rmtree(out, ignore_errors=True)
    ver = version()
    crates = packages()
    # A vendored folder standing in for crates.io: the crates of others,
    # fetched once, and PaperPaw's as packaged.
    unpacked = out / "unpacked"
    unpacked.mkdir(parents=True)
    for crate in crates:
        with tarfile.open(crate) as t:
            t.extractall(unpacked, filter="data")
    patch = "\n[patch.crates-io]\n" + "".join(
        f'{d.name.split("-0.")[0]} = {{ path = "{posix(d)}" }}\n' for d in sorted(unpacked.iterdir())
    )
    seed = out / "seed"
    project(seed, ver, patch)
    vendor = out / "vendor"
    run(["cargo", "vendor", "-q", "--versioned-dirs", str(vendor)], seed, stdout=subprocess.DEVNULL)
    for crate in crates:
        with tarfile.open(crate) as t:
            t.extractall(vendor, filter="data")
        folder = vendor / crate.name[: -len(".crate")]
        sha = hashlib.sha256(crate.read_bytes()).hexdigest()
        (folder / ".cargo-checksum.json").write_text(json.dumps({"files": {}, "package": sha}))
    run(["cargo", "build", "-q", "--release", "-p", "paperpaw-sign"], REPO)
    sign = REPO / "target" / "release" / ("paperpaw-sign.exe" if os.name == "nt" else "paperpaw-sign")
    files = []
    for where in ["a/hello", "b/deeper/folder/hello"]:
        top = out / where
        project(top, ver)
        # The registry: the vendored folder, in crates.io's place.
        with open(top / ".cargo" / "config.toml", "a", encoding="utf-8") as config:
            config.write(f'\n[source.crates-io]\nreplace-with = "vendored"\n\n[source.vendored]\ndirectory = "{posix(vendor)}"\n')
        args = out / f"remaps-{len(files)}.txt"
        args.write_text(remaps(top, vendor), encoding="utf-8")
        run(["cargo", "x4", "-q", "--config", f"target.{TARGET}.rustflags = ['@{posix(args)}']"], top)
        ppn = out / f"hello-x4-{len(files)}.ppn"
        run([str(sign), "pack", str(top / "target" / TARGET / "release" / "hello"), str(ppn)], REPO,
            stdout=subprocess.DEVNULL)
        files.append(ppn.read_bytes())
    if files[0] != files[1]:
        sys.exit("reproducible-plugin.py: the two builds differ")
    (out / "hello-x4.ppn").write_bytes(files[0])
    print(f"{hashlib.sha256(files[0]).hexdigest()}  hello-x4.ppn")


if __name__ == "__main__":
    main()
