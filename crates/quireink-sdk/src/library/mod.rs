//! The library as a service: what is known about books beyond their
//! files, for every plugin that shows or uses them (the library, the home
//! screen, the sleep screen, syncs). Data and the screens that show it
//! are apart (PLANNING §6.3): any plugin may offer another view of the
//! same books.
//!
//! - A book's details: title, author, series, tags and cover, read from
//!   the book once ([`index`]), its reading progress, and fields that
//!   plugins add (e.g. a rating or where a book came from), in
//!   `.quireink/library/books/<hash>.ini`.
//! - The books opened last, most recent first (`opened.txt`), and the
//!   favourites (`favourites.txt`).
//! - The catalog of all books under the library folder ([`catalog`]),
//!   and listings of a folder's books ([`listing`]).
//!
//! The kernel keeps them up to date from the standard reading events and
//! moves them with a book ([`record`], [`moved`]). Nothing here grows
//! with the size of the library: the lists are capped, and details are
//! read a book at a time.

pub mod catalog;
pub mod listing;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::config;
use crate::document::ImageRef;
use crate::{Env, Storage};
use quireink_digest::fnv1a64;

/// Where the library keeps what it knows.
pub const DIR: &str = crate::data_path!("library");

/// The library folder, where books are kept (e.g. `books`): the `files`
/// plugin offers it in its settings, and every plugin that puts books
/// somewhere reads it ([`folder`]).
pub const FOLDER: crate::setting::Setting<crate::setting::Text> =
    crate::setting::Setting::new("files.root");

/// The library folder ([`FOLDER`]) as a path in the storage, without
/// slashes around it (`""` for the storage's top).
pub fn folder<'e>(env: &Env<'e>) -> &'e str {
    env.settings.read(&FOLDER).trim_matches('/')
}
/// Books remembered for "recently read" order.
const MAX_OPENED: usize = 100;
/// Favourites kept at most.
const MAX_FAVOURITES: usize = 500;
/// What the details hold; books read with another version are read
/// again (2 added the series, 3 the tags).
const DETAILS: u32 = 3;

/// What the library knows of a book.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Book {
    /// File size when the details were read; another size means another
    /// file.
    pub size: u64,
    /// Title, author and cover were read from the book.
    pub indexed: bool,
    pub title: Option<String>,
    pub author: Option<String>,
    /// The series and the book's number in it, if it names one.
    pub series: Option<String>,
    pub series_index: Option<String>,
    /// Its tags (subjects), if it names any.
    pub tags: Vec<String>,
    pub cover: Option<ImageRef>,
    /// Reading progress in percent, once opened.
    pub progress: Option<u8>,
    /// Fields plugins keep with the book: id (e.g. `myplugin.rating`)
    /// and value.
    pub fields: Vec<(String, String)>,
}

impl Book {
    /// The value of field `id`, if set.
    pub fn field(&self, id: &str) -> Option<&str> {
        self.fields
            .iter()
            .find(|(k, _)| k == id)
            .map(|(_, v)| v.as_str())
    }

    /// The title, else the file's name without its extension.
    pub fn title_or<'a>(&'a self, path: &'a str) -> &'a str {
        match &self.title {
            Some(title) if !title.is_empty() => title,
            _ => {
                let name = crate::files::name(path);
                name.rsplit_once('.').map_or(name, |(stem, _)| stem)
            }
        }
    }
}

/// The key that names what is kept for the book at `path`: 16
/// hexadecimal digits of its hash. Folders of [`BookData`] name their
/// entries by it.
pub fn key(path: &str) -> String {
    format!("{:016x}", fnv1a64(path))
}

