//! The reader's screen: a book shown page by page, and what the reader
//! does on it.

mod book;
mod draw;
mod hands_free;
mod linking;
mod selecting;
mod typography;
mod words;

use alloc::boxed::Box;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use quireink_digest::fnv1a64;
use quireink_sdk::ask;
use quireink_sdk::builtin::{actions, events, fonts, params, slots};
use quireink_sdk::document::{Document, DocumentError, Link, Locator};
use quireink_sdk::event::EventData;
use quireink_sdk::layout::{
    Face, Hyphenate, LayoutContext, Page, PageImage, Style, Typesetter, layout_page, line_start,
};
use quireink_sdk::status::context as status_context;
use quireink_sdk::ui::{self, UiStyle};
use quireink_sdk::{
    Arg, Canvas, Color, Command, Cx, Env, FontHandle, FontSet, Gesture, GestureKind, Handled, Id,
    Params, Point, Rect, RefreshMode, Screen,
};

use crate::annotations::{self, Annotation, Kind, Mark};
use crate::index::PageIndex;
use crate::selection::{self, Item, Mode, Selection};
use crate::settings::{
    FONT_SIZE, FONT_WEIGHT, HYPHENATE, INDENT, JUSTIFY, LINE_SPACING, MARGIN, WORD_SPACING,
};
use crate::status;
use crate::{PLUGIN, state_path};

use self::book::*;
pub(crate) use self::book::{book_dir, remember_last};
use self::hands_free::*;
use self::linking::*;
use self::typography::*;
pub(crate) use self::words::WordBox;
use self::words::*;

/// Pages laid out per background step; small enough to keep input snappy.
const PAGES_PER_STEP: usize = 8;

/// On small devices, where laying out a page takes half a second (the X4),
/// one: buttons wait no longer than that.
const PAGES_PER_STEP_SMALL: usize = 1;

/// Pause between background steps.
const STEP_DELAY_MS: u64 = 2;

/// The position is written at most this often while pages turn; always
/// when the book is closed or the device sleeps. A card is slow to write
/// to, and turning pages quickly should not wait for it.
const SAVE_EVERY_MS: u64 = 5000;

/// Pages remembered for going back before the index covers them.
const HISTORY: usize = 64;

/// Places jumps were made from (links, contents, search, page numbers),
/// kept for *Back*.
const RETURNS: usize = 32;

/// Shows a book page by page.
pub(crate) struct ReaderScreen {
    book: Result<Book, String>,
    /// Text chosen on the page (see `selection`).
    selection: Option<Selection>,
    /// A link being chosen with the buttons.
    linking: Option<Linking>,
    /// The last search, offered again.
    last_search: String,
    /// What the status lines showed when last drawn.
    status_shown: String,
    /// The sentence being read aloud.
    aloud: Option<Aloud>,
    /// Where reading aloud was paused: the page and its word.
    resume: Option<(Locator, usize)>,
    /// When the page turns by itself next, while it does.
    auto_turn: Option<quireink_sdk::Instant>,
    /// Where skimming began, while skimming.
    skim_from: Option<Locator>,
    /// The book to open once the screen says it is being prepared (the
    /// first opening takes a while).
    preparing: Option<Params>,
}

impl ReaderScreen {
    pub(crate) fn open(env: &Env<'_>, params: &Params) -> Self {
        let prepared = params
            .get(params::PATH)
            .is_none_or(|path| env.formats.prepared(env.storage, path, &open_options(env)));
        let (book, preparing) = if prepared {
            (Book::open(env, params), None)
        } else {
            let message = env.i18n.tr("reader.preparing").to_string();
            (Err(message), Some(params.clone()))
        };
        Self {
            book,
            preparing,
            selection: None,
            linking: None,
            last_search: String::new(),
            status_shown: String::new(),
            aloud: None,
            resume: None,
            auto_turn: None,
            skim_from: None,
        }
    }

    /// Opens the book the screen says is being prepared, if it does;
    /// whether it did.
    fn prepare(&mut self, cx: &mut Cx<'_, '_>) -> bool {
        let Some(params) = self.preparing.take() else {
            return false;
        };
        let env = *cx.env();
        self.book = Book::open(&env, &params);
        // Shown as a screen opening is: the book replaces the notice.
        cx.request_redraw();
        let flash = env
            .settings
            .bool(quireink_sdk::builtin::settings::FLASH_SCREENS.as_str());
        cx.request_refresh(if flash {
            RefreshMode::Full
        } else {
            RefreshMode::Partial
        });
        self.on_show(cx);
        true
    }

