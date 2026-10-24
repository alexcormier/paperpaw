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
| `build.sh <device> <out> [--key <file>] [--fingerprint <hex>]` | builds, packs and checks the files for one reader: `<id>-<version>-<device>.ppn`. `<device>` is `x4` or `x4-pro`. With `--only <id> --version <version>`, one plugin alone (see below). |
| `anywhere.sh <x4\|x4-pro> [<work-dir>]` | checks that a plugin's file is the same wherever it is built, as the plugin library's rebuild needs: the sample, with the SDK from a registry as an author's repository has it, built in two folders and compared (CI runs it) |
| `sign.sh <dir> <paperpaw-sign> --key <file>` | signs the files apart from building them, so the key is on the disk only while it signs |
| `catalogue.py <dist> <version> <base-url>` | makes each plugin's folder as a ZIP with its `plugin.ini` and its How it works, `how/<lang>.txt`, which the store shows. It also makes the entry `catalogue/<id>.toml` for the plugin library, and `SHA256SUMS`. It takes `--date YYYY-MM-DD` too, and `--only <id>` for one plugin alone. `test_catalogue.py` tests it: `python3 -m unittest discover -s tools/plugin-files`. |

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

## One plugin between releases

A fix to one of these plugins can reach readers before the next release:
`.github/workflows/store-plugin.yml` (run by hand, with the plugin, its
new version, the release it is for and the branch with the fix) builds
it for that release's firmware, signs it and publishes it as a
pre-release of its own, with its files and its catalogue entry alone.
The plugin library adds the version to the plugin's entry; the others
keep theirs.

The fix is made from the release's tag, so the plugin fits its firmware
(the workflow checks the fingerprint). Devices read a pre-release as a
name and one number, so the new version is the release's with the
number one up (`0.5.0-beta.2` gives `0.5.0-beta.3`), set in the
plugin's own `Cargo.toml` (`version = "0.5.0-beta.3"` for
`version.workspace = true`) with its line in `Cargo.lock`. The next
release takes the number after it, so its plugins are newer again, and
by then the plugin is back on the workspace's version: `build.sh`
refuses a release with a plugin that has one of its own.
