# Changelog

What changes from one release of PaperPaw to the next. Versions follow
[Semantic Versioning](https://semver.org); before 1.0, a minor version may
change the plugin SDK.

## [Unreleased]

- Wi-Fi transfer: PaperPaw's settings file, `paperpaw.ini`, can be
  fetched but no longer deleted or replaced from the browser: it names
  the servers that get passwords and keys.
- Wi-Fi transfer: a folder is listed to the browser in parts of about
  8 KB, which the page puts together and sorts, so a folder of thousands
  of books no longer has to fit in the X4's memory at once.
- Wi-Fi transfer: where there is too little room for the QR code
  (large fonts, a short screen turned), the address is shown without it;
  the code's place was computed below zero. Typing on a phone: a text
  too long for the device is refused with a message on the phone, which
  keeps it; it arrived as an empty text, which cleared the field.
- Wi-Fi: the radio goes off again once what waited for the network has
  failed (no saved network in reach, or none joined in the time a
  request waits). It went on looking for networks every ten seconds,
  with a notice each time, until the device slept, as the kernel kept
  saying that work waits.
- Wi-Fi: only secured networks are joined by themselves, with their
  saved password. Anyone can set up an open network with any name, so
  open networks are joined only when chosen on the Wi-Fi page and are no
  longer saved, and an open network named as a saved secured one is not
  joined either.
- Rules: a rule can apply when the battery reaches a level (80 %, 90 %,
  full or another), once each time it gets there and not again while
  the level wavers around it; with *Only if … While charging* it says
  when to unplug, for devices that cannot stop charging by themselves
  (PLANNING D8). On the X4 the level comes from the battery's voltage,
  which reads high while charging, and the choice of level says so; the
  X4 Pro's fuel gauge reports it directly. In the rules file the
  trigger is `battery-at 80`.
- Updates on the X4 Pro and the X4 Classic: they check update files for
  their own chip, the ESP32-S3, where they checked for the X4's: an X4
  Pro refused its own updates and would have taken the X4's. Each also
  names its own firmware in its image (`paperpaw-xteink-pro`,
  `paperpaw-xteink-classic`; the X4's stays `paperpaw-xteink`), as the
  two share the chip but not the board. SDK: `firmware::CHIP_ESP32_S3`.
- Releases carry the firmware of the X4 Pro and the X4 Classic as they
  do the X4's: `paperpaw-x4-pro-<version>.bin` for the whole flash with
  its signature file (`.bin.sig`), the signed update
  (`-update.bin`), the plugins built in (`-plugins.txt`) and checksums,
  and the same for `paperpaw-x4-classic`; a release fails if an update
  is not for its device's chip. CI lints and builds all three. The
  firmware's toolchains are pinned, as native code plugins need the
  same compiler (ADR 0024): Rust 1.99.0 for the X4
  (`shells/xteink/rust-toolchain.toml`), espup's `esp` toolchain
  1.97.0.0 with the Xtensa GCC 14.2.0 for the ESP32-S3 devices
  (`shells/xteink/versions.env`). The firmware is a workspace of its
  own, so it also builds in a checkout inside another one.
- Security: PaperPaw's own data (`.paperpaw`: passwords, trusted
  firmware keys, settings) can no longer be reached by another spelling
  of its name. On the X4's card a name stands for an entry by its long
  name alone, by its short name only where it has no long one, so
  `PAPERP~1` reaches nothing; the storage of the emulator and Android
  takes no short name the system keeps for another (NTFS and FAT keep
  them) and, on Windows, no name with a dot or space at the end.
- Security: the Wi-Fi transfer finds every path a browser sends part by
  part in the folders' listings: only entries listed by exactly that
  name, nothing hidden, by any spelling (case, short names, dots or
  spaces at the end). Its key, and the remote keyboard's, is drawn from
  the platform's random source (65 bits, 13 letters and digits) once the
  server runs; after five wrong keys no request is answered for a
  minute. Tests send every hostile path we know of to every request, on
  a card, in memory and on a storage as lax as Windows.
- Security: Calibre's paths to books and Wallabag's ids of articles are
  taken only as plain names: a peer posing as Calibre cannot write into
  `.paperpaw` (e.g. trust a firmware key of its own), a hostile Wallabag
  server cannot have a file deleted.
- SDK: `Env::random` gives random bytes from the platform's source
  (`random::Random`: `fill`, `token`), filled by each shell with
  `KernelBuilder::random` (the system's on Android and desktops, the
  chip's on the X4); `files::is_plain_name` and `files::plain_path`
  check names and paths that come from outside.
- Joplin (PLANNING C15, S1): a book's highlights and notes, once
  exported, become a note in Joplin, one per book, in a notebook named
  in the settings. Exporting again updates the note, also where it was
  moved to in Joplin; a note whose text was edited in Joplin stays, and
  a new one is made beside it. The notes go where Joplin synchronises
  to (Joplin Cloud, Joplin Server, or a WebDAV folder such as
  Nextcloud), so they reach Joplin on every device with no computer
  running. *Joplin* in the tools sets it up, each row with what it is
  for; the password is kept with the passwords and keys. What cannot be
  sent waits and goes with the next export, by the action *Send to
  Joplin* (also for rules), or when Wi-Fi comes back; no connection, a
  wrong password or address, Joplin's end-to-end encryption and a sync
  target in another version of Joplin's format than PaperPaw writes (no
  notes are sent then) are said plainly. While an older Joplin holds an
  exclusive lock on the target to upgrade it, the notes wait; PaperPaw
  takes no lock of its own, as Joplin 3.7 takes none. Notes are read
  and written in pieces and sent from a file, Joplin's answers saved to
  files of which only the end is read: the X4's heap peaks no higher
  for it. Shipped switched off.
- SDK: `files::for_each_line` reads a text file line by line as it
  streams from the storage, so lists of any length take little memory;
  `LocalTime::to_seconds` is the way back from
  `LocalTime::from_seconds`; `web::http_date` reads the dates servers
  send (`Last-Modified`, WebDAV's `getlastmodified`).
- Settings: a live preview above the reading settings (PLANNING §6.7):
  two short paragraphs set exactly as book pages are (the same layout
  engine, type, margins at the sides, justification, indents and
  hyphenation, in the interface's language), following each value at
  once while a slider moves or the buttons change it, before it is
  kept. In the emulator's X4 profile an update takes a third to a half
  of the time one book page takes to lay out, and about 3 KB of the
  heap; only the preview and the row changed are drawn again, with a
  quick refresh, and the full picture with its grays comes once the
  value is chosen.
  SDK: a widget can preview a group of settings instead of being one
  for screens (`WidgetSpec::previews`, `Widgets::preview`); settings
  pages show it above the group, and `Widgets::iter` leaves such
  widgets out.
- Settings: on touch screens, numbers better set by eye are sliders on
  the settings pages (PLANNING §6.7): the text size, word and line
  spacing, the margins, the speed of reading aloud, the interface's
  size, row height and margins, the frontlight; with buttons alone they
  keep their compact rows with `−` and `+`. A number changed with the buttons now
  shows each value at once without keeping it; *Select* keeps it,
  *Back* goes back to the value before, and the row of hints says so
  (`−`, `+`, *Keep*, *Cancel*). By touch, a slider's knob follows the
  finger and the value is kept where it is lifted; `−` and `+` change it
  a step at once, as before. The reader's panel shows its sliders'
  values, and gives them `−` and `+` alone where the screen is too
  narrow for a track (the X4 Pro).
- SDK: sliders as a building block for any plugin (`ui::slider`:
  `Slider`, `SliderDrag`, `SliderFocus`, `SliderParts`, `SliderStep`,
  `SliderView`, `band`, `band_height`, `key_step`, `marked`, `parts`,
  `row_height`, `shows`; what a slider draws goes in a `SliderView`, so
  it can grow without breaking plugins): a track filled up to the value with a knob
  there, between a `−` and a `+` flush with the margins, the title and
  the value with its unit above it as in other rows. By touch a finger
  takes the knob along in the slider's steps and lifting it chooses the
  value, with the buttons left and right move it a step; `shows` says
  whether a setting gets one here (a touch screen). Screen readers
  hear it as `semantics::Role::Slider`. The panel's sliders (the
  frontlight) are drawn with it and keep to their steps. The help texts
  of the margins and the word spacing say more plainly what they do, and
  a test checks that every slider shows its unit.
- Screens draw without reading the card (PLANNING §12.3, B3). The X4
  draws a frame in five bands, and what a screen read or worked out as it
  drew, it did five times over, and again for every frame. Covers on the
  home screen, in the book widgets, on the book information and in OPDS
  catalogs are made small once and drawn from memory (the home screen
  read its cover's 65 KB from the card in every frame); the highlight,
  note, quote and word of the day widgets lay their text out where they
  are placed; the menu editor's lists are made when the menus change (a
  frame 30 times faster), and so are which profiles are in use; weather
  now lays itself out where placed, the keyboard's field and the
  translation's heading are measured once; the logo widget and the
  words marked in books read nothing while drawing. Screens below a full screen let go of what
  they keep to draw, so a book opened from the home screen has more
  memory, and a screen on top only for a moment (the home screen below a
  menu that opens another screen) reads nothing. A test opens every
  screen of the standard app as on an X4 and fails where drawing reads
  the card; the guide says the rule.
  SDK: `Widget::place` (where a widget is drawn, before it is),
  `widgets::Wrapped` (a text broken into lines once, drawn as often as
  needed), `Screen::on_hide`, `GridView::release`; `Screen::on_show`
  comes once the commands of a call are done. A decorator's signature is
  asked as pages are drawn: it says it from memory. Kernel:
  `Kernel::open_screen`, `Kernel::known_screens`.
- Emulator: `--trace` prints what drawing each frame asks of the
  storage, band by band, how long it took and what was asked before it;
  scripts open any screen with its parameters (`open:`) and draw the
  screen again (`redraw`).
- The Xteink X4 Classic ("X4 v2", an ESP32-S3 board with other pins than
  the X4's): the firmware builds for it with the feature `x4-classic`
  (`cargo +esp x4classic`; ADR 0013). Its panel's controller (SSD1677,
  UC8179 or UC8279) is found at start and keeps a frame in its 8 MB of
  PSRAM, as on the X4 Pro; its seven buttons are the X4's (with the row
  of hints below the screen), each on a pin of its own; SD card, fuel
  gauge and clock chip as on the X4 Pro. Its pins come from the FreeInk
  SDK's and Papyrix's notes; not yet tried on one. Device id
  `xteink-x4-classic`, `x4-classic` for plugin libraries.
  `paperpaw-x4`: `buttons::State::keys`, the buttons from a pin each.
- X4 and X4 Pro: the display controller is found at start, so devices
  from other batches show something: the first X4 Pros have an SSD1677,
  later ones a UC8179 or a UC8279 (ADR 0013). The firmware asks the panel
  on its lines before the SPI bus takes them, logs what it answered
  (`display: UC8179 (FLG 13, REV 00 00 01 FF FF, BUSY high at rest),
  driven as UC8179`) and drives it with its controller's driver; without
  a sure answer, with the one the device always had. Not yet tried on a
  device with another controller; on a UC8279, and on an X4 with an
  UltraChip, grays are shown in black and white for now.
  `paperpaw-x4`: `probe` (the controller asked), `panel` (`Panel`, the
  driver for the controller found, `Device`), `uc8179::Display` for the
  UC8279 too (`Chip`) and without a frame of its own (`Frame`, the bands
  written through windows of the controller's memory); its `new` takes
  the chip and where the frame is kept.
- X4: downloads into a file (updates, books, fonts) no longer run out of
  memory half way: pieces of an answer gathered before writing them to
  the card grew without bound when the connection handed over a lot at
  once, and the X4 started again (found with the crash file's calls;
  the out-of-memory crashes seen in testing were this). Downloads of
  10 MB one after another now go through, at the peak 222 KB of the heap's 262 KB.
- A plugin store on the device (*More plugins*, in the tools and in the
  plugin manager, PLANNING O6, ADR 0023): the data plugins of
  paperpaw.org's plugin library made for this device, PaperPaw version
  and data format, by group as on the website, named and described in
  the language shown, with *Search* (by name and what a plugin does)
  and *Fonts*, which opens the fonts browser. A plugin's page tells what
  it does, who made it, its licence, version and size, and installs,
  updates or removes it; downloads go on when the page is closed. The
  library sends a list made for the device
  (`plugins.json?device=…&lang=…`, format 2), which the store reads as it
  streams into an index on the card: however many plugins it lists,
  only a page of them is in memory (a list of 1001 plugins takes about
  3 KB on the X4). A plugin's files come with its own page
  (`plugins/<id>.json`). The setting *Plugin library* (developers) names
  another library.
  SDK: `install` (`Installer`, `Package`, `PackageFile`, `Progress`,
  `Received`, `fits`, `is_plugin_name`, `is_safe_path`, `MAX_FILES`):
  installing data plugins from the internet, file by file, each
  checked, the job kept on the storage, answers to jobs stopped passed
  over; the fonts browser uses it too. `Command::InstallPlugin`: a data
  plugin downloaded takes the place of every version of it on the
  storage (loaded, switched off or refused at start), stays off where
  it was switched off, and keeps what was chosen from it (updating the
  font one reads with keeps it). `KernelBuilder::catalog_device` and
  `Env::catalog_device`: what plugin libraries call the device (`x4`,
  `x4-pro`, `android`; the shells say it). `ui::Sections`: entries under
  the headings of their sections on pages, in memory that does not grow
  with the entries. `Plugins::owner`: the plugin that added an action,
  a screen or a setting. `DATA_FORMATS`: the data plugins' formats this
  PaperPaw reads. The plugin manager lists the actions of the menu
  `plugin-manager.more` above the plugins.
- X4: removing or copying a large folder (e.g. one with many books, or
  their caches) no longer starts the device again half way: work on the
  card keeps the watchdogs from taking it for hung (it took 20 s before
  they did, removing the books' caches over a minute).
- X4: more memory left while the radio is on (on USB power it stays on
  by default, and a book opening then came close to the heap's end): the
  radio keeps 5 receive buffers instead of 10 and takes at most 16 more
  each way instead of 32, about 6 KB less all the time and up to 11 KB
  less at the peaks, downloads as fast as before (6 MB in about 33 s).
  What each plugin took is kept once instead of twice.
  SDK: `Env::plugin_usage()` gives it, `Usage::plugins` is gone (it was a
  copy made on every call).
- X4 test builds, for finding what takes the memory: a panic keeps the
  calls that led to it (also a failed allocation), and
  `tools/x4-remote.py --crashes` names them from the firmware's symbols;
  `heap` says the most the heap held since start, the log says the heap
  after every frame, and failed requests are logged with their host
  (`wifi:status`, `wifi:scan`, `http:get:` as script steps).
- The X4's remote control over USB (test builds) does much more, for
  developing on the device: the card (`ls`, `stat`, `md5`, `rm` also of
  folders, `mkdir`, `mv`, `sync`; `tools/x4-remote.py` pushes and pulls
  whole folders and checks every file it puts by its MD5), what runs
  (`version` with the plugin crates built in, `plugins`, `actions`,
  `settings`, `screens`, `usage`: what each plugin took), the clock
  (`time`, `time set`), the log's level (`log`), events (`emit`), touch
  (`touch`, and the script's `tap`, `swipe`, `tap-label`), `restart` and
  `panic` (the script waits until the X4 is back and says why it
  started, with the panic). Scripts wait for and check what the screen
  shows (`until:`, `expect:`, `expect-not:`) and end with exit code 1 when
  a check fails. Kernel: `Kernel::screens`, `actions`, `usage`,
  `send_event`.
- Updates from the internet (X4): *Search online* on the update page
  (Settings, *Update*) asks paperpaw.org for a newer PaperPaw, and
  *Download* fetches it into `updates` on the card, to install as one
  copied there; the action `update.check` searches in the background
  (e.g. from a rule) and offers what it finds once. Nothing is sent
  unless asked; the setting *Update server* (developers) names another
  server (ADR 0022).
- The firmware knows its plugins: every image lists the plugin crates
  built in, right after its application descriptor, so that updates and
  the web flasher can bring the same ones; *About PaperPaw* shows them.
  An update that would leave plugins out says which, and the reader
  chooses *Update without them* or *Not now*.
  SDK: `Firmware::plugins`, `Update::plugins`, `Update::leaves_out`,
  `firmware::Plugins` and `PluginCrate` (from `paperpaw-update`, which
  also reads and writes the list). `paperpaw-sign plugins` prints an
  image's list; releases carry it as `paperpaw-x4-<version>-plugins.txt`.
- PaperPaw's logo, the paw on a sheet with a dog-ear: above "Sleeping" on
  the sleep screen (also what the X4 shows as it starts), at the top of
  *About PaperPaw*, and as a widget to add to the home screen (*PaperPaw
  logo*, alone, its name beside it if wanted). The Android app's icon is
  the logo too.
  `paperpaw_sdk::logo` draws it in any size, as shapes, nothing kept in
  memory; the setting `core.logo` names a picture to show in its place
  (a data plugin sets it in its `settings.ini`, see docs/plugins.md).
- Markdown files show the pictures beside them (`![alt](picture.png)`,
  or from the card's root with `/`); before, their alternative text.

- QuireInk is now called PaperPaw, everywhere. Crates `paperpaw-*` (for
  `quireink-*`), so a plugin uses `paperpaw_sdk` (for `quireink_sdk`).
  On the card `paperpaw.ini` and the folder `.paperpaw` (for QuireInk's
  own, which PaperPaw takes over once as it starts:
  `data::take_over_old_name`, so settings, statistics and passwords stay);
  the Android app is `io.github.hthoene.paperpaw`, a new app beside
  the old one. Formats with new magics: font packs `.ppfp` (were `.qifp`),
  hyphenation `.pphp` (packs and patterns made before, `QIFP` and `QIHP`
  inside, are still read: PaperPaw's own format), cached pictures `PPGI`
  and `PPCI`, made again where they are kept. The USB remote answers
  `PP:` lines. The website is
  paperpaw.org, the repository github.com/hthoene/paperpaw.
- More fonts, from the internet: *More fonts* below the fonts to choose
  from (Settings, Reading, Font, and the status lines' font) lists the
  fonts of paperpaw.org's font library, each name set in its own font;
  one shows larger with a sentence in it, its licence and size, and is
  downloaded, read with at once (no restart), set for the status lines,
  updated or removed. Nothing of a font is built in: devices with
  scalable fonts and the memory for them get its font files, the others
  (the X4) font packs made for them, read a glyph at a time from the
  card. Each file is checked by its size and SHA-1; a download goes on
  when the screen is left. First fonts: Atkinson Hyperlegible Next,
  Crimson Pro, EB Garamond, Figtree, Fredoka and Nunito (all OFL, none
  with a reserved font name). The library's address is a developer
  setting (`fonts-browser.catalog`). SDK: a data plugin names a family
  of its own in `[family]` (`name`, `regular`, `bold`, `italic`,
  `bold-italic`), its faces packs or font files (`FontFace`,
  `Registrar::add_font_face`, `Fonts::add_face`; `Fonts::remove_plugin`,
  `Presets::remove_plugin`, `Plugins::remove`); data plugins loaded and
  removed while running (`Command::LoadPlugin`, `Command::RemovePlugin`,
  the event `events::PLUGINS_CHANGED`); a menu named like a setting with
  choices holds more ways to choose, listed after its options (the
  settings' chooser, `ui::two_options`).

- X4 Pro: pictures in four grays, as on the X4. `uc8179::Display` shows
  the frame in black and white first, then lightens the grays with the
  FreeInk SDK's gray waveforms; the next refresh drives every pixel
  back to black or white.
- PaperPaw runs on the Xteink X4 Pro (ESP32-S3): the X4's firmware built
  with the feature `x4-pro` (`cargo +esp x4pro`). Its UC8179 display,
  touch screen and home key, page and power buttons, SD card (SDMMC),
  fuel gauge, clock chip (the time is kept while it is off), front light
  with brightness and warmth (in the quick panel and the reader's
  panel), sleep and Wi-Fi. Its 8 MB of PSRAM is the heap. It stays on
  while flashed (its power held through resets). `paperpaw-x4` has the
  drivers: `uc8179`, `gt911`, `cw2017`, `bm8563`. Cards whose partition
  table says more than the card holds (as an X4 Pro's card as sold) are
  read up to the card's end.

- Ready-made home screen layouts to start from: *Calm* (as it comes),
  *Covers* (the books first), *At a glance* (daily goal, today
  and the streak), *Clock* (a large clock face) and *Minimal* (the book
  alone). Each is shown on the home screen itself before it is kept,
  with a sheet below to go from one to the next and *Use this layout*;
  back leaves everything as it was. They are offered in the first-run
  setup and under *Add* while editing the home screen (action
  `home.choose-layout`), and everything on them can be changed after.
  Anyone can share one: a data plugin with a `presets.ini` (sample
  `samples/plugins/reading-nook`). The home screen lays itself out once
  for many changes at a time. SDK: presets, ready-made sets of setting
  values of a kind (`presets`: `Preset`, `Presets`, `Preset::changes`,
  `presets::commands`, `presets::applied`; `Registrar::add_presets`
  and `add_preset`, `Settings::presets`); values shown without being
  kept (`Command::PreviewSettings`, `Settings::set_preview`,
  `Settings::previewing`, the event `events::PREVIEW_CHANGED`), ended
  when the screen that asked closes; `Settings::is_set`.

- A new look, calmer and more modern, on every screen: rounded corners
  by default, the focus a rounded bar within the margins, thinner lines,
  dark rounded notices, and Source Sans 3 as the menus' font (warmer than
  Atkinson Hyperlegible Next, which it replaces). Empty screens say what
  belongs there and how to fill it, with an icon: the home screen before
  the first book (*Start reading*), the library, vocabulary, notes,
  rules, profiles, news, an empty folder, and Wi-Fi transfer without
  Wi-Fi. Every setting has a help text; notes speak of buttons on devices
  without touch; errors say what to do. Icons were chosen again, one
  idea one icon. Books without a title show their file's name readably
  ("Alice In Wonderland" for `alice-in-wonderland.txt`). The quick panel
  shows a short date. SDK: `ui::draw_empty`, `ui::draw_focus`,
  `ui::focus_rect`, `ui::tr_input` (a `<key>.buttons` text on devices
  without touch), `Command::NoNetwork` (work waiting for a network fails
  at once), `status_items::DATE_SHORT` with `time::short_date_text`,
  `library::readable_name` and `Book::shown_title`.

- Settings take much less memory: the kernel's and every first-party
  plugin's are kept in the program, also those with a unit, a section or
  an editor of their own. In the emulator's X4 profile the home screen
  needs 109 KiB instead of 152 and starting peaks at 140 KiB instead of
  207. SDK: `SettingExtra` is made in the program (`SettingExtra::new`
  and its `const` builders `unit`, `section`, `editor`, `step`,
  `slider`, `secret`) and given to fixed settings with
  `SettingSpec::with_extra`; `SettingSpec::scoped` and `advanced` are
  `const`; `Setting::static_str` and `Id::static_str` name a setting in
  `static` lists; `role_settings!` makes a decoration role's settings in
  the program (`decorations::add_role` stays for roles known only at
  run time); `Tone::id` and `LineKind::id` are `const`.

- *Now reading* shows the chapter and what is left of it ("22 pages
  left · about 11 min"), each a part to show or not. Plugins know the
  chapter from the reading events: `events::Reading` has `chapter` (its
  title) and `chapter_left` (pages after this one in it, once the pages
  are counted); the reader fills them.
- The status lines while reading have a font of their own (*Status
  lines → Font*): as the menus (as before), as the book, or any font
  installed. The bar the reader's menu opens at the top (the book,
  the chapter, the entries) is in that font too. SDK: `fonts::STATUS` (readers draw their status lines in
  it), `settings::STATUS_FONT`, `Fonts::set_status`.
- The same widget can be on the home screen more than once, each in a
  look of its own: every widget added has its own settings (its frame,
  size, what it shows), set from its *Settings* in the editor. Pages made
  before keep their widgets; one gets settings of its own when they are
  first opened. SDK: placements (`Id::placement`, `Id::placed`; ids such
  as `widgets.clock@2` and settings such as `widgets.clock@2.face`, saved
  in `paperpaw.ini` like others), `Settings::with_placement`,
  `Settings::spec`, `Settings::placement_values`, `widgets::base`;
  `Widgets::make` gives a placement's widget in its own values.
- *First day of the week* (System): Monday, Sunday, or as the language
  has it (Monday in English and German). The statistics' weeks and every
  calendar follow it (a calendar widget may choose its own). SDK:
  `settings::WEEK_START`, `time::WEEK_STARTS`, `time::week_start`.
- Many new widgets for the home screen (C26), each with settings of its
  own: what it shows, its style, its size and alignment, and a frame
  (none, a line, a light card, a dark card with light text) that every
  widget has. A widget pack (`widgets`): an analogue clock, a word clock
  (as a sentence or a grid of letters, English and German), a clock in
  tiles, a world clock, a calendar, the year so far, a countdown, the
  moon's phase, the day and date, a quote of the day (shipped quotes or
  one's own), a picture from a folder, a note, a to-do list with its own
  screen, a greeting, a book's progress as a ring, the book being read
  with the time left, the next book to read, a series, favourites, a
  highlight of one's own, the library in numbers and the battery. The
  reading statistics add a daily goal ring, a reading calendar, books
  this year (with a yearly goal) and a streak; vocabulary adds *Words
  saved*; a weather plugin (`weather`, Open-Meteo, nothing sent before a
  place is chosen) adds the weather now and a forecast. The widgets
  there before (clock, date, status line, continue reading, recently
  read, read today, the last days, the word of the day, news) have the
  same settings. Adding a widget shows the kinds of widgets first, then
  each widget of a kind drawn as it would show.
- SDK for widgets: a shared look (`widgets::Frame`, `Size`, `Look`,
  `draw_frame`, `frame_setting`, `size_setting`, `part_setting`,
  `shows`, `choice`, `draw_caption`, `draw_message`, `draw_wrapped`,
  `line_height`, `look::look`), kinds (`WidgetSpec::category`,
  `widgets::categories`, `Widgets::category`), `events::WIDGET_CHANGED`
  (`widgets::changed`) for a widget whose content changed. Graphics:
  `Canvas::fill_circle`, `stroke_circle`, `fill_ring` (arcs, smooth
  where the panel has grays), `draw_hand`, `fill_round_shaded`,
  `paperpaw_sdk::round` (angles as on a clock face, whole numbers).
  `Kernel::widgets`.
- SDK: settings kept in the program (`SettingSpec::fixed`,
  `fixed_switch`, `fixed_number`, `fixed_choice`, `fixed_text`,
  `described`, `for_advanced`; `Registrar::add_settings`;
  `widgets::frame_spec`, `size_spec`, `part_spec`, `align_spec`) take
  next to no memory, where one built at start takes about 200 bytes. A
  choice's values are `Choices` (`SettingKind::Choice`,
  `SettingSpec::choice`, `Settings::set_choices`): lists in the program
  cost nothing.
- X4: highlights were lost each time their book was opened: renaming a
  file that is not there removed the file at the new place (the reader
  looks for highlights kept where they were before). Such a rename now
  changes nothing, and the reader moves old highlights only where there
  are any. What is written reaches the card two seconds after the last
  work, not only before sleep, so a reset, a flash or an empty battery
  loses nothing; files sent over USB are on the card before the X4 says
  so. paperpaw-fat: `SharedDisk`, `FatStorage::with_disk`.
- Decorators get a word split over two lines (hyphenated) whole, in
  both parts, so words being learnt are marked there too. The page is
  drawn again at once when how marks look or what decorators mark
  changes (a setting of *Marks in the text*, *Mark vocabulary*), not
  only at the next page.
- Marks in the text (ADR 0021): plugins mark words on book pages through
  decorators, drawn the same by every reader. How a mark looks is
  settings, one per aspect (background, text, a line under the text that
  is solid, dotted, dashed, wavy or double, its tone and thickness, a
  frame, struck through), under *Marks in the text*; highlights, links,
  search hits and the sentence read aloud are such roles too. Highlights
  are visible on black and white panels (dithered; they were white
  before). A tap on a mark whose plugin hears taps goes to it; with the
  buttons, *Links and marks* reaches them. SDK: `decorations`
  (`Decoration`, `Tone`, `LineKind`, `add_role`, `draw_behind`,
  `draw_over`, `DecoratorSpec`, `Decorators`), `Registrar::add_decorator`,
  `Env::decorators`, `events::DECORATION_TAPPED`, `groups::MARKS`,
  `roles`, `words::WordSet` (whole words and phrases, many at once,
  without regard to case); graphics: `Canvas::fill_shaded`. The setting
  `reader.highlight-style` is gone (the highlight role's settings).
- Vocabulary: the words looked up can be marked where they occur in
  books of their language (*Mark vocabulary* in the reader's *More*,
  off by default); a tap on one, or choosing it with the buttons, looks
  it up again.
- A reader can be replaced in full: plugins reach it only through what
  the SDK defines for every reader, no longer through the shipped
  reader's ids. SDK: `events::GOTO` (`Goto`), `events::TEXT_CHOSEN`
  (`TextChosen`), `events::NOTES_EXPORTED` (`NotesExported`),
  `menus::READER`, `READER_MORE`, `READER_PANEL`, `SELECTION`,
  `settings::FONT_SIZE` (registered by the kernel), `groups::STATUS_LINES`,
  `setting_sections::ALOUD`; key bindings while reading on the slot's
  layer `[keymap.core.reader]`. Changed ids: `reader.goto` is
  `core.goto`, `reader.notes-exported` is `core.notes-exported`,
  `reader.text-chosen` is `core.text-chosen`, `reader.font-size` is
  `core.font-size` (set again once), the menus `reader.menu`,
  `reader.more`, `reader.layout` and `reader.selection` are
  `core.reader-menu`, `core.reader-more`, `core.reader-panel` and
  `core.selection-menu`, key bindings in `[keymap.reader.screen]` go to
  `[keymap.core.reader]`.
- Reader: *Smooth text* (Font, off by default, where the screen has no
  grays): book pages are shown in four grays, the letters with soft
  edges, as pages with pictures are; on the X4 a page turns in some
  1.9 s (0.7 s without).
- Text on black and white panels (the X4) is fuller: a pixel at the edge
  of a letter is drawn from three eighths covered (was half), so thin
  strokes of small text keep their weight and no longer look pale. The
  setting *Text darkness* (Display, where the screen has no grays)
  chooses light (as before), normal or full. SDK: `settings::MONO_TEXT`,
  `MONO_TEXT_LEVELS`, `Capabilities::black_and_white`,
  `MenuNeed::BlackAndWhite`; graphics: `Framebuffer::set_mono_coverage`,
  `MONO_COVERAGE`.
- Home: the screen shows without the focus until the first press, also
  on devices without touch, so it is seen as it is first; the setting
  *Show the focus at once* brings the old way back. Opening the home
  screen is faster: the recent books look up only as many as fit across
  the screen (was all that were set, six by default), and on the X4 no
  busy mark is shown after a page in grays (it took a refresh of the
  whole screen). Titles below the covers take the width of their
  column. SDK: `GridView::set_focus_shown`.
- Covers made small (lists, grids) no longer count as large pictures
  that leave ghosts: a page of the library's grid refreshes quickly on
  the X4 (about 0.5 s, was a full refresh of 1.7 s); large pictures and
  the periodic full refresh stay as they were. Graphics:
  `Canvas::small_pictures`.
- Covers are decoded as large as the screen shows them in a grid (three
  eighths of its longer side, at least 300 pixels): on large screens
  they no longer stay smaller than the places of books without a cover.
  The library reads books' details again once for it. SDK:
  `OpenOptions::cover_fit`, `cover_fit`, `COVER_FIT`.
- Library: the order, the view and the library folder have icons in the
  library's menu, as its other rows.
- Covers made small are kept as the panel shows them: in black and white,
  dithered, on panels of black and white alone (a quarter of the memory:
  a grid of covers on the X4 takes some 20 KB, was 90), in grays or in
  colour elsewhere. SDK: `ui::Thumbnail::new` and `library::cover` take
  the panel's `PixelFormat`.
- Library: the covers made small are let go while a book opened from it
  is read (on the X4 a grid of them took some 40 KB), and made again
  from their small files when it shows again.
- Night mode leaves pictures as they are (covers and photos were shown
  as negatives); "Pictures in night mode inverted" (Display) inverts
  them too. Graphics: `Framebuffer::set_pictures_kept`.
- What is kept for a book (its converted chapters, position and page
  index, notes, details) moves with it and is removed with it, and
  "Clean up storage" (Settings) removes what is left of books no longer
  there. Moving a book no longer converts it anew. SDK:
  `Registrar::add_book_data`, `library::{key, BookData, move_data,
  remove_data, clean_up, book_keys, books_in, removed}`,
  `events::BOOK_REMOVED`, `actions::CLEAN_UP`; the file manager says
  which books it deleted.
- X4: pages with pictures and the sleep screen's cover or picture show
  in four grays (the panel's gray waveform, as long as a full refresh);
  lists stay black and white. The time and the battery do not refresh a
  picture shown so; the next page does. SDK: `Screen::grays`, kernel:
  `Effects::grays`, `Kernel::render_gray`.
- Books: pictures are decoded when their page shows, not with their
  chapter (`Document::prepare_image`, `image::dimensions`): on the X4 no
  step of the background pagination of an illustrated book takes
  seconds any more.
- Home on the X4: the recently read shelf shows below the current book
  (it had no room at its full height and was left out). Widgets can do
  with less (`Widget::min_height`); the grid places them lower where
  their full height has no room; the shelf's covers then shrink.
- X4: the display driver shows four grays (`write_gray_rows`,
  `refresh_gray`, with the stock firmware's gray waveform); over USB,
  `gray` shows bars, a gradient and a cover in them to judge the panel.
  Not yet used for pictures. Kernel: `draw_again`.
- Emulator: the X4 profile has the X4's 256 KiB of heap (192 KiB and the
  64 KiB the boot loader used), not 192.
- Menus, the quick panel and questions wash out what lies below them
  (lighter gray; on the X4 every other pixel paper), so they stand out
  and the lines they cut through read as background. Overlays looked
  through (the keyboard, notices, skimming) keep it. SDK:
  `Screen::dims_below`; graphics: `Canvas::wash`.
- X4: the lines between the rows of lists, the tracks of sliders (the
  quick panel, skimming in the reader) and the statistics' lines show:
  light gray, they were drawn white on its black and white panel; there
  they are now every other pixel black. SDK/graphics:
  `Canvas::fill_tint`.
- Settings: each page ends with "Reset to defaults", the main page with
  "Reset all settings" (the language stays); both ask first. SDK:
  `Command::ResetSetting`, `Settings::reset` (the value forgotten, so
  that the default, or outside a book's own settings the global value,
  applies again).
- The row of hints over the buttons marks with a small hand the buttons
  that do something more when held (e.g. Back: home, Select: options),
  without growing. SDK: `ui::draw_button_hints` takes what holding each
  button does, `ui::HOLD_ICON`; the icon font has `hand-click`.
- Library: buttons reach the order at the top (a stop of the focus
  between the last book and the first); Confirm there opens the
  library's menu, as a tap does. SDK: `Header::note_focused`.
- Home: the book widgets are no longer read again each minute (on the
  X4 that read the recent books' details from the card, about 570 ms a
  minute, also on battery); widgets say whether they change by
  themselves (`Widget::live`, `GridView::update_live`). Only as many
  recent books as the widgets show are read, each once.
- Library: a page of books takes about 150 ms of work on the X4 instead
  of 500 to 700. Covers are kept made small (`library::cover`, from
  `ui::Thumbnail::keep`/`kept`), read as 3 KB instead of 60 each; a
  listing's entries carry their sizes (`Entry::size`,
  `library::book_sized`), so a book is not looked up again for them.
- X4: blocks of the card read one after the other are read eight at a
  time (a folder searched takes a third of the time), and a folder is
  looked up from its kept parent rather than from the root.
- X4: the screen turns ("Turn the screen": upright, on either side,
  upside down). Turned, the kernel draws bands of the same memory (fewer
  rows where the screen is wider) and frees the old frame before making
  the new one.
- Library: the covers of a page are made small once when it is shown and
  drawn from memory; a step of the focus draws only the two entries it
  changes. On the X4 a step in the library draws in about 105 ms (was
  455), a page in about 205 ms (was 650). SDK: `ui::Thumbnail` (a
  picture made small and kept, 4 bits a pixel), `ListView::request`
  (only the items a focus step changes).
- X4: the folders looked up last are kept, so a file is no longer looked
  up by walking every folder from the card's root: preparing a book the
  first time takes about 2.4 s instead of 2.9.
- Menus draw only the two rows a step of the focus changes (on the X4
  a step in the main menu draws in about 75 ms, was 155); SDK:
  `Rows::request` does so for screens where the focus changes nothing
  else.
- X4: opening a book for the first time overflowed the stack or ran out
  of memory (the X4 started again on the book before). Fixed at the
  causes: inflate states and JPEG tables are no longer built on the
  stack, pictures are written with a buffer taken once, the icon names
  stay in flash. A first opening now leaves about 14 KB of the stack.
- Plugins name any of Tabler's icons: `paperpaw_sdk::icons` knows all
  5934 names (a table in flash, about 150 KB on the X4, no memory); the
  drawings of those the icon font lacks come from icon packs.
- X4: opening a book for the first time could run out of memory (a
  crash, "memory allocation failed") where a chapter has blocks of many
  links or other short pieces: where each piece starts is now written as
  it comes. The first opening of a book with a large cover peaks some
  5 KiB lower.
- Emulator: `--trace-alloc` says where each allocation comes from.
- The X4's whole flash image in a release is signed too, in a signature
  file of its own (`paperpaw-x4-<version>.bin.sig`), which the web
  flasher checks before it writes (`paperpaw-sign sign-apart`, `check`;
  `paperpaw_update::check_apart`).
- Icon packs: `tools/icons/pack.py` builds a data plugin with the
  drawings of any Tabler icons PaperPaw's icon font lacks, read a glyph
  at a time from the card; `paperpaw-fontgen --chars` takes code points
  (`U+EA01-EA0F`). An icon without a drawing now shows nothing rather
  than a missing character.
- SDK: a widget is a `Widget` a screen keeps: it reads what it shows in
  `update` (when shown, each minute, after events) and only draws in
  `draw`, which may run several times a frame. `WidgetSpec::new` takes
  its factory, `WidgetSpec::drawn` a pair of functions for widgets with
  nothing to read (`Drawn`, which replaces `signature` with `shows`);
  `Widgets::make` makes one. The home screen, the sleep screen and the
  widgets of the statistics, news and vocabulary plugins work so: on
  the X4 a step in the main menu over the home screen drew in about
  0.15 s instead of 0.5 s.
- Reading: *Home* is in the menu's bar, last: from the first entry one
  press back reaches it (it was under *More*, at the end of a list).
- Buttons pressed quickly while the device is busy wait at most a frame:
  five presses in a row on a slow screen no longer run five times one
  after the other.
- Screens below a menu or another overlay are drawn only where it leaves
  them to be seen: moving in the main menu over the home screen no
  longer draws the home screen's widgets again.
- SDK: `widgets::grid`, the home screen's grid for any plugin: a layout
  of widgets and tools in cells as text (`parse`, `write`, `place`,
  `add`), and `GridView`, which shows it on a screen of the plugin's own
  and edits it in place (dragging, sizing, options, with buttons too).
  The home screen uses it. Also `widgets::has_settings` and
  `Menus::title`.
- Home: below the page, the pencil and the dots can go, and status items
  of any plugin can come (Settings > Look). *Edit home screen* in the main
  menu edits it, with buttons only too, as a long press does.
- Home: the status line at the top shows the status items chosen for its
  left, centre and right (the status line widget's settings), any
  plugin's, as the reader's lines do.
- PaperPaw builds with Rust 1.88 and later (it said 1.85, which was too
  old); CI checks it. A short [security model](docs/security-model.md)
  says what is protected and against whom.
- X4: a crash is written to the card at the next start
  (`.paperpaw/crashes.txt`, its version and why), for bug reports.
- Data plugins say which version of the formats they are made for,
  `formats = 1` in `plugin.ini`; a PaperPaw that reads older ones leaves
  such a plugin out and says why. Every data file PaperPaw keeps has a
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
  deprecated), `paperpaw_text::draw_text` a `TextStyle` (with `bolder`,
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
  are in `paperpaw-x4`, tested on a computer.
- Kernel: device defaults are typed, `setting_default(&SETTING, value)`;
  `setting_default_text` takes an id and text, for settings known only
  at run time.
- SDK: its compatibility is checked. CI fails on a change that would
  break plugins unless the version says the next release breaks
  (`cargo-semver-checks`), and `crates/paperpaw-sdk/api.txt` lists its
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
  change the SDK: `paperpaw-digest` (MD5, SHA-1, FNV-1a), `paperpaw-json`,
  `paperpaw-qr`; plugins that read SQLite use `paperpaw-sqlite`.
- SDK: `Registrar` is a struct now that collects `Contribution`s (a
  `#[non_exhaustive]` enum, the large kinds boxed); `add_action` and the
  rest are shorthands for `add`. Plugins take `&mut Registrar` instead of
  `&mut dyn Registrar`. Anything can see what a plugin adds without
  PaperPaw running (`Registrar::new`, `contributions()`); the kernel
  applies the list and checks key maps, translations and patterns then.
- SDK: `Env` is `#[non_exhaustive]` (new fields break no plugin). What
  the device and its shell can do is `env.caps` (`Capabilities`: touch,
  network, Wi-Fi, firmware, speech, the sleep screen, rotation, colours,
  the row of buttons, a system clipboard), replacing `env.network`,
  `env.speech` and `env.sleep`; menu entries' needs are checked against
  it (`Menus::set_capabilities`).
- SDK: PaperPaw's folder is one constant, `data::DIR`, with
  `data_path!` for constants; the about page is written in its plugin's
  folder.
- Versioned data files ([data-files.md](docs/data-files.md)): every file
  PaperPaw keeps on the card is listed as data, cache or another
  program's format. Data formats have a version (`paperpaw_sdk::data`,
  `# format 2` as the first line), are upgraded as they are read, and are
  never written over by an older PaperPaw (after going back to an older
  update). Files of the current formats are kept in `testdata` folders
  and read by the tests. Profiles are kept as a proper ini file now (names
  with spaces and brackets survive); older files are read and upgraded.
- `paperpaw-config`: changing a key there more than once (by hand)
  changes the entry that is read, the last; `quote()` is public and the
  reader and statistics use it (a book path with a line break broke the
  reader's file).
- One FNV-1a (`paperpaw_digest::fnv1a64`) instead of fifteen copies.
- Firmware updates on the X4 ([ADR 0019](docs/adr/0019-firmware-updates.md)):
  two app partitions (the web flasher sets them up once), an update file
  in the folder `updates` on the card installed from Settings, *Update*
  (or offered at start) in about 45 s, then a restart. A new firmware
  that does not get to its first frame is undone at the next start.
  Update files are signed (Ed25519, `paperpaw-update`, made with
  `paperpaw-sign`): PaperPaw's key is built in, one's own builds are
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
  from Settings (*Set up PaperPaw*). Plugins add steps of their own
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
  (`paperpaw-x4-<version>.bin`, with its SHA-256), which paperpaw.org
  offers for flashing in the browser.

- Wi-Fi on the X4 (new plugin `wifi`, under Settings, Device): the radio
  is off while reading and comes on by itself when something needs the
  network (a catalog, a sync), joins the strongest network joined before
  and goes off again a minute after the last use (a setting); on USB it
  stays on. Its page lists the networks in reach to join, with the
  password typed on the keyboard, and forgets saved ones (menu button or
  a long press). Saved networks are kept with the passwords and keys.
  Plain HTTP and, since, HTTPS.
- Network for plugins on devices where PaperPaw manages Wi-Fi
  (`paperpaw_sdk::wifi`, [ADR 0016](docs/adr/0016-network-and-wifi.md)):
  `Command::Wifi`, `Env::wifi`, and the events `core.wifi-changed`,
  `core.wifi-scanned`, `core.wifi-failed`, `core.network-wanted` and
  `core.network-idle`. Requests, servers and connections made without a
  network wait for one up to 30 seconds. Shells read a request's file to
  send themselves (`HttpCall::body_from`) and may write a download to its
  file as it comes (`HttpCall::save_to`, `HttpResponse::saved`).
- Settings: a group may bring its own page (`SettingGroupSpec::page`),
  and settings and menu entries may need a device where PaperPaw manages
  Wi-Fi (`MenuNeed::Wifi`).
- `paperpaw-http` (new crate): the HTTP/1.1 client protocol without I/O,
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
  The records are CSV files in `.paperpaw/plugins/battery` (`log.csv`,
  and `plugins.csv` every ten minutes awake), for analysis on a computer.
- What each plugin takes: the kernel measures the time each plugin's code
  runs (screens drawing and handling input, event and action handlers),
  the time its screens are shown with the refreshes then, and the storage
  it reads and writes; plugins see it in `Env::usage`
  (`paperpaw_sdk::usage`), with the refreshes, input and sleeps of the
  whole device. Shells give a clock for it (`KernelBuilder::clock`) and
  say how long the panel refreshed (`Kernel::panel_busy`).
- The battery's voltage where the device measures it
  (`Battery::millivolts`: the X4, Android), and an event each time the
  battery is measured (`events::BATTERY_MEASURED`); status lines redraw
  only when the percent or charging changes.
- X4 remote: `--get` copies a file off the card (e.g. the battery logs).
- A new icon: the capital Q of Besley in ink on e-paper, as on
  paperpaw.org (its source is `shells/android/icon.svg`).
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
  counts while PaperPaw is busy (before, the buttons did nothing while
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
- X4: the SD card is read and written by PaperPaw's own driver
  (`paperpaw_x4::card`), a command at a time with the bus held, not a
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
  when PaperPaw starts and when the device wakes up, set under System ›
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
  PaperPaw without looking at the screen.
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
  can publish it signed with PaperPaw's key. A dependency's macro that
  wrote its code in a random order is patched (patches/jni-macros).
- The app's library is stored in the APK and runs from there, without
  symbols: 6.8 MB to download, less space taken on the device.
- The licence names the copyright holder; README with screenshots.

## [0.1.0] - 2026-10-15

The first release: PaperPaw on Android e-readers (tested on a Boox Go 7)
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
  `paperpaw.ini`; profiles, rules, themes, translations and fonts as
  plugins without code (data plugins).
- Settings with descriptions and a search, passwords and keys in one
  place, short menus by default, usable by touch and by buttons alike.
- English and German.

### For developers

- The plugin SDK (`paperpaw-sdk`) with a guide ([Writing
  plugins](docs/plugins.md)) and a sample plugin; plugins are tested
  against the real kernel.
- The desktop emulator with device profiles and memory budgets.
- The Xteink X4's firmware, its display, buttons, battery and SD card
  tested against simulated hardware ([ADR 0013](docs/adr/0013-x4-firmware.md)).

[0.3.0]: https://github.com/hthoene/paperpaw/releases/tag/v0.3.0
[0.2.0]: https://github.com/hthoene/paperpaw/releases/tag/v0.2.0
[0.1.1]: https://github.com/hthoene/paperpaw/releases/tag/v0.1.1
[0.1.0]: https://github.com/hthoene/paperpaw/releases/tag/v0.1.0
