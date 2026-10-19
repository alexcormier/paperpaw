# 0018. Font packs for other scripts

- Status: Accepted
- Date: 2026-10-18

## Context

The X4 draws text from bitmap font packs built into its firmware: Literata
(four faces) and Atkinson Hyperlegible Next, each in seven sizes, with
Latin letters only. Russian, Ukrainian or Greek books, and keyboards for
them, showed question marks.

- Literata has Cyrillic and Greek; Atkinson Hyperlegible Next has neither.
- Cyrillic and Greek for Literata's four faces would add about 0.8 MB to
  the firmware (Greek Extended, for older texts, more); it is 5.2 MB of the
  6.4 MB an app partition has for updates over the air, and other scripts
  (Hebrew, Arabic, Chinese, Japanese) would never fit.
- The X4 has 16 GB of card and about 190 KB of heap, far too little to
  load a pack of several hundred kilobytes.

## Decision

- **Packs for other scripts are data plugins on the card**, built from
  Literata by `tools/script-pack.py` (`paperpaw-fontgen --chars`), each
  pack naming the built-in font it extends in `plugin.ini`
  (`[extends] literata.ppfp = fonts-default.literata`).
- **Read a glyph at a time** (`paperpaw_text::StoredPack`): only the
  characters a pack holds, and the advances of the sizes in use, stay in
  memory (a few hundred bytes each); a glyph is read from the card when
  first drawn and kept in the glyph cache (12 KB on the X4, empty until a
  pack is used).
- **Fonts ask others in turn** for a character they lack: their
  extensions, the fallback font (the reading font), the fallback's
  extensions (`FontHandle`). So menus in Atkinson show Cyrillic from
  Literata's pack.
- Whatever cannot be drawn is not offered: the keyboard lists only the
  layouts whose letters the fonts draw (`FontHandle::has_char`).

## Consequences

- Cyrillic and Greek (with Greek Extended) take 3.9 MB on the card, no
  flash, and about 1 KB of heap per face plus the glyph cache. On the X4 a
  page of Russian and Greek is drawn in 0.3 s.
- Any script can come the same way, as a plugin, without a new firmware.
- Packs are copied into `plugins/` on the card (or with Wi-Fi transfer);
  the plugin library will offer them.
- Devices with scalable fonts (the Boox) do not need them; there the
  packs are only asked for what the system's fonts lack.
