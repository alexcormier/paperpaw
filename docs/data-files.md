# Files on the card

What PaperPaw keeps on a device's storage, and how each file stays
readable as PaperPaw changes. PaperPaw's own files are in `.paperpaw`
(`paperpaw_sdk::data::DIR`), each plugin's in `.paperpaw/plugins/<id>`
(`Env::plugin_dir`). These are paths in the shell's storage: a shell may
keep the folder elsewhere than the books, e.g. in an app's own storage.
Every file is one of three kinds:

- **Data**: what the reader made or chose (settings, positions, notes,
  statistics). Every later version reads it. Its format has a version
  (`paperpaw_sdk::data::Format`): a text file says it in its first line,
  `# format 2`, and one without that line is version 1. A change that
  older readers would misread raises the version and adds the step from
  the version before; files are upgraded as they are read and written in
  the new format when they are saved next. Adding a key to an ini file
  needs no new version: readers skip keys they do not know. A version of
  PaperPaw older than a file (after going back to an older update) leaves
  the file as it is. Binary data files take a new name for a new version
  instead. Each version of each format has a file in a `testdata` folder
  that the tests read.
- **Cache**: made from the books or the network, made again when missing.
  Each carries a stamp of what it was made from and by which version of
  its maker; with another stamp it is made anew. Never upgraded.
- **Foreign**: formats of other programs (KOReader, Calibre), kept as
  those programs define them.

Reading positions and annotations are **locators**: a section (an EPUB's
spine item) and an offset into that section's source data, not into the
text a format plugin made of it (ADR 0007). So they stay valid when a
plugin converts books differently; a plugin must keep that mapping.

## Data

