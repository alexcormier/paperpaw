//! The library as a service: one place every view of the books asks
//! ([`service`]), which a plugin may replace with its own (a library kept
//! by Calibre's database, say), and what plugins add to every view of a
//! book: fields kept with it ([`BookField`]) and badges on its row
//! ([`BookBadge`]).
//!
//! The kernel's library (the default) keeps what it knows in
//! `.paperpaw/library`: each book's details, the books read last, the
//! favourites and the catalog ([`catalog`], [`index`](mod@super::index)). A replacement
//! publishes itself as the service [`SERVICE`]
//! ([`Registrar::publish`](crate::Registrar::publish), a
//! `Box<dyn Library>`) and answers the same questions its own way; views
//! that ask [`service`] show its books.

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::vec::Vec;
use core::fmt;

use super::catalog::{self, Query, Record, State};
use super::index::Index;
use super::{Book, what};
use crate::{Command, Env, Id};

/// The service id of the library ([`service`]).
pub const SERVICE: &str = "core.library";

/// What every view of the library asks: books, their details, what is
/// found, the index of all, and changes to a book. Every method has the
/// kernel's library as its default; a replacement overrides what it keeps
/// otherwise.
pub trait Library {
    /// What the library knows of the book at `path`.
    fn book(&self, env: &Env<'_>, path: &str) -> Option<Book> {
        super::book(env.storage, path)
    }

    /// Whether there is a catalog to ask ([`catalog::state`]).
    fn catalog_state(&self, env: &Env<'_>) -> State {
        catalog::state(env.storage)
    }

    /// `n` books `query` finds, after `after` ([`catalog::query`]).
    fn query(&self, env: &Env<'_>, query: &Query, after: Option<&Record>, n: usize) -> Vec<Record> {
        catalog::query(env.storage, query, after, n)
    }

    /// How many books `query` finds.
    fn found(&self, env: &Env<'_>, query: &Query) -> usize {
        catalog::found(env.storage, query)
    }

    /// The index of every book ([`Index::build`]): at most `cap`, those in
    /// the folder `archive` left out.
    fn index(&self, env: &Env<'_>, cap: usize, archive: &str) -> Index {
        Index::build(env.storage, cap, archive)
    }

    /// The book of item `i` of `index` ([`Index::record`]), e.g. for the
    /// page a view shows.
    fn record(&self, env: &Env<'_>, index: &Index, i: usize) -> Option<Record> {
        index.record(env.storage, i)
    }

    /// Marks the book at `path` finished, or unread; the command tells
    /// everyone ([`super::changed_book`]).
    fn set_finished(&self, env: &Env<'_>, path: &str, finished: bool) -> Command {
        super::set_finished(env.storage, path, finished);
        super::changed_book(path, what::STATUS | what::PROGRESS)
    }

    /// Makes the book at `path` a favourite, or no longer one.
    fn set_favourite(&self, env: &Env<'_>, path: &str, favourite: bool) -> Command {
        super::set_favourite(env.storage, path, favourite);
        super::changed_book(path, what::FAVOURITE)
    }
}

/// The kernel's library.
#[derive(Debug, Default, Clone, Copy)]
pub struct Kept;

impl Library for Kept {}

static KEPT: Kept = Kept;

/// The library to ask: one a plugin published as [`SERVICE`], else the
/// kernel's.
pub fn service<'e>(env: &Env<'e>) -> &'e dyn Library {
    match env.services.get::<Box<dyn Library>>(SERVICE) {
        Some(library) => library.as_ref(),
        None => &KEPT,
    }
}

/// A field plugins keep with books ([`super::set_field`]), e.g. a rating
/// or where a book came from: shown on its information page, and kept in
/// the catalog (to filter and sort by) where `in_catalog`. Added with
/// [`Registrar::add_book_field`](crate::Registrar::add_book_field).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct BookField {
    /// The field's id, e.g. `stars.rating`.
    pub id: Id,
    /// Translation key of its name.
    pub title: Cow<'static, str>,
    /// Kept in the catalog: short values only (a line a book).
    pub in_catalog: bool,
}

impl BookField {
    pub fn new(id: impl Into<Id>, title: impl Into<Cow<'static, str>>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            in_catalog: false,
        }
    }

    /// Kept in the catalog too.
    pub fn in_catalog(mut self) -> Self {
        self.in_catalog = true;
        self
    }
}

/// What a badge on a book's row shows.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Badge {
    /// A few letters ("4★", "Loaned").
    Text(alloc::string::String),
    /// An icon by its name.
    Icon(&'static str),
}

/// What a badge says of a book: asked for the books of a page when it is
/// read, never while drawing.
pub type BadgeFn = Box<dyn Fn(&Env<'_>, &Record) -> Option<Badge>>;

/// A badge plugins put on books' rows in views of the library, e.g. a
/// rating or "on loan"; views show two at most. Added with
/// [`Registrar::add_book_badge`](crate::Registrar::add_book_badge).
#[non_exhaustive]
pub struct BookBadge {
    pub id: Id,
    /// Translation key of its name (where views list them).
    pub title: Cow<'static, str>,
    pub badge: BadgeFn,
}

impl BookBadge {
    pub fn new(
        id: impl Into<Id>,
        title: impl Into<Cow<'static, str>>,
        badge: impl Fn(&Env<'_>, &Record) -> Option<Badge> + 'static,
    ) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            badge: Box::new(badge),
        }
    }
}

impl fmt::Debug for BookBadge {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BookBadge")
            .field("id", &self.id)
            .field("title", &self.title)
            .finish_non_exhaustive()
    }
}

/// The badges of `record`, at most `n`, from every plugin.
pub fn badges(env: &Env<'_>, record: &Record, n: usize) -> Vec<Badge> {
    env.book_badges
        .iter()
        .filter_map(|b| (b.badge)(env, record))
        .take(n)
        .collect()
}
