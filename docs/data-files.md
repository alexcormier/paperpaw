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
| `.paperpaw/secrets.txt` | kernel, SDK `secrets` | a line per password or key: id, label, flags, value veiled (tab separated) | version 1 |
| `.paperpaw/library/books/<hash>.ini` | SDK `library` | ini: a book's details, cover, progress, fields of plugins (the details read again when their `version` is older) | version 1 |
| `.paperpaw/library/opened.txt`, `favourites.txt` | SDK `library` | a path per line | version 1 |
| `.paperpaw/annotations/<hash>.txt` | SDK `annotations` | a line per bookmark or highlight (tab separated; 8 fields before times were kept, 10 since) | version 1 |
| `.paperpaw/firmware-keys.txt` | SDK `firmware` | a trusted key per line (64 hexadecimal digits) | version 1 |
| `.paperpaw/plugins/reader/state.ini` | reader | ini: the book read last | version 1 |
| `.paperpaw/plugins/reader/books/<hash>/progress.ini` | reader | ini: the position | version 1 |
| `.paperpaw/plugins/reader/status-lines.txt` | reader | the status line chosen last at the top, then at the bottom, a line each | version 1 |
| `.paperpaw/plugins/reader/books/<hash>/settings.ini` | kernel (a book's own settings) | ini, as `paperpaw.ini` | version 1 |
| `.paperpaw/plugins/stats/books.ini` | stats | ini: a section per book (title, authors, md5, pages, language, series, `last-open`, path; `finished`, when it was last read to the end, added later) | version 1 |
| `.paperpaw/plugins/stats/pages.bin` | stats | 16 bytes per stay on a page | version 1 |
| `.paperpaw/plugins/stats/webdav.ini` | stats | ini of earlier versions: its password moves to the passwords and keys, then it goes | version 1 |
| `.paperpaw/plugins/profiles/profiles.ini` | profiles | ini: a section per profile, its name and values | versions 1 and 2 |
| `.paperpaw/plugins/profiles/last.txt` | profiles | the profile applied last | version 1 |
| `.paperpaw/plugins/rules/rules.txt` | rules | a rule per line: trigger, effect, target, on, condition (tab separated) | version 1 |
| `.paperpaw/plugins/vocabulary/words.txt` | vocabulary | a word per line: word, language, place, book, title, sentence (tab separated) | version 1 |
| `.paperpaw/plugins/opds/catalogs.txt` | opds | a catalog per line: name, address, user, (password, empty), folder, `sync`, `webdav` | version 1 |
| `.paperpaw/plugins/news/feeds.txt` | news | a feed per line: address, name | version 1 |
| `.paperpaw/plugins/browser/bookmarks.txt`, `history.txt` | browser | a page per line: title, address | version 1 |
| `.paperpaw/plugins/kosync/device.ini`, `account.ini` | kosync | ini: the device's id; the key of earlier versions (moved to the passwords and keys) | version 1 |
| `.paperpaw/plugins/wallabag/articles.txt`, `finished.txt` | wallabag | an article per line: id and path; ids read to the end | version 1 |
| `.paperpaw/plugins/joplin/sent.txt`, `waiting.txt`, `notebook.txt` | joplin | a book per line: notes file, note id, MD5 of the note's text (tab separated); a notes file per line; the notebook's name and id | version 1 |
| `.paperpaw/plugins/battery/log.csv` | battery | CSV with a header | version 1 |
| `.paperpaw/crashes.txt` | X4 firmware | a line per panic: the version and why, written for bug reports, never read back | |
| `.paperpaw/plugins/battery/plugins.csv` | battery | CSV with a header, written for reading on a computer, never read back | |
| `.paperpaw/plugins/sleep/next.txt` | sleep | the next picture, counted | version 1 |
| `.paperpaw/plugins/setup/done` | setup | its presence | version 1 (the app's screen tests) |
| `.paperpaw/plugins/update/installing.txt`, `offered.txt` | update | a path and a version, a line each (`offered.txt`: `online` and the version for an update the server named) | version 1 |
| `.paperpaw/plugins/weather/weather.txt` | weather | the last forecast (place, coordinates, when, now and the days), first line `weather 1` | version 1 |
| `.paperpaw/plugins/widgets/todo.txt` | widgets | an item per line, done ones as `x <date> <text>` (as todo.txt has them) | version 1 |
| `.paperpaw/plugins/widgets/quotes.txt` | widgets | the reader's own quotes, a line each: `text \| author` | version 1 |
| `.paperpaw/plugins/widgets/quote.txt`, `picture.txt` | widgets | the day and the turns of the quote and the picture shown | version 1 |
| `.paperpaw/plugins/widgets/reading.ini` | widgets | ini: the book read last, its page and pages, the seconds read and the pages turned for the speed; `chapter` and `chapter-left` (the pages left in it) added later | version 1 |

## Caches

| Files | Owner | Stamp |
|-------|-------|-------|
| `.paperpaw/plugins/format-epub/<hash>/` | format-epub | `CACHE_VERSION` and the book's size and start; HTML, Markdown and FB2 their own |
| `.paperpaw/plugins/format-xtc/<hash>/` | format-xtc | the book's size and start |
| `.paperpaw/plugins/widgets/picture.pixels` | widgets | `picture.stamp`: the picture's path and size, the screen's size and colour |
| `.paperpaw/plugins/reader/books/<hash>/pages-<hash>.bin` | reader | the layout's settings, `paperpaw_layout::VERSION`, the book's size and the plugins' stylesheets, in the file's name; a new one removes the others once four are kept |
| `.paperpaw/plugins/reader/books/<hash>/headings.txt` | reader | the book's size on its first line: the headings found in a book without contents, found again when the size changes |
| `.paperpaw/library/catalog.txt` | SDK `library` | the folders' contents |
| `.paperpaw/plugins/stats/days.bin` | stats | the bytes of `pages.bin` it covers |
| `.paperpaw/plugins/dictionary/<hash>/` | dictionary | the dictionary's `.ifo` |
| `.paperpaw/plugins/opds/feed.json`, `news/headlines.txt`, `wallabag/entries.json` | their plugins | fetched again |
| `.paperpaw/plugins/joplin/info.json`, `locks.txt`, `found.md`, `note.md` | joplin | fetched or written again with each sending, removed after it |
| `.paperpaw/plugins/update/offer.txt`, `updates/.download` | update | the update server's last answer and a download until it is whole, fetched again; let go once the version is no longer newer |
| `.paperpaw/plugins/about/about.md`, `.paperpaw/tmp/` | about, kernel | written when needed |

## Foreign

| Files | Owner | Defined by |
|-------|-------|------------|
| `.paperpaw/plugins/stats/statistics.sqlite3` and the files of a sync | stats | KOReader's statistics |
| `<book>.sdr/metadata.<ext>.lua` | koreader | KOReader's book settings |
| `.paperpaw/plugins/calibre/books/*.json`, `device.json` | calibre | Calibre's device driver |