/// A folder where a plugin keeps something for each book, named by the
/// book's [`key`] after `prefix`, alone or followed by `.` and more (a
/// file or a folder): `cache/0123456789abcdef`, `epub-0123…/`,
/// `0123….ini`. Registered with
/// [`Registrar::add_book_data`](crate::Registrar::add_book_data), the
/// kernel moves it with its book ([`move_data`]), removes it with it
/// ([`remove_data`]) and when its book is no longer there ([`clean_up`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BookData {
    pub folder: alloc::borrow::Cow<'static, str>,
    pub prefix: alloc::borrow::Cow<'static, str>,
}

impl BookData {
    /// The book key entry `name` of the folder is named by, if it is one
    /// of these, and what follows it.
    pub fn key_of<'n>(&self, name: &'n str) -> Option<(&'n str, &'n str)> {
        let rest = name.strip_prefix(&*self.prefix)?;
        let key = rest.get(..16)?;
        let tail = &rest[16..];
        let hex = key.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
        (hex && (tail.is_empty() || tail.starts_with('.'))).then_some((key, tail))
    }

    /// The entries of the folder that are kept for books: name, whether a
    /// folder.
    fn entries(&self, storage: &dyn Storage) -> Vec<(String, bool)> {
        let mut entries = Vec::new();
        let _ = storage.list(&self.folder, &mut |entry| {
            if self.key_of(entry.name).is_some() {
                entries.push((String::from(entry.name), entry.is_dir));
            }
        });
        entries
    }
}

/// Moves what is kept for the book `from` in folders `data` to `to`.
pub fn move_data(storage: &dyn Storage, data: &[BookData], from: &str, to: &str) {
    let (old, new) = (key(from), key(to));
    for folder in data {
        for (name, _) in folder.entries(storage) {
            if let Some((found, tail)) = folder.key_of(&name)
                && found == old
            {
                let to = format!("{}/{}{new}{tail}", folder.folder, folder.prefix);
                let _ = storage.rename(&format!("{}/{name}", folder.folder), &to);
            }
        }
    }
}

/// Removes what is kept for the book at `path` in folders `data`.
pub fn remove_data(storage: &dyn Storage, data: &[BookData], path: &str) {
    let gone = key(path);
    clean(storage, data, &|key| key != gone);
}

/// Removes what is kept in folders `data` for books whose key `keep` does
/// not keep; how many entries went.
pub fn clean_up(storage: &dyn Storage, data: &[BookData], keep: &dyn Fn(&str) -> bool) -> u32 {
    clean(storage, data, keep)
}

fn clean(storage: &dyn Storage, data: &[BookData], keep: &dyn Fn(&str) -> bool) -> u32 {
    let mut removed = 0;
    for folder in data {
        for (name, is_dir) in folder.entries(storage) {
            let Some((key, _)) = folder.key_of(&name) else {
                continue;
            };
            if keep(key) {
                continue;
            }
            let path = format!("{}/{name}", folder.folder);
            let done = if is_dir {
                storage.remove_dir_all(&path)
            } else {
                storage.remove(&path)
            };
            removed += u32::from(done.is_ok());
        }
    }
    removed
}

/// The keys ([`key`]) of every book on the storage (every file a format
/// plugin opens, in any folder but QuireInk's own), sorted: what
/// [`clean_up`] keeps.
pub fn book_keys(storage: &dyn Storage, formats: &crate::Formats) -> Vec<u64> {
    let mut keys = Vec::new();
    books_in(storage, formats, "", &mut |path| keys.push(fnv1a64(path)));
    keys.sort_unstable();
    keys
}

/// Every book in `folder` and the folders in it (every file a format
/// plugin opens; not in hidden folders), by its path; the book itself if
/// `folder` is one. E.g. to say they are gone before a folder is deleted.
pub fn books_in(
    storage: &dyn Storage,
    formats: &crate::Formats,
    folder: &str,
    found: &mut dyn FnMut(&str),
) {
    let folder = folder.trim_matches('/');
    let name = folder.rsplit('/').next().unwrap_or(folder);
    if !folder.is_empty() && formats.supports(name) && storage.size(folder).is_ok() {
        return found(folder);
    }
    let mut folders = alloc::vec![String::from(folder)];
    while let Some(folder) = folders.pop() {
        let _ = storage.list(&folder, &mut |entry| {
            if entry.name.starts_with('.') {
                return;
            }
            let path = if folder.is_empty() {
                String::from(entry.name)
            } else {
                format!("{folder}/{}", entry.name)
            };
            if entry.is_dir {
                folders.push(path);
            } else if formats.supports(entry.name) {
                found(&path);
            }
        });
    }
}

