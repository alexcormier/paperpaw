//! The book being read: opening it, laying out its pages, where the
//! reader is (kept in `progress.ini`), the page index, chapters and
//! links.

use super::*;

/// The book as the layout sees it: every block through the text
/// filters of plugins (see `paperpaw_sdk::filters`).
pub(crate) struct Filtered<'d, 'e> {
    pub(crate) document: &'d dyn Document,
    pub(crate) env: &'d Env<'e>,
}

impl Document for Filtered<'_, '_> {
    fn metadata(&self) -> &paperpaw_sdk::document::Metadata {
        self.document.metadata()
    }

    fn sections(&self) -> u32 {
        self.document.sections()
    }

    fn section_len(&self, section: u32) -> u32 {
        self.document.section_len(section)
    }

    fn block_at(
        &self,
        storage: &dyn paperpaw_sdk::Storage,
        at: Locator,
    ) -> Result<Option<paperpaw_sdk::document::Block>, DocumentError> {
        let mut block = self.document.block_at(storage, at)?;
        if let Some(block) = &mut block {
            self.env.text_filters.apply(self.env, block);
        }
        Ok(block)
    }

    fn progress(&self, at: Locator) -> f32 {
        self.document.progress(at)
    }
}

/// The book being read.
pub(super) struct Book {
    pub(super) path: String,
    /// Directory for this book's data.
    pub(super) dir: String,
    pub(super) title: String,
    pub(super) document: Box<dyn Document>,
    /// The book's language, or the UI language if it declares none.
    pub(super) language: String,
    pub(super) typography: Typography,
    pub(super) page: Page,
    /// The words of the page, once measured (see [`Book::words`]).
    pub(super) words: core::cell::OnceCell<Vec<WordBox>>,
    /// The starts of the pages turned forward to this one, one after
    /// another (none before a jump).
    pub(super) history: Vec<Locator>,
    /// When the position was written last, and whether the page shown has
    /// changed since.
    pub(super) saved_at: Option<paperpaw_sdk::Instant>,
    pub(super) unsaved: bool,
    pub(super) index: Option<PageIndex>,
    /// Where background pagination continues.
    pub(super) indexing: Option<Locator>,
    /// Whether pagination stopped for this layout (a page that cannot be
    /// laid out, a storage that takes no more).
    pub(super) index_stopped: bool,
    /// Where the bookmarks and highlights are.
    pub(super) marks: Vec<Mark>,
    /// Text to point out on this page, e.g. a search hit.
    pub(super) found: Option<(Locator, Locator)>,
    /// The chapter of the page, for status lines and the reading events:
    /// where it starts, where the next one starts and its title (empty
    /// before the first).
    pub(super) chapter: Option<(Locator, Option<Locator>, String)>,
    /// The chapter's last page as the page index has it: where the next
    /// chapter starts, the index's pages then, and the page (found once
    /// a chapter, not every page turned).
    pub(super) chapter_end: core::cell::Cell<Option<(Locator, u32, u32)>>,
    /// The same for the page the chapter begins on.
    pub(super) chapter_first: core::cell::Cell<Option<(Locator, u32, u32)>>,
    /// The book's own fonts, if used.
    pub(super) embedded: Option<Embedded>,
    /// The links on the page.
    pub(super) links: Vec<Link>,
    /// Where links were followed from, the last last.
    pub(super) returns: Vec<Locator>,
    /// The status lines chosen last, top then bottom.
    pub(super) status_lines: [String; 2],
    /// Where the top-level chapters begin, as thousandths of the book
    /// (see `status_context::CHAPTERS`).
    pub(super) chapter_marks: String,
    /// The page the last link led to: it offers the way back.
    pub(super) landed: Option<Locator>,
    /// Read aloud (`volume`) or turning its pages by itself (`clock`):
    /// the icon shown in the status line.
    pub(super) hands_free: Option<&'static str>,
    /// Its last page was reached ([`events::BOOK_FINISHED`] was emitted).
    pub(super) finished: bool,
    /// What plugins' decorators mark on the page, and what they were
    /// asked for: the page's start and their signature.
    pub(super) decorated: Vec<paperpaw_sdk::decorations::Mark>,
    pub(super) decorated_for: Option<(Locator, u64)>,
}

