//! KOReader: the bookmarks and highlights of a book go back and forth
//! with KOReader, for books read in both on one device (or one library
//! folder synced between devices).
//!
//! KOReader keeps what it knows of a book beside it, in
//! `Book.sdr/metadata.epub.lua`. When PaperPaw opens a book that has such
//! a file, and when its bookmarks or highlights change, this plugin reads
//! it, takes over what is new there, and leaves what is new here in
//! `Book.sdr/Book.epub.annotations.lua`: KOReader takes that over the
//! next time it opens the book, and removes the file. Positions are
//! KOReader's XPointers
//! ([`Document::xpointer`](paperpaw_sdk::document::Document)), so EPUBs
//! only.
//!
//! What is removed on one side goes on the other too: the plugin keeps
//! which annotations both had at the last exchange, and their notes then.
//! A note changed on one side only goes to the other; one changed on both
//! keeps the later change.
//!
//! A file of either side that is there but cannot be read or understood
//! stops the exchange: nothing changes on either side until it can be.
//! So does a missing one of KOReader (as while a sync has brought the
//! folder but not yet the file in it). Annotations go to KOReader with
//! the time they were made: while neither they nor the clock tell it,
//! they wait here.

#![no_std]

extern crate alloc;

pub mod lua;

use alloc::borrow::Cow;
use alloc::format;
use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::RefCell;

use paperpaw_sdk::annotations::{self, Annotation, Kind, Mark};
use paperpaw_sdk::builtin::events;
use paperpaw_sdk::document::{Document, Locator};
use paperpaw_sdk::time::{self, LocalTime};
use paperpaw_sdk::{
    Env, Id, Manifest, MemoryClass, Plugin, RegisterError, Registrar, SDK_VERSION, Storage,
    StorageError,
};

use lua::{Table, Value};

pub const PLUGIN: Id = Id::new_static("koreader");
/// Who wrote the annotations file, for KOReader (which takes over only
/// those of other devices).
const DEVICE: &str = "PaperPaw";
/// Longest KOReader file read.
pub const MAX_FILE: usize = 4 << 20;
/// Longest KOReader file read on devices with a few hundred kilobytes
/// ([`MemoryClass::Small`]): some fifty annotations. The books of larger
/// ones are not exchanged there.
pub const MAX_FILE_SMALL: usize = 32 << 10;
/// Longest file of what is known read.
const MAX_KNOWN: usize = 1 << 20;
/// Most text taken from a book for a highlight.
const MAX_TEXT: usize = 4096;

/// A file that is there but cannot be read or understood (too large, an
/// error of the card, not KOReader's form): the exchange stops without
/// changing anything on either side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unreadable;

/// The longest KOReader file read on a device with `memory`.
pub fn max_file(memory: MemoryClass) -> usize {
    if memory == MemoryClass::Small {
        MAX_FILE_SMALL
    } else {
        MAX_FILE
    }
}

/// The file at `path`, `None` if there is none. At most `limit` bytes,
/// in memory asked for rather than taken, read whole or not at all.
fn load(storage: &dyn Storage, path: &str, limit: usize) -> Result<Option<Vec<u8>>, Unreadable> {
    let size = match storage.size(path) {
        Ok(size) => usize::try_from(size).map_err(|_| Unreadable)?,
        Err(StorageError::NotFound) => return Ok(None),
        Err(_) => return Err(Unreadable),
    };
    if size > limit {
        return Err(Unreadable);
    }
    let mut data = Vec::new();
    data.try_reserve_exact(size).map_err(|_| Unreadable)?;
    data.resize(size, 0);
    let mut filled = 0;
    while filled < size {
        match storage.read_at(path, filled as u64, &mut data[filled..]) {
            Ok(0) | Err(_) => return Err(Unreadable),
            Ok(n) => filled += n,
        }
    }
    Ok(Some(data))
}

/// KOReader's folder for the book at `book`: `Book.sdr` beside it.
pub fn sidecar(book: &str) -> String {
    let stem = book.rsplit_once('.').map_or(book, |(stem, _)| stem);
    format!("{stem}.sdr")
}

/// KOReader's own file in it: `metadata.<suffix>.lua`.
fn metadata_file(book: &str) -> String {
    let suffix = book.rsplit_once('.').map_or("_", |(_, suffix)| suffix);
    format!("{}/metadata.{suffix}.lua", sidecar(book))
}

