# 0026. Code plugins bound to a plugin interface version

- Status: Accepted; refines the fingerprint of
  [ADR 0024](0024-native-code-plugins.md) until the stable plugin API it
  plans for 1.0
- Date: 2026-10-23

## Context

ADR 0024 binds a code plugin file to the PaperPaw it was built for: a
fingerprint of compiler, target, SDK version and features, made by build
scripts of `paperpaw-sdk` and `paperpaw-native`. Two things go wrong
with that before the betas of 0.5:

- **Every release breaks every code plugin**, also a beta that changes
  nothing a plugin sees: the SDK's version is in the fingerprint. Betas
  come one after another, and authors would rebuild for each.
- **A build on Windows differs from one on Linux.** A build script in a
  plugin's dependencies is built for the computer, and Cargo hashes the
  computer's target into every crate's `-C metadata`, which flows into
  the SDK and the plugin's own crate (their `TypeId`s, their code). The
  plugin library rebuilds each file on Linux and compares, so an author
  on Windows never matched, however the paths were written.

## Decision

- **A plugin interface version, raised by hand.** `native::INTERFACE`
  (a `u16`) names what code plugins are built against: the SDK's items
  they may use, the layouts of the types that cross between plugin and
  PaperPaw, the firmware's table (`Api`), how a plugin is linked
  (`paperpaw-plugin.x`) and packed, and the release's compilers. It is
  raised when one of them changes, and only then. A plugin file runs on
  every PaperPaw of its interface, whichever release made it.
- **The fingerprint from constants alone.** `native::FINGERPRINT` is the
  interface in its top 16 bits, and below them a hash of the chip
  (`cfg!(target_arch)`), the pointer width, the features that change what
  a plugin's own copy of the code does with what crosses
  (`paperpaw_text::OUTLINES`; colour does not: Android and the emulator
  draw in colour, and plugins built without it run there), and
  `native::LAYOUT`: a digest, made at compile time, of the sizes and
  alignments of the types that cross (`Env`, `Registrar`, `Command`,
  `Canvas`, `Settings`, `Box<dyn Plugin>` and the others) and of where
  the firmware's table keeps each entry. No build script: the SDK's and
  `paperpaw-native`'s are gone. A file of another interface says which
  (`Fingerprint::misfit`): the X4's log and the plugin manager say so.
- **The linker script lies beside each plugin project**, as the sample
  (`samples/hello-native/paperpaw-plugin.x`) and `tools/plugin-files`
  have it; `.cargo/config.toml` links with it by its rustflags, which are
  not in `-C metadata`. The copy is part of the interface: a test keeps
  the repository's copies equal to `paperpaw-native`'s.
- **The compiler is pinned by the release, not hashed.** Without a build
  script no constant knows the compiler. Until 1.0 the SDK's Rust types
  cross as they are, and a new compiler may lay them out or call them
  otherwise in ways no size or alignment shows (the order in a vtable,
  how a `repr(Rust)` value is passed). So the compiler is part of the
  interface: the X4's from `shells/xteink/rust-toolchain.toml`, the X4
  Pro's from `versions.env`, named by `native::TOOLCHAIN` (a test keeps
  them equal), and a new one raises the interface. The layout digest
  catches most of what a compiler changes; the pin covers the rest.
- **CI fails where what plugins see changes without a raise.**
  `crates/paperpaw-sdk/interface.txt` keeps, for the interface, the
  layout digest, the fingerprint (of the SDK's own test build), the
  linker script's hash and the compilers; `interface-api.txt` keeps the
  SDK's items (`api.txt`) as they were. A test
  (`cargo test -p paperpaw-sdk --test interface`) fails where an item
  went or changed, or one of the others changed: raise `INTERFACE` and
  write both anew (`PAPERPAW_BLESS=1`). New items are free: a plugin
  built before does not use them.
- **Where a plugin's code lands binds it to nothing.** A `.ppn` file is
  linked at a fixed base and relocated where it is loaded; it reaches the
  firmware only through the table whose address the loader writes into
  it, and through what the SDK hands it. The X4's index of the plugins in
  flash names the firmware that placed them; behind another firmware the
  plugins are placed anew from the same files, at the next start. The X4
  Pro relocates into PSRAM at every start.

## Consequences

- Authors rebuild when the interface is raised, not with every release
  or beta. The release notes and the plugin library say which interface
  a release has; the store lists a build for each fingerprint, so a
  reader not updated still finds the one for it.
- A plugin built on Windows is the same as one built on Linux, given the
  paths are written as the same (`build.sh`, the rebuild, and
  `tools/reproducible-plugin.py`, which CI runs on both).
- Raising the interface stays a decision of its own: a pull request that
  changes a type that crosses fails until it raises it.
- The stable, C-shaped plugin API of ADR 0024 for 1.0 is still to come;
  then its major version becomes the interface version.
