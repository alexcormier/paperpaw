# Changelog

What changes from one release of PaperPaw to the next. Versions follow
[Semantic Versioning](https://semver.org); before 1.0, a minor version may
change the plugin SDK.

## [Unreleased]

- The X4 keeps less in memory from the start: ids take two words
  instead of three, translations index their messages in two bytes
  where they fit, and actions are not kept twice. Reading an EPUB peaks
  some 24 KiB lower.
- SDK: an HTTP answer's body is bytes as they came (`HttpReply::body`
  is a `Vec<u8>`, `text()` reads it as text), no longer decoded with
  losses, and it reaches the plugin's handler without a copy: an event
  may carry its data as it is (`Params::carrying`,
  `EventData::into_params`), which observers do not see. A failed
  request says why (`HttpReply::error`, an `HttpError`: no network,
  DNS, TLS, timeout, refused, replaced, cancelled, I/O), and a body cut
  to the most the request takes says so (`truncated`). `EventData`
  types are `Clone`.
- What a plugin tells of the PaperPaw it runs in comes from the kernel
  that runs, not the SDK the plugin was built with:
  `web::user_agent_of(env)` names its version, `install::fits_in(env, ..)`
  takes its SDK (`install::fits_sdk` where no `Env` is at hand), from
  `Plugins::kernel`. The kernel's manifest now has the version of its
  crate. `web::user_agent()` and `install::fits()` stay, for the SDK a
  plugin is built with; PaperPaw's own plugins use the new ones.
