# 0029. Kerning and ligatures

- Status: Accepted
- Date: 2026-10-25

## Context

[ADR 0008](0008-fonts-outlines-and-bitmap-packs.md) set text glyph by
glyph, each after the one before by its advance: no kerning, no
ligatures. Other e-readers kern (`AV`, `To` closer) and draw `fi` and
`fl` as one glyph, and their pages look better for it. A shaping step
such as HarfRust is too large for the X4 (ESP32-C3, no FPU, some
190 KB of heap), and the layout must stay what it is: deterministic,
measured a character at a time, the same lines from any line's start.

## Decision

- **Pairs, not shaping.** Kerning is a pair adjustment: how much a
  character moves after the one before. It comes from the font's GPOS
  `kern` feature (pair positioning, formats 1 and 2, the first glyph's
  advance, at the instance of a variable font), or from the legacy
  `kern` table where a font has no GPOS kerning
  (`OutlineFont::kerning_pairs`, `OutlineFont::kern`).
- **Packs keep a table of classes.** Characters that kern alike share a
  class, as fonts keep them: a pack stores its characters, a left and a
  right class for each, and a matrix of adjustments in 1/1024 em, read
  in place by binary search, without memory. The first strike's
  reserved field points to it (0: none), so the pack format stays
  version 1: older firmware ignores the table, and older packs read
  without kerning. A list of pairs would take some 650 KB of flash for
  the five default faces; the classes take 100 KB. Packs read a glyph at
  a time from storage (`StoredPack`) are not kerned: looking pairs up
  there costs too much time, and keeping them in memory too much RAM.
- **Fonts kern their text.** `Font::kern`, `FontHandle::kern`, and
  `measure` and the drawing functions apply the pairs within the text
  they get, so the menus are kerned too. `FontHandle::unkerned` sets
  text without.
- **The layout adds pairs within a word and a face.**
  `Typesetter::kern` (0 by default) gives the pair; the layout asks it
  for two characters of one face, never across a space or a line break,
  and for the hyphen after the letter it follows. Lines then measure
  what drawing word by word, each run of a face at once, places.
  `paperpaw_text::wrap_lines_kerned` and `wrap_lines_balanced_kerned`
  break lines so.
- **Ligatures are pairs too.** With `FontHandle::ligatures`, `fi` and
  `fl` are drawn as one glyph where the font has it, and the pair is set
  as wide as that glyph: the pair adjustment is the room it saves.
  Literata's `fi` is 0.08 em narrower than `f` and `i`, so a ligature
  that had to take the room of its letters would never be drawn with
  the default font. `ff`, `ffi` and `ffl` are drawn only where they take
  the room of their letters within half a pixel. The text stays as it
  is: positions, hyphenation and search see its characters. Never with
  letter spacing.
- **Settings.** `core.kerning` and `core.ligatures` (on, advanced, per
  book) go into the page key: books are laid out again when they
  change.

## Consequences

- Pages and menus look as the type designer meant; lines break
  elsewhere than before, and page indexes are made anew once.
- The default packs grow by about 134 KB of flash (kerning tables and
  the ligature glyphs), no RAM.
- Each character costs one more lookup while laying out: two binary
  searches in flash for a pack, a walk of the GPOS lookups for an
  outline font.
- Contextual kerning, mark positioning and scripts that need shaping
  are still not done; they would need a shaping step for Tier L.
