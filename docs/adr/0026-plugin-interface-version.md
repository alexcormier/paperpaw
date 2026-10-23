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
- **The compiler is pinned by the release, and each file names its
  own.** Until 1.0 the SDK's Rust types cross as they are, and a new
  compiler may lay them out or call them otherwise in ways no size or
  alignment shows (the order in a vtable, how a `repr(Rust)` value is
  passed). So the compiler is part of the interface: the X4's from
  `shells/xteink/rust-toolchain.toml`, the X4 Pro's from `versions.env`,
  Android's from its `versions.env`, named by `native::TOOLCHAIN` (a test
  keeps them equal, and the repository's plugin projects pinned to the
  X4's), and a new one raises the interface. Without a build script no
  constant of the SDK knows the compiler, but every object Rust makes
  names it (`rustc version 1.99.0 (b940084d7 2026-09-28)`, in `.comment`).
  `paperpaw-sign pack` takes it from there, refuses a plugin built by
  none or by two, and writes a hash of it into the file's header (format
  2). The firmware knows its own from its build script, which a shell may
  have, and loads only files of it. The emulator checks a library built
  for the computer the same way, by the release an ELF library names or
  by the commit in the paths of its `std`.
- **CI fails where what plugins see changes without a raise, and any
  change raises it.** The crates whose types cross are the SDK and every
  crate of PaperPaw it depends on (`crates/paperpaw-sdk/interface/
  crates.txt`; `native::CROSSING_CRATES` names the same). Each keeps
  `api.txt`, its public API, and `shape.txt`, the shape of its public
  types: their fields, variants and trait items in the source's order,
  private fields too, each with its type (`tools/interface.py`, from
  rustdoc's JSON, on the nightly of CI). `api.txt` alone does not do: it
  is sorted, so an inserted variant or two fields of one type swapped do
  not show, and it shows no private field. `interface/` keeps both as
  they were when the interface was last raised, and `interface.txt` the
  linker script's hash and the compilers. A test
  (`cargo test -p paperpaw-sdk --test interface`) compares them. By
  default a change raises the interface: a field, a variant or an item
  of a trait that was there, a change of order, of a private field or of
  a signature, anything gone. Free is only what no plugin built before
  can rely on: a new type, module or re-export, a new function or
  constant outside a trait, a new `impl`, and the members of a type that
  is itself new. A new item of a trait is never free: the firmware would
  call it through the table of a plugin built before, which does not have
  it, and the same holds for the firmware's own trait objects a plugin
  is handed. Either way the files are written anew (`PAPERPAW_BLESS=1`).
- **The layouts are checked on each chip by the fingerprint.**
  `native::LAYOUT` is made by the compiler for the chip it builds for,
  so a plugin for the X4 has the X4's layouts in its fingerprint, and the
  X4 loads only files with its own. The interface's files do not keep a
  digest of the computer's layouts: those differ from the chips'
  (`usize` is 32 bits there) and change with the computer's compiler,
  while what changes a type's layout on a chip is either in its source,
  which the gate sees, or the compiler, which the file names.
- **Services found by name only within the interface.** A service is
  found across builds by its type's name only where that type is made of
  the crates that cross, `core` and `alloc` alone, and is of the same
  size and alignment. A type of a plugin's own crate may be another one
  of the same name in another build, so such a service needs a contract
  (`ServiceType`), whose key its author raises when the type changes.
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
  changes a type that crosses fails until it raises it. Raising is the
  default, so the gate errs on the side of a new interface: a private
  field renamed raises it too.
- The gate sees types and signatures, not behaviour. A plugin carries
  its own copy of the SDK's code, so a function whose body changes what
  it writes into a value both sides read (a format, an invariant) is the
  same item to the gate, and such a change raises the interface by hand.
- Interface 1 was never released: 2 is the first, with the files of
  format 2 that name their compiler.
- The stable, C-shaped plugin API of ADR 0024 for 1.0 is still to come;
  then its major version becomes the interface version.