fn book_file(path: &str) -> String {
    format!("{DIR}/books/{}.ini", key(path))
}

fn cover_file(path: &str) -> String {
    format!("{DIR}/covers/{}.thumb", key(path))
}

/// The cover of `book` (the one at `path`) made small to fit `width` ×
/// `height`, as a panel of `format` shows it: kept in the library
/// (`covers/<hash>.thumb`), so views of many books (a list, a grid) read a
/// few kilobytes for each instead of its picture. Made anew where the
/// book or the size asked changed.
pub fn cover(
    storage: &dyn Storage,
    path: &str,
    book: &Book,
    width: u16,
    height: u16,
    format: quireink_platform::PixelFormat,
) -> Option<crate::ui::Thumbnail> {
    let image = book.cover.as_ref()?;
    let (w, h) = crate::ui::fit_image(image, width, height);
    let stamp = fnv1a64(format!(
        "{} {} {}x{} {w}x{h} {format:?}",
        book.size, image.path, image.width, image.height
    ));
    let file = cover_file(path);
    if let Some(thumb) = crate::ui::Thumbnail::kept(storage, &file, stamp) {
        return Some(thumb);
    }
    let thumb = crate::ui::Thumbnail::new(storage, image, w, h, format)?;
    thumb.keep(storage, &file, stamp);
    Some(thumb)
}

fn opened_file() -> String {
    format!("{DIR}/opened.txt")
}

fn favourites_file() -> String {
    format!("{DIR}/favourites.txt")
}

/// The lines of a list file.
fn read_list(storage: &dyn Storage, path: &str, max: usize) -> Vec<String> {
    let Ok(data) = crate::read_file(storage, path, max * 512) else {
        return Vec::new();
    };
    core::str::from_utf8(&data)
        .unwrap_or_default()
        .lines()
        .filter(|line| !line.is_empty())
        .map(ToString::to_string)
        .collect()
}

/// What the library knows of the book at `path`, if it belongs to this
/// file (not to another one of that name before).
pub fn book(storage: &dyn Storage, path: &str) -> Option<Book> {
    book_sized(storage, path, storage.size(path).ok()?)
}

/// [`book`] for a file whose size is known, e.g. from a listing
/// ([`listing::Entry::size`]): the file is not looked up again (on a card,
/// a search through its folder).
pub fn book_sized(storage: &dyn Storage, path: &str, size: u64) -> Option<Book> {
    let data = crate::read_file(storage, &book_file(path), 8192).ok()?;
    let source = core::str::from_utf8(&data).ok()?;
    let mut book = Book::default();
    let (mut cover_path, mut cover_width, mut cover_height) = (None, None, None);
    let mut version = 1;
    for entry in config::parse(source).filter_map(Result::ok) {
        let text = || entry.value.as_str().into_owned();
        match (entry.section, entry.key) {
            ("field", key) => book.fields.push((key.to_string(), text())),
            (_, "size") => book.size = entry.value.raw().parse().unwrap_or_default(),
            (_, "indexed") => book.indexed = entry.value.as_bool().unwrap_or_default(),
            (_, "title") => book.title = Some(text()),
            (_, "author") => book.author = Some(text()),
            (_, "series") => book.series = Some(text()),
            (_, "series-index") => book.series_index = Some(text()),
            (_, key) if key.starts_with("tag.") => book.tags.push(text()),
            (_, "version") => version = entry.value.as_u32().unwrap_or(1),
            (_, "cover") => cover_path = Some(text()),
            (_, "cover-width") => cover_width = entry.value.as_u32(),
            (_, "cover-height") => cover_height = entry.value.as_u32(),
            (_, "progress") => book.progress = entry.value.as_u32().map(|p| p.min(100) as u8),
            _ => {}
        }
    }
    if let (Some(path), Some(width), Some(height)) = (cover_path, cover_width, cover_height) {
        book.cover = Some(ImageRef {
            path,
            width,
            height,
        });
    }
    // Details of an older kind are read again.
    book.indexed &= version == DETAILS;
    (book.size == size).then_some(book)
}

