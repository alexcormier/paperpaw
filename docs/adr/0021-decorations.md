# 0021. Decorations of text

- Status: Accepted
- Date: 2026-10-19

## Context

Plugins want to mark text in books: words being learnt, the names of
characters, search hits, one's own highlights, the sentence read aloud.
Until now the reader drew each of these itself, each in its own way
(highlights in a light gray that a black and white panel shows as white,
the sentence read aloud underlined, links underlined in gray), and a
plugin had no way to mark anything. What a decoration looks like was
fixed in code; a reader of one's own would have had to draw all of it
again.

- Some marks change widths (bold, italic, another size or font): they
  need the page laid out again. Others are only looks (a background, a
  line under the text, a frame): they can be drawn over a page as it is,
  cheaply, and switched on and off at once.
- Panels differ: black and white alone (the X4), four or sixteen grays,
  colour. A plugin should not have to know which.
- Readers want to choose how marks look, as they choose everything else;
  themes should be able to set it, profiles and rules to switch it.
- The X4 has little memory: nothing may be worked out for a whole book.

## Decision

- **Two levels.** What changes widths stays a text filter before layout
  (`Registrar::add_text_filter`, C10). Pure looks are *decorations* after
  layout: a plugin's *decorator* gets the words of a page (their text
  and content positions) and gives back ranges, each with a *role*;
  nothing is laid out again.
- **A style model of its own**, `quireink_sdk::decorations::Decoration`:
  a background, the text's ink, a line under the text (solid, dotted,
  dashed, wavy, double) and its thickness, a frame, struck through. Tones
  are named (light gray to black, yellow, green, blue, pink, orange,
  red); a panel shows them as well as it can: colours on colour panels,
  their lightness elsewhere, backgrounds dithered where there are few
  grays. The SDK draws decorations (`draw_behind`, `draw_over`), so every
  reader draws them the same.
- **Roles, not looks.** Plugins name a role (`vocabulary.learning`) with a
  default look. Each aspect of a role is a setting of its own
  (`<role>.background`, `.ink`, `.line`, `.thickness`, `.frame`,
  `.struck`), in the group *Marks in the text* (`groups::MARKS`), a
  section per role: users change them in the settings, themes (data
  plugins) set their defaults, profiles and rules switch them like any
  setting.
- **Many sources at once.** Decorators run in their order; where two
  mark the same word, their decorations are combined aspect by aspect,
  the earlier one winning an aspect both set (a background from one, a
  line from the other). A decorator may name a switch (a setting) that
  turns it on and off, e.g. in the reader's menu.
- **The reader's own marks are sources too:** highlights
  (`core.highlight`), links (`core.link`), the search hit
  (`core.search-hit`) and the sentence read aloud (`core.aloud`) are
  roles of the kernel, drawn through the same path. The old setting for
  how highlights look is the highlight role's settings.
- **Taps.** A decorator that asks for taps hears
  `events::DECORATION_TAPPED` when a word it marked is tapped (instead
  of the page turning) or, with buttons, chosen in the reader's mode for
  links and marks.
- **Per page, never per book.** Readers ask decorators for a page when it
  is laid out or a decorator's signature changes (as text filters have
  one), and keep the answer with the page. Positions are content
  positions (section and offset), so they fit any layout and any reader.
- **A word search for all,** `quireink_sdk::words::WordSet`: whole words,
  without regard to case, phrases of several words, many at once, so
  plugins do not each write their own.

## Consequences

- Highlights show on black and white panels (dithered), and how they
  look is the reader's choice, with links, search hits and reading aloud.
- Plugins mark text with a few lines and know nothing of the reader or
  the panel; a reader of one's own draws all marks by calling the SDK.
- Bold or larger text as a mark stays a text filter, with the cost of
  laying the page out again.
- Every role adds six settings; they live in one group, out of the way.
