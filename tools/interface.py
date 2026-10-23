#!/usr/bin/env python3
"""Writes what code plugins see of each crate whose types cross between
a plugin and PaperPaw (ADR 0026), as the plugin interface's test
(`cargo test -p paperpaw-sdk --test interface`) compares it:

- `api.txt`, the crate's public API, as cargo-public-api lists it;
- `shape.txt`, the shape of its public types, from rustdoc's JSON: their
  fields, variants and trait items in the order the source has them,
  private fields too, each with its type. Order and private fields change
  how a type is laid out and how a trait's table of functions is made,
  which `api.txt` does not show: it is sorted, and public only.

    python3 tools/interface.py           # writes both files of each crate
    python3 tools/interface.py --check   # fails where one is not current

The crates are those `crates/paperpaw-sdk/interface/crates.txt` names.
Needs the nightly Rust that `NIGHTLY` names, with cargo-public-api.
"""

import json
import os
import pathlib
import subprocess
import sys
import tempfile

REPO = pathlib.Path(__file__).resolve().parent.parent
NIGHTLY = "nightly-2026-10-17"
CRATES = REPO / "crates" / "paperpaw-sdk" / "interface" / "crates.txt"
HEADER = (
    "# The shape of this crate's public types, as tools/interface.py writes\n"
    "# it: fields, variants and trait items in the source's order, private\n"
    "# fields too (ADR 0026).\n"
)


def crates():
    lines = CRATES.read_text(encoding="utf-8").splitlines()
    return [l.strip() for l in lines if l.strip() and not l.startswith("#")]


def api(crate):
    """The crate's public API, as `api.txt` keeps it."""
    run = subprocess.run(
        ["cargo", f"+{NIGHTLY}", "public-api", "-p", crate, "-sss"],
        cwd=REPO, capture_output=True, text=True, encoding="utf-8", check=True,
    )
    return run.stdout.replace("\r\n", "\n")


def rustdoc(crate, target):
    """The crate's rustdoc JSON, private items included."""
    subprocess.run(
        ["cargo", f"+{NIGHTLY}", "rustdoc", "-q", "-p", crate, "--lib", "--",
         "-Z", "unstable-options", "--output-format", "json",
         "--document-private-items"],
        cwd=REPO, check=True, env={**os.environ, "CARGO_TARGET_DIR": target},
    )
    path = pathlib.Path(target) / "doc" / (crate.replace("-", "_") + ".json")
    return json.loads(path.read_text(encoding="utf-8"))


def strip_ids(value):
    """`value` without rustdoc's ids, which change from build to build."""
    if isinstance(value, dict):
        return {k: strip_ids(v) for k, v in value.items() if k != "id"}
    if isinstance(value, list):
        return [strip_ids(v) for v in value]
    return value


def raw(value):
    return json.dumps(strip_ids(value), sort_keys=True)


def is_repr(attr):
    """Whether an attribute changes the layout: `repr` in any form."""
    return "repr" in (attr if isinstance(attr, str) else json.dumps(attr))


