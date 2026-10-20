//! The reader's screen: a book shown page by page, and what the reader
//! does on it.

mod book;
mod draw;
mod hands_free;
mod linking;
mod marks;
mod sample;
mod selecting;
mod typography;
mod words;

use alloc::boxed::Box;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_digest::fnv1a64;
use paperpaw_sdk::ask;
use paperpaw_sdk::builtin::{actions, events, fonts, params, slots};
use paperpaw_sdk::document::{Document, DocumentError, Link, Locator};
use paperpaw_sdk::event::EventData;
use paperpaw_sdk::layout::{
    Face, Hyphenate, LayoutContext, Page, PageImage, PageLine, Style, Typesetter, layout_page,
    line_start,
};
use paperpaw_sdk::status::context as status_context;
use paperpaw_sdk::ui::{self, UiStyle};
use paperpaw_sdk::{
    Arg, Canvas, Color, Command, Cx, Env, FontHandle, FontSet, Gesture, GestureKind, Handled, Id,
    Params, Point, Rect, RefreshMode, Screen,
};

use crate::annotations::{self, Annotation, Kind, Mark};
use crate::index::PageIndex;
use crate::selection::{self, Item, Mode, Selection};
use crate::settings::{
    FONT_SIZE, FONT_WEIGHT, HYPHENATE, INDENT, JUSTIFY, LINE_SPACING, MARGIN, MARGIN_ENDS,
    PARAGRAPH_SPACING, WORD_SPACING,
};
use crate::status;
use crate::{PLUGIN, state_path};

use self::book::*;
pub(crate) use self::book::{Filtered, book_dir, open_options, remember_last};
use self::hands_free::*;
use self::linking::*;
pub(crate) use self::sample::Sample;
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

/// On small devices a rest as long as a step: the CPU rests (and the X4
/// sleeps lightly) half the time while pages are counted, instead of
/// running flat out for minutes.
const STEP_DELAY_SMALL_MS: u64 = 500;

/// The position is written at most this often while pages turn; always
/// when the book is closed or the device sleeps. A card is slow to write
/// to, and turning pages quickly should not wait for it.
const SAVE_EVERY_MS: u64 = 5000;

/// Pages remembered for going back before the index covers them.
const HISTORY: usize = 64;

/// Page indexes of a book's layouts kept before a new one removes them.
const INDEXES_KEPT: usize = 4;

/// The least text, in bytes, a page back goes where the page index does
/// not reach (the page shown holding less, e.g. a picture).
const MIN_PAGE_BACK: u32 = 200;

/// Places jumps were made from (links, contents, search, page numbers),
/// kept for *Back*.
const RETURNS: usize = 32;

/// Shows a book page by page.
pub(crate) struct ReaderScreen {
    book: Result<Book, String>,
    /// Text chosen on the page (see `selection`), and the page's start.
    selection: Option<Selection>,
    selected_on: Locator,
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
    auto_turn: Option<paperpaw_sdk::Instant>,
    /// Where skimming began, while skimming.
    skim_from: Option<Locator>,
    /// The book to open once the screen says it is being prepared (the
    /// first opening takes a while).
    preparing: Option<Params>,
    /// Pages with smooth letters (in grays, where the panel can).
    smooth: bool,
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
            smooth: env.settings.read(&crate::settings::SMOOTH_TEXT),
            book,
            preparing,
            selection: None,
            selected_on: Locator::START,
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
            .bool(paperpaw_sdk::builtin::settings::FLASH_SCREENS.as_str());
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
        let next = paperpaw_sdk::time::next_minute(&env).unwrap_or(env.now.add_millis(60_000));
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
        } else if *action == crate::actions::GO_TO_PERCENT {
            ask::text(
                env.i18n.tr("reader.go-to-percent.prompt"),
                &crate::PERCENT_ENTERED,
            )
            .number()
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
            let text = page_text(&book.page.lines);
            let kept = annotations::add(
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
                    created: paperpaw_sdk::time::unix_seconds(&env).unwrap_or(0),
                    updated: 0,
                    text: annotations::snippet(&text),
                    note: String::new(),
                },
            );
            if !kept {
                marks_not_saved(cx);
            }
        }
        book.marks = annotations::marks(env.storage, &book.path);
        cx.command(annotations::changed(&book.path));
        cx.request_redraw_area(book.ribbon_area(&env));
        cx.request_refresh(RefreshMode::Fast);
    }
}

