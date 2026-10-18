# 0019. Firmware updates

- Status: Accepted
- Date: 2026-10-18

## Context

The X4's firmware is QuireInk with all its code plugins built in. Until
now a new version needed a computer: the web flasher or espflash over
USB. Readers should update on the device, and nothing about an update
may leave a device that does not start.

- The X4 has 16 MB of flash; the firmware takes 5.3 MB.
- The bootloader espflash brings (ESP-IDF's, `release/v6.1`) chooses
  between app partitions by the OTA data partition and skips images
  marked bad, but it is built without rollback: it does not mark a new
  image on trial, nor undo one that never confirmed itself.
- Anyone in the same network, or anything that changes a file on the
  card, could otherwise put firmware on a device.

## Decision

- **Two app partitions of 7.9 MB** (`shells/xteink/partitions.csv`),
  chosen by the OTA data partition. The web flasher writes them once;
  from then on updates come without a cable. `cargo run` erases the OTA
  data, so the firmware flashed is the one that starts.
- **The firmware undoes a failed update itself** (`shells/xteink/src/ota.rs`).
  A new image's first start marks it on trial, its first frame shown
  marks it good. An image found on trial at a start (it never got to its
  first frame, e.g. it panicked) is marked bad and the device restarts;
  the bootloader skips it, and the firmware before it takes its place
  back and says so.
- **Signed update files** (`quireink-update`): the app image followed by
  a trailer with its length, the signer's Ed25519 public key and the
  signature over both. The X4 checks the signature while it writes the
  image, a piece at a time, and chooses the new image only if it holds.
  It installs images signed with QuireInk's key, built in, or with a key
  the reader chose to trust (their own builds), listed on the card.
  `quireink-sign` makes keys, signs images and shows what a file holds;
  releases are signed with a key kept as a secret of the repository.
- **The shell installs, a plugin asks** (`quireink_sdk::firmware`): the
  shell does what `Command::Firmware` says (install a file, restart) and
  reports its progress; the `update` plugin finds update files in
  `updates` on the card, says what they are and who signed them, offers
  a newer one at start, and says after the restart how it went.

## Consequences

- An update from the card takes about 45 s on the X4, its signature
  checked on the way; a damaged or changed file is refused before it is
  chosen.
- An image that does not get to its first frame comes back by itself:
  tried on the X4 with an image that panics at start.
- Trusted keys are on the card: who can change the card could add one.
  So can anyone with the device and a USB cable, who can flash it
  anyway.
- Devices flashed before the two partitions came need the web flasher
  once more; until then the update page is not offered.
- Updates over the network (from quireink.org, O3 to O6 in the
  planning) build on this: they only fetch the file.
