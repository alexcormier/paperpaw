# 0020. SDK compatibility

- Status: Accepted
- Date: 2026-10-18

## Context

Plugins are built against one version of `paperpaw-sdk` and declare it
in their `Manifest` (`sdk`); the kernel loads a plugin only if its
version satisfies that one, following SemVer. (Superseded in part: the
`Manifest` names no SDK any more. A plugin with code is loaded by its
file's fingerprint and the plugin interface, [ADR 0026](0026-plugin-interface-version.md);
a data plugin by its `formats`; a built-in one is built with the kernel.) That promise is only as
good as the versions: a change that breaks plugins in a release that
says it does not would break them on readers' devices, where nobody can
rebuild them.

- Whether a change breaks plugins is easy to miss by eye: a new field in
  a public struct, a new variant in a public enum, a bound added to a
  trait.
- The SDK is built from several crates (`paperpaw-platform`,
  `paperpaw-graphics`, `paperpaw-text` and others it re-exports, and the
  helpers plugins use beside it: `paperpaw-digest`, `paperpaw-json`,
  `paperpaw-qr`, `paperpaw-sqlite`).
- Releases come from tags, with a commit that sets the version; between
  them, `main` changes the SDK every day.

## Decision

- **`main` carries the next version as a pre-release**, e.g.
  `0.4.0-dev` once 0.3.0 is out and something breaks; the release
  commit drops `-dev`. Firmware and apps built from `main` say so, and
  an update to the release counts as newer (`compare_versions`).
- **`cargo-semver-checks` in CI** compares the crates plugins build on
  with their last release on crates.io. A change that breaks them fails
  the build unless the version already says the next release breaks
  (the next minor version before 1.0, the next major after).
- **The public API as a file**: `crates/paperpaw-sdk/api.txt` lists
  every public item of the SDK (`cargo-public-api`), and CI fails if it
  is out of date. Every change to what plugins see is a diff in the
  commit that makes it, to read and to question.
- **Deprecated before removed**: what goes is marked
  `#[deprecated(since = "…", note = "…")]` with what to use instead, and
  stays until the next breaking release (the next minor before 1.0, the
  next major after). `#[non_exhaustive]` on structs and enums that will
  grow lets them grow without breaking.
- **The kernel's part behind a feature**: methods only the kernel and
  the shells call (registering, setting values, binding: e.g.
  `Settings::{define, set, set_scope}`, `Menus::{add, customize}`,
  `Fonts::add`, the registries' `add`) come with the SDK's `kernel`
  feature, off by default; paperpaw-core, paperpaw-app and the shells
  turn it on. Plugins see these types only to read through `Env`, and a
  plugin built on its own cannot call them. The docs show them marked
  with the feature (`doc(cfg)`); `api.txt`, made without features,
  leaves them out, so they may change with the kernel.

## Consequences

- A breaking change cannot slip into a release that promises
  compatibility; the version says what the next release is.
- The API file has to be brought up to date with the change. That is a
  step more, but it is the step that makes a change to the surface
  visible.
- The API file is made with a nightly toolchain (rustdoc's JSON output)
  pinned in CI, so its output stays the same between machines.
- New crates are checked from their first release on.
- A plugin's tests that build an environment as the kernel does (their
  own `Settings`, `Fonts`, `Menus`) turn on `kernel` in their
  dev-dependencies, as the SDK's tests do.