impl Screen for ReaderScreen {
    /// A page with pictures, or any page with smooth letters: in grays
    /// where the panel can.
    fn grays(&self) -> bool {
        self.book
            .as_ref()
            .is_ok_and(|book| self.smooth || book.pictured())
    }

    /// *Select* opens the reading menu, so its hint says so.
    fn hint(&self, action: &Id, env: &Env<'_>) -> paperpaw_sdk::Hint {
        if *action == actions::CONFIRM {
            paperpaw_sdk::Hint::Text(env.i18n.tr("core.hint.core.menu").to_string())
        } else {
            paperpaw_sdk::Hint::Default
        }
    }

    /// The book is closed: said, where it was left.
    fn on_close(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        if let Ok(book) = &mut self.book {
            book.save_due(cx, true);
        }
        if let Ok(book) = &self.book
            && !book.web()
        {
            cx.command(events::BOOK_CLOSED.emit(book.reading(&env)));
        }
    }

    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        self.draw_book(canvas, env);
        // Kept from the first band of a frame to its last only: on the X4
        // a page's words take some 15 KiB.
        if let Ok(book) = &mut self.book
            && canvas.clip().bottom() >= u32::from(env.device.display.height)
        {
            book.words.take();
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
        book.save_due(cx, false);
        if book.index_step(&env) {
            let delay = match env.device.memory {
                paperpaw_sdk::MemoryClass::Small => STEP_DELAY_SMALL_MS,
                _ => STEP_DELAY_MS,
            };
            cx.wake_at(env.now.add_millis(delay));
        }
        self.auto_turn_due(cx);
        self.let_go_of_other_pages(cx);
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
        // The arrows turn pages as the page buttons do: the X4's buttons
        // in front, a keyboard's arrows; not while text or a link is
        // chosen, where they move by words and lines.
        let choosing = self.selection.is_some() || self.linking.is_some();
        let turn = page_turn(action).filter(|_| !choosing);
        let action = turn.as_ref().unwrap_or(action);
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
                let focus = (!book.targets(&env).is_empty()).then_some(0);
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
        if *action == crate::actions::SEARCH
            || *action == crate::actions::GO_TO_PAGE
            || *action == crate::actions::GO_TO_PERCENT
        {
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
        } else if *action == crate::actions::NEXT_CHAPTER || *action == crate::actions::PREV_CHAPTER
        {
            let env = *cx.env();
            let to = match *action == crate::actions::NEXT_CHAPTER {
                true => "chapter:+1",
                false => "chapter:-1",
            };
            if let Some(target) = book.skim_target(&env, to) {
                let start = book.page_start(&env, target);
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
            .map(|link| (link.href.clone(), link_label(book, words, link)));
        // A mark whose plugin hears taps on it.
        let tapped = at(gesture.start).and_then(|i| book.tappable_at(words, i));
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
            (GestureKind::Tap, None) if tapped.is_some() => {
                if let Some(mark) = tapped {
                    book.tap_mark(words, mark, cx);
                }
                Handled::Yes
            }
            (GestureKind::LongPress, _) => match at(gesture.start) {
                Some(word) => {
                    self.set_selection(Some(Selection::touched(word)), cx);
                    Handled::Yes
                }
                None => Handled::No,
            },
            (GestureKind::Tap, Some(selection)) => {
                let items = self.items(&env, words);
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
        self.event(event, params, cx);
        self.let_go_of_other_pages(cx);
    }
}

impl ReaderScreen {
    /// The page, its marks and status lines, and what is chosen on it.
    fn draw_book(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let book = match &mut self.book {
            Ok(book) => book,
            Err(message) => {
                let area = Rect::from_size(env.device.display.width, env.device.display.height);
                return ui::draw_message(canvas, env, &UiStyle::new(env), area, message);
            }
        };
        // Word boxes are measured only when something is drawn on words.
        let has_highlights = book.marks.iter().any(|m| m.kind == Kind::Highlight);
        let measured = has_highlights
            || self.selection.is_some()
            || self.linking.is_some()
            || book.found.is_some()
            || self.aloud.is_some()
            || !book.links.is_empty()
            || !env.decorators.is_empty();
        book.update_decorations(env);
        let Ok(book) = &self.book else {
            return;
        };
        let words = if measured { book.words(env) } else { &[] };
        // The sentence read aloud, where it is on this page.
        let aloud = self
            .aloud
            .filter(|a| {
                a.now.page == book.page.start
                    && env.settings.read(&crate::settings::ALOUD_HIGHLIGHT)
            })
            .map(|a| (a.now.from, a.now.to));
        let marks = book.page_marks(env, aloud, words);
        marks.draw_behind(canvas, env);
        book.draw_page(canvas, env, &marks.inks);
        marks.draw_over(canvas, env);
        book.draw_note_signs(canvas, env, words);
        book.draw_back(canvas, env);
        book.draw_status(canvas, env);
        if book.bookmarked() {
            book.draw_ribbon(canvas, env);
        }
        // A bar at the bottom hides the status line under it entirely.
        let bar = self.linking.is_some() || self.selection.is_some_and(|s| s.mode == Mode::Bar);
        if let Some(status) = book.typography.status.filter(|_| bar) {
            canvas.fill(status, Color::WHITE);
        }
        if let Some(linking) = self.linking {
            draw_linking(canvas, env, book, words, linking);
        }
        let Some(selection) = self.selection else {
            return;
        };
        let (lo, hi) = selection.range();
        let line = env.metrics.tenth_mm(4).max(2);
        for b in selection::line_boxes(words, |i| (lo..=hi).contains(&i)) {
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
            let items = self.items(env, words);
            selection::draw_bar(canvas, env, &items, selection.focus);
        }
    }

    /// Lets go of text chosen on a page no longer shown: read aloud,
    /// skimmed or turned by itself, its words are those of another page.
    fn let_go_of_other_pages(&mut self, cx: &mut Cx<'_, '_>) {
        if let (Some(_), Ok(book)) = (self.selection, &self.book)
            && book.page.start != self.selected_on
        {
            self.set_selection(None, cx);
        }
    }

    fn event(&mut self, event: &Id, params: &Params, cx: &mut Cx<'_, '_>) {
        let Ok(book) = &mut self.book else {
            return;
        };
        // Asleep the device may lose its power: the position first.
        if *event == events::SUSPEND {
            book.save_due(cx, true);
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
        if let Some(goto) = events::GOTO.read(event, params)
            && goto.path == book.path
        {
            // Another page: no text stays selected.
            self.selection = None;
            let env = *cx.env();
            let at = goto.at();
            let start = book.page_start(&env, at);
            book.jump(cx, start);
            if let Some(end) = goto.end() {
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
            let now = paperpaw_sdk::time::unix_seconds(&env).unwrap_or(0);
            if !annotations::set_note(env.storage, &book.path, at, note, now) {
                marks_not_saved(cx);
            }
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
        if let Some(typed) = crate::PERCENT_ENTERED.read(event, params)
            && this_book
        {
            let env = *cx.env();
            // 100 % is the last page.
            let permille = typed
                .text
                .trim()
                .parse::<u32>()
                .ok()
                .map(|p| (p * 10).min(999));
            if let Some(target) =
                permille.and_then(|p| book.skim_target(&env, &format!("permille:{p}")))
            {
                self.selection = None;
                book.jump(cx, target);
            }
            return;
        }
        // Which setting changed, if one did.
        let changed = events::SETTING_CHANGED.read(event, params).map(|c| c.id);
        let id = changed.as_ref().map_or("", Id::as_str);
        // The kernel's settings the page's type depends on: the font and
        // its size (the reader's own all begin with `reader.`).
        let font_changed = id == paperpaw_sdk::builtin::settings::READING_FONT.as_str()
            || id == FONT_SIZE.as_str();
        // How marks look, or what decorators mark (a switch, their data):
        // the page drawn again as it is laid out.
        if changed.is_some()
            && let Ok(book) = &self.book
        {
            let env = *cx.env();
            let look = env
                .settings
                .specs()
                .any(|s| s.id.as_str() == id && s.group == paperpaw_sdk::builtin::groups::MARKS);
            let marked = book
                .decorated_for
                .is_some_and(|(_, signature)| signature != env.decorators.signature(&env))
                || book.decorated_for.is_none() && !env.decorators.is_empty();
            if look || marked {
                cx.request_redraw();
                cx.request_refresh(RefreshMode::Partial);
                return;
            }
        }
        if *event == events::STATUS_CHANGED
            || changed.is_some() && !font_changed && id.starts_with("core.")
        {
            // The battery, the clock format, the time zone, …
            self.status_shown.clear();
            self.update_status(cx);
            return;
        }
        if id == crate::settings::SMOOTH_TEXT.as_str() {
            // The same page, shown in grays or not from now on.
            self.smooth = cx.env().settings.read(&crate::settings::SMOOTH_TEXT);
            cx.request_redraw();
            cx.request_refresh(RefreshMode::Full);
            return;
        }
        let Ok(book) = &mut self.book else {
            return;
        };
        // The book's fonts are loaded with it.
        let reopen = id == crate::settings::PUBLISHER_STYLES.as_str()
            || id == paperpaw_sdk::builtin::settings::READING_FONT.as_str();
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

/// The text of `lines` as the book has it: a word broken at the end of
/// a line is whole again, without the hyphen the layout put there (a
/// hyphen of the book's own stays).
pub(super) fn page_text(lines: &[PageLine]) -> String {
    let mut out = String::new();
    let mut joined = false;
    for (i, line) in lines.iter().enumerate() {
        if i > 0 && !joined {
            out.push(' ');
        }
        let last = line.words.last().copied();
        let next = lines.get(i + 1).filter(|n| n.section == line.section);
        let first = next.and_then(|n| n.words.first().copied());
        // The word goes on in the next line, at once.
        joined = line.text.ends_with('-')
            && matches!((last, first), (Some((_, end)), Some((start, _))) if start == end);
        let mut text = line.text.as_str();
        if let Some((start, end)) = last.filter(|_| joined) {
            let word = text.rsplit(' ').next().unwrap_or(text);
            if (end.saturating_sub(start) as usize) < word.len() {
                text = &text[..text.len() - 1];
            }
        }
        out.push_str(text);
    }
    out
}

/// Says that a bookmark, highlight or note could not be kept: the book's
/// annotations could not be read or written.
pub(super) fn marks_not_saved(cx: &mut Cx<'_, '_>) {
    let text = cx.env().i18n.tr("reader.marks-not-saved").to_string();
    cx.command(Command::Notify(text));
}

/// The page turn an arrow stands for in a book: left and up back, right
/// and down on.
fn page_turn(action: &Id) -> Option<Id> {
    if *action == actions::LEFT || *action == actions::UP {
        Some(actions::PREV_PAGE)
    } else if *action == actions::RIGHT || *action == actions::DOWN {
        Some(actions::NEXT_PAGE)
    } else {
        None
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
            Some(paperpaw_sdk::document::Locator::new(7, 1234))
        );
    }

    #[test]
    fn chapters_in_parts_get_ticks_while_they_leave_room() {
        use super::draw::chapter_ticks;
        // Two parts of three chapters each.
        let book = [
            (0, 0),
            (1, 10),
            (1, 200),
            (1, 400),
            (0, 500),
            (1, 510),
            (1, 700),
        ];
        assert_eq!(chapter_ticks(&book), [0, 10, 200, 400, 500, 510, 700]);
        // Too many chapters for the bar: the parts only.
        let mut many = alloc::vec![(0, 0), (0, 500)];
        many.extend((1..200).map(|i| (1, i * 5)));
        many.sort_by_key(|&(_, at)| at);
        assert_eq!(chapter_ticks(&many), [0, 500]);
        assert!(chapter_ticks(&[]).is_empty());
        // Many chapters without parts: each its tick, along the whole bar.
        let flat: Vec<(u8, u32)> = (0..120).map(|i| (0, i * 8)).collect();
        let ticks = chapter_ticks(&flat);
        assert_eq!(ticks.len(), 120);
        assert_eq!(ticks.last(), Some(&952));
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
