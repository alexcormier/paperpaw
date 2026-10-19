//! The covers of recent books, from the library service
//! ([`paperpaw_sdk::library`]). Looking a cover up opens the book, so the
//! home screen does it a book at a time while it shows, and its widgets
//! only read what the library knows.

use paperpaw_sdk::document::ImageRef;
use paperpaw_sdk::{Env, library};

/// Looks up the cover of `path` (opening the book), for the library to
/// keep.
pub(crate) fn look_up(env: &Env<'_>, path: &str) -> Option<ImageRef> {
    library::index(env, path).cover
}