    /// Opens the skimming overlay where the book is.
    fn open_skim(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let Ok(book) = &self.book else {
            return;
        };
        self.skim_from.get_or_insert(book.page.start);
        cx.command(Command::OpenScreen(
            crate::SKIM_SCREEN,
            book.skim_params(&env)
                .with(crate::skim::MARKS, book.chapter_marks.clone()),
        ));
    }

    /// Moves as the skimming overlay asks, and tells it where the book is.
    fn skim(&mut self, to: &str, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let from = self.skim_from;
        let Ok(book) = &mut self.book else {
            return;
        };
        let target = if to == "back" {
            from
        } else {
            book.skim_target(&env, to)
        };
        if let Some(target) = target {
            let start = book.page_start(&env, target);
            book.show(cx, start, true);
        }
        cx.command(Command::Emit(crate::SKIMMED, book.skim_params(&env)));
    }

    /// Buttons while text is selected.
    /// Shows the next status line at the top (or bottom) and keeps the
    /// choice.
    fn next_status_line(&mut self, top: bool, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let Ok(book) = &mut self.book else {
            return;
        };
        let slot = usize::from(!top);
        let next = status::next(&env, top, &book.status_lines[slot]).to_string();
        if next == status::current(&env, top, &book.status_lines[slot]) {
            return;
        }
        book.status_lines[slot] = next;
        status::save_choice(env.storage, &book.status_lines);
        for (area, at_top) in book.status_areas() {
            if at_top == top {
                cx.request_redraw_area(area);
            }
        }
        cx.request_refresh(RefreshMode::Fast);
        self.status_shown.clear();
    }

    /// Redraws the status lines if what they show changed, and checks again
    /// when the minute changes.
    fn update_status(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let Ok(book) = &self.book else {
            return;
        };
        if book.status_areas().next().is_none() {
            return;
        }
        let signature = status::signature(&env, &book.status_context(&env), &book.status_lines);
        if signature != self.status_shown {
            self.status_shown = signature;
            for (area, _) in book.status_areas() {
                cx.request_redraw_area(area);
            }
            cx.request_refresh(RefreshMode::Fast);
        }
        let next = quireink_sdk::time::next_minute(&env).unwrap_or(env.now.add_millis(60_000));
        cx.wake_at(next);
        if let Some(at) = self.auto_turn {
            cx.wake_at(at);
        }
    }

    /// Asks on the keyboard what to search for, or which page to go to.
    fn ask(&mut self, action: &Id, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let Ok(book) = &self.book else {
            return;
        };
        let question = if *action == crate::actions::SEARCH {
            ask::text(env.i18n.tr("reader.search.prompt"), &crate::SEARCH_ENTERED)
                .initial(self.last_search.clone())
        } else {
            let (pages, complete) = book
                .index
                .as_ref()
                .map_or((0, false), |i| (i.pages(), i.is_complete()));
            let key = if complete {
                "reader.go-to-page.prompt"
            } else {
                "reader.go-to-page.counting"
            };
            let title = env
                .i18n
                .format(key, &[("pages", Arg::Int(i64::from(pages)))]);
            ask::text(title, &crate::PAGE_ENTERED).number()
        };
        cx.command(question.with(params::PATH, book.path.clone()).command());
    }

    /// Sets or removes the bookmark of the page.
    fn toggle_bookmark(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let Ok(book) = &mut self.book else {
            return;
        };
        let (start, end) = (book.page.start, book.page.end);
        let on_page = |m: &Mark| m.kind == Kind::Bookmark && m.start >= start && m.start < end;
        if !annotations::remove(env.storage, &book.path, on_page) {
            let text: Vec<&str> = book.page.lines.iter().map(|l| l.text.as_str()).collect();
            annotations::add(
                env.storage,
                &book.path,
                Annotation {
                    mark: Mark {
                        kind: Kind::Bookmark,
                        start,
                        end: start,
                        noted: false,
                    },
                    percent: (book.percent() + 0.5) as u8,
                    created: quireink_sdk::time::unix_seconds(&env).unwrap_or(0),
                    updated: 0,
                    text: annotations::snippet(&text.join(" ")),
                    note: String::new(),
                },
            );
        }
        book.marks = annotations::marks(env.storage, &book.path);
        cx.command(annotations::changed(&book.path));
        cx.request_redraw_area(book.ribbon_area(&env));
        cx.request_refresh(RefreshMode::Fast);
    }
}