/// Keeps `book` as what the library knows of the book at `path`.
pub fn save(storage: &dyn Storage, path: &str, book: &Book) {
    let mut ini = String::new();
    let mut set = |key: &str, value: &str| ini = config::set(&ini, "book", key, value);
    set("size", &book.size.to_string());
    set("indexed", if book.indexed { "on" } else { "off" });
    set("version", &DETAILS.to_string());
    if let Some(title) = &book.title {
        set("title", title);
    }
    if let Some(author) = &book.author {
        set("author", author);
    }
    if let Some(series) = &book.series {
        set("series", series);
    }
    if let Some(index) = &book.series_index {
        set("series-index", index);
    }
    for (i, tag) in book.tags.iter().enumerate() {
        set(&format!("tag.{}", i + 1), tag);
    }
    if let Some(cover) = &book.cover {
        set("cover", &cover.path);
        set("cover-width", &cover.width.to_string());
        set("cover-height", &cover.height.to_string());
    }
    if let Some(progress) = book.progress {
        set("progress", &progress.to_string());
    }
    for (id, value) in &book.fields {
        ini = config::set(&ini, "field", id, value);
    }
    let _ = storage.write(&book_file(path), ini.as_bytes());
}

/// What the library knows of the book at `path`, or a new entry for it.
fn book_or_new(storage: &dyn Storage, path: &str) -> Book {
    book(storage, path).unwrap_or_else(|| Book {
        size: storage.size(path).unwrap_or_default(),
        ..Book::default()
    })
}

/// Reads title, author, series, tags and cover from the book itself and
/// keeps them. Opening a book the first time may take a moment (e.g. an
/// EPUB's package is read), so views do this for one book at a time,
/// e.g. for the rows they show.
pub fn index(env: &Env<'_>, path: &str) -> Book {
    let storage = env.storage;
    let mut book = book_or_new(storage, path);
    book.size = storage.size(path).unwrap_or_default();
    book.indexed = true;
    if let Ok(document) = env.formats.open(storage, path) {
        // What the book names; else what its source said ([`provide`]).
        let metadata = document.metadata();
        book.title = metadata.title.clone().or(book.title.take());
        book.author = metadata.authors.first().cloned().or(book.author.take());
        book.series = metadata.series.clone().or(book.series.take());
        book.series_index = metadata.series_index.clone().or(book.series_index.take());
        if !metadata.subjects.is_empty() {
            book.tags = metadata.subjects.clone();
        }
        book.cover = document.cover(storage).ok().flatten();
    }
    save(storage, path, &book);
    book
}

/// What the library knows of the book at `path`, read from the book
/// first if it is not yet ([`index`]).
pub fn indexed(env: &Env<'_>, path: &str) -> Book {
    match book(env.storage, path) {
        Some(book) if book.indexed => book,
        _ => index(env, path),
    }
}

/// What a source knows of a book it brought, e.g. a catalog or Calibre
/// ([`provide`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Provided {
    pub title: Option<String>,
    pub author: Option<String>,
    pub series: Option<String>,
    pub series_index: Option<String>,
    pub tags: Vec<String>,
}