/// Annotations for KOReader to take over: `<file name>.annotations.lua`.
fn exchange_file(book: &str) -> String {
    format!(
        "{}/{}.annotations.lua",
        sidecar(book),
        paperpaw_sdk::files::name(book)
    )
}

/// Where the annotations both sides had at the last exchange are kept.
/// Named by the book's key, so that the kernel moves and removes it with
/// the book (registered as book data).
fn known_file(book: &str) -> String {
    format!(
        "{}/{}.txt",
        Env::plugin_dir(PLUGIN.as_str()),
        paperpaw_sdk::library::key(book)
    )
}

/// What an annotation is known by: its kind and place.
type Key = (Kind, Locator, Locator);

fn key_of(mark: &Mark) -> Key {
    match mark.kind {
        Kind::Bookmark => (Kind::Bookmark, mark.start, mark.start),
        Kind::Highlight => (Kind::Highlight, mark.start, mark.end),
    }
}

/// An annotation both sides had at the last exchange: where it is, and
/// a fingerprint of the note both had on it then (`None` where their
/// notes differed, or in what an older version kept).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Known {
    key: Key,
    note: Option<u64>,
}

/// A fingerprint of `note` (64-bit FNV-1a): whether it changed since.
fn fingerprint(note: &str) -> u64 {
    note.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, b| {
        (hash ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// What both sides had at the last exchange, a line each: kind, start,
/// end and the note's fingerprint (`b 3 120 3 120 5d2c…`).
fn load_known(storage: &dyn Storage, book: &str) -> Result<Vec<Known>, Unreadable> {
    let Some(data) = load(storage, &known_file(book), MAX_KNOWN)? else {
        return Ok(Vec::new());
    };
    let text = core::str::from_utf8(&data).map_err(|_| Unreadable)?;
    Ok(text
        .lines()
        .filter_map(|line| {
            let mut fields = line.split(' ');
            let kind = match fields.next()? {
                "b" => Kind::Bookmark,
                "h" => Kind::Highlight,
                _ => return None,
            };
            let mut number = || fields.next()?.parse::<u32>().ok();
            let start = Locator::new(number()?, number()?);
            let end = Locator::new(number()?, number()?);
            let note = fields
                .next()
                .and_then(|hex| u64::from_str_radix(hex, 16).ok());
            Some(Known {
                key: (kind, start, end),
                note,
            })
        })
        .collect())
}

fn save_known(storage: &dyn Storage, book: &str, known: &[Known]) {
    let mut out = String::new();
    for Known {
        key: (kind, start, end),
        note,
    } in known
    {
        let kind = if *kind == Kind::Bookmark { 'b' } else { 'h' };
        out.push_str(&format!(
            "{kind} {} {} {} {}",
            start.section, start.offset, end.section, end.offset
        ));
        if let Some(note) = note {
            out.push_str(&format!(" {note:016x}"));
        }
        out.push('\n');
    }
    let _ = storage.write(&known_file(book), out.as_bytes());
}

/// Whether a folder has anything in it.
fn has_entries(storage: &dyn Storage, dir: &str) -> bool {
    let mut any = false;
    let _ = storage.list(dir, &mut |_| any = true);
    any
}

/// The annotations of KOReader's file at `path`, read if it is at most
/// `limit` bytes; `None` if there is no such file, none if it keeps none.
fn read_items(
    storage: &dyn Storage,
    path: &str,
    limit: usize,
) -> Result<Option<Vec<Table>>, Unreadable> {
    let Some(data) = load(storage, path, limit)? else {
        return Ok(None);
    };
    let text = core::str::from_utf8(&data).map_err(|_| Unreadable)?;
    let Some(Value::Table(mut root)) = lua::parse(text) else {
        return Err(Unreadable);
    };
    drop(data);
    let at = root
        .fields
        .iter()
        .position(|(key, _)| matches!(key, Value::Str(s) if s == "annotations"));
    let Some(at) = at else {
        // Older KOReaders kept them apart, in a form not read here.
        return if root.get("bookmarks").is_some() || root.get("highlight").is_some() {
            Err(Unreadable)
        } else {
            Ok(Some(Vec::new()))
        };
    };
    let Value::Table(items) = root.fields.swap_remove(at).1 else {
        return Err(Unreadable);
    };
    drop(root);
    // A list of tables, nothing else: what is not understood is not
    // passed on, so it must not be there.
    let mut items = items
        .fields
        .into_iter()
        .map(|field| match field {
            (Value::Number(n), Value::Table(item)) => Some((n.parse::<u64>().ok()?, item)),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()
        .ok_or(Unreadable)?;
    items.sort_by_key(|(n, _)| *n);
    Ok(Some(items.into_iter().map(|(_, item)| item).collect()))
}

/// KOReader's side of an exchange.
#[derive(Debug)]
pub struct Theirs {
    /// What KOReader has, or, while it has not taken over the last
    /// exchange, what it will have.
    pub items: Vec<Table>,
    /// Whether the last exchange still waits for KOReader.
    pub pending: bool,
}

/// KOReader's side for the book at `book`, from files of at most `limit`
/// bytes. `None` while KOReader keeps no file for it, also while a sync
/// has brought its folder but not yet the file in it: then there is
/// nothing to exchange with, and what both had stays known.
pub fn read_theirs(
    storage: &dyn Storage,
    book: &str,
    limit: usize,
) -> Result<Option<Theirs>, Unreadable> {
    let pending = storage.exists(&exchange_file(book));
    let path = if pending {
        exchange_file(book)
    } else {
        metadata_file(book)
    };
    Ok(read_items(storage, &path, limit)?.map(|items| Theirs { items, pending }))
}

/// The annotations here of the book at `book`. Lines not understood are
/// left out, so a file that is there but gave none is taken as one that
/// cannot be read too.
fn load_ours(storage: &dyn Storage, book: &str) -> Result<Vec<Annotation>, Unreadable> {
    let ours = annotations::load_for_update(storage, book).map_err(|_| Unreadable)?;
    let gone = matches!(
        storage.size(&annotations::file(book)),
        Ok(0) | Err(StorageError::NotFound)
    );
    if ours.is_empty() && !gone {
        Err(Unreadable)
    } else {
        Ok(ours)
    }
}

/// What an exchange works with.
pub struct Exchange<'a> {
    pub storage: &'a dyn Storage,
    /// The book, opened.
    pub doc: &'a dyn Document,
    /// Seconds since 1970 (UTC) now, 0 if not known.
    pub now: i64,
    /// Local seconds since 1970 of seconds since 1970 (UTC).
    pub local: &'a dyn Fn(i64) -> i64,
    /// How new highlights look in KOReader: `lighten` or `underscore`.
    pub drawer: &'a str,
}

impl core::fmt::Debug for Exchange<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Exchange")
            .field("now", &self.now)
            .field("drawer", &self.drawer)
            .finish_non_exhaustive()
    }
}

/// KOReader's times: local, `2026-10-16 14:05:00`.
fn datetime(x: &Exchange<'_>, unix: i64) -> String {
    let t = LocalTime::from_seconds((x.local)(unix));
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        t.year, t.month, t.day, t.hour, t.minute, t.second
    )
}

/// Seconds since 1970 (UTC) of one of KOReader's times; 0 if none.
fn unix_of(x: &Exchange<'_>, text: &str) -> i64 {
    let number = |range: core::ops::Range<usize>| text.get(range)?.parse::<i64>().ok();
    let (Some(year), Some(month), Some(day)) = (number(0..4), number(5..7), number(8..10)) else {
        return 0;
    };
    let (hour, minute, second) = (
        number(11..13).unwrap_or(0),
        number(14..16).unwrap_or(0),
        number(17..19).unwrap_or(0),
    );
    // Days since 1970 of the date (proleptic Gregorian).
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let local = days * 86_400 + hour * 3600 + minute * 60 + second;
    // The zone's offset then, near enough.
    local - ((x.local)(local) - local)
}

/// The text of a book from `start` to `end`, paragraphs on lines of
/// their own.
fn text_between(doc: &dyn Document, storage: &dyn Storage, start: Locator, end: Locator) -> String {
    let mut out = String::new();
    let mut at = start;
    for _ in 0..64 {
        if at >= end || out.len() >= MAX_TEXT {
            break;
        }
        let Ok(Some(block)) = doc.block_at(storage, at) else {
            if at.section + 1 >= doc.sections() {
                break;
            }
            at = Locator::new(at.section + 1, 0);
            continue;
        };
        let from = block.text_index(at.offset.max(block.start));
        let to = if at.section == end.section && end.offset < block.end {
            block.text_index(end.offset)
        } else {
            block.text.len()
        };
        if let Some(text) = block.text.get(from..to.max(from)) {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(text);
        }
        at = Locator::new(at.section, block.end.max(at.offset + 1));
    }
    out
}

/// The title of the chapter `at` is in.
fn chapter(doc: &dyn Document, storage: &dyn Storage, at: Locator) -> Option<String> {
    let mut title = None;
    for i in 0..doc.toc_len(storage) {
        let entry = doc.toc_entry(storage, i)?;
        if entry.at > at {
            break;
        }
        title = Some(entry.title);
    }
    title
}

/// Where KOReader's annotation `item` is here; `None` where it cannot be
/// placed (it then stays as it is).
fn locate(x: &Exchange<'_>, item: &Table) -> Option<Key> {
    let (doc, storage) = (x.doc, x.storage);
    if item.get("drawer").is_some() {
        let start = doc.locate_xpointer(storage, item.str("pos0")?)?;
        let end = doc.locate_xpointer(storage, item.str("pos1")?)?;
        (start < end).then_some((Kind::Highlight, start, end))
    } else {
        let at = doc.locate_xpointer(storage, item.str("page")?)?;
        Some((Kind::Bookmark, at, at))
    }
}

/// KOReader's annotation `item` as one here, at `key`.
fn annotation_of(x: &Exchange<'_>, item: &Table, (kind, start, end): Key) -> Annotation {
    let (doc, storage) = (x.doc, x.storage);
    let note = annotations::one_line(item.str("note").unwrap_or_default());
    let text = match kind {
        Kind::Highlight => item
            .str("text")
            .map(String::from)
            .unwrap_or_else(|| text_between(doc, storage, start, end)),
        // Bookmarks show the beginning of their page.
        Kind::Bookmark => {
            let mut text = text_between(doc, storage, start, Locator::new(start.section, u32::MAX));
            text.truncate(text.char_indices().nth(120).map_or(text.len(), |(i, _)| i));
            text
        }
    };
    Annotation {
        mark: Mark {
            kind,
            start,
            end,
            noted: !note.is_empty(),
        },
        percent: (doc.progress(start) * 100.0 + 0.5) as u8,
        created: unix_of(x, item.str("datetime").unwrap_or_default()),
        updated: unix_of(x, item.str("datetime_updated").unwrap_or_default()),
        text: annotations::snippet(&text),
        note,
    }
}

/// Annotation `a` as KOReader keeps it; `None` where it cannot be placed
/// there, or while neither it nor the clock tells when it was made
/// (KOReader knows annotations by that time).
fn item_of(x: &Exchange<'_>, a: &Annotation) -> Option<Value> {
    let (doc, storage) = (x.doc, x.storage);
    let created = known_time([a.created, x.now])?;
    let page = doc.xpointer(storage, a.mark.start)?;
    let optional = |v: Option<String>| v.map_or(Value::Nil, |v| lua::string(&v));
    let note = optional((!a.note.is_empty()).then(|| a.note.clone()));
    let updated = optional((a.updated > 0).then(|| datetime(x, a.updated)));
    let chapter = optional(chapter(doc, storage, a.mark.start));
    Some(match a.mark.kind {
        Kind::Bookmark => lua::table([
            ("chapter", chapter),
            ("datetime", lua::string(&datetime(x, created))),
            ("datetime_updated", updated),
            ("note", note),
            ("page", lua::string(&page)),
        ]),
        Kind::Highlight => {
            let end = doc.xpointer(storage, a.mark.end)?;
            let text = text_between(doc, storage, a.mark.start, a.mark.end);
            let text = if text.is_empty() {
                a.text.clone()
            } else {
                text
            };
            lua::table([
                ("chapter", chapter),
                ("datetime", lua::string(&datetime(x, created))),
                ("datetime_updated", updated),
                ("drawer", lua::string(x.drawer)),
                ("note", note),
                ("page", lua::string(&page)),
                ("pos0", lua::string(&page)),
                ("pos1", lua::string(&end)),
                ("text", lua::string(&text)),
            ])
        }
    })
}

/// The first of `times` that is known (seconds since 1970 over 0).
fn known_time(times: impl IntoIterator<Item = i64>) -> Option<i64> {
    times.into_iter().find(|&t| t > 0)
}

/// Exchanges the annotations of the book at `book` with KOReader's side
/// `theirs`; `true` if those here changed. Nothing changes on either side
/// if a file here cannot be read.
pub fn exchange(x: &Exchange<'_>, book: &str, theirs: Theirs) -> Result<bool, Unreadable> {
    let (doc, storage) = (x.doc, x.storage);
    // Only where KOReader's positions are understood.
    if doc.xpointer(storage, Locator::new(0, 0)).is_none() {
        return Ok(false);
    }
    let Theirs {
        items: theirs,
        pending,
    } = theirs;
    let known = load_known(storage, book)?;
    let mut ours = load_ours(storage, book)?;
    let known_of = |key: Key| known.iter().find(|k| k.key == key);
    let mut matched = alloc::vec![false; ours.len()];
    let (mut ours_changed, mut theirs_changed) = (false, false);
    let mut items: Vec<Value> = Vec::new();
    // What both have after this exchange.
    let mut shared: Vec<Known> = Vec::new();
    for mut item in theirs {
        let Some(key) = locate(x, &item) else {
            items.push(Value::Table(item));
            continue;
        };
        if let Some(i) = ours.iter().position(|a| key_of(&a.mark) == key) {
            matched[i] = true;
            let a = &mut ours[i];
            let note = annotations::one_line(item.str("note").unwrap_or_default());
            let mut same = true;
            if note != a.note {
                let changed = item
                    .str("datetime_updated")
                    .or(item.str("datetime"))
                    .unwrap_or_default();
                let their_time = unix_of(x, changed);
                let our_time = a.updated.max(a.created);
                // A note changed on one side since the last exchange goes
                // to the other; one changed on both, the later change,
                // where both times are known. Else both stay as they are.
                let before = known_of(key).and_then(|k| k.note);
                let theirs_wins = match before {
                    Some(n) if n == fingerprint(&a.note) => Some(true),
                    Some(n) if n == fingerprint(&note) => Some(false),
                    _ if their_time > 0 && our_time > 0 => Some(their_time > our_time),
                    _ => None,
                };
                match theirs_wins {
                    Some(true) => {
                        a.mark.noted = !note.is_empty();
                        a.note = note;
                        if their_time > 0 {
                            a.updated = their_time;
                        }
                        ours_changed = true;
                    }
                    Some(false) => {
                        let ours_note = (!a.note.is_empty()).then(|| lua::string(&a.note));
                        item.set("note", ours_note.unwrap_or(Value::Nil));
                        if let Some(time) = known_time([a.updated, a.created, x.now]) {
                            item.set("datetime_updated", lua::string(&datetime(x, time)));
                        }
                        theirs_changed = true;
                    }
                    None => same = false,
                }
            }
            shared.push(Known {
                key,
                note: same.then(|| fingerprint(&a.note)),
            });
            items.push(Value::Table(item));
        } else if known_of(key).is_some() {
            // Removed here: not passed on.
            theirs_changed = true;
        } else {
            let a = annotation_of(x, &item, key);
            shared.push(Known {
                key,
                note: Some(fingerprint(&a.note)),
            });
            ours.push(a);
            matched.push(true);
            ours_changed = true;
            items.push(Value::Table(item));
        }
    }
    // Ours that KOReader does not have: new here, or removed there.
    let mut kept = Vec::with_capacity(ours.len());
    for (a, matched) in ours.into_iter().zip(matched) {
        if matched {
            kept.push(a);
        } else if known_of(key_of(&a.mark)).is_some() && !pending {
            ours_changed = true;
        } else {
            // Those not passed on now are not known to both, so they are
            // offered again next time.
            if let Some(item) = item_of(x, &a) {
                items.push(item);
                theirs_changed = true;
                shared.push(Known {
                    key: key_of(&a.mark),
                    note: Some(fingerprint(&a.note)),
                });
            }
            kept.push(a);
        }
    }
    // Not kept here, what came from KOReader must not count as known:
    // the next exchange would take it for removed here.
    if ours_changed && !annotations::save(storage, book, &kept) {
        return Err(Unreadable);
    }
    if theirs_changed {
        let at = known_time([x.now]).map(|now| lua::string(&datetime(x, now)));
        let file = lua::table([
            ("annotations", Value::Table(Table::from_list(items))),
            ("datetime", at.unwrap_or(Value::Nil)),
            ("device_id", lua::string(DEVICE)),
        ]);
        let _ = storage.write(&exchange_file(book), lua::write(&file).as_bytes());
    }
    if shared != known {
        save_known(storage, book, &shared);
    }
    Ok(ours_changed)
}

/// Exchanges the annotations of the book at `book` with KOReader's, if
/// KOReader keeps any for it; `true` if those here changed.
fn exchange_book(env: &Env<'_>, book: &str) -> bool {
    // KOReader's side first: most books have none, and then the book
    // need not be opened.
    let limit = max_file(env.device.memory);
    let Ok(Some(theirs)) = read_theirs(env.storage, book, limit) else {
        return false;
    };
    let Ok(doc) = env.formats.open(env.storage, book) else {
        return false;
    };
    let local = |unix| time::local_seconds(env, unix);
    let x = Exchange {
        storage: env.storage,
        doc: doc.as_ref(),
        now: time::unix_seconds(env).unwrap_or(0),
        local: &local,
        // As highlights look here: a line under them, else a marker.
        drawer: if paperpaw_sdk::decorations::Decoration::of_role(
            env,
            &paperpaw_sdk::builtin::roles::HIGHLIGHT,
        )
        .line
        .is_some()
        {
            "underscore"
        } else {
            "lighten"
        },
    };
    exchange(&x, book, theirs).unwrap_or(false)
}

/// The book whose annotations this plugin announced as changed last: the
/// announcement comes back to it, and needs no exchange.
#[derive(Debug, Default)]
pub struct Announced(RefCell<Option<String>>);

impl Announced {
    /// Remembers that the change of `book`'s annotations is announced.
    pub fn set(&self, book: &str) {
        *self.0.borrow_mut() = Some(String::from(book));
    }

    /// Whether the change of `book`'s annotations is the one announced
    /// (once).
    pub fn is_own(&self, book: &str) -> bool {
        let mut announced = self.0.borrow_mut();
        let own = announced.as_deref() == Some(book);
        if own {
            *announced = None;
        }
        own
    }
}

/// A book moved: KOReader's folder goes along (what is known moves as
/// book data).
fn moved_book(env: &Env<'_>, from: &str, to: &str) {
    let storage = env.storage;
    if has_entries(storage, &sidecar(from)) {
        let _ = storage.rename(&sidecar(from), &sidecar(to));
        let old = format!(
            "{}/{}.annotations.lua",
            sidecar(to),
            paperpaw_sdk::files::name(from)
        );
        let _ = storage.rename(&old, &exchange_file(to));
    }
}

/// The KOReader plugin.
#[derive(Debug, Default)]
pub struct KoReader;

impl Plugin for KoReader {
    fn manifest(&self) -> Manifest {
        Manifest {
            id: PLUGIN,
            name: Cow::Borrowed("KOReader"),
            description: Cow::Borrowed(
                "Shares bookmarks and highlights with KOReader for books read in both.",
            ),
            // Released with PaperPaw, also as a file from the store.
            version: paperpaw_sdk::package_version!(),
            sdk: SDK_VERSION,
            uses: paperpaw_sdk::Uses::NOTHING,
        }
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        // What both sides had at the last exchange, by book: moved and
        // removed with it.
        r.add_book_data(Env::plugin_dir(PLUGIN.as_str()), "")?;
        let announced = Rc::new(Announced::default());
        let opened = Rc::clone(&announced);
        r.on(events::BOOK_OPENED, move |env, reading, out| {
            if exchange_book(env, &reading.path) {
                opened.set(&reading.path);
                out.push(annotations::changed(&reading.path));
            }
        })?;
        r.on(events::ANNOTATIONS_CHANGED, move |env, changed, out| {
            if !announced.is_own(&changed.path) && exchange_book(env, &changed.path) {
                announced.set(&changed.path);
                out.push(annotations::changed(&changed.path));
            }
        })?;
        r.on(events::BOOK_MOVED, |env, moved, _| {
            moved_book(env, &moved.from, &moved.path);
        })?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/en.ini")))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/de.ini")))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
