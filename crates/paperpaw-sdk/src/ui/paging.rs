//! Which page of a paged list shows, of how many.

/// The page shown (0-based) and the number of pages, e.g. for
/// [`Header::draw`](super::Header::draw) and [`Pager::draw`](super::Pager::draw).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Paging {
    /// The page shown, from 0.
    pub page: usize,
    /// How many there are, at least 1.
    pub pages: usize,
}

impl Paging {
    /// Page `page` (from 0) of `pages`.
    pub const fn new(page: usize, pages: usize) -> Self {
        Self { page, pages }
    }

    /// Whether this is the first page.
    pub const fn is_first(&self) -> bool {
        self.page == 0
    }

    /// Whether this is the last page.
    pub const fn is_last(&self) -> bool {
        self.page + 1 >= self.pages
    }
}