/// Keeps what a source knows of the book at `path`, for books that name
/// nothing themselves (text files, PDFs): what the book names wins when
/// it is read ([`index`]).
pub fn provide(storage: &dyn Storage, path: &str, provided: Provided) {
    let mut book = book_or_new(storage, path);
    let some = |s: Option<String>| s.filter(|s| !s.trim().is_empty());
    book.title = some(provided.title).or(book.title);
    book.author = some(provided.author).or(book.author);
    book.series = some(provided.series).or(book.series);
    book.series_index = some(provided.series_index).or(book.series_index);
    if !provided.tags.is_empty() {
        book.tags = provided.tags;
    }
    // Read from the book anew, keeping this where it names nothing.
    book.indexed = false;
    save(storage, path, &book);
}

/// Sets field `id` of the book at `path` (a plugin's, e.g.
/// `myplugin.rating`); an empty value removes it.
pub fn set_field(storage: &dyn Storage, path: &str, id: &str, value: &str) {
    let mut book = book_or_new(storage, path);
    book.fields.retain(|(k, _)| k != id);
    if !value.is_empty() {
        book.fields.push((id.to_string(), value.to_string()));
    }
    save(storage, path, &book);
}

/// Records a reading event (`core.book-opened`, `core.page-turned`): the
/// book's progress, its title until it is read from the book, and for an
/// opened book its place in the "recently read" order.
pub fn record(env: &Env<'_>, reading: &crate::builtin::events::Reading, opened: bool) {
    let path = reading.path.as_str();
    let storage = env.storage;
    if opened {
        let mut list = opened_books(storage);
        list.retain(|p| p != path);
        list.insert(0, path.to_string());
        list.truncate(MAX_OPENED);
        let _ = storage.write(&opened_file(), list.join("\n").as_bytes());
    }
    let progress = Some((reading.progress + 0.5).clamp(0.0, 100.0) as u8);
    let mut book = book_or_new(storage, path);
    let mut changed = false;
    let title = reading.title.as_str();
    if !title.is_empty() && !book.indexed && book.title.as_deref() != Some(title) {
        book.title = Some(title.to_string());
        changed = true;
    }
    // Written only when the shown percentage changes.
    if progress.is_some() && book.progress != progress {
        book.progress = progress;
        changed = true;
    }
    if changed {
        save(storage, path, &book);
    }
}

/// Moves what the library knows of a book from `from` to `to`: its
/// details, its place among the recently opened and the favourites.
pub fn moved(storage: &dyn Storage, from: &str, to: &str) {
    let _ = storage.rename(&book_file(from), &book_file(to));
    let _ = storage.rename(&cover_file(from), &cover_file(to));
    // A cover kept by a format's data for the book moves with it too
    // ([`BookData`]): the details name it where it is now.
    if let Some(mut book) = book(storage, to)
        && let Some(cover) = book.cover.as_mut()
        && cover.path.contains(&key(from))
    {
        cover.path = cover.path.replace(&key(from), &key(to));
        save(storage, to, &book);
    }
    let replace = |list: Vec<String>| -> Vec<String> {
        list.into_iter()
            .map(|p| if p == from { to.to_string() } else { p })
            .collect()
    };
    let opened = replace(opened_books(storage));
    let _ = storage.write(&opened_file(), opened.join("\n").as_bytes());
    let favourites = replace(favourites(storage));
    let _ = storage.write(&favourites_file(), favourites.join("\n").as_bytes());
}

/// Forgets the book at `path`, deleted: its details, its cover made
/// small, its place among the recently opened and the favourites.
pub fn removed(storage: &dyn Storage, path: &str) {
    let _ = storage.remove(&book_file(path));
    let _ = storage.remove(&cover_file(path));
    let without =
        |list: Vec<String>| -> Vec<String> { list.into_iter().filter(|p| p != path).collect() };
    let opened = without(opened_books(storage));
    let _ = storage.write(&opened_file(), opened.join("\n").as_bytes());
    let favourites = without(favourites(storage));
    let _ = storage.write(&favourites_file(), favourites.join("\n").as_bytes());
}

