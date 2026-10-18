# QuireInk on the Xteink X4

The firmware for the Xteink X4 (ESP32-C3). See
[ADR 0013](../../docs/adr/0013-x4-firmware.md) for how it is built; its
hardware parts are in [`quireink-x4`](../../crates/quireink-x4) and
[`quireink-fat`](../../crates/quireink-fat), tested on a computer.

> Save the stock firmware first (below), so the X4 can always go back.

It is for the X4 with an SSD1677 display controller. Not for the X4 Pro,
which has another chip.

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