impl Screen for ReaderScreen {
    /// *Select* opens the reading menu, so its hint says so.
    fn hint(&self, action: &Id, env: &Env<'_>) -> quireink_sdk::Hint {
        if *action == actions::CONFIRM {
            quireink_sdk::Hint::Text(env.i18n.tr("core.hint.core.menu").to_string())
        } else {
            quireink_sdk::Hint::Default
        }
    }

    /// The book is closed: said, where it was left.
    fn on_close(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        if let Ok(book) = &mut self.book {
            book.save_due(&env, true);
        }
        if let Ok(book) = &self.book
            && !book.web()
        {
            cx.command(events::BOOK_CLOSED.emit(book.reading(&env)));
        }
    }

    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let book = match &self.book {
            Ok(book) => book,
            Err(message) => return draw_message(canvas, env, message),
        };
        // Word boxes are measured only when something is drawn on words.
        let has_highlights = book.marks.iter().any(|m| m.kind == Kind::Highlight);
        let words = if has_highlights
            || self.selection.is_some()
            || self.linking.is_some()
            || book.found.is_some()
            || self.aloud.is_some()
            || !book.links.is_empty()
        {
            book.words(env)
        } else {
            Vec::new()
        };
        book.draw_highlights(canvas, env, &words);
        book.draw_page(canvas, env);
        book.draw_links(canvas, env, &words);
        book.draw_back(canvas, env);
        book.draw_status(canvas, env);
        if book.bookmarked() {
            book.draw_ribbon(canvas, env);
        }
        // The sentence read aloud, underlined like a search hit.
        if let Some(aloud) = self.aloud
            && aloud.now.page == book.page.start
            && env.settings.read(&crate::settings::ALOUD_HIGHLIGHT)
        {
            let line = env.metrics.tenth_mm(5).max(2);
            let sentence = aloud.now.from..aloud.now.to;
            for b in selection::line_boxes(&words, |i| sentence.contains(&i)) {
                canvas.fill_rect(
                    i32::from(b.x),
                    b.bottom() as i32 + 1,
                    u32::from(b.width),
                    line,
                    Color::BLACK,
                );
            }
        }
        if let Some((start, end)) = book.found {
            // Underlined, so it stays visible under a highlight.
            let line = env.metrics.tenth_mm(5).max(2);
            let found = |i: usize| words[i].end > start && words[i].start < end;
            for b in selection::line_boxes(&words, found) {
                canvas.fill_rect(
                    i32::from(b.x),
                    b.bottom() as i32 + 1,
                    u32::from(b.width),
                    line,
                    Color::BLACK,
                );
            }
        }
        // A bar at the bottom hides the status line under it entirely.
        let bar = self.linking.is_some() || self.selection.is_some_and(|s| s.mode == Mode::Bar);
        if let Some(status) = book.typography.status.filter(|_| bar) {
            canvas.fill(status, Color::WHITE);
        }
        if let Some(linking) = self.linking {
            draw_linking(canvas, env, book, &words, linking);
        }
        let Some(selection) = self.selection else {
            return;
        };
        let (lo, hi) = selection.range();
        let line = env.metrics.tenth_mm(4).max(2);
        for b in selection::line_boxes(&words, |i| (lo..=hi).contains(&i)) {
            canvas.stroke_rect(
                i32::from(b.x) - 4,
                i32::from(b.y) - 2,
                u32::from(b.width) + 8,
                u32::from(b.height) + 4,
                line,
                Color::BLACK,
            );
        }
        if selection.mode == Mode::Bar {
            let items = self.items(env, &words);
            selection::draw_bar(canvas, env, &items, selection.focus);
        }
    }

    fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        if self.preparing.is_some() {
            // Shown saying so first, then prepared.
            let now = cx.env().now;
            cx.wake_at(now);
            return;
        }
        // Done skimming: *Back* returns to where it began.
        if let (Some(from), Ok(book)) = (self.skim_from.take(), &mut self.book)
            && book.page.start != from
        {
            if book.returns.len() == RETURNS {
                book.returns.remove(0);
            }
            book.returns.push(from);
            book.landed = Some(book.page.start);
        }
        // *Back* is drawn anew: screens over it may have hidden it.
        if let Ok(book) = &self.book
            && let Some(area) = book.back_area(cx.env())
        {
            cx.request_redraw_area(area);
        }
        if let Ok(book) = &self.book {
            // The book's own settings ("Only this book") while it is open.
            cx.command(Command::SetScope(Some(format!(
                "{}/settings.ini",
                book.dir
            ))));
            if !book.web() {
                cx.command(events::BOOK_OPENED.emit(book.reading(cx.env())));
            }
            let now = cx.env().now;
            cx.wake_at(now);
        }
        // Time passed while another screen was open.
        self.update_status(cx);
    }

    fn on_timer(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        if self.prepare(cx) {
            return;
        }
        let Ok(book) = &mut self.book else {
            return;
        };
        book.save_due(&env, false);
        if book.index_step(&env) {
            cx.wake_at(env.now.add_millis(STEP_DELAY_MS));
        }
        self.auto_turn_due(cx);
        // Page numbers once counted, the time, the battery, …
        self.update_status(cx);
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        // A button pressed while the book is being prepared waits for it.
        self.prepare(cx);
        // Any button pauses reading aloud (also "Read aloud"), and does
        // nothing else.
        if self.aloud.is_some() {
            self.pause_aloud(cx);
            return Handled::Yes;
        }
        if *action == crate::actions::READ_ALOUD {
            self.start_aloud(cx);
            return Handled::Yes;
        }
        if *action == crate::actions::AUTO_TURN {
            self.toggle_auto_turn(cx);
            return Handled::Yes;
        }
        if *action == crate::actions::SKIM_BOOK {
            self.open_skim(cx);
            return Handled::Yes;
        }
        // While pages turn by themselves, turning one starts the count
        // anew; the menu (a tap in the middle) stops it instead.
        if self.auto_turn.is_some() {
            let env = *cx.env();
            if *action == actions::MENU || *action == actions::CONFIRM {
                self.toggle_auto_turn(cx);
                return Handled::Yes;
            }
            if *action == actions::NEXT_PAGE || *action == actions::PREV_PAGE {
                let seconds = self.auto_turn_seconds(&env);
                self.auto_turn = Some(env.now.add_millis(seconds * 1000));
                if let Some(at) = self.auto_turn {
                    cx.wake_at(at);
                }
            }
        }
        if let Some(selection) = self.selection {
            return self.on_selection(action, selection, cx);
        }
        if let Some(linking) = self.linking {
            return self.on_linking(action, linking, cx);
        }
        if *action == crate::actions::LINKS {
            let env = *cx.env();
            if let Ok(book) = &self.book {
                let focus = (!book.followable(&env).is_empty()).then_some(0);
                self.set_linking(Some(Linking { focus }), cx);
            }
            return Handled::Yes;
        }
        if *action == crate::actions::SELECT {
            // Choose text with the buttons, starting at the first word.
            let env = *cx.env();
            if let Ok(book) = &self.book {
                let words = book.words(&env);
                let first = words.iter().position(|w| !w.lookup_text().is_empty());
                self.set_selection(first.map(Selection::choosing), cx);
            }
            return Handled::Yes;
        }
        if *action == crate::actions::TOGGLE_BOOKMARK {
            self.toggle_bookmark(cx);
            return Handled::Yes;
        }
        if *action == crate::actions::NEXT_STATUS_LINE
            || *action == crate::actions::NEXT_TOP_STATUS_LINE
        {
            let env = *cx.env();
            let top =
                *action == crate::actions::NEXT_TOP_STATUS_LINE || !status::shown(&env, false);
            self.next_status_line(top, cx);
            return Handled::Yes;
        }
        if *action == crate::actions::SEARCH || *action == crate::actions::GO_TO_PAGE {
            self.ask(action, cx);
            return Handled::Yes;
        }
        let Ok(book) = &mut self.book else {
            return Handled::No;
        };
        if *action == actions::NEXT_PAGE {
            let end = book.page.end;
            book.show(cx, end, true);
            Handled::Yes
        } else if *action == actions::PREV_PAGE {
            let env = *cx.env();
            if let Some(start) = book.previous_start(&env) {
                book.show(cx, start, false);
            }
            Handled::Yes
        } else if *action == crate::actions::BOOK_INFO {
            cx.command(Command::OpenSlot(
                slots::BOOK_INFO,
                Params::new().with(params::PATH, book.path.clone()),
            ));
            Handled::Yes
        } else if *action == crate::actions::TOC {
            cx.command(Command::OpenScreen(
                crate::TOC_SCREEN,
                crate::toc::locator_params(&book.path, book.page.start),
            ));
            Handled::Yes
        } else if *action == crate::actions::ANNOTATIONS {
            cx.command(Command::OpenScreen(
                crate::ANNOTATIONS_SCREEN,
                Params::new().with(params::PATH, book.path.clone()),
            ));
            Handled::Yes
        } else if *action == actions::BACK && !book.returns.is_empty() {
            book.go_back(cx);
            Handled::Yes
        } else if *action == actions::MENU || *action == actions::CONFIRM {
            // As a bar at the top, with the book and where one is in it.
            let env = *cx.env();
            let chapter = book
                .chapter
                .as_ref()
                .map(|(_, _, title)| title.clone())
                .unwrap_or_default();
            let reading = book.reading(&env);
            let page = match (reading.page, reading.pages) {
                (Some(page), Some(pages)) => format!("{page} / {pages}"),
                _ => format!("{} %", reading.progress),
            };
            // Pictures of pages: no text to search, hear or set.
            let mut hide: Vec<Id> = Vec::new();
            if book.document.whole_pages() {
                hide.extend([
                    crate::actions::SEARCH,
                    crate::actions::READ_ALOUD,
                    crate::actions::QUICK_SETTINGS,
                ]);
                if book.document.toc_len(env.storage) == 0 {
                    hide.push(crate::actions::TOC);
                }
            }
            let hide: Vec<&str> = hide.iter().map(Id::as_str).collect();
            cx.command(Command::OpenSlot(
                slots::MENU,
                Params::new()
                    .with(params::MENU, crate::MENU.as_str())
                    .with(params::HIDE, hide.join(","))
                    .with(params::MODE, "bar")
                    .with(params::TITLE, book.title.clone())
                    .with(params::TEXT, chapter)
                    .with(params::PAGE, page)
                    .with(params::ACTION, crate::actions::BOOK_INFO.as_str()),
            ));
            Handled::Yes
        } else {
            Handled::No
        }
    }

    /// A tap on a link follows it, a long press selects the word under
    /// the finger; while text is selected, taps extend the selection or
    /// choose an item of the bar.
    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        // As does any tap.
        if self.aloud.is_some() {
            self.pause_aloud(cx);
            return Handled::Yes;
        }
        if self.linking.is_some() {
            self.set_linking(None, cx);
            return Handled::Yes;
        }
        let env = *cx.env();
        // A tap on a status line shows the next one (not where the
        // bookmark corner or the way back are).
        if let (GestureKind::Tap, None, Ok(book)) = (gesture.kind, self.selection, &self.book) {
            let p = gesture.start;
            let corner = env.settings.read(&crate::settings::CORNER_BOOKMARK)
                && book.corner_area(&env).contains(p);
            let back = book.back_area(&env).is_some_and(|a| a.contains(p));
            if !corner
                && !back
                && let Some(top) = book.status_hit(&env, p)
            {
                self.next_status_line(top, cx);
                return Handled::Yes;
            }
        }
        let Ok(book) = &self.book else {
            return Handled::No;
        };
        let words = book.words(&env);
        let at = |point: Point| {
            words.iter().position(|word| {
                let area = word.area;
                // A little slack between words and lines.
                let slack = 4;
                point.x >= i32::from(area.x) - slack
                    && point.x < area.right() as i32 + slack
                    && point.y >= i32::from(area.y)
                    && point.y < area.bottom() as i32
            })
        };
        let link = at(gesture.start)
            .and_then(|i| book.link_of(&words[i]))
            .map(|link| (link.href.clone(), link_label(book, &words, link)));
        match (gesture.kind, self.selection) {
            (GestureKind::Tap, None)
                if env.settings.read(&crate::settings::CORNER_BOOKMARK)
                    && book.corner_area(&env).contains(gesture.start) =>
            {
                self.toggle_bookmark(cx);
                Handled::Yes
            }
            (GestureKind::Tap, None)
                if book
                    .back_area(&env)
                    .is_some_and(|area| area.contains(gesture.start)) =>
            {
                if let Ok(book) = &mut self.book {
                    if book.returns.is_empty() {
                        // Out of the web page, to the one before.
                        cx.command(Command::Back);
                    } else {
                        book.go_back(cx);
                    }
                }
                Handled::Yes
            }
            (GestureKind::Tap, None) if link.is_some() => {
                let (href, label) = link.unwrap_or_default();
                self.open_link(&href, &label, cx)
            }
            (GestureKind::LongPress, _) => match at(gesture.start) {
                Some(word) => {
                    self.set_selection(Some(Selection::touched(word)), cx);
                    Handled::Yes
                }
                None => Handled::No,
            },
            (GestureKind::Tap, Some(selection)) => {
                let items = self.items(&env, &words);
                if let Some(item) = selection::item_at(&env, &items, gesture.start) {
                    self.activate(&item.id, cx);
                } else if let Some(word) = at(gesture.start) {
                    let to = Selection {
                        cursor: word,
                        mode: Mode::Bar,
                        ..selection
                    };
                    self.set_selection(Some(to), cx);
                } else {
                    self.set_selection(None, cx);
                }
                Handled::Yes
            }
            _ => Handled::No,
        }
    }

    fn on_event(&mut self, event: &Id, params: &Params, cx: &mut Cx<'_, '_>) {
        let Ok(book) = &mut self.book else {
            return;
        };
        // Asleep the device may lose its power: the position first.
        if *event == events::SUSPEND {
            book.save_due(cx.env(), true);
        }
        let this_book = params.get(params::PATH) == Some(book.path.as_str());
        if *event == crate::SPOKEN && this_book {
            self.spoken(params, cx);
            return;
        }
        // Changed elsewhere too, e.g. by a sync.
        if events::ANNOTATIONS_CHANGED
            .read(event, params)
            .is_some_and(|a| a.path == book.path)
        {
            let marks = annotations::marks(cx.env().storage, &book.path);
            if marks != book.marks {
                book.marks = marks;
                cx.request_redraw();
                cx.request_refresh(RefreshMode::Partial);
            }
            return;
        }
        if *event == crate::FOLLOW && this_book {
            let href = params
                .get(crate::footnote::HREF)
                .unwrap_or_default()
                .to_string();
            self.follow(&href, cx);
            return;
        }
        if *event == crate::GOTO
            && this_book
            && let Some(at) = crate::toc::locator_of(params)
        {
            // Another page: no text stays selected.
            self.selection = None;
            let env = *cx.env();
            let start = book.page_start(&env, at);
            book.jump(cx, start);
            let end = params
                .get(crate::END_SECTION)
                .zip(params.get(crate::END_OFFSET))
                .and_then(|(s, o)| Some(Locator::new(s.parse().ok()?, o.parse().ok()?)));
            if let Some(end) = end {
                book.found = Some((at, end));
                cx.request_redraw();
            }
            return;
        }
        if *event == crate::SKIM && this_book {
            let to = params.get(crate::skim::TO).unwrap_or_default().to_string();
            self.skim(&to, cx);
            return;
        }
        if let Some(typed) = crate::NOTE_ENTERED.read(event, params)
            && this_book
            && let Some(at) = crate::toc::locator_of(&typed.context)
        {
            let env = *cx.env();
            let note = typed.text.as_str();
            let now = quireink_sdk::time::unix_seconds(&env).unwrap_or(0);
            annotations::set_note(env.storage, &book.path, at, note, now);
            book.marks = annotations::marks(env.storage, &book.path);
            cx.command(annotations::changed(&book.path));
            cx.request_redraw();
            return;
        }
        if let Some(typed) = crate::SEARCH_ENTERED.read(event, params)
            && this_book
        {
            let text = typed.text.trim();
            if !text.is_empty() {
                self.last_search = text.to_string();
                cx.command(Command::OpenScreen(
                    crate::SEARCH_SCREEN,
                    Params::new()
                        .with(params::PATH, book.path.clone())
                        .with(params::TEXT, text.to_string()),
                ));
            }
            return;
        }
        if let Some(typed) = crate::PAGE_ENTERED.read(event, params)
            && this_book
        {
            let env = *cx.env();
            let number = typed.text.trim().parse::<u32>().ok();
            if let (Some(number), Some(index)) = (number, &book.index)
                && index.pages() > 0
            {
                let page = number.clamp(1, index.pages()) - 1;
                if let Some(start) = index.entry(env.storage, page) {
                    self.selection = None;
                    book.jump(cx, start);
                }
            }
            return;
        }
        // Which setting changed, if one did.
        let changed = events::SETTING_CHANGED.read(event, params).map(|c| c.id);
        let id = changed.as_ref().map_or("", Id::as_str);
        let font_changed = id == quireink_sdk::builtin::settings::READING_FONT.as_str();
        if *event == events::STATUS_CHANGED
            || changed.is_some() && !font_changed && id.starts_with("core.")
        {
            // The battery, the clock format, the time zone, …
            self.status_shown.clear();
            self.update_status(cx);
            return;
        }
        let Ok(book) = &mut self.book else {
            return;
        };
        // The book's fonts are loaded with it.
        let reopen = id == crate::settings::PUBLISHER_STYLES.as_str()
            || id == quireink_sdk::builtin::settings::READING_FONT.as_str();
        if changed.is_some() && reopen {
            // Another conversion of the book: open it again where it was.
            let env = *cx.env();
            let path = book.path.clone();
            self.selection = None;
            self.book = Book::open(&env, &Params::new().with(params::PATH, path));
            cx.request_redraw();
            cx.wake_at(env.now);
            return;
        }
        let filters = env_filters_changed(cx.env(), book);
        let affects_layout = font_changed
            || filters
            || id.starts_with("reader.") && id != crate::settings::OPEN_LAST.as_str();
        if changed.is_some() && affects_layout {
            self.selection = None;
            let env = *cx.env();
            book.relayout(&env);
            cx.request_redraw();
            let now = env.now;
            cx.wake_at(now);
        }
    }
}

