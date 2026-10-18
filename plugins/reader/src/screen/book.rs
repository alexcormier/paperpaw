//! The book being read: opening it, laying out its pages, where the
//! reader is (kept in `progress.ini`), the page index, chapters and
//! links.

use super::*;

/// The book as the layout sees it: every block through the text
/// filters of plugins (see `quireink_sdk::filters`).
pub(super) struct Filtered<'d, 'e> {
    pub(super) document: &'d dyn Document,
    pub(super) env: &'d Env<'e>,
}

impl Document for Filtered<'_, '_> {
    fn metadata(&self) -> &quireink_sdk::document::Metadata {
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
        storage: &dyn quireink_sdk::Storage,
        at: Locator,
    ) -> Result<Option<quireink_sdk::document::Block>, DocumentError> {
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
    pub(super) history: Vec<Locator>,
    /// When the position was written last, and whether the page shown has
    /// changed since.
    pub(super) saved_at: Option<quireink_sdk::Instant>,
    pub(super) unsaved: bool,
    pub(super) index: Option<PageIndex>,
    /// Where background pagination continues.
    pub(super) indexing: Option<Locator>,
    /// Where the bookmarks and highlights are.
    pub(super) marks: Vec<Mark>,
    /// Text to point out on this page, e.g. a search hit.
    pub(super) found: Option<(Locator, Locator)>,
    /// The chapter of the page, for status lines: where it starts, where
    /// the next one starts and its title (empty before the first).
    pub(super) chapter: Option<(Locator, Option<Locator>, String)>,
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
}

/// The position kept in a book's `progress.ini`.
pub(super) fn position_in(text: &str) -> Option<Locator> {
    let (mut section, mut offset) = (None, None);
    for entry in quireink_sdk::config::parse(text).filter_map(Result::ok) {
        match (entry.section, entry.key) {
            ("progress", "section") => section = entry.value.as_u32(),
            ("progress", "offset") => offset = entry.value.as_u32(),
            _ => {}
        }
    }
    Some(Locator::new(section?, offset?))
}

/// Remembers `path` as the book read last (reopened at start-up).
pub(crate) fn remember_last(storage: &dyn quireink_sdk::Storage, path: &str) {
    let state = format!("[reader]\nlast = {}\n", quireink_sdk::config::quote(path));
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
        // them.
        let _ = env.storage.rename(
            &format!("{dir}/annotations.txt"),
            &quireink_sdk::annotations::file(path),
        );
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
            page: Page::default(),
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
            embedded: None,
            links: Vec::new(),
            returns: Vec::new(),
            status_lines: status::load_choice(env.storage),
            chapter_marks: String::new(),
            landed: None,
            hands_free: None,
            finished: false,
        };
        book.chapter_marks = book.chapter_marks(env);
        book.embedded = Embedded::load(env, book.document.as_ref());
        // Kept with another layout maybe: from the start of its line.
        let start = book
            .saved_position(env)
            .map_or(Locator::START, |at| book.line_start(env, at));
        book.page = match book.layout(env, start).map_err(error)? {
            page if page.is_past_end() && start != Locator::START => {
                book.layout(env, Locator::START).map_err(error)?
            }
            page => page,
        };
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
        events::Reading {
            path: self.path.clone(),
            title: self.title.clone(),
            // One decimal, as written before.
            progress: (self.percent() * 10.0 + 0.5) as u32 as f32 / 10.0,
            section: self.page.start.section,
            offset: self.page.start.offset,
            page: counted.map(|(page, _)| page),
            pages: counted.map(|(_, pages)| pages),
        }
    }

    pub(super) fn open_index(&self, env: &Env<'_>) -> PageIndex {
        PageIndex::open(
            env.storage,
            format!("{}/pages-{}.bin", self.dir, self.typography.key),
        )
    }

    /// Applies changed reading settings: new typography, the current
    /// position laid out again, and a page index for the new layout.
    pub(super) fn relayout(&mut self, env: &Env<'_>) {
        let Some(font) = self.font(env) else {
            return;
        };
        self.typography = Typography::new(env, &font, &self.language, self.document.whole_pages());
        let start = self.line_start(env, self.page.start);
        if let Ok(page) = self.layout(env, start) {
            self.page = page;
        }
        self.update_links(env);
        self.history.clear();
        self.index = Some(self.open_index(env));
        self.indexing = None;
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
        let data = quireink_sdk::read_file(env.storage, &self.progress_path(), 4096).ok()?;
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
    /// session), else from the page index.
    pub(super) fn previous_start(&mut self, env: &Env<'_>) -> Option<Locator> {
        if let Some(start) = self.history.pop() {
            return Some(start);
        }
        let index = self.index.as_ref()?;
        match index.locate(env.storage, self.page.start)? {
            (page, true) if page > 0 => index.entry(env.storage, page - 1),
            (page, false) => index.entry(env.storage, page),
            _ => None,
        }
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
        if forward {
            if self.history.len() == HISTORY {
                self.history.remove(0);
            }
            self.history.push(self.page.start);
        }
        self.page = page;
        self.update_chapter(&env);
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

    /// One step of background pagination. Returns whether more work remains.
    pub(super) fn index_step(&mut self, env: &Env<'_>) -> bool {
        let Some(mut index) = self.index.take() else {
            return false;
        };
        if index.is_complete() {
            self.index = Some(index);
            return false;
        }
        let mut next = match self.indexing {
            Some(next) => next,
            None => match index.last(env.storage) {
                Some(last) => match self.layout(env, last) {
                    Ok(page) => page.end,
                    Err(_) => return false,
                },
                None => Locator::START,
            },
        };
        let per_step = match env.device.memory {
            quireink_sdk::MemoryClass::Small => PAGES_PER_STEP_SMALL,
            _ => PAGES_PER_STEP,
        };
        let mut starts = Vec::with_capacity(per_step);
        let mut done = false;
        for _ in 0..per_step {
            let Ok(page) = self.layout(env, next) else {
                // Unreadable: give up indexing, reading still works.
                return false;
            };
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
        if !index.append(env.storage, &starts) {
            // Read-only storage: no index.
            return false;
        }
        if done {
            index.finish(env.storage);
        }
        self.indexing = Some(next);
        self.index = Some(index);
        !done
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
        quireink_sdk::files::is_hidden(&self.path)
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

    /// The page's links that can be followed (to the web only if a
    /// browser is there), in reading order: indexes into `links`.
    pub(super) fn followable(&self, env: &Env<'_>) -> Vec<usize> {
        let web = env.has_slot(&slots::BROWSER);
        let mut order: Vec<usize> = (0..self.links.len())
            .filter(|&i| web || !quireink_sdk::document::is_external(&self.links[i].href))
            .collect();
        order.sort_by_key(|&i| self.links[i].start);
        order
    }

    /// The text at the target of link `href`, as a note shows it: the
    /// block there and the ones after it, up to a heading or a few hundred
    /// characters.
    pub(super) fn note_text(&self, env: &Env<'_>, href: &str) -> Option<String> {
        let mut at = self.document.link_target(env.storage, href)?;
        let mut text = String::new();
        for _ in 0..4 {
            let Ok(Some(block)) = self.document.block_at(env.storage, at) else {
                break;
            };
            if matches!(block.kind, quireink_sdk::document::BlockKind::Heading(_))
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
                // Where the top-level entries begin, exactly (their anchors).
                let storage = env.storage;
                let mut starts: Vec<Locator> = (0..self.document.toc_len(storage).min(2000))
                    .filter(|&i| {
                        self.document
                            .toc_entry(storage, i)
                            .is_some_and(|e| e.level == 0)
                    })
                    .filter_map(|i| self.document.toc_target(storage, i))
                    .collect();
                starts.sort();
                starts.dedup();
                let here = self.page.start;
                let current = starts.iter().rposition(|&s| s <= here);
                let target = match (value < 0, current) {
                    // Back: to the start of this chapter, or the one before.
                    (true, Some(i)) if starts[i] < here => Some(i),
                    (true, Some(i)) => i.checked_sub(1),
                    (true, None) => None,
                    (false, Some(i)) => Some(i + 1),
                    (false, None) => Some(0),
                };
                target.and_then(|i| starts.get(i).copied())
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

    /// Finds the chapter of the page when a status line shows it; the
    /// table of contents is read only when the page leaves the chapter.
    pub(super) fn update_chapter(&mut self, env: &Env<'_>) {
        let at = self.page.start;
        if let Some((from, to, _)) = &self.chapter
            && *from <= at
            && to.is_none_or(|to| at < to)
        {
            return;
        }
        self.chapter = None;
        if !status::uses(env, &status::items::CHAPTER) {
            return;
        }
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
/// the plugins' stylesheets.
pub(super) fn open_options(env: &Env<'_>) -> quireink_sdk::OpenOptions {
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