/// What the buttons choose on a page, as links are chosen: a link, or a
/// mark whose plugin hears taps on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Target {
    /// Of [`Book::links`].
    Link(usize),
    /// Of [`Book::decorated`].
    Mark(usize),
}

/// The position kept in a book's `progress.ini`.
pub(super) fn position_in(text: &str) -> Option<Locator> {
    let (mut section, mut offset) = (None, None);
    for entry in paperpaw_sdk::config::parse(text).filter_map(Result::ok) {
        match (entry.section, entry.key) {
            ("progress", "section") => section = entry.value.as_u32(),
            ("progress", "offset") => offset = entry.value.as_u32(),
            _ => {}
        }
    }
    Some(Locator::new(section?, offset?))
}

/// Remembers `path` as the book read last (reopened at start-up).
pub(crate) fn remember_last(storage: &dyn paperpaw_sdk::Storage, path: &str) {
    let state = format!("[reader]\nlast = {}\n", paperpaw_sdk::config::quote(path));
    let _ = storage.write(&state_path(), state.as_bytes());
}

/// The data folder of the book at `path`.
pub(crate) fn book_dir(path: &str) -> String {
    format!(
        "{}/books/{:016x}",
        Env::plugin_dir(PLUGIN.as_str()),
        fnv1a64(path)
    )
}

impl Book {
    /// Opens the book named by the `path` parameter; errors are messages
    /// to show instead of the book.
    pub(super) fn open(env: &Env<'_>, params: &Params) -> Result<Book, String> {
        let path = params
            .get(params::PATH)
            .ok_or_else(|| env.i18n.tr("reader.no-book").to_string())?;
        let error = |e: DocumentError| format!("{}\n{e}", env.i18n.tr("reader.error"));
        let options = open_options(env);
        let document = env
            .formats
            .open_with(env.storage, path, &options)
            .map_err(error)?;
        let font = env
            .fonts
            .role(fonts::READING.as_str())
            .ok_or_else(|| env.i18n.tr("reader.no-font").to_string())?;
        let language = document
            .metadata()
            .language
            .clone()
            .unwrap_or_else(|| env.i18n.language().to_string());
        let typography = Typography::new(env, &font, &language, document.whole_pages());
        let dir = book_dir(path);
        // Kept with the reader's data of the book before plugins shared
        // them: moved where they are now, if any are there.
        let old = format!("{dir}/annotations.txt");
        if env.storage.exists(&old) {
            let _ = env
                .storage
                .rename(&old, &paperpaw_sdk::annotations::file(path));
        }
        let marks = annotations::marks(env.storage, path);
        let title = document
            .metadata()
            .title
            .clone()
            .unwrap_or_else(|| path.to_string());

        let mut book = Book {
            path: path.to_string(),
            index: None,
            indexing: None,
            index_stopped: false,
            page: Page::default(),
            words: core::cell::OnceCell::new(),
            history: Vec::new(),
            saved_at: None,
            unsaved: false,
            dir,
            title,
            document,
            language,
            typography,
            marks,
            found: None,
            chapter: None,
            chapter_end: core::cell::Cell::new(None),
            chapter_first: core::cell::Cell::new(None),
            embedded: None,
            links: Vec::new(),
            returns: Vec::new(),
            status_lines: status::load_choice(env.storage),
            chapter_marks: String::new(),
            landed: None,
            hands_free: None,
            finished: false,
            decorated: Vec::new(),
            decorated_for: None,
        };
        book.chapter_marks = book.chapter_marks(env);
        book.embedded = Embedded::load(env, book.document.as_ref());
        // Kept with another layout maybe: from the start of its line.
        // Where the book cannot be shown there (it changed, say), from its
        // start.
        let start = book
            .saved_position(env)
            .map_or(Locator::START, |at| book.line_start(env, at));
        book.page = match book.layout(env, start) {
            Ok(page) if !page.is_past_end() => page,
            _ if start != Locator::START => book.layout(env, Locator::START).map_err(error)?,
            page => page.map_err(error)?,
        };
        book.prepare_images(env);
        book.index = Some(book.open_index(env));
        book.update_chapter(env);
        book.update_links(env);
        if !book.web() {
            remember_last(env.storage, &book.path);
        }
        Ok(book)
    }

