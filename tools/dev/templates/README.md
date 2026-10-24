# paperpaw-dev in a plugin's own repository

`paperpaw-dev` runs PaperPaw's standard app on emulated devices (the
Xteink X4s and the Boox) with your plugin added, and checks it before you
publish it: every screen and widget drawn on every device, in light and
dark, in every language and look; text cut or overflowing, touch targets,
contrast; translations; flows; time and memory against the X4's budgets;
snapshots to compare with; and, for a data plugin, whether the device's
installer and the plugin library at paperpaw.org take it.

These files start it in a plugin's repository. Copy them, then replace
`my_plugin::MyPlugin` with your plugin's type and `my-plugin` with its id.

| File | What it is |
| --- | --- |
| `examples/paperpaw-dev.rs` | The harness: `paperpaw_dev::main` with your plugin added. `cargo run --example paperpaw-dev -- check --plugin my-plugin` |
| `tests/snapshots.rs` | The snapshot test: the cases of `tests/snapshots/cases.ini` compared with the pictures beside it. |
| `tests/snapshots/cases.ini` | The cases: screens and widgets, the device, settings, script steps. |
| `tests/flows/open-and-close.flow` | A flow: steps and what should be true after them. Write one for every action of the plugin. |
| `.github/workflows/paperpaw-dev.yml` | CI: the tests, then `check --plugin my-plugin --quick` and the flows varied at the edges, the reports kept as the run's artifact. |

## The dependency

`paperpaw-dev` is not on crates.io: it brings the emulator and the whole
standard app. Name it by git, with the tag of the PaperPaw release your
plugin is made for, as a dev-dependency (examples and tests may use those,
the plugin itself never sees it):

```toml
[dependencies]
paperpaw-sdk = "0.5.0-beta.2"

[dev-dependencies]
paperpaw-dev = { git = "https://github.com/hthoene/paperpaw", tag = "v0.5.0-beta.2" }

[patch.crates-io]
paperpaw-sdk = { git = "https://github.com/hthoene/paperpaw", tag = "v0.5.0-beta.2" }
```

The patch matters. Your plugin builds on `paperpaw-sdk` from crates.io;
`paperpaw-dev` on the one in the repository at that tag. To Cargo those are
two crates, and a plugin made with one is no plugin to an app made with the
other (`the trait Plugin is not implemented for MyPlugin`). The patch makes
your `paperpaw-sdk` the repository's at the same tag, one crate for both;
patch the other PaperPaw crates your plugin uses from crates.io the same
way (`paperpaw-digest`, `paperpaw-json`, and so on). Keep the two tags the
same, and move them together when you move to a new release. Tags before
`v0.5.0-beta.1` have no `paperpaw-dev`.

`cargo update -p paperpaw-dev` is never needed for a tag; with
`branch = "main"` instead (for PaperPaw's newest SDK, which changes every
day) it fetches the newest.

## Running it

```sh
cargo run --example paperpaw-dev -- check --plugin my-plugin --quick
cargo run --example paperpaw-dev -- flows --plugin my-plugin --flow tests/flows
cargo run --example paperpaw-dev -- gallery --plugin my-plugin --profile xteink-x4
cargo run --example paperpaw-dev -- --help
```

The report is in `target/paperpaw-dev`: `report.html` (open it in a
browser), `report.json` for tools, and the pictures. Errors make the run
fail, warnings too with `--strict`.

## Snapshots

`cargo test --test snapshots` draws every case of
`tests/snapshots/cases.ini` and compares it with `<name>.png` beside it. A
few pixels may differ between computers (glyph edges); more fails, and
`target/paperpaw-dev/test/report.html` shows the expected picture, the
actual one and the pixels that differ.

After a change meant to alter what is drawn, write the pictures anew, look
at each before committing it:

```sh
PAPERPAW_BLESS=1 cargo test --test snapshots
```

(In PowerShell: `$env:PAPERPAW_BLESS = 1; cargo test --test snapshots`.)
`paperpaw-dev check` and `paperpaw-dev snapshots` look at the same folder
and cases (`--bless` writes them there too). Cases can also be written in
code, with `paperpaw_dev::snapshot::check` and `Case::screen`,
`Case::widget`, `Case::home`; keep their pictures in a folder of their own
then, since `check` takes `tests/snapshots` as the folder of `cases.ini`.

When CI fails on a snapshot, the run's artifact `paperpaw-dev` has the
pictures that differ.

## A data plugin

A plugin without code needs no harness: install the command once and give
it the folder.

```sh
cargo install --locked --git https://github.com/hthoene/paperpaw --tag v0.5.0-beta.2 paperpaw-dev
paperpaw-dev check path/to/my-theme --quick
paperpaw-dev package path/to/my-theme --release 0.5.0-beta.2
```

Or take `paperpaw-dev` ready built from a release. It is in
`paperpaw-emu-<version>-<computer>.zip`, beside the emulator.

`package` checks it as the device's installer and the plugin library take
it, and installs it on emulated devices as the store does: what either
would refuse is an error. In CI, the job above with these two steps in
place of the tests and the example.