| File | Owner | Format | Tested |
|------|-------|--------|--------|
| `paperpaw.ini` | kernel | ini: settings by plugin, `[plugins]` switched on or off, `[menus]`, `[keymap.*]`, `[fonts]`, `[slots]` | version 1 |
| `.paperpaw/power.ini` | kernel | ini: the power mode worn (`mode`), whether *Automatic* chose it, the settings the reader took back from it (`released`) | |
| `.paperpaw/cleaned-up.txt` | kernel | when the storage was last cleaned up by itself, in seconds since 1970 | |
| `.paperpaw/secrets.txt` | kernel, SDK `secrets` | a line per password or key: id, label, flags, value veiled (tab separated) | version 1 |
| `.paperpaw/library/books/<hash>.ini` | SDK `library` | ini: a book's details, cover, progress, fields of plugins (the details read again when their `version` is older) | version 1 |
| `.paperpaw/library/opened.txt`, `favourites.txt` | SDK `library` | a path per line | version 1 |
| `.paperpaw/readers/<hash>.txt`, `.paperpaw/readers-folders.txt` | SDK `readers` | the reading app chosen for a book; for folders a line each: reading app, folder (tab separated) | |
| `.paperpaw/annotations/<hash>.txt` | SDK `annotations` | a line per bookmark or highlight (tab separated; 8 fields before times were kept, 10 since) | version 1 |
| `.paperpaw/plugins/reader/state.ini` | reader | ini: the book read last | version 1 |
| `.paperpaw/positions/<hash>.txt` | SDK `positions` | ini: where a book was left (`[position]` section, offset, percent, `conversion`, `xpointer` when it was closed, `updated`) | version 1 |
| `.paperpaw/plugins/reader/books/<hash>/progress.ini` | reader | ini: the position before positions were shared; read while a book has none | version 1 |
| `.paperpaw/plugins/reader/status-lines.txt` | reader | the status line chosen last at the top, then at the bottom, a line each | version 1 |
| `.paperpaw/plugins/reader/books/<hash>/settings.ini` | kernel (a book's own settings) | ini, as `paperpaw.ini` | version 1 |
| `.paperpaw/plugins/stats/books.ini` | stats | ini: a section per book (title, authors, md5, pages, language, series, `last-open`, path; `finished`, when it was last read to the end, added later) | version 1 |
| `.paperpaw/plugins/stats/pages.bin` | stats | 16 bytes per stay on a page | version 1 |
| `.paperpaw/plugins/stats/webdav.ini` | stats | ini of earlier versions: its password moves to the passwords and keys, then it goes | version 1 |
| `.paperpaw/plugins/profiles/profiles.ini` | profiles | ini: a section per profile, its name and values | versions 1 and 2 |
| `.paperpaw/plugins/profiles/last.txt` | profiles | the profile applied last | version 1 |
| `.paperpaw/plugins/rules/rules.txt` | rules | a rule per line: trigger, effect, target, on, condition (tab separated) | version 1 |
| `.paperpaw/plugins/vocabulary/words.txt` | vocabulary | a word per line: word, language, place, book, title, sentence (tab separated) | version 1 |
| `.paperpaw/plugins/vocabulary/cards/decks.txt`, `deck-<id>.txt` | vocabulary | `paperpaw-decks 1` / `paperpaw-cards 1`, then a deck per line (id, name) or a card per line (id, due, last, stability, difficulty, reps, lapses, front, back, example; tab separated, texts escaped) | version 1 |
| `.paperpaw/plugins/vocabulary/cards/answers.txt`, `due.txt`, `state.txt` | vocabulary | answers not yet written into their decks; when each deck's cards are due, by quarter hour; the time last known and the new cards shown that day | version 1 |
| `.paperpaw/plugins/vocabulary/cards/looked-up.txt`, `adding.txt`, `asleep.txt` | vocabulary | words looked up waiting to become cards (word, sentence); the deck cards are being added to; the card the sleep screen showed (deck, id) | version 1 |
| `.paperpaw/plugins/lock/tries.txt` | lock | the wrong tries in a row, a number | version 1 |
| `.paperpaw/plugins/plugin-manager/screens.txt` | plugin-manager | the screens offered for a slot that were already asked about, `slot screen` a line | |
| `.paperpaw/plugins/store/queue.txt`, `queue-quiet.txt` | store | the plugins waiting to be installed, a line each, the one whose turn it is first (marked `> `, those of the setup `~`); what became of the setup's, `+<id>` or `-<id>` a line | |
| `.paperpaw/plugins/store/author.txt` | store | the key the plugin library names for the author of the code plugin being downloaded, until it is installed | |
| `.paperpaw/plugins/<id>/install.txt` | SDK `install` | a download of a plugin not yet complete, in the folder of the plugin that installs (the store, the fonts browser) | |
| `.paperpaw/plugins/calibre/host.txt` | calibre | the address of the computer the reader chose to trust | version 1 |
| `.paperpaw/plugins/opds/catalogs.txt` | opds | a catalog per line: name, address, user, (password, empty), folder, `sync`, `webdav` | version 1 |
| `.paperpaw/plugins/opds/synced/<md5 of the address>.txt` | opds | the books a catalog's sync fetched, by their ids in the catalog, so that one deleted is not fetched again | |
| `.paperpaw/plugins/news/feeds.txt` | news | a feed per line: address, name | version 1 |
| `.paperpaw/plugins/browser/bookmarks.txt`, `history.txt` | browser | a page per line: title, address | version 1 |
| `.paperpaw/plugins/kosync/device.ini`, `account.ini` | kosync | ini: the device's id; the key of earlier versions (moved to the passwords and keys) | version 1 |
| `.paperpaw/plugins/wallabag/articles.txt`, `finished.txt` | wallabag | an article per line: id and path; ids read to the end | version 1 |
| `.paperpaw/plugins/joplin/sent.txt`, `sending.txt`, `waiting.txt`, `notebook.txt` | joplin | a book per line: notes file, note id, MD5 of the note's text (tab separated); the same for notes sent whose answer did not come; a notes file per line; the notebook's name and id | version 1 |
| `.paperpaw/plugins/battery/log.csv` | battery | CSV with a header | version 1 |
| `.paperpaw/crashes.txt` | X4 firmware | a line per panic: the version and why, written for bug reports, never read back | |
| `.paperpaw/plugins/battery/plugins.csv` | battery | CSV with a header, written for reading on a computer, never read back | |
| `.paperpaw/plugins/battery/log.old.csv`, `plugins.old.csv` | battery | the logs before, kept once a log grew large | |
| `.paperpaw/plugins/sleep/next.txt` | sleep | the next picture, counted | version 1 |
| `.paperpaw/plugins/reading-goal/today.txt` | reading-goal | the day (days since 1970, local time) and the seconds read on it, as the statistics told them | version 1 |
| `.paperpaw/plugins/setup/done` | setup | its presence | version 1 (the app's screen tests) |
| `.paperpaw/plugins/help/seen.txt` | help | a screen per line whose hints were shown | version 1 |
| `.paperpaw/plugins/update/installing.txt`, `offered.txt` | update | a path and a version, a line each (`offered.txt`: `online` and the version for an update the server named) | version 1 |
| `.paperpaw/plugins/update/waiting.txt` | update | an update that waits for plugins: its version, `since <unix seconds or ->`, `checked <unix seconds or ->`, `asked`, `seen` and `told` lines where set, then what it waits for, `plugin <id> <version>` and `crate <name>` lines | version 1 |
| `.paperpaw/plugin-authors.txt`, `plugin-code.txt` | SDK `native` | a code plugin per line: its id, a tab, its author's key remembered at the first install; its id, a tab, the SHA-256 of its file's header as installed (only such a file is loaded) | |
| `.paperpaw/plugin-code-fetched.txt` | SDK `native` | a code plugin per line: its id, a tab, the SHA-256 of its file's header fetched for a firmware update (into `plugins/.<id>.next`), which the updated firmware installs at its start | version 1 |
| `.paperpaw/plugins/weather/weather.txt` | weather | the last forecast (place, coordinates, when, now and the days), first line `weather 1` | version 1 |
| `.paperpaw/plugins/widgets/todo.txt` | widgets | an item per line, done ones as `x <date> <text>` (as todo.txt has them); an open item that would read as done (beginning with `x `) kept with the date `1970-01-01` in front, where todo.txt has the day an item was made | version 1 |
| `.paperpaw/plugins/widgets/quotes.txt` | widgets | the reader's own quotes, a line each: `text \| author` | version 1 |
| `.paperpaw/plugins/widgets/quote.txt`, `picture.txt` | widgets | the day and the turns of the quote and the picture shown | version 1 |
| `.paperpaw/plugins/widgets/seen.bits`, `newest.txt` | widgets | the books *Next up* saw, by the hashes of their paths, as a set of bits; the books that came since, a few | |
| `.paperpaw/plugins/widgets/reading.ini` | widgets | ini: the book read last, its page and pages, the seconds read and the pages turned for the speed; `chapter` and `chapter-left` (the pages left in it) added later | version 1 |

## Caches

| Files | Owner | Stamp |
|-------|-------|-------|
| `.paperpaw/plugins/format-epub/<hash>/` | format-epub | `CACHE_VERSION` and the book's size and start; HTML, Markdown and FB2 their own |
| `.paperpaw/plugins/format-xtc/<hash>/` | format-xtc | the book's size and start |
| `.paperpaw/plugins/widgets/picture.pixels` | widgets | `picture.stamp`: the picture's path and size, the screen's size and colour |
| `.paperpaw/logo.pixels` | SDK `logo` | `logo.stamp`: the picture's path and size, the size drawn, colour or not |
| `.paperpaw/library/covers/<hash>.thumb`, `<hash>.<variant>.thumb` | SDK `library` | covers made small for views of many books, made again when missing |
| `.paperpaw/plugins/sleep/cat-<side>.gray`, `picture.gray` | sleep | the sleeping cat decoded for a screen, its side in the name; the picture of the pictures folder shown last |
| `.paperpaw/plugins/sleep/overlay.gray`, `overlay-failed.txt` | sleep | `overlay.key`: the picture over the page and its size; why it did not show at the last sleep, written only when that changed |
| `.paperpaw/plugins/reading-goal/cat-<mood>-<bytes>.gray`, `own-<mood>-<bytes>.gray` | reading-goal | the mood's picture decoded (the shipped one, or the reader's own), the size of its file in the name |
| `.paperpaw/plugins/reader/books/<hash>/pages-<hash>.bin` | reader | the layout's settings, `paperpaw_layout::VERSION`, the book's size and the plugins' stylesheets, in the file's name; a new one removes the others once four are kept |
| `.paperpaw/plugins/reader/books/<hash>/headings.txt` | reader | the book's size on its first line: the headings found in a book without contents, found again when the size changes |
| `.paperpaw/library/catalog.txt` | SDK `library` | the folders' contents |
| `.paperpaw/plugins/stats/days.bin` | stats | the bytes of `pages.bin` it covers |
| `.paperpaw/plugins/dictionary/<hash>/` | dictionary | the dictionary's `.ifo` |
| `.paperpaw/plugins/opds/feed.json`, `news/headlines.txt`, `wallabag/entries.json`, `fonts-browser/catalog.json` | their plugins | fetched again |
| `.paperpaw/plugins/opds/covers/<md5 of the address>.gray` | opds | a catalog's cover decoded, by its address |
| `.paperpaw/plugins/store/index.txt`, `index.head`, `index.bin`, `search.bin`, `plugin-<id>.part`, `shot.gray` | store | the plugin library's list for this device, fetched again; a plugin's page and a screenshot as they are shown |
| `.paperpaw/plugins/joplin/info.json`, `locks.txt`, `found.md`, `note.md` | joplin | fetched or written again with each sending, removed after it |
| `.paperpaw/plugins/update/offer.txt`, `updates/.download` | update | the update server's last answer and a download until it is whole, fetched again; let go once the version is no longer newer |
| `.paperpaw/plugins/update/fetch.txt`, `fetch-current.txt`, `fetch-failed.txt`, `plugins/.<id>.next/` | update | the code plugins left to fetch for a downloaded update, the one being fetched, those that could not be; the folders fetched, installed by the updated firmware |
| `.paperpaw/plugins/about/licences.md`, `about/plugins.md`, `plugin-manager/how.txt`, `store/how.txt`, `.paperpaw/tmp/` | about, plugin-manager, store, kernel | written when they are opened to read |
| `.paperpaw/code-room.txt` | SDK `native` | the room code plugins have, as the shell found it at this start |

## Foreign

| Files | Owner | Defined by |
|-------|-------|------------|
| `.paperpaw/plugins/stats/statistics.sqlite3` and the files of a sync | stats | KOReader's statistics |
| `<book>.sdr/metadata.<ext>.lua` | koreader | KOReader's book settings |
| `wifi.ini` (at the top of the card) | wifi | networks to join, `ssid = ...` and `password = ...` lines, written by the web flasher or by hand; kept with the passwords and keys at a start, then removed |
| `.paperpaw/plugins/calibre/books/*.json`, `device.json` | calibre | Calibre's device driver |
