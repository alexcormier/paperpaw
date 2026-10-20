# PaperPaw's own plugins as files

The plugins the store offers as files (ADR 0024) that PaperPaw makes
itself: statistics, widgets, Calibre, KOReader sync and the others in
`plugins.txt`. The X4 and the X4 Pro leave them out of their firmware
(`crates/paperpaw-app`, feature `store-plugins`), and readers install
those they want from the store. A workspace of its own, as a plugin's
repository is, set up as `samples/hello-native`: one binary for each
plugin, `paperpaw_sdk::plugin!` around it.

The release pipeline (`.github/workflows/release.yml`) builds them for
every release and device, as the release's firmware is built, signs
them with the first-party plugin key, and publishes them with their
folders and their entries for the plugin library:

| | What |
|---|---|
| `plugins.txt` | the plugins, with their group in the plugin library |
| `build.sh <device> <out> [--key <file>] [--fingerprint <hex>]` | builds, packs and checks the files for one reader: `<id>-<version>-<device>.ppn` |
| `catalogue.py <dist> <version> <base-url>` | each plugin's folder as a ZIP (its `plugin.ini`), its entry `catalogue/<id>.toml` for the plugin library, `SHA256SUMS` |

A plugin added to `plugins.txt` gets a binary here (`src/bin/<id>.rs`),
a dependency in `Cargo.toml`, and its place among the optional plugins
of `crates/paperpaw-app`. Its manifest says the crate's version
(`paperpaw_sdk::package_version!()`), so each release is a new version
of it, and it keeps no writable statics (`paperpaw-sign pack` refuses a
file with any).
