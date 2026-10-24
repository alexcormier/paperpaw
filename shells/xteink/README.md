# PaperPaw on the Xteink X4

The firmware for the Xteink X4 and the X4 Pro, in beta with
0.5.0-beta.4. See
[ADR 0013](../../docs/adr/0013-x4-firmware.md) for how it is built; its
hardware parts are in [`paperpaw-x4`](../../crates/paperpaw-x4) and
[`paperpaw-fat`](../../crates/paperpaw-fat), tested on a computer.

> Save the stock firmware first (below), so the X4 can always go back.

It is for the X4 and, built with the feature `x4-pro`, for the X4 Pro
(ESP32-S3); the display controller (SSD1677, UC8179 or UC8279) is
found at start. A build for the X4 Classic is there too, but not part
of 0.5.0-beta.4 ([below](#the-x4-classic)).

## Build

The Rust release pinned in this folder's `rust-toolchain.toml` builds
it (rustup fetches it, with the target `riscv32imc-unknown-none-elf`):
native code plugins run only on firmware built by the same compiler
(ADR 0026), so each release names its toolchains; `versions.env` names
the X4 Pro's. mbedTLS is built with the firmware (only what its HTTPS
client uses, see `Cargo.toml`): that takes CMake, Ninja and, for the X4,
clang (`versions.env` names the version CI uses), found on the `PATH`
or by `CLANG_PATH`, with `CMAKE_GENERATOR=Ninja` set.

```sh
cd shells/xteink
cargo build --release
```

## Save the stock firmware

Install [espflash](https://github.com/esp-rs/espflash)
(`cargo install espflash`), connect the X4 by USB and read its whole
flash (16 MB) into a file. Keep that file.

```sh
espflash read-flash 0x0 0x1000000 x4-stock-firmware.bin
```

To go back to it later:

```sh
espflash write-bin 0x0 x4-stock-firmware.bin
```

## Flash and run

```sh
cargo run --release
```

flashes the X4 and shows its log. Books go on a microSD card formatted
with FAT32, e.g. in `Books`. PaperPaw does not read exFAT, which large
cards often come with.

## Remote control

Over USB, [`tools/x4-remote.py`](../../tools/x4-remote.py) presses the
X4's buttons, takes screenshots and prints what the screen shows, with
the steps of the emulator's scripts. Firmware built here has it (the
`remote` feature, on by default); released firmware leaves it out. The
transfer over the cable, which installs plugins and adds books once the
reader allows it, is in every firmware
([docs/usb-transfer.md](../../docs/usb-transfer.md)).

It also waits for and checks what the screen shows (`until:`, `expect:`,
with an exit code for scripts), taps and swipes (the X4 Pro), runs
actions, sends events, reads and sets settings and the clock, lists the
plugins, actions, settings and screens open, says what each plugin took
(`usage`), works with the card (folders pushed and pulled, files checked
by their MD5, `ls`, `rm`, `mv`), changes the log's level, restarts the X4
or makes it panic and waits until it is back. `--help` lists it all; the
protocol is in `paperpaw_x4::remote`.

```sh
python tools/x4-remote.py --script "page-forward,right,describe" --screenshot out/x4.png
python tools/x4-remote.py --script "run:update.open,until:Search online,expect:Wi-Fi transfer"
python tools/x4-remote.py --push my-fonts .paperpaw/fonts --info
```

## What works, what is to come

- The screen (black and white, fast and full refreshes), the seven
  buttons, the battery, the SD card with long file names, sleep (the
  power button switches it off and on again, where it was), light sleep
  between button reads on battery.
- Wi-Fi (ADR 0016): on when something needs the network, chosen on the
  Wi-Fi page in the settings; HTTP and HTTPS (ADR 0017) for plugins,
  downloads straight to the card.
- Updates from the card (ADR 0019): two app partitions
  (`partitions.csv`, which `espflash.toml` names), signed update files
  made with `paperpaw-sign`, a failed update undone at the next start;
  or fetched over the network by the `update` plugin.
- The UltraChip UC8179 of newer X4s as well as the SSD1677, whichever
  answers at the start.
- Code plugins as files on the card (ADR 0024). After failed starts, or
  with *Back* held at power-on, PaperPaw starts in safe mode without
  them. On the X4 Pro, hold *Home*.
- A crash is written to `.paperpaw/crashes.txt` on the card, for bug
  reports.
- The time of day from the network. The X4 forgets it while it is off.
- Wi-Fi set up with a phone, or the network the web flasher was given.

## The X4 Pro

Its chip is an ESP32-S3 (Xtensa): install the `esp` toolchain with
[espup](https://github.com/esp-rs/espup), in the versions of
`versions.env` (`espup install --targets esp32s3 --toolchain-version
1.97.0.0 --crosstool-toolchain-version 14.2.0_20241119`; newer GCCs
reject esp-hal's linker scripts), load its environment (`export-esp.ps1`
or `export-esp.sh`), then

```sh
cargo +esp x4pro          # build
cargo +esp x4pro-run      # flash and show the log
cargo +esp x4pro-clippy   # lints
```

The first flash over the stock firmware needs its power button held
until the flashing ends (its boot loader does not keep it on); PaperPaw
holds its power through resets, so later flashes need nothing held. Its
USB comes through the pogo pins (Espressif's `303A:1001`).

What works: the screen (black and white, and four grays for pictures),
touch and the home key, the page and power buttons,
the SD card (SDMMC), the battery from its fuel gauge, the clock chip,
the front light (brightness and warmth, in the quick panel), sleep, the
network as on the X4, the UC8279 of other X4 Pros. To come: battery life
measured.

## The X4 Classic

Not part of 0.5.0-beta.4: built, but not yet tried on one.

The X4 Classic ("X4 v2") is an ESP32-S3 board like the X4 Pro, with the
X4's buttons and no touch or front light; an X4 image cannot run on it.
With the `esp` toolchain as for the X4 Pro:

```sh
cargo +esp x4classic          # build
cargo +esp x4classic-run      # flash and show the log
cargo +esp x4classic-clippy   # lints
```

It answers on USB (through its pogo adapter) as the X4 Pro does, so
check which one is connected before flashing. Its pins come from the
FreeInk SDK's and Papyrix's notes; not yet tried on one.