    /// Parameters of the standard reading events; the page and the number
    /// of pages once they are counted.
    /// Where the book is read, for [`events::BOOK_OPENED`] and the others.
    pub(super) fn reading(&self, env: &Env<'_>) -> events::Reading {
        let counted = self
            .index
            .as_ref()
            .filter(|i| i.is_complete())
            .and_then(|index| match index.locate(env.storage, self.page.start) {
                Some((page, true)) => Some((page + 1, index.pages())),
                _ => None,
            });
        let chapter = self.chapter.as_ref().map(|(_, _, title)| title.clone());
        events::Reading {
            path: self.path.clone(),
            title: self.title.clone(),
            // One decimal, as written before.
            progress: (self.percent() * 10.0 + 0.5) as u32 as f32 / 10.0,
            section: self.page.start.section,
            offset: self.page.start.offset,
            page: counted.map(|(page, _)| page),
            pages: counted.map(|(_, pages)| pages),
            chapter: chapter.unwrap_or_default(),
            chapter_left: counted.and_then(|(page, _)| self.chapter_left(env, page - 1)),
        }
    }

    /// The pages after page `page` (from 0) up to the chapter's end, where
    /// the chapter is known and the pages are counted.
    fn chapter_left(&self, env: &Env<'_>, page: u32) -> Option<u32> {
        let index = self.index.as_ref().filter(|i| i.is_complete())?;
        let (_, next, title) = self.chapter.as_ref()?;
        if title.is_empty() {
            return None;
        }
        let last = match *next {
            // The last chapter ends with the book.
            None => index.pages().checked_sub(1)?,
            Some(next) => match self.chapter_end.get() {
                Some((at, pages, last)) if at == next && pages == index.pages() => last,
                _ => {
                    // The page the next chapter starts on, or the one
                    // before if it starts with its page.
                    let last = match index.locate(env.storage, next)? {
                        (page, true) => page.checked_sub(1)?,
                        (page, false) => page,
                    };
                    self.chapter_end.set(Some((next, index.pages(), last)));
                    last
                }
            },
        };
        Some(last.saturating_sub(page))
    }

    /// The page (from 0) the chapter begins on, where the chapter is
    /// known and the pages are counted.
    pub(super) fn chapter_first(&self, env: &Env<'_>) -> Option<u32> {
        let index = self.index.as_ref().filter(|i| i.is_complete())?;
        let (start, _, title) = self.chapter.as_ref()?;
        if title.is_empty() {
            return None;
        }
        if let Some((at, pages, first)) = self.chapter_first.get()
            && at == *start
            && pages == index.pages()
        {
            return Some(first);
        }
        let (first, _) = index.locate(env.storage, *start)?;
        self.chapter_first.set(Some((*start, index.pages(), first)));
        Some(first)
    }

    /// The page index of this layout of the book as it is now: its name
    /// tells layouts and editions of the book (by size) apart. A new one
    /// removes those of other layouts once more than a few are kept;
    /// switching back finds the others.
    pub(super) fn open_index(&self, env: &Env<'_>) -> PageIndex {
        let storage = env.storage;
        let size = storage.size(&self.path).unwrap_or(0);
        let name = format!(
            "pages-{:016x}.bin",
            fnv1a64(format!("{}|{size}", self.typography.key))
        );
        let path = format!("{}/{name}", self.dir);
        if !storage.exists(&path) {
            let mut others = Vec::new();
            let _ = storage.list(&self.dir, &mut |entry| {
                if entry.name.starts_with("pages-") && !entry.is_dir {
                    others.push(format!("{}/{}", self.dir, entry.name));
                }
            });
            if others.len() >= INDEXES_KEPT {
                for other in others {
                    let _ = storage.remove(&other);
                }
            }
        }
        PageIndex::open(storage, path)
    }

