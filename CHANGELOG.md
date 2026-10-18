# Changelog

What changes from one release of QuireInk to the next. Versions follow
[Semantic Versioning](https://semver.org); before 1.0, a minor version may
change the plugin SDK.

## [Unreleased]

- Home: the status line at the top shows the status items chosen for its
  left, centre and right (the status line widget's settings), any
  plugin's, as the reader's lines do.
- QuireInk builds with Rust 1.88 and later (it said 1.85, which was too
  old); CI checks it. A short [security model](docs/security-model.md)
  says what is protected and against whom.
- X4: a crash is written to the card at the next start
  (`.quireink/crashes.txt`, its version and why), for bug reports.
- Data plugins say which version of the formats they are made for,
  `formats = 1` in `plugin.ini`; a QuireInk that reads older ones leaves
  such a plugin out and says why. Every data file QuireInk keeps has a
  file of its version in `testdata`, read by the tests.
- SDK: a setting's unit, editor, step, slider, section and whether it is
  a secret are read with `SettingSpec::extra()` (they are kept apart, so
  that the settings take less memory: some 8 KiB less on the X4);
  `SettingSpec` is `#[non_exhaustive]`, made with its constructors and
  builder methods as before.
- HTML, FictionBook, Markdown and comic books are plugins of their own
  (`format-html`, `format-fb2`, `format-markdown`, `format-cbz`), no
  longer part of the EPUB plugin: each can be switched off or replaced
  alone.
- Drawing, text and layout take their parts together: `Canvas::fill`,
  `stroke`, `fill_round` and `stroke_round` take a `Rect` (`ui::fill` is
  deprecated), `quireink_text::draw_text` a `TextStyle` (with `bolder`,
  in place of `draw_text_bolder`), and `layout_page` and `line_start` a
  `LayoutContext`.
- The keyboard's shift, delete and space keys and the grip that moves a
  row say what they are (screen readers, remote control); plugins draw
  such buttons with `ui::draw_icon_button`.
- SDK: lists move by the navigation actions with `ListView::navigate`
  (or `ui::list_step` for a focus of one's own), and
  `ListChange::request` asks for the redraw and refresh a change needs,
  the same in every list.
- Kernel: shells take everything they are to do at once,
  `Kernel::take_io()`, as `Io` (requests, servers, sockets, speech, the
  clipboard, the radio, the firmware), matched in full so that a shell
  cannot forget one; the eight `take_…` methods are gone. On the X4,
  plugins reading the clipboard or speaking no longer wait for ever.
  Inside, each of these is a part of its own.
- SDK: `ActionSpec::new(id, title)` with `.handler(…)` and `.hint(…)`;
  the struct is `#[non_exhaustive]`, so actions can learn more without
  breaking plugins. How the row of hints shows an action (its title, a
  word of its own, an icon or nothing) is the action's `ActionHint`,
  no longer decided by the kernel.
- X4 firmware: released firmware leaves out remote control over USB
  (the `remote` feature, for testing), 34 KB less. Inside, the main loop
  is a `Firmware` with a method for each step and the pins are in one
  board module; the USB protocol, SNTP and the frame kept for the panel
  are in `quireink-x4`, tested on a computer.
- Kernel: device defaults are typed, `setting_default(&SETTING, value)`;
  `setting_default_text` takes an id and text, for settings known only
  at run time.
- SDK: its compatibility is checked. CI fails on a change that would
  break plugins unless the version says the next release breaks
  (`cargo-semver-checks`), and `crates/quireink-sdk/api.txt` lists its
  public API, so every change to it shows (ADR 0020). Builds between
  releases are the next version with `-dev`, and an update to the
  release counts as newer.
- SDK: typed answers. The answer to an HTTP request is an `HttpReply`
  (status, body, headers and the request's own parameters as
  `context`), what happens to a server a `ServerNotice`, to a socket a
  `SocketNotice`, what the reader typed a `Typed`, the folder chosen a
  `Chosen`, the option picked a `choice::Picked`. `ask::text` and
  `ask::folder` ask for them; `HttpRequest::new` and `choice::open`
  take the `Event` that brings the answer.
- SDK: typed events. An `Event<P>` names an event and what it carries (a
  struct made with `event_data!`); `r.on(EVENT, |env, data, out| …)`
  hears it, `EVENT.emit(data)` emits it, `EVENT.read(event, params)`
  reads it on a screen. Every built-in event is one: the reading events
  carry `Reading` (path, title, progress, position, page), a move
  `BookMoved`, a changed setting `SettingChanged`, and so on.
  `r.subscribe` takes any id, for events known only at run time.
- SDK: typed settings. A `Setting<T>` (`bool`, `i64` or `Text`) names a
  setting and the type of its value; `env.settings.read(&SIZE)` reads
  it, `Command::set(&SIZE, 12)` changes it, `SettingSpec::switch`,
  `number`, `choice`, `language`, `list` and `text` declare it with a
  default of its type. Settings by their ids stay for those only known
  at run time. The library folder is `library::FOLDER` and
  `library::folder(env)` for every plugin, no longer `"files.root"`
  spelled out in nine places. Registrar methods that name an item take
  `impl Into<Id>`.
- SDK: its helpers are crates of their own, so a change to one does not
  change the SDK: `quireink-digest` (MD5, SHA-1, FNV-1a), `quireink-json`,
  `quireink-qr`; plugins that read SQLite use `quireink-sqlite`.
- SDK: `Registrar` is a struct now that collects `Contribution`s (a
  `#[non_exhaustive]` enum, the large kinds boxed); `add_action` and the
  rest are shorthands for `add`. Plugins take `&mut Registrar` instead of
  `&mut dyn Registrar`. Anything can see what a plugin adds without
  QuireInk running (`Registrar::new`, `contributions()`); the kernel
  applies the list and checks key maps, translations and patterns then.
- SDK: `Env` is `#[non_exhaustive]` (new fields break no plugin). What
  the device and its shell can do is `env.caps` (`Capabilities`: touch,
  network, Wi-Fi, firmware, speech, the sleep screen, rotation, colours,
  the row of buttons, a system clipboard), replacing `env.network`,
  `env.speech` and `env.sleep`; menu entries' needs are checked against
  it (`Menus::set_capabilities`).
- SDK: QuireInk's folder is one constant, `data::DIR`, with
  `data_path!` for constants; the about page is written in its plugin's
  folder.
- Versioned data files ([data-files.md](docs/data-files.md)): every file
  QuireInk keeps on the card is listed as data, cache or another
  program's format. Data formats have a version (`quireink_sdk::data`,
  `# format 2` as the first line), are upgraded as they are read, and are
  never written over by an older QuireInk (after going back to an older
  update). Files of the current formats are kept in `testdata` folders
  and read by the tests. Profiles are kept as a proper ini file now (names
  with spaces and brackets survive); older files are read and upgraded.
- `quireink-config`: changing a key there more than once (by hand)
  changes the entry that is read, the last; `quote()` is public and the
  reader and statistics use it (a book path with a line break broke the
  reader's file).
- One FNV-1a (`quireink_digest::fnv1a64`) instead of fifteen copies.
- Firmware updates on the X4 ([ADR 0019](docs/adr/0019-firmware-updates.md)):
  two app partitions (the web flasher sets them up once), an update file
  in the folder `updates` on the card installed from Settings, *Update*
  (or offered at start) in about 45 s, then a restart. A new firmware
  that does not get to its first frame is undone at the next start.
  Update files are signed (Ed25519, `quireink-update`, made with
  `quireink-sign`): QuireInk's key is built in, one's own builds are
  installed once their key is trusted on the device. Releases carry the
  signed update file beside the image for the web flasher.
- HTTPS on the X4 ([ADR 0017](docs/adr/0017-tls-on-the-x4.md)): mbedTLS
  (TLS 1.2 and 1.3), the certificate checked against Mozilla's roots,
  which stay in flash, and against the host; refused certificates fail
  the request with the reason. One secure connection at a time (about
  40 KB); a handshake takes 1.3 to 2.7 s. Expired certificates and ones
  not valid yet are refused too.
- The X4 knows the time once online (SNTP, again every 12 hours) and
  shows it in the status line and on the home screen; the time zone is
  chosen under Settings, System (UTC on "Auto": the X4 has none of its
  own). Gone when it is switched off, until it is online again.
- Other scripts on the X4 ([ADR 0018](docs/adr/0018-font-packs-for-other-scripts.md)):
  font packs as data plugins on the card, read a glyph at a time, add the
  letters the built-in fonts lack; `tools/script-pack.py` builds Cyrillic
  and Greek (also polytonic) from Literata. With it, Russian, Ukrainian
  and Greek books and keyboards work on the X4, menus included. Fonts ask
  each other in turn for a character they lack.
- Wi-Fi transfer: an upload that fails says so on the device (*Not
  received*), instead of the last one received staying shown.
- `Kernel::open_slot` opens a slot's screen for a shell (a file to open);
  over USB the X4 opens a book with `open <path>`.
- The keyboard offers only layouts whose letters the device's fonts can
  draw: on the X4 the Russian, Ukrainian and Greek ones are left out
  until a font pack for their scripts comes (they showed question marks).
  `FontHandle::has_char` tells plugins whether a character can be drawn.
- Notes at the bottom of a screen (what a tap or a hold does) wrap to
  two lines instead of being cut (`ui::draw_note_line`, used by catalogs,
  rules and profiles); German labels that were cut are shorter (*Fortschritt
  abgleichen*, *Einstellungen suchen*). Every screen was looked at in
  German too (`tools/screens.py --lang de`).
- Keyboard: no key shows only "…" any more. Narrow buttons keep less
  room at their sides, a letter wider than its key (an `m` on the X4's
  German layout) is set smaller, *OK* is black and plain to see, the key
  to symbols says `123`, and the key to the next layout is wide enough
  for its code. Checked with every Latin layout. Hints in German say
  *Wählen*, which fits.
- Home screen, with buttons only (X4): items are moved up and down and
  made taller or lower with the page buttons, which the X4 has instead of
  up and down (before, only their width could be changed); where another
  item is in the way, a notice says so.
- Headers are light by default (a line below the title instead of a
  black bar), calmer and as the home screen and the setup look; dark
  headers remain a choice under Appearance.
- X4: texts showed question marks where the font packs lack a character
  (arrows in the battery view, names of languages in other scripts); a
  test now keeps every translation to the characters the packs carry.
- The emulator's scripts can run actions (`run:stats.open`), and
  `tools/screens.py` draws every screen for a look at all of them.
- A first-run setup (new plugin `setup`): on the first start a short
  guide through the language, the buttons, the text size, the time zone,
  Wi-Fi, the keyboard and the screen lock, a page each with a good
  default chosen, so *Next* is always right; skippable, and opened again
  from Settings (*Set up QuireInk*). Plugins add steps of their own
  (`Registrar::add_setup_step`); steps a device does not need are left
  out.
- The row of hints under the screen says what the buttons do there: only
  *Back* where nothing else does anything (battery, a book's statistics,
  Wi-Fi transfer), *Menu* in a book, *Delete* while unlocking.
- A status item *Battery when low* that shows the battery only at or
  below a level (Settings, System, advanced; 20 % by default), for a
  calm status line while there is enough.
- In lists and menus, moving on past the last entry goes round to the
  first, and back from the first to the last (turning pages still stops
  at the end).
- A setting with two choices (e.g. the clock's 24 or 12 hours) switches
  at once in the settings and menus, instead of opening a list to pick
  the other one; with more choices (a plugin may add some) it opens the
  list as before.
- A press that starts slow work shows at once that it was taken: the
  pressed button's hint turns black (or an hourglass shows in a corner)
  while the work goes on, on the X4 at no cost to it. Only where that
  work took 0.4 s or more the last time; quick screens come up as before.
- The icon font has an hourglass; any of Tabler's icons can be added by
  name (`tools/icons/subset.py`).
- Pressing again while another screen comes up no longer runs those
  presses afterwards (on the X4 they could jump far ahead): presses made
  before the new screen shows are dropped. Turning pages quickly keeps
  every press. Shells say when a frame shows (`Kernel::shown`).
- Progress sync (kosync) no longer keeps the X4's radio on while
  reading: turning pages sends the position only while the radio is on
  anyway; it is sent when the book is closed and when the device sleeps.
- A Wi-Fi sign in the status lines (home screen, book, quick panel)
  while the device is in a network: the status item `wifi.status` of the
  `wifi` plugin, before the battery by default.
- Screens come up faster on the X4: what a screen reads while it is
  drawn in bands comes from the card once, not once per band (the home
  screen after a book: 0.3 s instead of 1.9 s to draw). Plugins get this
  without any change.
- The screen lock could not be switched off with buttons: the chosen row
  was white on white. Its kinds are a list as in menus now.
- Wi-Fi transfer and Calibre on the X4: plugins' servers and sockets work
  there now (books up from a browser at about 190 KB/s, from Calibre at
  about 110 KB/s, files down at about 140 KB/s). The transfer's address is
  shown whole, on two lines if need be.
- X4: joining a network with several access points (a mesh) takes the
  strongest one, not the first one heard; joining after a start no longer
  fails a few times first.
- Calibre: the button row says "Try again" where it does something; the
  screen no longer asks to press a button by a name the device has not.
- The X4 can be told over USB to run any action (`run calibre.open`), for
  testing a tool without finding it in the menus.
- X4: requests fail with a reason instead of restarting the X4 when memory
  is short or in pieces; `heap` over USB also says the largest block that
  could be had.
- The licence check covers the X4 firmware's dependencies too.
- Releases carry the X4's firmware as one image for its whole flash
  (`quireink-x4-<version>.bin`, with its SHA-256), which quireink.org
  offers for flashing in the browser.

- Wi-Fi on the X4 (new plugin `wifi`, under Settings, Device): the radio
  is off while reading and comes on by itself when something needs the
  network (a catalog, a sync), joins the strongest network joined before
  and goes off again a minute after the last use (a setting); on USB it
  stays on. Its page lists the networks in reach to join, with the
  password typed on the keyboard, and forgets saved ones (menu button or
  a long press). Saved networks are kept with the passwords and keys.
  Plain HTTP and, since, HTTPS.
- Network for plugins on devices where QuireInk manages Wi-Fi
  (`quireink_sdk::wifi`, [ADR 0016](docs/adr/0016-network-and-wifi.md)):
  `Command::Wifi`, `Env::wifi`, and the events `core.wifi-changed`,
  `core.wifi-scanned`, `core.wifi-failed`, `core.network-wanted` and
  `core.network-idle`. Requests, servers and connections made without a
  network wait for one up to 30 seconds. Shells read a request's file to
  send themselves (`HttpCall::body_from`) and may write a download to its
  file as it comes (`HttpCall::save_to`, `HttpResponse::saved`).
- Settings: a group may bring its own page (`SettingGroupSpec::page`),
  and settings and menu entries may need a device where QuireInk manages
  Wi-Fi (`MenuNeed::Wifi`).
- `quireink-http` (new crate): the HTTP/1.1 client protocol without I/O,
  for shells with their own network stack.
- The emulator's X4 profiles pretend to manage Wi-Fi, with made-up
  networks, to try the `wifi` plugin without a device.
- Translations take a quarter of the memory: a catalog keeps where each
  message's line starts (4 bytes, 16 before) and reads key and value from
  it when looked up; about 16 KiB less on the X4, as fast.
- X4: the stack no longer runs into the memory below it (the FAT cache
  and the inflate state of pictures went to the heap); `heap` over USB
  also says how much of the stack was never used.

- Developer options: an advanced setting under System
  (`core.developer`) shows a group of settings for developers
  (`groups::DEVELOPER`), in which any plugin and shell may put its own;
  the battery's page lists what each plugin took only there
  (`battery.by-plugin`), and the X4's light sleep on battery can be
  switched off to measure the difference (`x4.light-sleep`).
- X4: on battery the processor sleeps (light sleep, about 0.1 mA) while
  there is nothing to do, waking every 30 ms to read the buttons and for
  what is due; before, it spun at full speed (about 20 mA) all the time
  it was awake. On USB it stays awake for the computer (`idle sleep
  <seconds>` over USB tries the battery's way for a while, then starts
  again).
- X4: two watchdogs start it again if the firmware stops coming by: the
  RTC's after 20 s, counting in light sleep too (a sleep that does not
  end), and a timer group's after 30 s; before, a hang kept it on until
  the battery was empty. Where it was is kept through the reset (`boot`
  over USB says it, with the reason).
- Battery use (new plugin, under Tools): records the battery about once
  a minute while the device is awake, with what it did, and shows the
  charge now, this time awake (pages, refreshes), the use on battery an
  hour awake and for 100 pages, how long it lasts at that pace, the recent
  times awake, and which plugins were shown and kept the processor busy.
  The records are CSV files in `.quireink/plugins/battery` (`log.csv`,
  and `plugins.csv` every ten minutes awake), for analysis on a computer.
- What each plugin takes: the kernel measures the time each plugin's code
  runs (screens drawing and handling input, event and action handlers),
  the time its screens are shown with the refreshes then, and the storage
  it reads and writes; plugins see it in `Env::usage`
  (`quireink_sdk::usage`), with the refreshes, input and sleeps of the
  whole device. Shells give a clock for it (`KernelBuilder::clock`) and
  say how long the panel refreshed (`Kernel::panel_busy`).
- The battery's voltage where the device measures it
  (`Battery::millivolts`: the X4, Android), and an event each time the
  battery is measured (`events::BATTERY_MEASURED`); status lines redraw
  only when the percent or charging changes.
- X4 remote: `--get` copies a file off the card (e.g. the battery logs).
- A new icon: the capital Q of Besley in ink on e-paper, as on
  quireink.org (its source is `shells/android/icon.svg`).
- X4: SD cards with a partition table (MBR or GPT), as nearly all come,
  are read; before, only cards with the file system from the first block
  were. A card in exFAT is named as such in the log.
- X4: remote control over USB for testing on the device:
  `tools/x4-remote.py` presses its buttons, takes screenshots and tells
  what the screen shows, with the steps of the emulator's scripts.
  `Kernel::capture` draws the whole screen for a shell to send elsewhere.
- What the buttons do, at the bottom of the screen: where buttons lie in
  a row below it (the X4's four), a row above them names what each does
  on the screen shown: *Back*, *Select*, arrows, or a screen's own word.
  The reader keeps the whole page: there the row shows for a few seconds
  after a button of the row is pressed. Switched off under Controls ›
  *Show what the buttons do*. Screens say when they show it
  (`Registrar::set_screen_hints`, `Command::ShowHints`) and name their
  buttons (`Screen::hint`); shells give the row with
  `KernelBuilder::button_row`.
- X4: books and other files go onto the card over USB
  (`tools/x4-remote.py --put`); code may use all 8 MB of flash the chip
  maps, not 4 shared with the fonts; the reader counts pages one at a
  time in the background, so buttons answer within half a second.
- Reader: the status lines sit close to the edge of the screen (1.5 mm,
  under Status lines › *Distance from the edge*), and the text has the
  room their margin took: two more lines a page on the X4.
- X4: the buttons are read in a timer's interrupt every 5 ms, so a press
  counts while QuireInk is busy (before, the buttons did nothing while
  the reader counted a book's pages); presses wait in a queue with the
  time they came.
- X4: a page turns in about 1.5 s instead of 4 to 6. It draws in black and
  white, what its panel shows (its four grays are still to come), in
  bands twice as tall; the frame sent is kept in runs of bytes and sent
  again after a fast refresh instead of drawn anew; gray formats blend
  glyph edges in gray, not through red, green and blue; the panel gets
  its bytes straight from the frame, in bigger pieces.
- Storage on SD cards (the X4): the 8 blocks used last stay in memory,
  and the 4 files read last stay open. The library opens in under 2 s
  instead of 24, a page is laid out in a quarter of a second.
- *Flash when screens change* (Display, advanced): off, another screen
  opens with a fast refresh and counts towards the full refresh after so
  many pages. Off on the X4, where a full refresh takes 3.5 s.
- Faster on small devices, measured on the X4 (a page now turns in about
  0.7 s): font packs find a glyph's size in whole numbers (the X4 has no
  floating point unit, and every character asked); the EPUB reader keeps
  the chapter it reads and finds the next block right after the last;
  the reader writes its position at most every 5 s (always when the book
  closes or the device sleeps) and remembers the page it looked up last;
  bands are cleared a byte at a time; glyphs of the usual weight skip
  the loop of bold ones; the X4 turns black and white bands eight by
  eight pixels at once for its panel. A long chapter of notes no longer
  runs out of memory while converted.
- X4: the display is driven with the stock firmware's sequences (as the
  FreeInk SDK records them): fast `FC`, which loads the temperature each
  time, half `D7`, full `F7`, the stock booster and border; after every
  refresh both memories of the controller hold the frame shown. Before,
  a fast refresh sometimes left a page's text on the panel: the page
  before (or after) stood under the new page number until the next full
  refresh, which looked like a page turned by itself.
- X4: the full refresh against ghosts takes the shorter waveform of a warm
  panel (1.6 s instead of 3.5); the slow one only after starting.
- Pictures of books are decoded no larger than the screen shows them,
  upright or turned (`OpenOptions::image_fit`, a `Fit` from the device;
  it was a longer side), in their proportions to the pixel: a cover on
  the X4 is kept at 480 × 676, not 1500 × 2114, and its page drawn in
  1 s instead of 3.4. A JPEG made smaller is made smaller as it is
  decoded, from the lower frequencies of its blocks, then averaged to its
  size; pictures are dithered in whole numbers. Sleep screen pictures are
  kept at the screen's size too.
- Storage on SD cards (the X4): files appended to stay open and are
  written back when they are wanted otherwise, when another file needs
  the room, or on `Storage::sync` (new; the kernel calls it before the
  device sleeps). A cover decoded in hundreds of pieces is no longer
  looked up and written back for each.
- X4: the first opening of a book with a large cover takes 5 s instead
  of 22 (and says so after 1 s), through the changes above and below.
- Pictures compressed in a book are decoded as they are inflated
  (`ZipArchive::reader`, an `EntryReader`), not inflated to a temporary
  file first; with memory short for both, as before. JPEGs decode a
  third faster: Huffman codes looked up by their first 8 bits, bits
  taken a few bytes at a time, the inverse DCT in 32-bit numbers (at
  half size with 6 multiplications a line instead of 16), and blocks of
  one level filled at once. Pictures are read and written several blocks
  of a card at a time (rows drawn in chunks of 8 KB).
- X4: the SPI bus uses DMA for transfers of 32 bytes and more.
- Storage on SD cards: whole blocks of a file pass by the blocks kept,
  several read or written in one command (`Blocks::read_blocks`,
  `write_blocks`), so a picture streamed through no longer pushes the FAT
  and the folders out; 24 blocks are kept instead of 8, and 16 files
  open instead of 8.
- The reader says when a book is opened for the first time and takes a
  moment ("This takes a moment"), then opens it: formats say whether a
  document opens at once (`FormatSpec::prepared`, `Formats::prepared`);
  EPUB does once its first chapter is converted.
- X4: the SD card is read and written by QuireInk's own driver
  (`quireink_x4::card`), a command at a time with the bus held, not a
  bus transaction for every byte as `embedded-sdmmc` did.
- X4: pages with pictures are sent to the panel's second memory as they
  were kept, packed, in 80 ms, not drawn again in 2 s: the heap has the
  64 KB more the bootloader leaves free.
- X4 remote: `--profile` shows where the time goes (the functions the
  processor was in, every 5 ms), `--trace` what is read and written.
- A button with nothing bound to its long press counts as it goes down,
  not when it is let go: pages turn as the button is pressed.
- X4: a button counts once its reading held 15 ms, and a ladder goes from
  one button to another only through nothing pressed: let go, the lower
  page button passed the level of the upper one and turned a page back
  by itself.
- X4: the buttons are read with the ADC calibrated for its offset.
  Uncalibrated, every reading came out some 1600 too high: the lower page
  button turned back, and the two left front buttons did nothing.
- A screen with large pictures (over 15 % of it, such as a book's cover
  page) is left with a full refresh: e-paper keeps strong ghosts of them
  after a fast one. Covers in a list stay below that.
- X4: on USB power, sleep shows the sleep screen and keeps the USB
  connection, so a computer can still read the log and wake it.

## [0.3.0] - 2026-10-16

- Screen lock (off by default): a PIN, a pattern or a password asked for
  when QuireInk starts and when the device wakes up, set under System ›
  *Screen lock*. The code is kept only as a salted hash. Locked, nothing
  else can be used; after five wrong tries the next one waits half a
  minute. *Lock now* locks at once from any button, gesture or rule.
  It looks like a phone's: the time and the date above, round keys
  below, rings for the digits still to come.
- `Canvas::draw_line`.
- Plugin SDK: input filters (`Registrar::add_input_filter`) see every
  button and finger before screens and key maps do, and may take it or
  run an action instead; refresh filters (`Registrar::add_refresh_filter`)
  change how the panel is refreshed. `InputEvent` and `ButtonState` are
  part of the SDK.

- Plugin SDK: services, what one plugin offers others
  (`Registrar::add_service`, `Env::services`), and points plugins define
  for others to contribute to (`Registrar::contribute`); the event
  `core.library-changed` (`library::changed`), emitted when books come
  into the library or leave it (Wi-Fi transfer, catalogs, Calibre, the
  file manager), and the library shows them at once.
- Rules can have a condition: only between two times of day, only while
  reading or while not, only on battery or while charging, only while a
  switch is on or off (hold a rule, *Only if …*). They can also set a
  number (e.g. the light to 20) and show a message.

- Drag and drop in lists, as on phones: entries of menus (*Customise
  menus*), the widgets of the sleep screen and the items of status lines
  carry a grip (≡) at their end; drag it to move the row, a bar shows
  where it lands. The status line's chosen items now come first, in
  their order. Buttons move rows as before.
- Plugin SDK: `ui::Reorder` (rows moved by dragging a grip, for any
  `ListView`), and screens get a finger before the swipes in from the
  edges do (`Screen::on_pointer`), so grips and handles at the edges
  work.
- The quick panel has its name in *Customise menus*.

- Rules know many more moments: at a time of day on chosen days (every
  day, Monday to Friday, at weekends, one day), every so many minutes,
  when a book is closed or read to its end, when the device goes to
  sleep, when the battery falls below a percentage, when charging begins
  or ends, and when a switch is turned on or off.
- Plugin SDK: events `core.book-closed` and `core.book-finished` (the
  reader emits them), and `Screen::on_close`, called when a screen is
  closed.

- Gestures as on phones: up from the bottom edge goes home, down from the
  top edge opens the quick panel, in from a side goes back (while reading
  it turns the page). The quick panel holds night mode, the light,
  turning the screen, refreshing the screen, a screenshot, sleep and the
  settings, with the time, date and battery above (what shows there and
  how large is set under Appearance); plugins and the menu editor change
  what it holds. *Refresh the screen* (`core.refresh-screen`) clears
  e-ink ghosting with a full refresh, from the quick panel or any button
  or gesture it is bound to. Swipes in from the edges come before the screen sees them.
- More gestures as on phones: up on the home screen shows every tool at
  once (*All tools*, `home.all-tools`), down on it searches the library,
  and two fingers apart or together make the text larger or smaller
  while reading (`pinch.out`, `pinch.in`, in a section *Two fingers* of
  *Buttons and gestures*). The first home screen on a touch screen
  explains the gestures once (*Hint on the gestures* under Appearance
  shows it again).
- Gestures per tool: as standard, the tool's own (gestures go to it, a
  swipe in from an edge needs a second one: for games and drawing) or
  off. A tool sets its default (`gestures.policy` in its key map); the
  user changes it in *Buttons and gestures*.
- *Buttons and gestures* is easier to find one's way in: where a change
  applies (everywhere, home screen, reading, library, any tool), inputs
  in sections (edges, swipes, taps with a picture of the zone, held
  taps, buttons, held buttons), and the actions to choose from in groups
  with a search, *As standard* and *Nothing* first.

- The home screen is a grid, as on a phone: 4 columns by default (rows
  to fit the screen; both settings), each widget taking a rectangle of
  cells, each tool an icon in a cell of its own; empty cells stay empty.
  A long press or the pencil shows the grid: drag an item to move it, by
  its corner to size it, tap it for its options (move or size it with
  the buttons, its settings, remove it); *Add* adds widgets, tools and
  pages. The status line sits above the grid. Pages written before are
  placed in the grid by themselves.
- Widgets have more settings of their own: the clock's size (also as
  large as its cells allow), the date's size, whether the continue card
  shows its cover, its progress and a frame, and for the recently read
  books their heading, their titles, where the titles sit and how many
  books show. Tools with or without their names.

## [0.2.0] - 2026-10-16

- Books from a catalog (OPDS) or from Calibre keep the title, author,
  series and tags the source gives, where the book names none itself
  (text files, PDFs): `library::provide`.
- *Say what is chosen* (System, where the device speaks): says the row
  or button the buttons move to, with a switch's state, for using
  QuireInk without looking at the screen.
- Screens say what they show (PLANNING G11): rows with their text and
  focus, buttons, switches and headings, told by the drawing helpers as
  they draw (`Canvas::describe`, `semantics::parts`,
  `Kernel::describe`). The emulator's scripts tap by name
  (`tap-on:Display`) and print a screen's parts (`describe`).
- The library is a service (`library` in the plugin SDK): what is known
  of books (title, author, series, tags, cover, progress, and fields
  plugins add), the books read last and the favourites, kept by the
  kernel from the reading events. The library, the home screen and the
  sleep screen show the same data instead of keeping copies of their
  own; what the library kept before is taken over.
- Bookmarks and highlights shared with KOReader (`koreader` plugin), for
  books read in both on one device or in one synced library folder:
  what KOReader has in `Book.sdr` comes over when a book is opened, and
  what is new here goes to KOReader the next time it opens the book;
  removing one, or a later note, goes across too. EPUB only.
- EPUB positions as KOReader writes them, down to the character: reading
  positions synced with KOReader land on the word, not the paragraph.
- Plugin SDK: bookmarks and highlights are kept by `annotations`, for
  every plugin, with when they were made and changed; event
  `core.annotations-changed`. `Document::xpointer` and
  `Document::locate_xpointer` are exact to the character.
- Colour pictures on colour screens (ADR 0015): covers, pictures in
  books, comics, OPDS covers and the sleep screen's pictures are shown
  in colour on phones and tablets and on colour e-ink (Boox models named
  "Color" or "C", with colours strengthened for the filter); text and
  the interface stay gray, monochrome e-ink is unchanged. JPEG (also
  progressive), PNG and GIF are decoded in colour where asked. Plugin
  SDK: `ImageSink::wants_color` and `row_rgb`, `ColorFile`,
  `ColorWriter`, `ImageFile`, `image::picture_from_file`,
  `OpenOptions::color`, `Formats::options`, `Canvas::draw_rgb_row`,
  `Framebuffer::set_saturation`, `ColorPanel::boost_percent`.
- Smaller text by default: menus 9 pt (was 11), status lines 6 pt
  (was 8). Both stay adjustable in the settings.
- Dictionary: the page of a long entry no longer covers *Online*.
- Text and paper colours on Android (Appearance): eight of each, or
  one's own colour code (e.g. #5b4636); night mode swaps them.
- Room between widgets: *Space*, a gap (small, medium or large), and
  *Free space*, which takes the room the others leave, e.g. to put the
  clock in the middle or the tools at the bottom. Both may be added more
  than once, on the home screen and the sleep screen.
- Appearance: *Corners* (square, slightly rounded, round) for
  highlighted rows, buttons and boxes, and *Contrast*: strong draws gray
  hints and notes in black, for screens with few grays. The Airy theme
  sample rounds its corners. Plugin SDK: `UiStyle` gains `corner` and
  `strong` (with `UiStyle::faint` for gray text), `ui::fill_round`,
  `ui::stroke_round`, and the canvas rounded rectangles.
- GIF pictures in books, comics and the sleep screen's own pictures: the
  first picture of the file (also of an animation, interlaced or with
  transparency), decoded to gray row by row like JPEG and PNG.
- Plugins can bring a stylesheet for books: a data plugin's
  `styles.css`, or `Registrar::add_stylesheet`, applied after a book's
  own and before one's own (EPUB). SDK: `Env::stylesheets`,
  `OpenOptions::css` (no longer `Copy`).
- Books' embedded fonts in WOFF are used too (WOFF 2 is not read).
- Calibre's wireless device connection (`calibre` plugin, in the tools):
  the device finds Calibre in the network (or connects to the address
  set), and Calibre sends books to the library, lists and deletes them
  as with a reader on a cable; a password set in Calibre is asked for
  once. SDK: `sha1`, and `json::raw_member`, `json::unquote`,
  `json::string_array`.
- Sockets for plugins (ADR 0014): TCP connections that read text or
  files of any size into storage, and UDP broadcasts to find services,
  in the emulator and on Android (`Command::Connect`, `SocketSend`,
  `SocketRead`, `Disconnect`, `Broadcast`).
- Small capitals (CSS `font-variant: small-caps`, `font-variant-caps`)
  in EPUB, HTML and the other converted formats: lower case letters as
  smaller capitals, the text itself unchanged (search, selection and
  notes as before). Books are prepared anew once. Document model:
  `TextStyle::SMALL_CAPS`; embedded font families at most 31 (was 63).
- Snapshot tests: screens of the X4 and the Boox are drawn headless and
  compared with reference pictures, so layout faults fail a test.
- The folder chooser says when it is still opening a folder (slow SD
  cards) and ignores taps until it shows it.
- Widgets have settings of their own, from the home screen's editor:
  where the status line, date, clock, today's reading and the word of
  the day sit (left, centre, right), and whether the clock shows the
  date. Plugins give their widgets settings the same way
  (`widgets::settings_group`, `widgets::align_setting`).

## [0.1.1] - 2026-10-15

The Android app for F-Droid.

- The app is built reproducibly: with the same tools (shells/android/
  versions.env) it comes out byte for byte the same anywhere, so F-Droid
  can publish it signed with QuireInk's key. A dependency's macro that
  wrote its code in a random order is patched (patches/jni-macros).
- The app's library is stored in the APK and runs from there, without
  symbols: 6.8 MB to download, less space taken on the device.
- The licence names the copyright holder; README with screenshots.

## [0.1.0] - 2026-10-15

The first release: QuireInk on Android e-readers (tested on a Boox Go 7)
and in the desktop emulator, with its plugin SDK. The firmware for the
Xteink X4 is built but has not run on a device yet.

### Reading

- EPUB 2 and 3 (with the book's styles, embedded fonts and pictures, or
  one's own stylesheets per folder or book), FB2, Markdown, plain text,
  comic archives (CBZ), Xteink's XTC and XTCH, and PDF on Android.
- Contents, search, bookmarks and highlights with notes, export of notes
  as Markdown, a dictionary (StarDict), Wikipedia and translation
  (DeepL), a vocabulary list of words looked up.
- Hyphenation (German, English), reading fonts with weight and size,
  margins, spacing, status lines to taste, reading aloud, pages turned
  by themselves, turning the screen.
- Reading statistics, shared with KOReader (also over WebDAV), and
  reading positions synced with KOReader's sync server.

### Books and more

- A home screen of widgets on pages (date, battery, continue reading,
  recently read, tools, statistics, headlines), editable by touch or
  buttons.
- The library by folders, authors, series and tags; a file manager;
  books from OPDS catalogs and WebDAV folders (Nextcloud), kept in sync.
- News feeds as books; Wallabag's read-later articles, highlights to
  Readwise and a browser, switched off until wanted; transfers from a
  computer or phone over Wi-Fi, typing on a phone.
- A sleep screen (cover, picture or widgets), screenshots.

### Making it one's own

- Every screen, menu, key and gesture can be changed, in the app or in
  `quireink.ini`; profiles, rules, themes, translations and fonts as
  plugins without code (data plugins).
- Settings with descriptions and a search, passwords and keys in one
  place, short menus by default, usable by touch and by buttons alike.
- English and German.

### For developers

- The plugin SDK (`quireink-sdk`) with a guide ([Writing
  plugins](docs/plugins.md)) and a sample plugin; plugins are tested
  against the real kernel.
- The desktop emulator with device profiles and memory budgets.
- The Xteink X4's firmware, its display, buttons, battery and SD card
  tested against simulated hardware ([ADR 0013](docs/adr/0013-x4-firmware.md)).

[0.3.0]: https://github.com/hthoene/quireink/releases/tag/v0.3.0
[0.2.0]: https://github.com/hthoene/quireink/releases/tag/v0.2.0
[0.1.1]: https://github.com/hthoene/quireink/releases/tag/v0.1.1
[0.1.0]: https://github.com/hthoene/quireink/releases/tag/v0.1.0
