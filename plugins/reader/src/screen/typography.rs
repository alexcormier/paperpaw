//! The page's type: the SDK's (see `paperpaw_sdk::reading::PageType`),
//! with room for the reader's status lines.

use super::*;

pub(super) use paperpaw_sdk::reading::{
    EmbeddedFonts as Embedded, PageType as Typography, ReadingFaces as Faces, StatusRoom,
};

/// The type of a book's pages in `language`, with room for the status
/// lines shown; a book of `whole_pages` is shown edge to edge.
pub(super) fn typography(
    env: &Env<'_>,
    font: &FontHandle<'_>,
    language: &str,
    whole_pages: bool,
) -> Typography {
    let edge = env.settings().read(&status::EDGE).clamp(0, 150) as u32;
    let room = StatusRoom::new(
        status::shown(env, true),
        status::shown(env, false),
        status::height(env),
        env.metrics().tenth_mm(edge) as f32,
    );
    Typography::new(env, font, language, whole_pages, room)
}
