# Files on the card

What QuireInk keeps on a device's storage, and how each file stays
readable as QuireInk changes. QuireInk's own files are in `.quireink`
(`quireink_sdk::data::DIR`), each plugin's in `.quireink/plugins/<id>`
(`Env::plugin_dir`). These are paths in the shell's storage: a shell may
keep the folder elsewhere than the books, e.g. in an app's own storage.
Every file is one of three kinds:

- **Data**: what the reader made or chose (settings, positions, notes,
  statistics). Every later version reads it. Its format has a version
  (`quireink_sdk::data::Format`): a text file says it in its first line,
  `# format 2`, and one without that line is version 1. A change that
  older readers would misread raises the version and adds the step from
  the version before; files are upgraded as they are read and written in
  the new format when they are saved next. Adding a key to an ini file
  needs no new version: readers skip keys they do not know. A version of
  QuireInk older than a file (after going back to an older update) leaves
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
| `quireink.ini` | kernel | ini: settings by plugin, `[plugins]` switched on or off, `[menus]`, `[keymap.*]`, `[fonts]`, `[slots]` | version 1 |
| `.quireink/secrets.txt` | kernel, SDK `secrets` | a line per password or key: id, label, flags, value veiled (tab separated) | version 1 |
| `.quireink/library/books/<hash>.ini` | SDK `library` | ini: a book's details, cover, progress, fields of plugins (the details read again when their `version` is older) | version 1 |
| `.quireink/library/opened.txt`, `favourites.txt` | SDK `library` | a path per line | version 1 |
| `.quireink/annotations/<hash>.txt` | SDK `annotations` | a line per bookmark or highlight (tab separated; 8 fields before times were kept, 10 since) | version 1 |
| `.quireink/firmware-keys.txt` | SDK `firmware` | a trusted key per line (64 hexadecimal digits) | version 1 |
| `.quireink/plugins/reader/state.ini` | reader | ini: the book read last | version 1 |
| `.quireink/plugins/reader/books/<hash>/progress.ini` | reader | ini: the position | version 1 |
| `.quireink/plugins/reader/books/<hash>/settings.ini` | kernel (a book's own settings) | ini, as `quireink.ini` | version 1 |
| `.quireink/plugins/stats/books.ini` | stats | ini: a section per book | version 1 |
| `.quireink/plugins/stats/pages.bin` | stats | 16 bytes per stay on a page | version 1 |
| `.quireink/plugins/stats/webdav.ini` | stats | ini of earlier versions: its password moves to the passwords and keys, then it goes | version 1 |
| `.quireink/plugins/profiles/profiles.ini` | profiles | ini: a section per profile, its name and values | versions 1 and 2 |
| `.quireink/plugins/profiles/last.txt` | profiles | the profile applied last | version 1 |
| `.quireink/plugins/rules/rules.txt` | rules | a rule per line: trigger, effect, target, on, condition (tab separated) | version 1 |
| `.quireink/plugins/vocabulary/words.txt` | vocabulary | a word per line: word, language, place, book, title, sentence (tab separated) | version 1 |
| `.quireink/plugins/opds/catalogs.txt` | opds | a catalog per line: name, address, user, (password, empty), folder, `sync`, `webdav` | version 1 |
| `.quireink/plugins/news/feeds.txt` | news | a feed per line: address, name | version 1 |
| `.quireink/plugins/browser/bookmarks.txt`, `history.txt` | browser | a page per line: title, address | version 1 |
| `.quireink/plugins/kosync/device.ini`, `account.ini` | kosync | ini: the device's id; the key of earlier versions (moved to the passwords and keys) | version 1 |
| `.quireink/plugins/wallabag/articles.txt`, `finished.txt` | wallabag | an article per line: id and path; ids read to the end | version 1 |
| `.quireink/plugins/battery/log.csv` | battery | CSV with a header | version 1 |
| `.quireink/plugins/battery/plugins.csv` | battery | CSV with a header, written for reading on a computer, never read back | |
| `.quireink/plugins/sleep/next.txt` | sleep | the next picture, counted | version 1 |
| `.quireink/plugins/setup/done` | setup | its presence | version 1 (the app's screen tests) |
| `.quireink/plugins/update/installing.txt`, `offered.txt` | update | a path and a version, a line each | version 1 |

## Caches

| Files | Owner | Stamp |
|-------|-------|-------|
| `.quireink/plugins/format-epub/<hash>/` | format-epub | `CACHE_VERSION` and the book's size and start; HTML, Markdown and FB2 their own |
| `.quireink/plugins/format-xtc/<hash>/` | format-xtc | the book's size and start |
| `.quireink/plugins/reader/books/<hash>/index.bin` | reader | the layout's settings and `quireink_layout::VERSION`, in the folder's name |
| `.quireink/library/catalog.txt` | SDK `library` | the folders' contents |
| `.quireink/plugins/stats/days.bin` | stats | the bytes of `pages.bin` it covers |
| `.quireink/plugins/dictionary/<hash>/` | dictionary | the dictionary's `.ifo` |
| `.quireink/plugins/opds/feed.json`, `news/headlines.txt`, `wallabag/entries.json` | their plugins | fetched again |
| `.quireink/plugins/about/about.md`, `.quireink/tmp/` | about, kernel | written when needed |

## Foreign

| Files | Owner | Defined by |
|-------|-------|------------|
| `.quireink/plugins/stats/statistics.sqlite3` and the files of a sync | stats | KOReader's statistics |
| `<book>.sdr/metadata.<ext>.lua` | koreader | KOReader's book settings |
| `.quireink/plugins/calibre/books/*.json`, `device.json` | calibre | Calibre's device driver |
