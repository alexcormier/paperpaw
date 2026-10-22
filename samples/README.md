# Samples

`library/` is the default storage of the desktop emulator
(`cargo run -p paperpaw-emulator`); pass `--library <dir>` to use another
folder. The emulator writes reading positions and page indexes to
`library/.paperpaw/`, which is ignored by Git.

## Books

Both books are in the public domain and come unchanged from
[Project Gutenberg](https://www.gutenberg.org), including its licence text:

| File | Book | Source |
|------|------|--------|
| `library/books/alice-in-wonderland.txt` | Lewis Carroll, *Alice's Adventures in Wonderland* (English) | [eBook #11](https://www.gutenberg.org/ebooks/11) |
| `library/books/die-verwandlung.txt` | Franz Kafka, *Die Verwandlung* (German) | [eBook #22367](https://www.gutenberg.org/ebooks/22367) |

## Plugins

`plugins/large-print` is a data plugin: larger text in books and menus,
with its name translated to German. To try it, copy it into
`library/plugins/` (or the `plugins` folder of a device) and restart.

`plugins/airy-theme` is a theme as a data plugin: new defaults for the
appearance settings.

`plugins/reading-nook` is a home screen as a data plugin: new defaults
for the home screen's settings in `settings.ini`, shown until the reader
arranges it.

`hello-plugin` is a plugin with code to start from: an action in the
tools, a screen for touch and buttons, a setting, an event and
translations. [Writing plugins](../docs/plugins.md) walks through it; try
it with `cargo run -p paperpaw-emulator --features hello`.

`hello-native` is the same plugin as a file the X4 and the X4 Pro load
at their start, without a new firmware, in a workspace of its own with a
CI job for a plugin's repository; see its [README](hello-native/README.md)
and [Plugins as files](../docs/plugins.md#plugins-as-files).