/// Marks the book at `path` as read to the end, or as not read at all.
pub fn set_finished(storage: &dyn Storage, path: &str, finished: bool) {
    let mut book = book_or_new(storage, path);
    book.progress = finished.then_some(100);
    save(storage, path, &book);
}

/// Paths of the books opened last, most recent first (also of books no
/// longer there).
pub fn opened_books(storage: &dyn Storage) -> Vec<String> {
    read_list(storage, &opened_file(), MAX_OPENED)
}

/// The paths of the favourite books, the newest first.
pub fn favourites(storage: &dyn Storage) -> Vec<String> {
    read_list(storage, &favourites_file(), MAX_FAVOURITES)
}

/// Makes the book at `path` a favourite, or no longer one.
pub fn set_favourite(storage: &dyn Storage, path: &str, favourite: bool) {
    let mut list = favourites(storage);
    list.retain(|p| p != path);
    if favourite {
        list.insert(0, path.to_string());
        list.truncate(MAX_FAVOURITES);
    }
    let _ = storage.write(&favourites_file(), list.join("\n").as_bytes());
}

/// Takes over what the library plugin kept before the library was a
/// service (`.quireink/plugins/files`), once.
pub fn take_over_old(storage: &dyn Storage) {
    if storage.exists(&opened_file()) || storage.exists(&favourites_file()) {
        return;
    }
    let old = crate::Env::plugin_dir("files");
    for name in ["opened.txt", "favourites.txt"] {
        let _ = storage.rename(&format!("{old}/{name}"), &format!("{DIR}/{name}"));
    }
    let _ = storage.rename(&format!("{old}/books"), &format!("{DIR}/books"));
    // The catalog is made anew where it is now.
    let _ = storage.remove(&format!("{old}/catalog.txt"));
}

