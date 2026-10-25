# 0031. A developer link over USB for plugins

- Status: Accepted; reserved in plugin interface 4, carried out later
- Date: 2026-10-25

## Context

Test builds of the X4's firmware have a remote control over USB
(`shells/xteink/src/remote.rs`, feature `remote`): files in and out,
presses, the screen, the heap, timings. Plugin authors need the same on
released firmware, to try their plugins on a device without building
PaperPaw. Built into every firmware it would cost flash on the X4 and
open the device to whatever is plugged in.

## Decision

The remote control becomes a store plugin, *USB developer tools*, that
only those who want it install. The firmware brings a link over USB to
plugins (`paperpaw_sdk::developer`) and keeps it safe itself:

- **The link**: a plugin declaring `Uses::DEVELOPER` opens it
  (`developer::open`), writes bytes (`write`), asks for the frame shown
  (`screen`) and closes it (`close`), all as `Command::Developer`.
  Everything that happens comes on the plugin's event as a `DevEvent`:
  opened, bytes received, the screen, closed (`DevClose`), failed
  (`DevError`). Devices that have the link say `Capability::USB_LINK`.
- **The rest is the plugin's already**: presses (`Command::Input`),
  actions (`Command::RunAction`), files (`Env::storage`) and the memory
  (`Env::memory`) need no commands of their own.
- **Safety, by the kernel**: the link opens only while the reader
  allowed developer tools under *Customise > Developer*, confirmed in a
  question on the device; the status line shows it while it is open; a
  restart ends it unless the reader kept it on; secrets are never read
  over it (the files of `secrets` are refused, as `remote.rs` refuses
  them); the firmware's own log and the update path go first on the
  wire.
- **Room**: `DevCommand`, `DevEvent`, `DevClose` and `DevError` are not
  exhaustive, `Command::Developer` boxes its request, and
  `Command::Extended` takes what comes after.

In interface 4 only the types are there: every `open` is answered
`DevError::NotAvailable`, so a plugin built now learns it cleanly and
works unchanged once a firmware carries the link out. `remote.rs` stays
as it is for test builds until then; it may move behind this API later.

## Consequences

- The plugin can come after beta.8 without a new plugin interface.
- The firmware stays slim: the link is a few hundred bytes of kernel,
  the tools are a download.
- What the kernel enforces (the setting, the question, the indicator,
  the end at a restart, secrets) has to be built before the link is
  switched on; until then nothing goes over USB for plugins.