    /// Applies changed reading settings: new typography, the current
    /// position laid out again, and a page index for the new layout.
    pub(super) fn relayout(&mut self, env: &Env<'_>) {
        let Some(font) = self.font(env) else {
            return;
        };
        self.typography = Typography::new(env, &font, &self.language, self.document.whole_pages());
        self.words = core::cell::OnceCell::new();
        let start = self.line_start(env, self.page.start);
        if let Ok(page) = self.layout(env, start) {
            self.page = page;
            self.prepare_images(env);
        }
        self.update_chapter(env);
        self.update_links(env);
        self.history.clear();
        self.index = Some(self.open_index(env));
        self.chapter_end.set(None);
        self.chapter_first.set(None);
        self.indexing = None;
        self.index_stopped = false;
    }

    /// Decodes the pictures of the page shown that are not yet: books
    /// decode them when their page shows, not while pages are laid out
    /// in the background.
    pub(super) fn prepare_images(&mut self, env: &Env<'_>) {
        let document = &self.document;
        for shown in &mut self.page.images {
            if let Some(image) = document.prepare_image(env.storage, &shown.image) {
                shown.image = image;
            }
        }
    }

    /// Whether the page shown has pictures: shown in grays where the
    /// panel can ([`Screen::grays`](paperpaw_sdk::Screen::grays)).
    pub(super) fn pictured(&self) -> bool {
        !self.page.images.is_empty()
    }