class Shapes:
    """The types of one crate's rustdoc JSON, as lines."""

    def __init__(self, doc):
        self.index = doc["index"]
        self.paths = doc["paths"]

    def name(self, path):
        entry = self.paths.get(str(path.get("id")))
        name = "::".join(entry["path"]) if entry else path["path"]
        return name + self.args(path.get("args"))

    def ty(self, t):
        (kind, v), = t.items()
        if kind in ("primitive", "generic"):
            return v
        if kind == "resolved_path":
            return self.name(v)
        if kind == "borrowed_ref":
            life = v["lifetime"] + " " if v.get("lifetime") else ""
            return "&" + life + ("mut " if v["is_mutable"] else "") + self.ty(v["type"])
        if kind == "raw_pointer":
            return ("*mut " if v["is_mutable"] else "*const ") + self.ty(v["type"])
        if kind == "slice":
            return "[" + self.ty(v) + "]"
        if kind == "array":
            return "[" + self.ty(v["type"]) + "; " + str(v["len"]) + "]"
        if kind == "tuple":
            return "(" + ", ".join(self.ty(x) for x in v) + ")"
        if kind == "dyn_trait":
            bounds = [self.name(b["trait"]) for b in v["traits"]]
            bounds += [v["lifetime"]] if v.get("lifetime") else []
            return "dyn " + " + ".join(bounds)
        if kind == "function_pointer":
            sig = v["sig"]
            out = " -> " + self.ty(sig["output"]) if sig.get("output") else ""
            return "fn(" + ", ".join(self.ty(x[1]) for x in sig["inputs"]) + ")" + out
        if kind == "qualified_path":
            return "<" + self.ty(v["self_type"]) + ">::" + v["name"]
        return raw(t)

    def args(self, args):
        if not args:
            return ""
        if "angle_bracketed" in args:
            parts = []
            for arg in args["angle_bracketed"]["args"]:
                (kind, v), = arg.items()
                parts.append(self.ty(v) if kind == "type" else str(v))
            parts += [raw(c) for c in args["angle_bracketed"].get("constraints", [])]
            return "<" + ", ".join(parts) + ">" if parts else ""
        if "parenthesized" in args:
            p = args["parenthesized"]
            out = " -> " + self.ty(p["output"]) if p.get("output") else ""
            return "(" + ", ".join(self.ty(x) for x in p["inputs"]) + ")" + out
        return raw(args)

    def field(self, id_):
        if id_ is None:
            return "_"
        item = self.index[str(id_)]
        return item["name"] + ": " + self.ty(item["inner"]["struct_field"])

    def fields(self, kind):
        """A struct's or a variant's fields: `{ a: T }`, `(0: T)` or none."""
        if not isinstance(kind, dict):
            return ""
        (form, v), = kind.items()
        if form == "tuple":
            return "(" + ", ".join(self.field(f) for f in v) + ")"
        return " { " + ", ".join(self.field(f) for f in v["fields"]) + " }"

    def variant(self, id_):
        item = self.index[str(id_)]
        v = item["inner"]["variant"]
        text = item["name"] + self.fields(v["kind"])
        if v.get("discriminant"):
            text += " = " + v["discriminant"]["expr"]
        return text

    def member(self, id_):
        item = self.index[str(id_)]
        (kind, _), = item["inner"].items()
        return kind + " " + item["name"]

    def line(self, item):
        (kind, v), = item["inner"].items()
        entry = self.paths.get(str(item["id"]))
        if kind not in ("struct", "enum", "union", "trait") or entry is None:
            return None
        if item["visibility"] != "public":
            return None
        head = kind + " " + "::".join(entry["path"])
        params = [p["name"] for p in v.get("generics", {}).get("params", [])]
        if params:
            head += "<" + ", ".join(params) + ">"
        head += "".join(" #" + raw(a) for a in item["attrs"] if is_repr(a))
        if kind == "struct":
            body = self.fields(v["kind"])
        elif kind == "union":
            body = " { " + ", ".join(self.field(f) for f in v["fields"]) + " }"
        elif kind == "enum":
            body = " { " + ", ".join(self.variant(x) for x in v["variants"]) + " }"
        else:
            body = " { " + ", ".join(self.member(x) for x in v["items"]) + " }"
        return head + body


def shape(crate, target):
    doc = rustdoc(crate, target)
    shapes = Shapes(doc)
    lines = {shapes.line(i) for i in doc["index"].values() if i["crate_id"] == 0}
    lines.discard(None)
    return HEADER + "".join(line + "\n" for line in sorted(lines))


def main():
    check = "--check" in sys.argv[1:]
    stale = []
    with tempfile.TemporaryDirectory(prefix="interface-") as target:
        for crate in crates():
            for name, text in (("api.txt", api(crate)), ("shape.txt", shape(crate, target))):
                path = REPO / "crates" / crate / name
                old = path.read_text(encoding="utf-8").replace("\r\n", "\n") if path.exists() else None
                if old == text:
                    continue
                if check:
                    stale.append(str(path.relative_to(REPO)))
                else:
                    path.write_text(text, encoding="utf-8", newline="\n")
                    print("wrote", path.relative_to(REPO))
    if stale:
        sys.exit("not current; write them with python3 tools/interface.py:\n  " + "\n  ".join(stale))


if __name__ == "__main__":
    main()