/// Centred, wrapped message text (errors, missing book).
fn draw_message(canvas: &mut Canvas<'_>, env: &Env<'_>, message: &str) {
    let Some(font) = env.fonts.role(fonts::UI.as_str()) else {
        return;
    };
    let display = env.device.display;
    let size = env.metrics.pt(12.0);
    let margin = env.metrics.mm(8) as f32;
    let width = f32::from(display.width) - 2.0 * margin;
    let line_height = font.line_metrics(size).line_height;
    let lines = font.wrap(size, message, width);
    let mut baseline =
        (f32::from(display.height) - line_height * lines.len() as f32) / 2.0 + line_height;
    for range in lines {
        let text = &message[range];
        let x = (f32::from(display.width) - font.measure(size, text)) / 2.0;
        font.draw(canvas, size, x, baseline as i32, text, Color::BLACK);
        baseline += line_height;
    }
}

#[cfg(test)]
mod tests {
    use alloc::vec::Vec;

    /// The position as version 1 wrote it stays readable (see
    /// `docs/data-files.md`).
    #[test]
    fn positions_of_version_1_are_read() {
        assert_eq!(
            super::book::position_in(include_str!("../../testdata/progress.ini.v1")),
            Some(quireink_sdk::document::Locator::new(7, 1234))
        );
    }

    #[test]
    fn small_caps_split_lower_case() {
        let parts: Vec<(&str, bool)> =
            super::typography::small_caps_parts("Mr. Gregor Samsa").collect();
        assert_eq!(
            parts,
            [
                ("M", false),
                ("r", true),
                (". G", false),
                ("regor", true),
                (" S", false),
                ("amsa", true)
            ]
        );
        assert_eq!(super::typography::small_caps_parts("").count(), 0);
        assert_eq!(
            super::typography::small_caps_parts("straße").collect::<Vec<_>>(),
            [("straße", true)]
        );
    }
}
