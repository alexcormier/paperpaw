# Contributing to PaperPaw

Thank you for helping! This guide explains how to get started and what we
expect from contributions.

## Ground rules

- Be kind and constructive; see the [Code of Conduct](CODE_OF_CONDUCT.md).
- Everything in the repository is written in **English**: code, comments,
  documentation, commit messages, issues and pull requests.
- Discuss larger changes in an issue first, especially changes to the SDK
  (`crates/paperpaw-sdk`), which is a public API.

## Development setup

1. Install Rust via [rustup](https://rustup.rs). `rust-toolchain.toml` pulls
   in `rustfmt`, `clippy` and the embedded/WASM targets automatically.
2. Run the checks CI runs:

   ```bash
   cargo fmt --all --check
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace
   cargo build --release --target riscv32imc-unknown-none-elf -p paperpaw-core -p paperpaw-plugin-diagnostics
   ```

3. Try your change in the emulator:
   `cargo run -p paperpaw-emulator -- --profile xteink-x4`.

## Architecture rules

These keep PaperPaw small, portable and extensible. Reviews check them.

1. **The kernel contains no product features.** Screens, formats, themes and
   the like are plugins.
2. **Plugins depend on `paperpaw-sdk` only**, never on `paperpaw-core`.
   First-party plugins have no special access.
3. **If a community plugin could not build a feature, extend the SDK first.**
4. **Core crates are `no_std`.** They must build for
   `riscv32imc-unknown-none-elf` (ESP32-C3, ~380 KB RAM). Avoid hidden
   allocations in hot paths and never load whole files into memory.
5. **Namespaced ids** (`plugin-id.name`) for every contribution; `core.*` is
   reserved for the kernel.
6. **No panics on bad input.** Configuration, books and plugins can be
   broken; report a warning and carry on with defaults.
7. **SDK APIs must work over a message boundary** (serializable arguments,
   no borrowed host state kept across calls), so WebAssembly plugins can use
   them too.
8. **The SDK changes visibly and compatibly** ([ADR 0020](docs/adr/0020-sdk-compatibility.md)):
   a change to its public API comes with `crates/paperpaw-sdk/api.txt`
   brought up to date
   (`cargo +nightly-2026-10-17 public-api -p paperpaw-sdk -sss > crates/paperpaw-sdk/api.txt`),
   and so does a change to a crate it passes on (config, document,
   layout, xml, image, hyphen, zip, json, digest: each its own
   `api.txt`),
   and what goes is `#[deprecated]` first. The SDK's features `kernel`
   and `unstable` add only functions and modules, never a field, variant
   or trait item: a plugin's fingerprint leaves them out
   (`ADDING_ONLY` in `crates/paperpaw-sdk/build.rs`).

Significant decisions are recorded as [Architecture Decision Records](docs/adr/).

## Adding a translation

Each plugin keeps its catalogs in `i18n/<language>.ini`. To add a language:

1. Copy `i18n/en.ini` of the kernel (`crates/paperpaw-core/i18n/`) and of each
   plugin to `i18n/<language>.ini`.
2. Set `language`, `name` and `plural` in `[meta]` (see `PluralRule` in
   `crates/paperpaw-i18n/src/plural.rs` for the available rules).
3. Translate the messages; keep `{placeholders}` unchanged.
4. Register the catalog in the plugin's `register` function and run
   `cargo test`: the tests report missing, unknown and mismatched messages.

## Adding a device profile

Emulator profiles live in `shells/desktop/src/profiles.rs`. Please state
where the specifications come from and mark values you could not verify.

## Commits and pull requests

- Use [Conventional Commits](https://www.conventionalcommits.org/)
  (`feat:`, `fix:`, `docs:`, `refactor:`, `test:`, `chore:` …), with an
  optional scope such as `feat(sdk): …`.
- Keep pull requests focused; include tests for new behaviour.
- Sign off your commits (`git commit -s`) to certify the
  [Developer Certificate of Origin](https://developercertificate.org/).
  By contributing, you agree that your contribution is licensed under the
  [Apache License 2.0](LICENSE).