/// The command that tells everyone books came into the library or left
/// it ([`events::LIBRARY_CHANGED`](crate::builtin::events::LIBRARY_CHANGED)).
pub fn changed() -> crate::Command {
    crate::builtin::events::LIBRARY_CHANGED.emit(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MemoryStorage;

    #[test]
    fn details_belong_to_one_file_version() {
        let storage = MemoryStorage::new().with("books/a.txt", "hello");
        let book = Book {
            size: 5,
            indexed: true,
            title: Some("A = \"title\"".into()),
            author: Some("Ann".into()),
            series: Some("Tales".into()),
            series_index: Some("2".into()),
            tags: alloc::vec!["Fantasy".into(), "To read".into()],
            cover: Some(ImageRef {
                path: "x.gray".into(),
                width: 30,
                height: 40,
            }),
            progress: Some(12),
            fields: alloc::vec![("stars.rating".into(), "4".into())],
        };
        save(&storage, "books/a.txt", &book);
        let read = super::book(&storage, "books/a.txt");
        assert_eq!(read, Some(book));
        assert_eq!(read.unwrap().field("stars.rating"), Some("4"));
        set_field(&storage, "books/a.txt", "stars.rating", "");
        assert!(
            super::book(&storage, "books/a.txt")
                .unwrap()
                .fields
                .is_empty()
        );
        // Replaced by a file of another size: forgotten.
        let storage = storage.with("books/a.txt", "hello!");
        assert_eq!(super::book(&storage, "books/a.txt"), None);
    }

    #[test]
    fn covers_made_small_are_kept_for_the_book() {
        use quireink_image::{GrayWriter, ImageSink};
        let storage = MemoryStorage::new().with("books/a.epub", "12345");
        let mut writer = GrayWriter::new(&storage, "cache/cover.gray");
        writer.start(40, 60).unwrap();
        for y in 0..60u8 {
            writer.row(&[y * 4; 40]).unwrap();
        }
        writer.finish().unwrap();
        let mut book = Book {
            size: 5,
            cover: Some(ImageRef {
                path: "cache/cover.gray".into(),
                width: 40,
                height: 60,
            }),
            ..Book::default()
        };
        let made = cover(
            &storage,
            "books/a.epub",
            &book,
            20,
            20,
            quireink_platform::PixelFormat::Gray8,
        )
        .unwrap();
        assert_eq!((made.width(), made.height()), (13, 20));
        // Kept: read again without the picture.
        storage.remove("cache/cover.gray").unwrap();
        let kept = cover(
            &storage,
            "books/a.epub",
            &book,
            20,
            20,
            quireink_platform::PixelFormat::Gray8,
        )
        .unwrap();
        assert_eq!((kept.width(), kept.height()), (13, 20));
        // Moved with the book.
        moved(&storage, "books/a.epub", "books/b.epub");
        assert!(
            cover(
                &storage,
                "books/b.epub",
                &book,
                20,
                20,
                quireink_platform::PixelFormat::Gray8
            )
            .is_some()
        );
        // Another size asked, another book of that name: made anew (here
        // it cannot be, its picture gone).
        assert!(
            cover(
                &storage,
                "books/b.epub",
                &book,
                30,
                30,
                quireink_platform::PixelFormat::Gray8
            )
            .is_none()
        );
        book.size = 6;
        assert!(
            cover(
                &storage,
                "books/b.epub",
                &book,
                20,
                20,
                quireink_platform::PixelFormat::Gray8
            )
            .is_none()
        );
    }

    #[test]
    fn what_is_kept_for_a_book_follows_it() {
        let (a, b) = ("books/a.epub", "books/b.epub");
        let (ka, kb) = (key(a), key(b));
        let storage = MemoryStorage::new()
            .with(a, "book")
            .with(&format!("cache/{ka}/0.txt"), "chapter")
            .with(&format!("cache/cbz-{ka}/source"), "x")
            .with(&format!("cache/{ka}.ini"), "x")
            .with("cache/other.txt", "not a book's")
            .with(&format!("cache/{ka}x"), "not a key");
        let data = [
            BookData {
                folder: "cache".into(),
                prefix: "".into(),
            },
            BookData {
                folder: "cache".into(),
                prefix: "cbz-".into(),
            },
        ];
        assert_eq!(
            data[0].key_of(&format!("{ka}.ini")),
            Some((ka.as_str(), ".ini"))
        );
        assert_eq!(data[0].key_of("other.txt"), None);
        // Moved: every entry of its key, by the new one.
        move_data(&storage, &data, a, b);
        assert!(storage.exists(&format!("cache/{kb}/0.txt")));
        assert!(storage.exists(&format!("cache/cbz-{kb}/source")));
        assert!(storage.exists(&format!("cache/{kb}.ini")));
        assert!(!storage.exists(&format!("cache/{ka}.ini")));
        // Removed: all of it; the rest stays.
        remove_data(&storage, &data, b);
        assert!(!storage.exists(&format!("cache/{kb}/0.txt")));
        assert!(!storage.exists(&format!("cache/{kb}.ini")));
        assert!(storage.exists("cache/other.txt"));
        assert!(storage.exists(&format!("cache/{ka}x")));
        // Cleaned up: what is kept for books no longer there.
        let c = "books/c.txt";
        let storage = storage
            .with(c, "text")
            .with(&format!("cache/{}.ini", key(c)), "x");
        let _ = storage.write(&format!("cache/{ka}.ini"), b"left over");
        let mut formats = crate::Formats::new();
        formats
            .add(crate::FormatSpec {
                id: crate::Id::new_static("test.txt"),
                title: alloc::borrow::Cow::Borrowed("TXT"),
                extensions: alloc::vec![
                    alloc::borrow::Cow::Borrowed("txt"),
                    alloc::borrow::Cow::Borrowed("epub")
                ],
                open: alloc::boxed::Box::new(|_, _, _| {
                    Err(crate::document::DocumentError::UnsupportedFormat)
                }),
                prepared: None,
            })
            .unwrap();
        let books = book_keys(&storage, &formats);
        let keep =
            |k: &str| u64::from_str_radix(k, 16).is_ok_and(|k| books.binary_search(&k).is_ok());
        // The epub at a is there, c too: nothing goes; a gone, its data goes.
        assert_eq!(clean_up(&storage, &data, &keep), 0);
        storage.remove(a).unwrap();
        let books = book_keys(&storage, &formats);
        let keep =
            |k: &str| u64::from_str_radix(k, 16).is_ok_and(|k| books.binary_search(&k).is_ok());
        assert_eq!(clean_up(&storage, &data, &keep), 1);
        assert!(storage.exists(&format!("cache/{}.ini", key(c))));
        // The books in a folder, or the book itself.
        let mut found = Vec::new();
        books_in(&storage, &formats, "books", &mut |p| {
            found.push(String::from(p))
        });
        assert_eq!(found, [c]);
        found.clear();
        books_in(&storage, &formats, c, &mut |p| found.push(String::from(p)));
        assert_eq!(found, [c]);
    }

    #[test]
    fn removed_books_are_forgotten() {
        let storage = MemoryStorage::new().with("books/a.txt", "hello");
        save(
            &storage,
            "books/a.txt",
            &Book {
                size: 5,
                ..Book::default()
            },
        );
        set_favourite(&storage, "books/a.txt", true);
        let _ = storage.write(
            &opened_file(),
            b"books/a.txt
books/b.txt",
        );
        removed(&storage, "books/a.txt");
        assert!(book(&storage, "books/a.txt").is_none());
        assert!(favourites(&storage).is_empty());
        assert_eq!(opened_books(&storage), ["books/b.txt"]);
    }

    #[test]
    fn what_sources_provide_is_kept() {
        let storage = MemoryStorage::new().with("books/a.txt", "hello");
        provide(
            &storage,
            "books/a.txt",
            Provided {
                title: Some("Alice".into()),
                author: Some("Lewis Carroll".into()),
                tags: alloc::vec!["Classics".into()],
                ..Provided::default()
            },
        );
        let book = super::book(&storage, "books/a.txt").unwrap();
        assert_eq!(book.title.as_deref(), Some("Alice"));
        assert_eq!(book.tags, ["Classics"]);
        assert!(!book.indexed);
    }

    #[test]
    fn favourites_newest_first() {
        let storage = MemoryStorage::new();
        set_favourite(&storage, "a.epub", true);
        set_favourite(&storage, "b.epub", true);
        set_favourite(&storage, "a.epub", true);
        assert_eq!(favourites(&storage), ["a.epub", "b.epub"]);
        set_favourite(&storage, "a.epub", false);
        assert_eq!(favourites(&storage), ["b.epub"]);
        moved(&storage, "b.epub", "c.epub");
        assert_eq!(favourites(&storage), ["c.epub"]);
    }

    #[test]
    fn titles_fall_back_to_the_file_name() {
        let book = Book::default();
        assert_eq!(
            book.title_or("books/Alice in Wonderland.epub"),
            "Alice in Wonderland"
        );
        let book = Book {
            title: Some("Alice".into()),
            ..Book::default()
        };
        assert_eq!(book.title_or("books/x.epub"), "Alice");
    }

    #[test]
    fn what_the_library_plugin_kept_is_taken_over() {
        let storage = MemoryStorage::new()
            .with(".quireink/plugins/files/opened.txt", "books/a.epub")
            .with(".quireink/plugins/files/books/0000000000000001.ini", "x");
        take_over_old(&storage);
        assert_eq!(opened_books(&storage), ["books/a.epub"]);
        assert!(storage.exists(".quireink/library/books/0000000000000001.ini"));
    }
}