- The files of a data plugin are checked by their SHA-256 where the
  plugin library gives it (`sha256` beside `sha1` in `latest.files` and
  in an update's `files`), else by their SHA-1 as before; downloads
  kept on the card with `sha1:` go on (`install::Digest::take`).
- Font size offers only the sizes the reading font has: on the X4 and
  X4 Pro, whose built-in fonts stop at about 12 pt, no larger ones that
  looked the same. A size set before past them gives way to the
  largest. At the largest, the help says that a font with larger sizes
  gives more, and *Discover fonts* on the reading settings opens the
  font browser (or its page in the store, where it is not installed).
  The setup's sample of the font size is a shorter sentence, shown
  whole. Scalable fonts keep every size. For plugins:
  `FontHandle::largest_size`; for the kernel, `Settings::set_range` and
  `Settings::set_description`.
- A book's position and annotations say which book they are of by its
  content, its size and KOReader's partial MD5, beside the hash of its
  path they are named by (`library::Fingerprint`, `positions::book`,
  `annotations::book`). The reader takes it once when it opens a book
  (12 reads of 1 KiB at most) and keeps it with the position
  (`positions::save_with_book`) and the annotations
  (`annotations::stamp`, a line `# book <size> <md5>`). Both files keep
  their versions: PaperPaw of before reads them as before.
- Data plugins of formats 3 (not yet released) are written alike in
  every file. A list is a key a line: an action's steps are a `run`
  line each, a line one step with its commas, so a value with a comma
  needs no escape (`entries` and `hide` of `menus.ini` still read ids
  separated by commas). `notify:` takes a key of the plugin's
  translations and says its text in the reader's language. In
  `menus.ini`, `[menu <id>]` is another plugin's menu (`add`, `hide`)
  and any other section a menu of one's own (`title`, `entries`);
  `add` and `hide` in a plain section are still read. `paperpaw-dev
  check` says where a file is written the earlier way
  (`package.actions`, `package.menus`). The ini files take a section
  name of words with single spaces between them.
- First-run setup: the sample of the font size is in the book's font,
  at the size chosen, as the reader shows it: no longer in the
  interface's font, its second line cut. The card grows with the size
  (fewer words where there is no more room), and follows the size as it
  changes.
- The web emulator's release asset (`paperpaw-web-<version>.zip`) is
  signed as the emulators are, and holds everything its pages load. Its
  frame loads no inline style or script, so it runs under a strict
  Content-Security-Policy from any path, as paperpaw.org's "Try it out"
  serves it (docs/web-emulator.md says what a server needs). The frame
  has no background of its own.
- The emulators' device profiles say where each physical key sits on
  the device's body and which computer keys press it (`body` in
  `paperpaw-emu --registry` and in the web emulator's device). The web
  emulator's frame draws the body with the keys there, for the mouse,
  touch and the keyboard, in the embedding page's colours (`look`).
- Lock touch says how to unlock it whenever it turns on, however it was
  switched (the quick panel, the reader's menu, Settings, a button), and
  says when it is unlocked. While it is on, a tap or a swipe says again
  how to unlock, at most every ten seconds, calmly (no full refresh),
  never between the steps of the way out.
- Unlocking touch works on slow screens: on the X4 Pro the two taps and
  the hold failed when a refresh held the taps back. The taps may now
  be up to half a second long and a second apart, and a hold unlocks
  as soon as it is long enough if the finger moves a little. The X4
  Pro's home key held unlocks too.
### 0.5.0-beta.1 (draft)

This is a beta, for testers and developers. It is the first release of
PaperPaw's firmware for the Xteink X4 and X4 Pro, and it brings much
that is new on every device. Expect bugs, and expect some features to
change before 0.5.0. Keep a copy of your books and notes. When
something goes wrong, please open an
[issue](https://github.com/hthoene/paperpaw/issues). Say which device
and version you use and what you did. On the X4, a crash is written to
`.paperpaw/crashes.txt` on the card at the next start; please add that
file to your report.

#### Reading

- Holding a page button goes to the next or previous chapter; *Hold a
  page button for the next chapter* under Controls turns this off.
  *Next chapter* and *Previous chapter* can go on any button or gesture.
- On the X4, the front buttons Left and Right turn pages in a book.
  *Swap the page buttons* under Controls turns them round.
- Holding *Select* while reading sets or removes the page's bookmark.
  *Bookmark* and *Home* are in the reader's bar. *Go to a percentage*
  and *Go to page* are in *More*.
- New status items: the page in the chapter, the pages left in it, and
  a short bar for how far into it you are. *Now reading* shows the
  pages and minutes left in the chapter.
- The status lines have a font of their own: as the menus, as the book,
  or any font installed. They sit closer to the edge, so a page holds
  more text. By default they are calm: the time and the page.
- More control over the page: margins at the top and bottom, space
  between paragraphs and indents as two switches, and line spacing down
  to 80 %. A live preview above the reading settings shows each change
  at once.
- A new text size lays out the open book again at once. *Turn the
  screen* and the full refresh can be a book's own, with *Only this
  book*.
- Text on black and white panels is fuller; *Text darkness* under
  Display chooses light, normal or full. *Smooth text* shows pages in
  four grays with soft letter edges, but pages turn more slowly.
- *Marks in the text* sets how each kind of mark looks: highlights,
  links, search hits and the words plugins mark. Highlights now show on
  black and white panels. *Mark vocabulary* marks the words you looked
  up.
- The dictionary can look a word up on Wikipedia: choose *Online* in its
  panel, and the summary shows beside the dictionary entries.
- Books whose pages go from right to left mirror taps and swipes.

#### Books and fonts

- HTML, FictionBook, Markdown and comic books are plugins of their own,
  so each can be switched off or replaced. Markdown files show the
  pictures beside them.
- *More fonts* below the font choice lists the fonts of paperpaw.org's
  font library. Download one and read with it at once. The first are
  Atkinson Hyperlegible Next, Crimson Pro, EB Garamond, Figtree, Fredoka
  and Nunito.
- On the X4, font packs on the card add other scripts. Packs for
  Cyrillic and Greek make Russian, Ukrainian and Greek books, menus and
  keyboards work.
- On the X4 and X4 Pro, pages with pictures, covers and sleep screen
  pictures show in four grays. *Pictures in grays* under Display does
  the same for screens with pictures, such as the library's covers.
- Pictures on the X4s are dithered in a new way: line drawings stay
  clean, and covers and photos keep their tones. Night mode leaves
  pictures as they are, unless *Pictures in night mode inverted* is on.
- Damaged or crafted books, pictures, fonts and dictionaries can no
  longer restart the X4 or use up its memory. What cannot be read is
  left out, and the rest carries on.
- Many fixes in EPUB, FictionBook, plain text, Markdown, comics and XTC.
  Plain text reads UTF-16 files, and file names that begin with a letter
  beyond ASCII ("Ärger.epub") no longer crash the X4.

#### The library

- The library is a calm list. The book you are reading comes first, as a
  card. Then come all books with cover, title, author and progress; a
  check marks finished books, and *New* marks books added recently.
- Filters above the books: All books, Reading, Unread, Finished, Next in
  your series, Shelves and Find a book. Folders, favourites, authors and
  series are on the shelves. *Find a book* picks a book by a letter.
- *Show books as: Covers* shows a grid of covers with the progress on
  each; the books of a series form one stack.
- Hold a book for its sheet: go on reading, its series, finished or
  unread, favourite, information, archive and delete. Deleting asks
  first and removes what is kept for the book.
- *Open with* lists the reading apps that read a book. The reading
  settings choose a default app and one per format, and the file manager
  sets one per folder.
- The list of books is kept up to date in the background, never while a
  book is open. Large libraries work on the X4, and the library holds
  next to nothing while you read.
- What is kept for a book moves with it. *Clean up storage* under System
  removes what is left of books that are gone; it also runs by itself
  while the device charges, at most once a week.
- The file manager opens folders of any size a page at a time. Moving a
  folder takes the books' progress and notes along.

#### The home screen and widgets

- The home screen is a grid of 3 columns by default, with up to eight
  pages. *Edit home screen* in the main menu edits it, by touch or with
  the buttons. On the X4 Pro, dragging keeps up with the finger.
- Many new widgets: an analogue clock, a word clock, a world clock, a
  calendar, the year so far, a countdown, the moon's phase, a quote, a
  picture, a note, a to-do list, the next book, a series, favourites, a
  highlight, the library in numbers, the battery and PaperPaw's logo.
- The reading statistics add a daily goal ring, a reading calendar,
  books this year and a streak. A weather plugin (Open-Meteo) adds the
  weather now and a forecast; nothing is sent before you choose a place.
- Every widget has settings of its own: what it shows, its style, size,
  alignment, spacing and frame. The same widget can be placed more than
  once. *Widget look* and *Frames of widgets* change all widgets at once.
- The clock has clearly different sizes: small, medium (the default),
  large, and as large as fits. On the X4s a large clock is truly large.
- The home screen shows no focus until the first press; *Show the focus
  at once* brings the old way back. Fewer columns or rows no longer
  leave a page empty.

#### The sleep screen

- By default the sleep screen shows a sleeping cat, drawn for PaperPaw.
  The cover, pictures, a message and the other choices remain.
- *The page with a picture*: the page you read stays, and a picture or a
  widget goes over it. You choose where it sits, its size and how much
  of the page shows through.

#### Menus, settings and the look

- A new look, rounder and friendlier: soft corners, pills for buttons
  and switches, plain headers, faint lines between rows and margins of
  4 mm. The menus' font is Source Sans 3. Empty screens say what belongs
  there and how to fill it.
- With advanced settings, Appearance sets the header, corners,
  highlight, lines between rows, cards and the size of titles, margins
  and rows. Themes are listed under *Theme*; the Pebble theme comes
  switched off.
- No label is cut any more. Long titles and button words get a little
  smaller or wrap onto a second line, at every text size and language.
- Hints: the first time a screen opens, a short note says what its
  controls do. *No more hints* switches them off, and *Show hints again*
  under System brings them back. Every plugin also says how it works, in
  the plugin manager and in the store.
- On touch screens, numbers you set by eye are sliders. A slider moves
  with the finger from where it was and never jumps. With the buttons,
  a changed value shows at once; *Select* keeps it and *Back* goes back.
- The quick panel and the reader's panel have wide sliders and larger
  buttons on touch screens. The front light is there now, no longer in
  the main menu.
- Under Settings, Plugins, *Screens*, you choose which plugin shows each
  screen: the home screen, the library, the reader, the lock, the
  keyboard, the sleep screen and more.
- Each settings page ends with *Reset to defaults*, the main page with
  *Reset all settings*. A setting with two choices switches at once, and
  moving past the last entry of a list goes round to the first.
- *Device name* under System names your reader for Calibre, the progress
  sync, Wi-Fi transfer and your router. *First day of the week* sets the
  start of the week for the statistics and calendars.
- New status items: *Battery when low* shows the battery only below a
  level, and a Wi-Fi sign shows while the device is in a network.
- A plugin's settings group that names no place on the settings' main
  page is listed at the end of *Customise*, by name, with a plugin's
  icon, instead of first among the books' groups.
- No explanation is cut short any more. Every setting's description fits
  the two lines of its row on the X4 and the X4 Pro, in English and
  German, and a test keeps it so. The text below *Progress sync*'s rows
  takes the room there is; where it still has none, a row *More about
  this* shows it whole.

#### Touch, buttons and the keyboard

- The quick panel by the buttons: hold Right (on the X4), or choose
  *Quick panel* at the top of the main menu. The tips on the home screen
  and in a book say so.
- A number in the first-run setup and in a menu is changed with the
  buttons as on the settings pages: Select starts, left and right change
  it, Select keeps it, Back goes back to the value before, and the hint
  bar says so. Left and right no longer stay on a number in the setup,
  and the quick panel's sliders name Keep and Cancel too.
- The keyboard on touch screens has round keys and takes less of the
  screen; *Shape of keys* makes them a little taller or wider.
  *Keyboard size* (Small, Medium, Large) makes it a quarter lower or
  higher: with round keys about 30 %, 40 % or 50 % of the screen, never
  keys below 4 mm. German has ten keys a row: hold a, o, u or s for ä,
  ö, ü and ß, or switch on *Umlaut keys*. On narrow screens, a row too
  wide is split in two. The keyboard takes up to 2,000 characters.
- Touch lock: *Lock touch* in the quick panel and the reader's menu.
  Taps and swipes do nothing, and the buttons still work. To unlock, tap
  twice and keep the finger down, or hold the power button on the X4
  Pro.
- On the X4, a row above the buttons names what each does; *Show what
  the buttons do* under Controls switches it off.
- Buttons answer better: a press counts as soon as it holds, and presses
  made before a new screen shows are dropped. *Button response: Faster*
  reads the buttons more often, for a little more battery.
- On the X4, *Buttons and gestures* refuses a change that leaves no
  button to select or go back with. *Reset buttons and gestures* takes
  every change back.

#### Tools

- Flash cards, in the vocabulary tool, with spaced repetition (FSRS,
  Anki's scheduler). Edit decks with your phone, also from CSV or an
  Anki deck. Words you look up can become cards, and a card due can be
  the sleep screen.
- Reading goal: a daily goal for the reading statistics, and a cat whose
  mood follows today's reading. The *Reading cat* shows it on the home
  screen or as the sleep screen.
- Joplin: a book's highlights and notes become a note in Joplin, through
  Joplin Cloud, Joplin Server or WebDAV. It is off until you want it.
- Battery use records the battery while the device is awake. It shows
  the use per hour and per 100 pages, how long the battery lasts, and
  which plugins kept the device busy.
- A guided device check in the diagnostics tool tests every part of a
  device, one page at a time, and writes a report to the card.

#### Wi-Fi, sync and the store

- The store tells betas apart: a plugin's versions are compared whole,
  pre-release included (`0.5.0-beta.2` after `0.5.0-beta.1`, `0.5.0`
  after both), as firmware updates are. A newer beta is offered only
  where the device runs a beta or takes betas (*Beta updates*). Its
  message for code built for another plugin interface or compiler says
  so.
- The store marks PaperPaw's own plugins *By PaperPaw*, on their row
  and their page. The mark is taken only from PaperPaw's own library
  (paperpaw.org, over HTTPS), and for a plugin with code only where its
  key is PaperPaw's: a list from elsewhere cannot give it.
- A code plugin's version is its `plugin.ini`'s, everywhere: the
  plugin manager, the store's updates and the transfer's list all go by
  it. Code whose manifest names another version runs at `plugin.ini`'s,
  with a warning in the log; `paperpaw-dev package` finds it before.
- Wi-Fi on the X4 and X4 Pro: the radio is off while you read. It comes
  on when something needs the network, joins the strongest saved
  network and goes off a minute after the last use. HTTPS works, with
  the certificate checked. The X4 sets its clock once online.
- Set up Wi-Fi with your phone: the reader opens a network of its own
  and shows QR codes, and you pick your network and type its password on
  the phone. Only secured networks are joined by themselves.
- Wi-Fi transfer and Calibre work on the X4 and X4 Pro. *Without a
  router* on the transfer screen opens the reader's own network. A
  scan that finds no saved network is made again, up to three times,
  before the device says there is none.
- Calibre: a burst of books no longer stalls the connection as often,
  a connection Calibre stops answering ends after 45 seconds, and the
  device tells Calibre how much its card really has free.
- The Wi-Fi transfer installs plugins from a computer too, such as
  one's own build. The version before stays until the new one is whole,
  and a plugin with code is installed only after you say yes on the
  device.
- In Offline mode, the store, catalogs, Calibre and Wi-Fi transfer offer
  to go online once, instead of failing.
- A plugin store on the device: *More plugins*, in the tools and in the
  plugin manager, with search, groups, sorting and *Update all*. Code
  plugins come from it too, signed by their author; an update signed by
  someone else needs your consent. The store updates only the plugins
  it installed: one put in place otherwise (the Wi-Fi transfer, a card,
  one's own build) is local, and the plugin manager says which is which.
- The firmware is lean: many plugins, such as the statistics, widgets,
  Calibre, KOReader sync, weather and rules, come from the store instead
  of being built in. This leaves more memory.
- A first-run setup guides you through the language, the buttons, the
  text size, the time zone, Wi-Fi, the keyboard and the screen lock.
  *Add more* offers four plugins most readers add.
- Progress sync (kosync) fetches the position on the server before it
  sends its own, so a further position from KOReader is kept. It no
  longer keeps the radio on while you read.
- KOReader's notes exchange, the statistics sync, OPDS, Wallabag, news
  and Readwise no longer delete anything when an answer or a file cannot
  be read.

#### Power and battery

- Power modes: Normal, Saving, Offline and Automatic, in the quick panel
  or under Settings, System, Power. Automatic goes to Saving below 20 %
  and to Offline below 10 %. *Network per plugin* sets the network for
  each plugin.
- Quiet reading: background work no longer shows notices over your book,
  and it waits for a good moment, such as when the radio is on anyway.
- The X4 and X4 Pro save much more power: the processor sleeps on
  battery when there is nothing to do, the radio sleeps between beacons,
  and the panel's power goes off after a refresh.

#### Devices and updates

- Transfer over USB, further: a plugin's files are put together in a
  folder of the cable's own and installed only if they are those
  offered, each with the SHA-256 offered; the cable and the open Wi-Fi
  transfer never send the same plugin at once. Offers are read a line
  at a time (up to 64 files and 64 books each), and only closing the
  question refuses an offer.
- The Xteink X4 Pro is supported: its display, touch screen, buttons,
  card, battery gauge, clock chip, front light with warmth, sleep and
  Wi-Fi. Its front light dims evenly in single percents.
- The X4 is much faster: a page turns in about 0.7 s and the library
  opens in under 2 s. The screen can be turned.
- Updates from the internet: *Search online* on the update page finds a
  newer PaperPaw, and *Download* fetches it. Updates are signed, and one
  that fails early in its start is undone.
- An update waits while code plugins in use have no build for it yet.
  The update page names them and offers *Keep waiting* or *Update
  anyway*.
- Releases carry signed firmware images of the X4 and X4 Pro, which
  paperpaw.org offers for flashing in the browser.
- Safe mode: after two starts that fail, PaperPaw starts without code
  plugins. Holding *Back* on the X4 or *Home* on the X4 Pro at power-on
  does the same.
- QuireInk is now called PaperPaw. Its settings, statistics and
  passwords are taken over once, at the first start.
- Android: a beta of the app comes with each release, as an APK. It is
  now `org.paperpaw.app` and installs beside the old one. It runs on
  devices with 16 KB memory pages, and on Android 8 to 10 it can write
  to the shared storage.
- Transfer over USB on the X4 and X4 Pro: a computer at the other end of
  the cable installs a plugin or adds books. The device shows what is to
  come, with names and sizes, and writes nothing until you allow it,
  never over a book.
- Betas: *Beta updates* in the update's settings, off at first, asks
  for betas; a device on a pre-release gets the next anyway, and the
  device takes a pre-release only so, whatever the server answers.
  Betas come in order before their release (`0.5.0-beta.2` after
  `beta.1`, `0.5.0` after both) and show calmly, as `0.5.0 Beta 1`.
  Android's version codes follow the same order (of 1.2.3: beta.1 to
  beta.49, then `-dev`, then rc.1 to rc.48, then the release), and a
  version they cannot order is refused.

#### Your data and security

- PaperPaw never writes over a file it cannot read, such as
  `paperpaw.ini`, passwords, notes, statistics, profiles or rules.
- Writes are safe against power cuts: a file holds its old content or
  its new one, whole, also on the X4's card, where a file moved or
  renamed is at its old place or its new one, never at neither. Going
  back to an older update no longer writes over data that a newer
  PaperPaw wrote.
- An audit of everything PaperPaw keeps found ways it could lose notes,
  places, statistics, cards or passwords; each is closed, with a test.
  Among them:
  - *Clean up storage* removes by itself only what can be made again.
    The notes and places of books not found go only when you agree.
  - A reading place that cannot be read is never written over with the
    start of the book; it is kept aside and you are told.
  - Highlights and notes in EPUB books keep their place when the book's
    own styles are switched on or off.
  - Progress sync, KOReader's notes and the statistics sync delete
    nothing on a failed fetch or an unclear file, and ask where one side
    lost everything.
  - Lines PaperPaw does not understand, from a newer version or written
    by hand, are kept. An old copy of the passwords goes only once the
    passwords file holds it.
  - Removing a profile, a password or a vocabulary word asks first;
    *Reset to defaults* keeps folders, accounts and passwords; the file
    manager protects PaperPaw's own folders.
- While the device is locked, nothing shows above the lock screen but
  what the lock opens itself. The lock's code is kept with a random
  salt.
- A redirect no longer takes a password to another server, nor goes
  from `https` down to `http`. Calibre connects only to a computer you
  chose, and a plugin cannot change another plugin's passwords.

#### For developers

- The SDK's design rules ([Designing the plugin SDK](docs/sdk-design.md)):
  how it grows without breaking plugins. Tests check four of them over
  the crates whose types cross: growing types are `#[non_exhaustive]`,
  handlers take no `&mut Vec<Command>`, nothing is `#[doc(hidden)]`
  outside the `kernel` feature, and no deprecated item reaches a
  release. What they found before is listed and may only get less.
- SDK, cleaned up before its first release (UI): `ui::fill` and
  `ui::HOLD_ICON` are gone (`Canvas::fill`, `icons::HOLD`);
  `ui::PAW_ICON` is `icons::PAW`. What only the kernel uses is no longer
  part of the plugin API: `ui::draw_button_hints`,
  `ui::button_hints_height`, `semantics::parts` and the constructors of
  `PageLayers`, `Decorators` and `Widgets` come with the `kernel`
  feature, and `Canvas`'s drawing internals are hidden. A cover drawn
  small is `ui::draw_small_image`; whether the device has a row of
  button hints is `env.caps.button_row`.
- SDK, cleaned up before its first release (data): the writers of data
  say why they did not write, `Result<(), data::Error>` in place of a
  `bool`: `annotations::save` and `add`; `annotations::set_note` and
  `remove` give `Ok(false)` where there was nothing to change;
  `library::save`, `Index::save`, `install::marks::write` and
  `catalog::write`. `data::Error` takes a `StorageError` (`?`).
- SDK: a storage tells when a file was last changed: `DirEntry::modified`
  (`with_modified`) and `Storage::modified`, `None` where it does not
  tell; the memory card's FAT and the computer's file system say it.
  `MemoryUse` tells the largest block the heap could give now
  (`largest_free`) and the heap's regions (`MemoryRegion`: the X4 Pro's
  internal RAM and PSRAM). `read_file` takes its buffer fallibly: a file
  the heap cannot take now is `StorageError::TooLarge`, not the end.
- SDK, cleaned up before its first release (installers): what only the
  kernel and the plugins that install do comes with the `kernel`
  feature, the reading parts staying for every plugin. `native`:
  `remember_author`, `remember_code`, `remember_fetched`, `fetched_for`,
  `keep_code_room`, `installed_code_need`, `check_file`,
  `CROSSING_CRATES`, `LAYOUT` and `CodeRoom`'s constructors (`known_author`,
  `installed_code`, `code_room` and `CodeRoom`'s reading stay);
  `install::local` (`Staging`), `install::marks`, `remember_store_install`,
  `forget_store_install` and `adopt` (`origin`, `Origins` stay);
  `firmware::Update` and `FirmwareCommand::install`, the one way to ask
  for an install (`FirmwareCommand::Install` is matched, not built);
  `data::take_over_old_name`; `Secrets::encode` and `decode`;
  `library::save`, `catalog::write` and `Index::save`.
- SDK, cleaned up before its first release (platform): the types that
  grow are `#[non_exhaustive]`, with constructors where plugins build
  them: `DirEntry::new`, `files::Entry::new`, `LocalTime::new` (its
  weekday from the date), `MemoryUse::new` and `with_stack`,
  `PluginUsage::new`, `WifiNetwork::new`, `Firmware::new` with `keys`,
  `room`, `undone`, `state` and `plugins`, `BookData::new` and
  `SecretChange::changeable`; `install::Digest::text` writes a digest as
  a job keeps it. A match on `WifiCommand`, `PowerLevel`,
  `served::Access`, `install::Digest`, `FirmwareCommand` or
  `BookDataKind` needs an arm for what is to come. `Usage::refreshes`
  and `PluginUsage::refreshes` name their modes (`usage::Refreshes`:
  `fast`, `partial`, `full`, `total`) in place of an array.
- SDK: types that may grow are `#[non_exhaustive]`, with constructors
  where plugins build them: `decorations::{PageWord, PageText,
  Decorated}::new`, `semantics::Part::new` and `Described::new` (with
  `.focused()`, `.on()`, `.clip()`); also `decorations::Mark`,
  `RowLines`, `SliderParts`, `PickerPart`, `Offer`, `Padding`. A match
  on `Offer` needs an arm for answers to come. `Header` and `Pager` keep
  their state: `area()` reads it, `Header::note_focused()` and
  `note_button()` set the note.
- SDK: `canvas.describe(role, text, area)` says more by its builder
  (`.focused()`, `.on()`, and `.disabled()` for what does nothing now),
  in place of five arguments; parts (`Described`, `semantics::Part`)
  tell whether they are disabled.
- SDK: signatures are numbers, made without allocating: a text
  filter's and a status line's signature returns a `u64` (0: does
  nothing), like a decorator's. `digest::Fnv1a64` makes one, fed piece
  by piece, as a `Hasher` or with `write!`. A menu item's label and badge
  are a `Cow<'static, str>`, so a fixed text costs nothing.
- SDK: the registrar's methods take ids as `impl Into<Id>` and text as
  `impl Into<Cow<'static, str>>` or `impl Into<String>`, so `&ID`, a
  setting or an event and a `&'static str` or `String` go in as they
  are. `Id`, `Setting` and `Event` are `AsRef<str>`.
  `SettingExtra::follows` and `shown_if` name the setting by its `Id`.
- SDK: what was deprecated is gone: `Registrar::add_service` and
  `Contribution::Service` (publish with `Registrar::publish`),
  `Service::any` and `Id::placed` (`Id::unplaced`).
- SDK: `EnvParts`, `WorkStates::set_works`, `WorkStates::set_note` and
  `ScreenKey::new` come with the `kernel` feature: only the kernel and
  tests make an `Env`, set work states or number screens.
- SDK: actions have one shape, with params everywhere. A handler is
  `.handler(|env, params, out| ..)`; `handler_with` and
  `ActionParamsHandler` are gone. `Command::RunAction(id, params)` takes
  the place of `RunActionWith`, and `Command::run(id)` runs one without
  params. A screen reads them with `cx.params()` in `on_action`.
- SDK: every handler queues its commands in `Commands` instead of a
  `&mut Vec<Command>`: actions, events, observers, work steps, voices,
  widgets' input and the installer. It has `push`, `extend` and `emit`
  and reads as a slice. Its fields are private, so it can carry more
  later without a new signature. A closure such as `|env, out|` stays as
  it is; a parameter typed `&mut Vec<Command>` becomes `&mut Commands`.
  tell whether they are disabled, and `canvas.disabling(true)` tells
  everything drawn into it so. Faint menu and quick panel entries are
  told as disabled, and the remote control's `describe` (and the web
  emulator's) says `disabled` in a part's state.
- SDK: which page of how many a list shows is a `ui::Paging` (`page`,
  `pages`, `is_first()`, `is_last()`), in place of a pair of numbers:
  `Rows::paging()`, `Sections::paging()`, `ListView::paging()`, and what
  `Header` and `Pager` draw and hit.
- SDK: dates and times as values: `time::Date` and `time::Time`, which
  `ui::DatePicker` and `ui::TimePicker` take and give; `line_at` of a
  page's layers and the widget grid's `cell_at` take a `Point`.
- SDK: `UiStyle` keeps its fields and is read with methods: sizes in
  pixels as `u16` (`row()`, `margin()`, `gap()`, `line()`, `corner()`,
  `header()`, `finger()`, `control_row()`), text sizes as `f32`
  (`text()`, `small()`, `title()`, `head()`), and its looks
  (`focus()`, `separators()`, `cards()`, `pill()`, `strong()`,
  `header_kind()` with `HeaderKind::{Light, Dark, Plain}`). How many
  grays the panel shows is `black_and_white()` or `smooth_grays()`. A
  style changed for a screen of its own is made with `with_row`,
  `with_margin`, `with_corner`, `with_pill`, `with_text` and the like.
- SDK: one path and one name per item of the UI. The types of the
  modules `ui::slider`, `ui::sections`, `ui::reorder` and `ui::offline`
  are reached through them (`ui::slider::Slider`,
  `ui::offline::OfflineOffer` …). The widgets' look is in `widgets` itself:
  `WidgetLook` (was `widgets::look::Look`, apart from `ui::look::Look`),
  `widget_look()`, `TextSize` (was `Size`; `SizeClass` is a widget area's
  class, `TextRole` the interface's text), `choice_setting()` (was
  `choice`), `WidgetStack` (was `widgets::Stack`, apart from
  `ui::Stack`). `ui::Grid` is `ui::CellGrid`, apart from the widgets'
  `grid`.
- SDK: `ui::choice::open` and `GridView::new` are gone (`ask::choice`,
  `GridView::asking`).
- SDK: `ui::Chips` and `ui::Segments` keep their labels: they are given
  once when the part is made (`Segments::new` takes the labels, not
  their count) and `draw` no longer takes them; `label(index)` reads one.
- SDK: more types that may grow are `#[non_exhaustive]`: `HttpReply`
  and `Chosen` (with `new`), `ServerResponse`, `SocketRead`, `MenuItem`,
  `help::{Input, Zone, Hint}` and in `work` `WorkState`, `Retry`, `Why`,
  `Refusal`, `WorkRefused` and `Note` (with `new`), `Step` and
  `Importance`. A match on them needs an arm for what comes later.
  `Handled` is exhaustive: an event is handled or not, and a plugin
  matches both.
- SDK: a look's painters take a `ui::look::PaintCx` (the canvas and the
  style, more to come) and the part: `fn(&mut PaintCx, &SwitchPart) ->
  bool`.
- SDK: the interface's colours come from the palette: `faint(Ink)` and
  `focus_faint(focused, Ink)` take a role (`Ink::Note`, the new
  `Ink::Detail` for what is told beside a row's title, `Ink::Muted` …)
  in place of a gray; the focus and the header use the palette's text
  and paper. `Palette` holds colours (`color(ink)`), `Ink::ALL` is a
  slice, and `Ink::CardPattern` is gone (cards on black and white
  panels are a sparser pattern of `Ink::Card`). The palette setting
  names `detail` too.
- SDK: `ui::NumberEdit` changes a number setting with the buttons as
  the settings pages do (Select, a preview while it moves, Keep or
  Cancel), with its hint bar (`ui::editing_hint`).
- The emulator's release ZIP for each computer
  (`paperpaw-emu-<version>-<target>.zip`) holds `paperpaw-sign` beside
  `paperpaw-emu` and `paperpaw-dev`, for signing plugins without a build
  of one's own.
- SDK: `Version::parse` reads a version with a pre-release or build
  after its three numbers as those numbers, so a plugin of a beta loads.
  `install::folder_release` gives the version as `plugin.ini` writes it;
  `Origins::release` the version a store named;
  `remember_store_install` takes either. `install::local::Staging::apart`
  puts a plugin's files together in a folder of its own, for one way of
  sending.
- `paperpaw-dev` takes font packs (`.ppfp`) in a data plugin's `fonts/`,
  where the kernel reads them, and names one elsewhere. It no longer
  checks the X4 Classic by default, and `--list-profiles` says it is not
  supported yet. A release no longer lists the X4 Classic's firmware:
  it is built, but not yet tried on one.
- `paperpaw-dev` checks a plugin on the emulated X4, X4 Pro and Boox, in
  every language, light and dark. It finds cut text, uneven margins,
  small touch targets, missing translations and screens that buttons
  cannot reach, and measures heap and frame time. `paperpaw-dev flows`
  plays scripted flows, also at their edges.
- The web emulator runs PaperPaw in a browser as WebAssembly. The
  desktop emulator can be driven over `--stdio`, writes a registry of
  settings, widgets and actions (`--registry`) and checks a plugin
  (`--check`).
- Native code plugins as files (`code.ppn`): the X4 and X4 Pro load
  them from the card at start. `paperpaw-sign pack` makes and signs
  them, and `samples/hello-native` shows how.
- Data plugins of formats 3 add menus, actions, icons, setup steps and
  fonts (`menus.ini`, `actions.ini`, `icons.ini`, `setup.ini`,
  `points/*.txt`), and mark words with `marks.ini`.
- The SDK grew a lot: reading apps, lookup providers, voices, page
  layers, places for widgets, building blocks for screens, one choice
  and confirm dialog, declared work and power modes, typed events and
  settings, where an installed plugin came from (`install::origin`),
  installs file by file (`install::local::Staging`), serving over the
  USB cable (`served::CABLE_PORT`), typing into a text field
  (`events::TYPE_TEXT`), and book data that is made again apart from the
  reader's own (`add_book_cache`, `add_book_data`). It has breaking
  changes before 1.0; `api.txt` in `crates/paperpaw-sdk` lists its
  public API, and CI checks it.
- `tools/usb-transfer.py` and the emulator's `--cable` speak the USB
  transfer's lines (`docs/usb-transfer.md`); `Kernel::type_text` and the
  X4's USB remote type into a keyboard.
- Each release also brings the desktop emulator for Windows, macOS and
  Linux with `paperpaw-dev`, the web emulator and the handbook, each
  signed. `docs/porting.md` says how to add a device.
- Code plugins are bound to a plugin interface, not to a release
  (ADR 0026): a plugin file runs on every PaperPaw of its interface
  (`native::INTERFACE`, 2, in the fingerprint's top bits), betas and
  releases alike, and authors rebuild only when a release raises it. The
  SDK and `paperpaw-native` have no build scripts, so a plugin built on
  Windows is the same as one built on Linux (`tools/reproducible-plugin.py`,
  in CI on both); a plugin project keeps `paperpaw-plugin.x` beside its
  `.cargo/config.toml`. CI fails where what plugins see changes without
  the interface raised, and by default any change raises it: the public
  API and the shape of the types (fields, variants and trait items in
  order, private ones too, functions' pointers with their ABI) and the
  values of the constants of the SDK and every crate of PaperPaw below
  it, with every feature on (`api.txt` and `shape.txt` of each,
  `tools/interface.py`), and the crates of others whose types cross,
  each pinned to one version. Only a new type, a function outside a
  trait, a constant or an `impl` goes free.
- A code plugin's file names the compiler that built it (format 2 of
  `.ppn`, taken by `paperpaw-sign pack` from `.comment`, which
  `paperpaw-plugin.x` now keeps). The X4s, the store and an update's
  fetch take only files of the firmware's fingerprint and compiler
  (`Native::fits`), the emulator only libraries of its own compiler; an
  update answer may name both (`abi`, `compiler`). Files of format 1 are
  refused as built for an older PaperPaw. The repository's
  `rust-toolchain.toml` pins the release's compiler, 1.99.0.
- A service is found by its type's name across builds only where the
  type is made of the SDK's crates, `core` and `alloc`, with the same
  size and alignment; a plugin's own type needs a contract
  (`ServiceType`). SDK: `native::CROSSING_CRATES`, `native::compiler`,
  `firmware::beta`.
- The workspace publishes to crates.io as a pre-release
  (`tools/publish-dry-run.sh 0.5.0-beta.1`, and the *Publish check*
  workflow).
- `paperpaw-dev` warns of a settings group without an order or an icon,
  and of a widget's settings in a group of the main page rather than the
  widget's own (`widgets::settings_group`). SDK: `Kernel::menus`.
- `paperpaw-dev` finds an explanation of several lines cut short
  (`visual.explanation-cut`, an error): the SDK's text blocks now say
  what they draw, "…" included, and a text of the language with a "…" of
  its own no longer passes every "…" as one. It warns of a setting's
  description longer than its row (`lint.help-cut`). SDK:
  `ui::note_fits`, `ask::confirm(…).only_yes()`.

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
- Screens say what they show: rows with their text and
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
