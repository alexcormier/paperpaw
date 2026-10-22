# The sample plugin as a file

The SDK's sample plugin ([`../hello-plugin`](../hello-plugin)), unchanged,
as a native plugin file (`code.ppn`) that the Xteink X4 and X4 Pro load
at their start, without a new firmware (ADR 0024,
[Plugins as files](../../docs/plugins.md#plugins-as-files)). It is a
workspace of its own, as a plugin's own repository is, to start one
from:

| | What |
|---|---|
| `src/main.rs` | the file's crate: one line, `plugin!` around the plugin |
| `.cargo/config.toml` | how a plugin is linked, for both chips, and the aliases `x4` and `x4pro` |
| `rust-toolchain.toml` | the compiler for the X4 (the X4 Pro's comes from espup) |
| `plugin/plugin.ini` | the plugin's folder, to which `code.ppn` is added |
| `plugin/store/store.ini` | what the plugin store shows of it, for the upload form; readers never install `store/` |
| `build.sh` | builds both files, packs and signs them, zips the folder, into `dist/` |
| `.github/workflows/build.yml` | a CI job for a plugin's repository: `build.sh` on every tag, its files as a release |
| `.gitignore` | keeps `target/`, `dist/` and any `*.key` out of the repository |
| `examples/second.rs` | a second plugin, to try several at once |
| `examples/crash.rs` | a plugin that panics at every start, to try safe mode |

Here it uses the SDK from this repository; a plugin's own repository
names the release it is built for (`paperpaw-sdk = "0.5"`).

## Building, packing, trying

```sh
cargo x4                  # the X4: riscv32imc-unknown-none-elf
cargo +esp x4pro          # the X4 Pro: xtensa-esp32s3-none-elf, espup's toolchain
cargo x4 --example crash  # an example, the same way
```

Then the plugin's folder with the file in it, signed with the author's
key (`paperpaw-sign key` makes one), onto the card (from the
repository's top):

```sh
cp -r samples/hello-native/plugin hello
cargo run -p paperpaw-sign -- pack samples/hello-native/target/riscv32imc-unknown-none-elf/release/hello hello/code.ppn --key author.key
python tools/x4-remote.py --push hello plugins/hello
python tools/x4-remote.py --trust-code hello
```

The file runs only on a PaperPaw built by the same compiler from the
same SDK; *About* on the device names both. And only once installed: a
file copied into a plugin's folder does not run until it is named as
installed, as the store and the web flasher do, here over USB
(`--trust-code`, which only a firmware built for development answers).

## Size

| | X4 | X4 Pro |
|---|---|---|
| The sample's file (`opt-level = "s"`) | 62.9 KB | 55.0 KB |
| With `opt-level = "z"` | 59.5 KB | 53.7 KB |

Most of it is the SDK code the plugin carries (drawing text, settings,
translations, `core`'s formatting); the sample's own code is some 9 KB.
