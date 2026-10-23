# PaperPaw's own plugins as files

The plugins the store offers as files (ADR 0024) that PaperPaw makes
itself: statistics, widgets, Calibre, KOReader sync and the others in
`plugins.txt`. The X4 and the X4 Pro leave them out of their released
firmware: `crates/paperpaw-app` builds them only with the feature
`store-plugins`, and the diagnostics only with `diagnostics`. Readers
install those they want from the store. A workspace of its own, as a plugin's
repository is, set up as `samples/hello-native`: one binary for each
plugin, `paperpaw_sdk::plugin!` around it.

The release pipeline (`.github/workflows/release.yml`) builds them for
every release and device, as the release's firmware is built, signs
them with the first-party plugin key, and publishes them with their
folders and their entries for the plugin library:

| | What |
|---|---|
| `plugins.txt` | the plugins, with their group in the plugin library |
| `build.sh <device> <out> [--key <file>] [--fingerprint <hex>]` | builds, packs and checks the files for one reader: `<id>-<version>-<device>.ppn`. `<device>` is `x4` or `x4-pro`. |
| `anywhere.sh <x4\|x4-pro> [<work-dir>]` | checks that a plugin's file is the same wherever it is built, as the plugin library's rebuild needs: the sample, with the SDK from a registry as an author's repository has it, built in two folders and compared (CI runs it) |
| `sign.sh <dir> <paperpaw-sign> --key <file>` | signs the files apart from building them, so the key is on the disk only while it signs |
| `catalogue.py <dist> <version> <base-url>` | makes each plugin's folder as a ZIP with its `plugin.ini` and its How it works, `how/<lang>.txt`, which the store shows. It also makes the entry `catalogue/<id>.toml` for the plugin library, and `SHA256SUMS`. It takes `--date YYYY-MM-DD` too. `test_catalogue.py` tests it: `python3 -m unittest discover -s tools/plugin-files`. |

A plugin added to `plugins.txt` gets a binary here (`src/bin/<id>.rs`),
a dependency in `Cargo.toml`, and its place among the optional plugins
of `crates/paperpaw-app`. Its manifest says the crate's version
(`paperpaw_sdk::package_version!()`), so each release is a new version
of it, and it keeps no writable statics (`paperpaw-sign pack` refuses a
file with any).

`Cargo.lock` is committed: the release builds with `--locked`, and the
plugin library rebuilds each file from it. When a plugin's dependencies
change, it is brought up to date here with `cargo update -w --offline`
(CI fails until it is).
