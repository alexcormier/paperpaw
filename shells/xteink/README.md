# QuireInk on the Xteink X4

The firmware for the Xteink X4 (ESP32-C3). See
[ADR 0013](../../docs/adr/0013-x4-firmware.md) for how it is built; its
hardware parts are in [`quireink-x4`](../../crates/quireink-x4) and
[`quireink-fat`](../../crates/quireink-fat), tested on a computer.

> Save the stock firmware first (below), so the X4 can always go back.

It is for the X4 with an SSD1677 display controller and, built with the
feature `x4-pro`, for the X4 Pro (ESP32-S3) with a UC8179.

## Build

The stable Rust toolchain of the repository builds it (the target
`riscv32imc-unknown-none-elf` comes with `rust-toolchain.toml`):

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
with FAT32, e.g. in `Books`.

## Remote control

Over USB, [`tools/x4-remote.py`](../../tools/x4-remote.py) presses the
X4's buttons, takes screenshots and prints what the screen shows, with
the steps of the emulator's scripts. Firmware built here has it (the
`remote` feature, on by default); released firmware leaves it out.

```sh
python tools/x4-remote.py --script "page-forward,right,describe" --screenshot out/x4.png
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
  made with `quireink-sign`, a failed update undone at the next start.
- To come: 4 gray levels, the UC8179 controller of newer X4s, updates
  fetched over the network.

## The X4 Pro

Its chip is an ESP32-S3 (Xtensa): install the `esp` toolchain with
[espup](https://github.com/esp-rs/espup) (with the Xtensa GCC 14.2:
`espup install --targets esp32s3 --crosstool-toolchain-version
14.2.0_20241119`; newer ones reject esp-hal's linker scripts), load its
environment (`export-esp.ps1` or `export-esp.sh`), then

```sh
cargo +esp x4pro          # build
cargo +esp x4pro-run      # flash and show the log
cargo +esp x4pro-clippy   # lints
```

The first flash over the stock firmware needs its power button held
until the flashing ends (its boot loader does not keep it on); QuireInk
holds its power through resets, so later flashes need nothing held. Its
USB comes through the pogo pins (Espressif's `303A:1001`).

What works: the screen (black and white; four grays are shown in black
and white for now), touch and the home key, the page and power buttons,
the SD card (SDMMC), the battery from its fuel gauge, the clock chip,
the front light (brightness and warmth, in the quick panel), sleep, the
network as on the X4. To come: four grays on the UC8179, the UC8279 of
other X4 Pros, battery life measured.