    pub(super) fn font<'a>(&self, env: &Env<'a>) -> Option<FontHandle<'a>> {
        env.fonts.role(fonts::READING.as_str())
    }

    /// The start of the line `at` is in, as this book is laid out now
    /// (see `layout::line_start`).
    pub(super) fn line_start(&self, env: &Env<'_>, at: Locator) -> Locator {
        let t = &self.typography;
        let Some(faces) = Faces::new(env, t, self.embedded.as_ref()) else {
            return at;
        };
        let patterns = t
            .hyphenation
            .as_deref()
            .and_then(|language| env.hyphenation.get(language));
        let hyphenate = |word: &str, out: &mut dyn FnMut(usize)| {
            if let Some(patterns) = patterns {
                patterns.hyphenate(word, out);
            }
        };
        let filtered = Filtered {
            document: self.document.as_ref(),
            env,
        };
        let cx = LayoutContext {
            document: &filtered,
            storage: env.storage,
            width: t.width,
            style: &t.style,
            typesetter: &faces,
            hyphenate: patterns.is_some().then_some(&hyphenate as Hyphenate<'_>),
        };
        line_start(&cx, at).unwrap_or(at)
    }

    pub(super) fn layout(&self, env: &Env<'_>, at: Locator) -> Result<Page, DocumentError> {
        let t = &self.typography;
        let Some(faces) = Faces::new(env, t, self.embedded.as_ref()) else {
            return Ok(Page::default());
        };
        let patterns = t
            .hyphenation
            .as_deref()
            .and_then(|language| env.hyphenation.get(language));
        let hyphenate = |word: &str, out: &mut dyn FnMut(usize)| {
            if let Some(patterns) = patterns {
                patterns.hyphenate(word, out);
            }
        };
        let filtered = Filtered {
            document: self.document.as_ref(),
            env,
        };
        let cx = LayoutContext {
            document: &filtered,
            storage: env.storage,
            width: t.width,
            style: &t.style,
            typesetter: &faces,
            hyphenate: patterns.is_some().then_some(&hyphenate as Hyphenate<'_>),
        };
        layout_page(&cx, at, t.height)
    }

    pub(super) fn progress_path(&self) -> String {
        format!("{}/progress.ini", self.dir)
    }

    pub(super) fn saved_position(&self, env: &Env<'_>) -> Option<Locator> {
        let data = paperpaw_sdk::read_file(env.storage, &self.progress_path(), 4096).ok()?;
        position_in(core::str::from_utf8(&data).ok()?)
    }

    pub(super) fn percent(&self) -> f32 {
        self.document.progress(self.page.start) * 100.0
    }

    /// Writes the position now, or soon if it was written a moment ago;
    /// `true` while it waits ([`Book::save_due`]).
    pub(super) fn position_changed(&mut self, env: &Env<'_>) -> bool {
        let recent = self
            .saved_at
            .is_some_and(|at| env.now.since(at) < SAVE_EVERY_MS);
        self.unsaved = recent;
        if !recent {
            self.save_position(env);
        }
        recent
    }

    /// Writes the position if it waits; `force` also before its time.
    pub(super) fn save_due(&mut self, env: &Env<'_>, force: bool) {
        let due = self
            .saved_at
            .is_none_or(|at| env.now.since(at) >= SAVE_EVERY_MS);
        if self.unsaved && (force || due) {
            self.unsaved = false;
            self.save_position(env);
        }
    }

    pub(super) fn save_position(&mut self, env: &Env<'_>) {
        self.saved_at = Some(env.now);
        let start = self.page.start;
        let text = format!(
            "[progress]\nsection = {}\noffset = {}\npercent = {:.1}\n",
            start.section,
            start.offset,
            self.percent()
        );
        let _ = env.storage.write(&self.progress_path(), text.as_bytes());
    }

    /// The start of the previous page: the page shown before (this
    /// session), else from the page index, else about a page back.
    pub(super) fn previous_start(&mut self, env: &Env<'_>) -> Option<Locator> {
        if let Some(start) = self.history.pop() {
            return Some(start);
        }
        let here = self.page.start;
        if let Some(index) = &self.index {
            match index.locate(env.storage, here) {
                Some((0, true)) => return None,
                Some((page, true)) => return index.entry(env.storage, page - 1),
                // Inside a page whose end the index knows: its start.
                Some((page, false)) if index.is_complete() || page + 1 < index.pages() => {
                    return index.entry(env.storage, page);
                }
                // Past the pages counted so far.
                _ => {}
            }
        }
        self.page_back(env, here)
    }

    /// About a page before `here`, where the page index does not reach
    /// yet: as much text back as the page shown holds, from the start of
    /// its line. The page there may show a line again, but leaves nothing
    /// out.
    fn page_back(&self, env: &Env<'_>, here: Locator) -> Option<Locator> {
        let document = &self.document;
        let page = &self.page;
        let end = match page.end.section == page.start.section {
            true => page.end.offset,
            false => document.section_len(page.start.section),
        };
        let length = end.saturating_sub(page.start.offset).max(MIN_PAGE_BACK);
        // At the start of its section, the end of the one before.
        let first = document
            .block_at(env.storage, Locator::new(here.section, 0))
            .ok()
            .flatten()
            .map_or(0, |block| block.start);
        let (section, end) = if here.offset > first {
            (here.section, here.offset)
        } else {
            let section = (0..here.section)
                .rev()
                .find(|&s| document.section_len(s) > 0)?;
            (section, document.section_len(section))
        };
        let from = Locator::new(section, end.saturating_sub(length));
        Some(self.line_start(env, from))
    }

    pub(super) fn show(&mut self, cx: &mut Cx<'_, '_>, start: Locator, forward: bool) -> bool {
        let env = *cx.env();
        let Ok(page) = self.layout(&env, start) else {
            return false;
        };
        if page.is_past_end() || page.start == self.page.start {
            return false;
        }
        self.found = None;
        // The pages turned one after another, to turn back the same way;
        // after a jump the page before is the index's, not the page left.
        if forward && page.start == self.page.end {
            if self.history.len() == HISTORY {
                self.history.remove(0);
            }
            self.history.push(self.page.start);
        } else if page.end != self.page.start {
            self.history.clear();
        }
        self.page = page;
        self.words = core::cell::OnceCell::new();
        self.prepare_images(&env);
        self.update_chapter(&env);
        // Found now, so the status lines read nothing while drawn.
        self.chapter_first(&env);
        self.update_links(&env);
        if self.position_changed(&env) {
            cx.wake_at(env.now.add_millis(SAVE_EVERY_MS));
        }
        if !self.web() {
            cx.command(events::PAGE_TURNED.emit(self.reading(&env)));
            // The last page: the book is read to the end.
            if !self.finished && self.document.progress(self.page.end) >= 0.999 {
                self.finished = true;
                cx.command(events::BOOK_FINISHED.emit(self.reading(&env)));
            }
        }
        cx.request_redraw();
        true
    }

    /// One step of background pagination. Returns whether more work
    /// remains. A page that cannot be laid out or a storage that takes no
    /// more end it for this layout; the pages counted so far are used.
    pub(super) fn index_step(&mut self, env: &Env<'_>) -> bool {
        if self.index_stopped || self.index.as_ref().is_none_or(PageIndex::is_complete) {
            return false;
        }
        let more = self.index_pages(env);
        self.index_stopped = more.is_none();
        more == Some(true)
    }

    /// Lays out the next pages and adds them to the index: whether more
    /// remain, `None` if it cannot go on.
    fn index_pages(&mut self, env: &Env<'_>) -> Option<bool> {
        let index = self.index.as_ref()?;
        let mut next = match self.indexing {
            Some(next) => next,
            None => match index.last(env.storage) {
                Some(last) => self.layout(env, last).ok()?.end,
                None => Locator::START,
            },
        };
        let per_step = match env.device.memory {
            paperpaw_sdk::MemoryClass::Small => PAGES_PER_STEP_SMALL,
            _ => PAGES_PER_STEP,
        };
        let mut starts = Vec::with_capacity(per_step);
        let mut done = false;
        for _ in 0..per_step {
            // Unreadable: reading still works.
            let page = self.layout(env, next).ok()?;
            if page.is_past_end() {
                done = true;
                break;
            }
            starts.push(page.start);
            if page.end <= page.start {
                done = true;
                break;
            }
            next = page.end;
        }
        let index = self.index.as_mut()?;
        // A storage that is read-only or full.
        if !index.append(env.storage, &starts) {
            return None;
        }
        if done {
            index.finish(env.storage);
        }
        self.indexing = Some(next);
        Some(!done)
    }

    /// "12 / 340" once the index covers the page exactly, else a percentage.
    pub(super) fn position_label(&self, env: &Env<'_>) -> String {
        if let Some(index) = &self.index
            && index.is_complete()
            && let Some((page, true)) = index.locate(env.storage, self.page.start)
        {
            return env.i18n.format(
                "reader.page",
                &[
                    ("page", Arg::Int(i64::from(page) + 1)),
                    ("pages", Arg::Int(i64::from(index.pages()))),
                ],
            );
        }
        env.i18n.format(
            "reader.percent",
            &[("percent", Arg::Int(self.percent() as i64))],
        )
    }

    /// Whether this is no book but a document a plugin keeps for itself,
    /// e.g. a web page: it is not reported as read (see
    /// [`events::BOOK_OPENED`]) nor opened again at start-up.
    pub(super) fn web(&self) -> bool {
        paperpaw_sdk::files::is_hidden(&self.path)
    }

    /// Reads the links of the page.
    pub(super) fn update_links(&mut self, env: &Env<'_>) {
        let (start, end) = (self.page.start, self.page.end);
        let to = if end.section == start.section {
            end.offset
        } else {
            u32::MAX
        };
        self.links = self
            .document
            .links(env.storage, start.section, start.offset, to);
    }

    /// What the buttons choose on the page, in reading order: the links
    /// that can be followed (to the web only if a browser is there) and
    /// the marks whose plugins hear taps on them.
    pub(super) fn targets(&self, env: &Env<'_>) -> Vec<Target> {
        let web = env.has_slot(&slots::BROWSER);
        let section = self.page.start.section;
        let mut order: Vec<(Locator, Target)> = (0..self.links.len())
            .filter(|&i| web || !paperpaw_sdk::document::is_external(&self.links[i].href))
            .map(|i| (Locator::new(section, self.links[i].start), Target::Link(i)))
            .collect();
        // Marks whose plugins hear taps on them, chosen as links are.
        order.extend(
            self.decorated
                .iter()
                .enumerate()
                .filter(|(_, m)| m.taps)
                .map(|(i, m)| (m.start, Target::Mark(i))),
        );
        order.sort_by_key(|(at, _)| *at);
        order.into_iter().map(|(_, target)| target).collect()
    }

    /// Where target `target` is on the page.
    pub(super) fn target_range(&self, target: Target) -> Option<(Locator, Locator)> {
        let section = self.page.start.section;
        match target {
            Target::Link(i) => self
                .links
                .get(i)
                .map(|l| (Locator::new(section, l.start), Locator::new(section, l.end))),
            Target::Mark(i) => self.decorated.get(i).map(|m| (m.start, m.end)),
        }
    }

    /// The text at the target of link `href`, as a note shows it: the
    /// block there and the ones after it, up to a heading or a few hundred
    /// characters.
    pub(super) fn note_text(&self, env: &Env<'_>, href: &str) -> Option<String> {
        let mut at = self.document.link_target(env.storage, href)?;
        let mut text = String::new();
        // As the page shows it.
        let filtered = Filtered {
            document: self.document.as_ref(),
            env,
        };
        for _ in 0..4 {
            let Ok(Some(block)) = filtered.block_at(env.storage, at) else {
                break;
            };
            if matches!(block.kind, paperpaw_sdk::document::BlockKind::Heading(_))
                && !text.is_empty()
            {
                break;
            }
            let part = block.text.trim();
            if !part.is_empty() {
                if !text.is_empty() {
                    text.push(' ');
                }
                text.push_str(part);
            }
            // A note that is only its number goes on in the next block.
            if text.chars().count() > 300 || text.chars().count() > 12 && block.end > block.start {
                break;
            }
            at = Locator::new(at.section, block.end);
        }
        let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
        (!text.is_empty()).then_some(text)
    }

    /// The link `word` is part of, if any.
    pub(super) fn link_of(&self, word: &WordBox) -> Option<&Link> {
        if word.start.section != self.page.start.section {
            return None;
        }
        self.links
            .iter()
            .find(|l| word.end.offset > l.start && word.start.offset < l.end)
    }

    /// Shows the page that starts at `start`, far from here (a link, the
    /// contents, a search hit, a page number): *Back* returns here.
    pub(super) fn jump(&mut self, cx: &mut Cx<'_, '_>, start: Locator) -> bool {
        let from = self.page.start;
        // A search hit or a position from elsewhere: its whole line.
        let start = self.line_start(cx.env(), start);
        if !self.show(cx, start, true) {
            return false;
        }
        if self.returns.len() == RETURNS {
            self.returns.remove(0);
        }
        self.returns.push(from);
        self.landed = Some(self.page.start);
        true
    }

    /// Back to where the last jump was made from.
    pub(super) fn go_back(&mut self, cx: &mut Cx<'_, '_>) {
        if let Some(from) = self.returns.pop() {
            self.landed = None;
            self.show(cx, from, false);
        }
    }

    /// Whether the page has a bookmark.
    pub(super) fn bookmarked(&self) -> bool {
        let (start, end) = (self.page.start, self.page.end);
        self.marks
            .iter()
            .any(|m| m.kind == Kind::Bookmark && m.start >= start && m.start < end)
    }

    /// Where the bookmark ribbon is: in the top margin, on the right.
    /// The start of the page that contains `at`, as the page index has it
    /// (so pages look the same as when paging there); `at` itself where
    /// the index does not reach yet.
    /// Where the book is, for the skimming overlay: the page, the chapter
    /// and how far, in thousandths.
    pub(super) fn skim_params(&self, env: &Env<'_>) -> Params {
        let chapter = self
            .chapter
            .as_ref()
            .map(|(_, _, title)| title.clone())
            .unwrap_or_default();
        Params::new()
            .with(params::PATH, self.path.clone())
            .with(params::PAGE, self.position_label(env))
            .with(params::TEXT, chapter)
            .with(
                crate::skim::AT,
                format!("{}", (self.percent() * 10.0) as u32),
            )
    }

    /// Where `to` leads: `page:±n` pages on (or as many tenths of a
    /// percent while pages are counted), `permille:n` that far into the
    /// book, `chapter:±1` the chapter before or after.
    pub(super) fn skim_target(&self, env: &Env<'_>, to: &str) -> Option<Locator> {
        let (kind, value) = to.split_once(':')?;
        let value: i64 = value.trim_start_matches('+').parse().ok()?;
        let pages = self
            .index
            .as_ref()
            .filter(|i| i.is_complete() && i.pages() > 0);
        match kind {
            "page" => {
                if let Some(index) = pages
                    && let Some((page, _)) = index.locate(env.storage, self.page.start)
                {
                    let target = (i64::from(page) + value).clamp(0, i64::from(index.pages()) - 1);
                    return index.entry(env.storage, target as u32);
                }
                let at = (self.percent() as f64 * 10.0 + value as f64).clamp(0.0, 999.0);
                Some(self.locator_at(at / 1000.0))
            }
            "permille" => {
                let fraction = value.clamp(0, 999) as f64 / 1000.0;
                if let Some(index) = pages {
                    let page = (fraction * f64::from(index.pages())) as u32;
                    return index.entry(env.storage, page.min(index.pages() - 1));
                }
                Some(self.locator_at(fraction))
            }
            "chapter" => {
                // Where the entries begin, exactly (their anchors): the
                // chapters the status lines name, also those in parts.
                let storage = env.storage;
                let mut starts: Vec<Locator> = (0..self.document.toc_len(storage).min(2000))
                    .filter_map(|i| self.document.toc_target(storage, i))
                    .collect();
                starts.sort();
                starts.dedup();
                let here = self.page.start;
                if value > 0 {
                    // On: the first chapter that begins on a later page.
                    return starts.into_iter().find(|&s| s >= self.page.end);
                }
                // Back: to the start of this chapter, or the one before.
                let current = starts.iter().rposition(|&s| s <= here)?;
                match starts[current] < here {
                    true => Some(starts[current]),
                    false => current.checked_sub(1).map(|i| starts[i]),
                }
            }
            _ => None,
        }
    }

    /// The position `fraction` of the way through the book, by the
    /// sections' lengths.
    pub(super) fn locator_at(&self, fraction: f64) -> Locator {
        let sections = self.document.sections();
        let total: u64 = (0..sections)
            .map(|s| u64::from(self.document.section_len(s)))
            .sum();
        let mut left = (fraction.clamp(0.0, 1.0) * total as f64) as u64;
        for section in 0..sections {
            let len = u64::from(self.document.section_len(section));
            if left < len {
                return Locator::new(section, left as u32);
            }
            left -= len;
        }
        Locator::new(sections.saturating_sub(1), 0)
    }

    pub(super) fn page_start(&self, env: &Env<'_>, at: Locator) -> Locator {
        let Some(index) = &self.index else {
            return at;
        };
        match index.locate(env.storage, at) {
            Some((page, _)) if index.is_complete() || page + 1 < index.pages() => {
                index.entry(env.storage, page).unwrap_or(at)
            }
            _ => at,
        }
    }

    /// Finds the chapter of the page, before the page is shown and said
    /// ([`events::PAGE_TURNED`]); the table of contents is read only when
    /// the page leaves the chapter.
    pub(super) fn update_chapter(&mut self, env: &Env<'_>) {
        let at = self.page.start;
        if let Some((from, to, _)) = &self.chapter
            && *from <= at
            && to.is_none_or(|to| at < to)
        {
            return;
        }
        self.chapter = None;
        let (document, storage) = (&self.document, env.storage);
        let mut chapter = (Locator::START, None, String::new());
        for index in 0..document.toc_len(storage) {
            let Some(entry) = document.toc_entry(storage, index) else {
                continue;
            };
            // Entries in the page's section need their exact position.
            let start = if entry.at.section == at.section {
                document.toc_target(storage, index).unwrap_or(entry.at)
            } else {
                entry.at
            };
            if start <= at {
                chapter = (start, None, entry.title);
            } else {
                chapter.1 = Some(start);
                break;
            }
        }
        self.chapter = Some(chapter);
    }
}

/// How the reader opens books: with the publisher's styles as set, and
/// the plugins' stylesheets. Its search and contents open them so too,
/// or they would convert the book again (and find other places).
pub(crate) fn open_options(env: &Env<'_>) -> paperpaw_sdk::OpenOptions {
    let styles = env
        .settings
        .bool(crate::settings::PUBLISHER_STYLES.as_str());
    env.formats
        .options()
        .publisher_styles(styles)
        .css(env.stylesheets.join("\n"))
}

/// Whether the text filters do something else now than when the pages
/// were laid out.
pub(super) fn env_filters_changed(env: &Env<'_>, book: &Book) -> bool {
    book.typography.filters != env.text_filters.signature(env)
}
