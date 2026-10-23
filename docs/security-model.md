# Security model

Who can do what with a device running PaperPaw, and what protects the
reader's books, settings and passwords. It is short on purpose: a map of
the boundaries, so that changes can be judged against it. Vulnerabilities
are reported as [SECURITY.md](../SECURITY.md) says.

## What is protected

- **The firmware.** A device installs only firmware signed with
  PaperPaw's key (Ed25519), checked while it is written; a new version that
  does not start cleanly is undone ([ADR 0019](adr/0019-firmware-updates.md)).
  The image paperpaw.org offers for flashing in the browser is signed
  too; the site and the browser check it before it is written.
  Firmware signed with another key (one's own builds) installs only
  when the reader accepts that key, shown by its fingerprint, at that
  install, and an older firmware only when the reader chooses it, told
  it is older: nothing on the card makes a key trusted.
- **Passwords and keys** (Wi-Fi, catalogs, sync accounts) are kept in
  `.paperpaw/secrets.txt`, veiled so that they do not show at a glance,
  **not encrypted**: whoever has the card has them. They are never shown
  in full; screens show dots and ask for them afresh. Wi-Fi networks the
  web flasher gives with the firmware (X4s) go to the card as `wifi.ini`,
  are kept with the passwords at the next start and the file removed;
  the flash keeps them only until the card has them.
- **Books and notes** are files on the card, as readable as the card is.

## Who can reach the device

- **Physical access (the card, USB).** Whoever holds the device can read
  and change the card and flash any firmware over USB (the chip's own
  bootloader); PaperPaw does not try to stop that. Firmware built for
  testing has a remote control over USB (files in and out, presses, the
  screen); released firmware is built without it (the X4's `remote`
  feature).
- **The local network.** The device listens only while a screen that
  serves a page is open (the Wi-Fi transfer, typing on a phone, the
  flash cards' *Edit with your phone*, setting up Wi-Fi with a phone).
  Nothing listens in the background. The transfer, typing on a phone
  and the flash cards show an address (as text and a QR code) with a key
  drawn anew each time (`?k=`, 65 bits) that every request must carry;
  after five wrong keys no request is answered for a minute
  (`paperpaw_sdk::served`).
- **The reader's own network.** Where PaperPaw runs Wi-Fi itself (the
  X4s), the Wi-Fi transfer without a router and the setup of Wi-Fi with
  a phone open a network of the device's own (WPA, its password drawn
  anew each time and shown only on the screen, as a QR code to join).
  The Wi-Fi setup serves its page only there and takes only the form it
  gave out. The network closes with the screen, or after ten minutes
  in which nothing comes.
- **The internet.** The device connects out only (catalogs, sync,
  updates, news), over TLS where the address says `https`, with the
  certificates checked against the clock (set over the network first).
  An update search sends the chip, the version, the plugin crates built
  in, the fingerprint code plugins need and the code plugins installed
  with their versions, no identifier: when the
  reader asks, and once a day while an update found waits for plugins
  ([ADR 0022](adr/0022-plugins-in-updates.md)). The store asks for its
  list with the kind of device and the language, no identifier either.
- **Files.** Books, fonts, pictures and catalogs are untrusted input: the
  parsers bound their memory and time, reject what they cannot read, and
  are fuzzed in CI (EPUB, text, XML, hyphenation, images, Lua).

## Plugins

- **Code plugins** run with the kernel's rights: all of the card, the
  network, the passwords through the SDK, and past the SDK, as native
  code without a sandbox. They are built into the firmware (or the app),
  or on the X4s come as plugin files (`code.ppn`) from the store, with a
  firmware update or from the web flasher
  ([ADR 0024](adr/0024-native-code-plugins.md)). A published file is
  signed by its author (Ed25519, `paperpaw-sign`); PaperPaw's own plugin
  files are signed with the project's plugin key. The store and a
  firmware update install a file only when it is made for this firmware
  and signed by the key the plugin library names for its author; the device remembers
  that key at the first install, and an update signed by another waits
  until the reader, warned, installs it anyway. From the web flasher the
  firmware also takes a file not signed (one's own build), but only for
  a plugin that has no author's key remembered yet. Only code installed
  so runs: a file put into a plugin's folder by hand is not loaded (test
  firmware can name one over USB). Code plugins are trusted as their
  author is.
- **Data plugins** (folders with `plugin.ini`) carry no code: translations,
  key maps, defaults of settings, presets, menus, actions made of steps,
  marks in the text, icons, steps of the first-run setup, lines for
  other plugins' points, fonts and stylesheets. They can change what
  PaperPaw shows and does through those, not reach past them, and never
  set a setting that keeps the device safe (such as the lock's).

## The screen lock

The lock (a PIN, a pattern or a password, asked for at start and on
waking) keeps a person who picks up the device from using it: from
reading what is open, changing settings or seeing passwords on screen.
It is a convenience, not encryption: the card is as readable as without
it, and whoever takes the card out or flashes firmware gets past it. The
code is kept as a salted hash, never as it is.

Plugins are trusted (above), so the lock does not guard against them.
What the SDK makes sure, for plugins used honestly:

- **Codes are the lock's alone.** While the lock guards the device, what
  its screen or the keyboard it opened gives (the password typed) goes
  to the lock and that keyboard only: no other plugin's subscription,
  screen, bar or rule hears it. The kept code is a secret no other
  plugin is given.
- **Its settings are its own.** Settings marked guarded (the lock's kind
  and when it locks) are changed only by their plugin and the settings
  editor: not by presets, power modes, data plugins' defaults or other
  plugins' commands.
- **Nothing gets past it.** Screens opened while it guards go below it;
  the actions, notices and bars of other plugins wait; only the lock's
  own input filters see input. A screen of another plugin left above it
  (a sleep screen) gets no input and closes at the next input or as the
  device wakes, so the reader is never locked out.

The slots stay replaceable: a keyboard or a settings app put in place of
PaperPaw's is a trusted customisation and sees what the original would.

## What is not protected

- A lost or stolen device with its card: the passwords are only veiled.
- Firmware flashed over USB by someone holding the device.
- What a code plugin does with its rights, built in or installed.
