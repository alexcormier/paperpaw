# Security model

Who can do what with a device running QuireInk, and what protects the
reader's books, settings and passwords. It is short on purpose: a map of
the boundaries, so that changes can be judged against it. Vulnerabilities
are reported as [SECURITY.md](../SECURITY.md) says.

## What is protected

- **The firmware.** A device installs only firmware signed with
  QuireInk's key (Ed25519), checked while it is written; a new version that
  does not start cleanly is undone ([ADR 0019](adr/0019-firmware-updates.md)).
  The image quireink.org offers for flashing in the browser is signed
  too; the site and the browser check it before it is written.
  Further keys are trusted only by the reader's choice
  (`.quireink/firmware-keys.txt`).
- **Passwords and keys** (Wi-Fi, catalogs, sync accounts) are kept in
  `.quireink/secrets.txt`, veiled so that they do not show at a glance,
  **not encrypted**: whoever has the card has them. They are never shown
  in full; screens show dots and ask for them afresh.
- **Books and notes** are files on the card, as readable as the card is.

## Who can reach the device

- **Physical access (the card, USB).** Whoever holds the device can read
  and change the card and flash any firmware over USB (the chip's own
  bootloader); QuireInk does not try to stop that. Firmware built for
  testing has a remote control over USB (files in and out, presses, the
  screen); released firmware is built without it (the X4's `remote`
  feature).
- **The local network.** The device listens only while a tool asks for it
  and its screen is open (e.g. Wi-Fi transfer); such a tool shows an
  address with a key (`?k=`) that every request must carry, so others in
  the network cannot use it by guessing. Nothing listens in the
  background.
- **The internet.** The device connects out only (catalogs, sync,
  updates, news), over TLS where the address says `https`, with the
  certificates checked against the clock (set over the network first).
- **Files.** Books, fonts, pictures and catalogs are untrusted input: the
  parsers bound their memory and time, reject what they cannot read, and
  are fuzzed in CI (EPUB, text, XML, hyphenation, images, Lua).

## Plugins

- **Code plugins** are built into the firmware (or the app) and run with
  the kernel's rights: all of the card, the network, the passwords
  through the SDK. They are trusted as the firmware is; a plugin store
  will deliver them only in signed firmware (§12.8 of the planning).
  Planned WebAssembly plugins would run without these rights unless
  granted ([ADR 0004](adr/0004-everything-is-a-plugin.md)).
- **Data plugins** (folders with `plugin.ini`) carry no code: translations,
  key maps, defaults of settings, fonts and stylesheets. They can change
  what QuireInk shows and does through those, not reach past them.

## What is not protected

- A lost or stolen device with its card: the passwords are only veiled.
- Firmware flashed over USB by someone holding the device.
- What a code plugin built in does with its rights.
