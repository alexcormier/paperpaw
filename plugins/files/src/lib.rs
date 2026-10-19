//! The library: the books and folders of the library folder.
//!
//! Each book shows its cover, title and author (read from the book the
//! first time it is shown, then cached) and how far it has been read.
//! Books are ordered by when they were last read, or by name; the library's
//! menu switches the order. Folders come first and act as collections.
//! Covers and details are read in the background for the visible rows
//! only, and only one page of a folder is held in memory, so libraries of
//! any size work on small devices.
//!
//! The library folder is a setting; its editor (`files.choose-folder`)
//! browses the device's folders, so no path has to be typed.
//!
//! Works with buttons only (up/down or left/right move the selection,
//! page buttons scroll, confirm opens, back goes to the parent folder, the
//! menu button opens the library's menu) and with touch (tap a row; tap the
//! header for the menu). Bound to `core.library`, and to `core.home` only
//! as a fallback when no home screen plugin is installed.

#![no_std]

extern crate alloc;

mod folders;
mod info;
mod shelves;

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use quireink_sdk::ask;
use quireink_sdk::builtin::{
    actions as core_actions, events, groups, params, settings as core_settings, slots,
};
use quireink_sdk::event::Event;
use quireink_sdk::files;
use quireink_sdk::library::listing::{Entry, Listing, UNRANKED};
use quireink_sdk::library::{self, Book};
use quireink_sdk::replies::Typed;
use quireink_sdk::setting::{Setting, Text};
use quireink_sdk::ui::{self, ListChange, ListView, Thumbnail, UiStyle};
use quireink_sdk::{
    Arg, Canvas, Color, Command, Cx, Env, Gesture, GestureKind, Handled, Id, Manifest, MenuEntry,
    Params, Plugin, Priority, Rect, RefreshMode, RegisterError, Registrar, SDK_VERSION, Screen,
    ScreenSpec, SettingSpec, Version,
};

pub const PLUGIN: Id = Id::new_static("files");
pub const SCREEN: Id = Id::new_static("files.browser");
/// The library folder, relative to the storage root.
pub const ROOT: Setting<Text> = quireink_sdk::library::FOLDER;
/// The order of books: [`SORT_RECENT`] or [`SORT_NAME`].
pub const SORT: Setting<Text> = Setting::new("files.sort");
/// Recently read books first, then the others by name.
pub const SORT_RECENT: &str = "files.sort.recent";
/// By file name.
pub const SORT_NAME: &str = "files.sort.name";
/// The folder finished books are moved to (by the book's options).
pub const ARCHIVE: Setting<Text> = Setting::new("files.archive");
/// How books are shown: as a list ([`VIEW_LIST`]) or a grid of covers
/// ([`VIEW_COVERS`]).
pub const VIEW: Setting<Text> = Setting::new("files.view");
pub const VIEW_LIST: &str = "files.view.list";
pub const VIEW_COVERS: &str = "files.view.covers";
/// Chooses a folder for the setting named by the parameter `id`.
pub const CHOOSE_FOLDER: Id = Id::new_static("files.choose-folder");
/// The library's menu.
pub const MENU: Id = Id::new_static("files.menu");
/// The library by author, by series and by tag.
pub const BY_AUTHOR: Id = Id::new_static("files.by-author");
pub const BY_SERIES: Id = Id::new_static("files.by-series");
pub const BY_TAG: Id = Id::new_static("files.by-tag");
/// The favourite books.
pub const FAVOURITES: Id = Id::new_static("files.favourites");
/// Searches the library (asks for words on the keyboard).
pub const SEARCH: Id = Id::new_static("files.search");
/// The keyboard's reply with the words.
const SEARCH_ENTERED: Event<Typed> = Event::new("files.search-entered");
/// What can be done with a book (held in the list).
const CHOICE: Id = Id::new_static("files.choice");
const CHOSEN: Event<quireink_sdk::ui::choice::Picked> = Event::new("files.book-chosen");
/// Book information; parameter `path`.
pub const INFO_SCREEN: Id = Id::new_static("files.info");
/// The submenu with the views above ("Shelves").
pub const SHELVES_MENU: Id = Id::new_static("files.shelves");
const SHELF_SCREEN: Id = Id::new_static("files.shelf");

/// The library plugin.
#[derive(Debug, Default)]
pub struct Files;

/// Folders chosen on the folder chooser.
static FOLDER: quireink_sdk::SettingExtra = quireink_sdk::SettingExtra::new().editor(CHOOSE_FOLDER);

/// Where the books and the archive are, and how the library shows them.
static SETTINGS: [SettingSpec; 4] = [
    SettingSpec::fixed_text(
        ROOT.static_str(),
        "files.setting.root",
        "books",
        groups::LIBRARY,
    )
    .with_extra(&FOLDER)
    .described("files.setting.root.help"),
    SettingSpec::fixed_text(
        ARCHIVE.static_str(),
        "files.setting.archive",
        "books/Archive",
        groups::LIBRARY,
    )
    .with_extra(&FOLDER)
    .advanced()
    .described("files.setting.archive.help"),
    SettingSpec::fixed_choice(
        VIEW.static_str(),
        "files.setting.view",
        &[VIEW_LIST, VIEW_COVERS],
        VIEW_LIST,
        groups::LIBRARY,
    )
    .described("files.setting.view.help"),
    SettingSpec::fixed_choice(
        SORT.static_str(),
        "files.setting.sort",
        &[SORT_RECENT, SORT_NAME],
        SORT_RECENT,
        groups::LIBRARY,
    )
    .described("files.setting.sort.help"),
];

impl Plugin for Files {
    fn manifest(&self) -> Manifest {
        Manifest {
            id: PLUGIN,
            name: Cow::Borrowed("File browser"),
            description: Cow::Borrowed("Lists the books and folders on the device and opens them."),
            version: Version::new(0, 1, 0),
            sdk: SDK_VERSION,
        }
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_screen(ScreenSpec {
            id: SCREEN,
            title: Cow::Borrowed("files.title"),
            factory: Box::new(|env, params| Box::new(Browser::new(env, params))),
        })?;
        r.add_screen(ScreenSpec {
            id: CHOOSE_FOLDER,
            title: Cow::Borrowed("files.choose.title"),
            factory: Box::new(|env, params| Box::new(folders::FolderChooser::new(env, params))),
        })?;
        r.add_screen(ScreenSpec {
            id: CHOICE,
            title: Cow::Borrowed("files.title"),
            factory: Box::new(|env, params| {
                Box::new(quireink_sdk::ui::choice::Choice::new(env, params))
            }),
        })?;
        r.add_screen(ScreenSpec {
            id: INFO_SCREEN,
            title: Cow::Borrowed("files.info"),
            factory: Box::new(|env, params| Box::new(info::InfoScreen::new(env, params))),
        })?;
        r.add_screen(ScreenSpec {
            id: SHELF_SCREEN,
            title: Cow::Borrowed("files.title"),
            factory: Box::new(|env, params| Box::new(shelves::ShelfScreen::new(env, params))),
        })?;
        for (action, mode, title, icon) in [
            (BY_AUTHOR, shelves::MODE_AUTHORS, "files.by-author", "user"),
            (
                BY_SERIES,
                shelves::MODE_SERIES,
                "files.by-series",
                "list-details",
            ),
            (BY_TAG, shelves::MODE_TAGS, "files.by-tag", "tag"),
            (
                FAVOURITES,
                shelves::MODE_FAVOURITES,
                "files.favourites",
                "star",
            ),
        ] {
            r.add_action(
                quireink_sdk::ActionSpec::new(action.clone(), title).handler(
                    move |_: &Env<'_>, out: &mut Vec<Command>| {
                        out.push(Command::OpenScreen(
                            SHELF_SCREEN,
                            Params::new().with(params::MODE, mode),
                        ));
                    },
                ),
            )?;
            r.add_icon(action, Cow::Borrowed(icon))?;
        }
        r.bind_slot(slots::CHOOSE_FOLDER, CHOOSE_FOLDER, Priority::DEFAULT)?;
        r.bind_slot(slots::BOOK_INFO, INFO_SCREEN, Priority::DEFAULT)?;
        r.bind_slot(slots::HOME, SCREEN, Priority::FALLBACK)?;
        r.bind_slot(slots::LIBRARY, SCREEN, Priority::DEFAULT)?;
        r.add_settings(&SETTINGS)?;
        r.add_action(
            quireink_sdk::ActionSpec::new(SEARCH, "files.search").handler(
                |env: &Env<'_>, out: &mut Vec<Command>| {
                    out.push(
                        ask::text(env.i18n.tr("files.search.prompt"), &SEARCH_ENTERED).command(),
                    );
                },
            ),
        )?;
        r.add_icon(SEARCH, Cow::Borrowed("search"))?;
        // The menu's settings, with icons as its other rows.
        for (setting, icon) in [
            (Id::from(SORT), "sort-ascending"),
            (VIEW.into(), "layout"),
            (ROOT.into(), "folder"),
        ] {
            r.add_icon(setting, Cow::Borrowed(icon))?;
        }
        r.on(SEARCH_ENTERED, |_, typed, out| {
            let text = typed.text.trim();
            if !text.is_empty() {
                out.push(Command::OpenScreen(
                    SHELF_SCREEN,
                    Params::new()
                        .with(params::MODE, shelves::MODE_SEARCH)
                        .with(params::TEXT, text.to_string()),
                ));
            }
        })?;
        // The shelves as a submenu: an action named like its menu.
        r.add_action(
            quireink_sdk::ActionSpec::new(SHELVES_MENU, "files.shelves").handler(
                |_: &Env<'_>, out: &mut Vec<Command>| {
                    out.push(Command::OpenSlot(
                        slots::MENU,
                        Params::new().with(params::MENU, String::from(SHELVES_MENU.as_str())),
                    ));
                },
            ),
        )?;
        r.add_icon(SHELVES_MENU, Cow::Borrowed("books"))?;
        for (item, order) in [
            (FAVOURITES, 5),
            (BY_AUTHOR, 10),
            (BY_SERIES, 20),
            (BY_TAG, 30),
        ] {
            r.add_menu_entry(MenuEntry {
                menu: SHELVES_MENU,
                item,
                order,
            })?;
        }
        for (item, order) in [
            (Id::from(SORT), 10),
            (VIEW.into(), 10),
            (SEARCH, 11),
            (SHELVES_MENU, 12),
            (ROOT.into(), 20),
            (core_settings::NIGHT_MODE.into(), 30),
            (core_actions::SETTINGS, 40),
            (core_actions::HOME, 50),
        ] {
            r.add_menu_entry(MenuEntry {
                menu: MENU,
                item,
                order,
            })?;
        }
        // As on phones: down on the home screen searches.
        r.add_keymap(Cow::Borrowed(
            "[keymap.core.home]\nswipe.down = files.search\n",
        ))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/en.ini")))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/de.ini")))?;
        Ok(())
    }
}

/// Delay between two books read in the background, so input stays quick.
const STEP_DELAY_MS: u64 = 20;

#[derive(Debug)]
struct Browser {
    root: String,
    dir: String,
    /// Recently read files of this folder: `(name, rank)`.
    recent: Vec<(String, u32)>,
    /// Number of entries in the folder.
    len: usize,
    /// Index of the first entry of `page`.
    first: usize,
    /// The entries of the visible page, with their cached details.
    page: Vec<(Entry, Option<Book>)>,
    /// The covers of `page`, made small once (drawing a cover from its
    /// file in every band of every frame took most of a page's time).
    thumbs: Vec<Option<Thumbnail>>,
    /// Just loaded: the first `on_show` need not load again.
    fresh: bool,
    style: UiStyle,
    header: ui::Header,
    list: ListView,
}

impl Browser {
    fn new(env: &Env<'_>, params: &Params) -> Self {
        let root = env
            .settings
            .text(ROOT.as_str())
            .trim_matches('/')
            .to_string();
        let dir = params.get(params::PATH).unwrap_or(&root).to_string();
        let style = UiStyle::new(env);
        let display = env.device.display;
        let header = ui::Header::new(env, &style, true);
        let top = header.area.height + style.gap as u16;
        let area = Rect::new(0, top, display.width, display.height.saturating_sub(top));
        let mut browser = Self {
            root,
            dir,
            recent: Vec::new(),
            len: 0,
            first: 0,
            page: Vec::new(),
            thumbs: Vec::new(),
            fresh: true,
            list: list_view(env, &style, area),
            style,
            header,
        };
        browser.load(env);
        browser
    }

    /// The folder's entries, in the chosen order.
    fn listing<'a>(&'a self, env: &'a Env<'_>, rank: &'a dyn Fn(&str) -> u32) -> Listing<'a> {
        let by_recent = env.settings.read(&SORT) != SORT_NAME;
        Listing {
            storage: env.storage,
            formats: env.formats,
            dir: &self.dir,
            folders_only: false,
            rank: by_recent.then_some(rank),
        }
    }

    /// The rank of a recently read file of this folder.
    fn rank_of(recent: &[(String, u32)], name: &str) -> u32 {
        recent
            .iter()
            .find(|(n, _)| n == name)
            .map_or(UNRANKED, |(_, rank)| *rank)
    }

    /// Shows the first page of the current folder.
    fn load(&mut self, env: &Env<'_>) {
        let prefix = if self.dir.is_empty() {
            String::new()
        } else {
            format!("{}/", self.dir)
        };
        self.recent = library::opened_books(env.storage)
            .into_iter()
            .enumerate()
            .filter_map(|(rank, path)| {
                let name = path.strip_prefix(&prefix)?;
                (!name.contains('/')).then(|| (name.to_string(), rank as u32))
            })
            .collect();
        let recent = &self.recent;
        let rank = |name: &str| Self::rank_of(recent, name);
        let (len, page) = {
            let listing = self.listing(env, &rank);
            (listing.count(), listing.after(None, self.list.rows()))
        };
        self.len = len;
        self.list.focus = 0;
        self.first = 0;
        self.set_page(env, page);
    }

    /// Takes `entries` as the visible page, with their cached details.
    fn set_page(&mut self, env: &Env<'_>, entries: Vec<Entry>) {
        let page = entries
            .into_iter()
            .map(|entry| {
                let book = (!entry.is_dir)
                    .then(|| library::book_sized(env.storage, &self.path_of(&entry), entry.size))
                    .flatten();
                (entry, book)
            })
            .collect();
        self.page = page;
        self.thumbs = (0..self.page.len())
            .map(|slot| self.thumb(env, slot))
            .collect();
    }

    /// The cover of entry `slot` of the page made small, at the size it is
    /// drawn, if it has one.
    fn thumb(&self, env: &Env<'_>, slot: usize) -> Option<Thumbnail> {
        let (entry, book) = self.page.get(slot)?;
        let book = book.as_ref()?;
        let rect = self.list.row_rect(0);
        let place = if self.list.column_count() > 1 {
            cell_cover(env, &self.style, rect)?
        } else {
            row_thumb(&self.style, rect)
        };
        let format = env.device.display.format;
        let path = self.path_of(entry);
        library::cover(env.storage, &path, book, place.width, place.height, format)
    }

    /// Loads the page the focus is on, if it is not loaded.
    fn show_focus(&mut self, env: &Env<'_>) {
        let rows = self.list.rows();
        let first = self.list.page() * rows;
        if first == self.first && !self.page.is_empty() {
            return;
        }
        let recent = &self.recent;
        let rank = |name: &str| Self::rank_of(recent, name);
        let page = {
            let listing = self.listing(env, &rank);
            let last = self.page.last().map(|(e, _)| e);
            let head = self.page.first().map(|(e, _)| e);
            if first == self.first + rows {
                listing.after(last, rows)
            } else if first + rows == self.first {
                listing.before(head, rows)
            } else {
                listing.page(self.list.page(), rows, self.len)
            }
        };
        self.first = first;
        self.set_page(env, page);
    }

    fn entry(&self, index: usize) -> Option<&(Entry, Option<Book>)> {
        self.page.get(index.checked_sub(self.first)?)
    }

    fn path_of(&self, entry: &Entry) -> String {
        if self.dir.is_empty() {
            entry.name.clone()
        } else {
            format!("{}/{}", self.dir, entry.name)
        }
    }

    /// Moves the focus by `delta` (`row` a row of the grid). The order at
    /// the top is a stop between the last entry and the first, so that
    /// buttons reach the menu as a tap on it does.
    fn step(&mut self, delta: isize, row: isize, cx: &mut Cx<'_, '_>) {
        let len = self.len;
        let rows = self.list.rows();
        let single = delta.abs() <= row.max(1);
        let focus = self.list.focus;
        let off_top = delta < 0 && (focus as isize) < -delta;
        let off_end = delta > 0 && focus + 1 == len;
        if self.header.note_focused {
            // Back into the list: at its first entry, or its last.
            self.header.note_focused = false;
            self.list.active = true;
            cx.request_redraw_area(self.header.area);
            if single {
                let to = if delta > 0 { 0 } else { len.saturating_sub(1) };
                let page = self.list.page();
                self.list.focus = to;
                if self.list.page() == page {
                    cx.request_redraw_area(self.list.row_rect(to % rows));
                    cx.request_refresh(RefreshMode::Fast);
                } else {
                    self.moved(ListChange::Page, cx);
                }
                return;
            }
        } else if single && self.list.shown && len > 0 && (off_top || off_end) {
            self.header.note_focused = true;
            self.list.active = false;
            cx.request_redraw_area(self.header.area);
            cx.request_redraw_area(self.list.row_rect(focus % rows));
            cx.request_refresh(RefreshMode::Fast);
            return;
        }
        let change = self.list.move_focus(delta, len);
        self.moved(change, cx);
    }

    fn moved(&mut self, change: ListChange, cx: &mut Cx<'_, '_>) {
        if change == ListChange::Page {
            let env = *cx.env();
            self.show_focus(&env);
            cx.wake_at(env.now);
        }
        // The focus changes nothing but its items: only they are drawn.
        self.list.request(change, cx);
    }

    fn open(&mut self, index: usize, cx: &mut Cx<'_, '_>) {
        let Some((entry, _)) = self.entry(index).cloned() else {
            return;
        };
        let path = self.path_of(&entry);
        if entry.is_dir {
            self.dir = path;
            let env = *cx.env();
            self.load(&env);
            cx.request_redraw();
            cx.wake_at(env.now);
        } else {
            // The covers made small go while the book is read: memory for
            // it (on the X4 a grid of them takes some 40 KB). Made again
            // from their small files when the library shows again.
            self.thumbs = Vec::new();
            cx.command(Command::OpenSlot(
                slots::READER,
                Params::new().with(params::PATH, path),
            ));
        }
    }

    /// Goes to the parent folder; `false` at the library root.
    fn up(&mut self, cx: &mut Cx<'_, '_>) -> bool {
        if self.dir == self.root || self.dir.is_empty() {
            return false;
        }
        let child = Entry::folder(self.dir.rsplit('/').next().unwrap_or_default());
        self.dir = self
            .dir
            .rsplit_once('/')
            .map_or(String::new(), |(parent, _)| parent.to_string());
        let env = *cx.env();
        self.load(&env);
        let recent = &self.recent;
        let rank = |name: &str| Self::rank_of(recent, name);
        let (first, page) = self.listing(&env, &rank).page_of(&child, self.list.rows());
        self.list.focus = (first + page.iter().position(|e| *e == child).unwrap_or(0))
            .min(self.len.saturating_sub(1));
        self.first = first;
        self.set_page(&env, page);
        cx.request_redraw();
        cx.wake_at(env.now);
        true
    }

    /// Offers what can be done with book `index`; `false` for a folder.
    fn offer(&mut self, index: usize, cx: &mut Cx<'_, '_>) -> bool {
        let Some((entry, book)) = self.entry(index).cloned() else {
            return false;
        };
        if entry.is_dir {
            return false;
        }
        let env = *cx.env();
        let path = self.path_of(&entry);
        let favourite = library::favourites(env.storage).contains(&path);
        let finished = book.as_ref().and_then(|b| b.progress) == Some(100);
        let mut options = alloc::vec![
            "open",
            if favourite {
                "unfavourite"
            } else {
                "favourite"
            },
            if finished { "unfinished" } else { "finished" },
            "info",
        ];
        // Not for books already in the archive.
        let archive = env.settings.read(&ARCHIVE).trim_matches('/');
        if !archive.is_empty() && files::parent(&path) != archive {
            options.push("archive");
        }
        if !env.device.input.touch {
            options.push("menu");
        }
        let title = book
            .and_then(|b| b.title)
            .unwrap_or_else(|| entry.name.clone());
        cx.command(quireink_sdk::ui::choice::open(
            CHOICE,
            &CHOSEN,
            &title,
            &path,
            "files.option.",
            &options,
        ));
        true
    }

    /// Does what was chosen for the book at `path`.
    fn chosen(&mut self, option: &str, path: &str, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        match option {
            "open" => cx.command(Command::OpenSlot(
                slots::READER,
                Params::new().with(params::PATH, path.to_string()),
            )),
            "favourite" | "unfavourite" => {
                let favourite = option == "favourite";
                library::set_favourite(env.storage, path, favourite);
                let key = if favourite {
                    "files.favourite-added"
                } else {
                    "files.favourite-removed"
                };
                cx.command(Command::Notify(env.i18n.tr(key).to_string()));
            }
            "finished" | "unfinished" => {
                library::set_finished(env.storage, path, option == "finished");
                self.load(&env);
                cx.request_redraw();
            }
            "archive" => {
                let archive = env
                    .settings
                    .text(ARCHIVE.as_str())
                    .trim_matches('/')
                    .to_string();
                let _ = env.storage.create_dir(&archive);
                let target = files::free_path(env.storage, &archive, files::name(path));
                match files::move_to(env.storage, path, &target) {
                    Ok(()) => {
                        cx.command(events::BOOK_MOVED.emit(events::BookMoved {
                            from: path.to_string(),
                            path: target,
                        }));
                        cx.command(Command::Notify(env.i18n.tr("files.archived").to_string()));
                    }
                    Err(_) => cx.command(Command::Notify(
                        env.i18n.tr("files.archive-failed").to_string(),
                    )),
                }
                self.load(&env);
                cx.request_redraw();
            }
            "info" => cx.command(Command::OpenScreen(
                INFO_SCREEN,
                Params::new().with(params::PATH, path.to_string()),
            )),
            "menu" => Self::open_menu(cx),
            _ => {}
        }
    }

    fn open_menu(cx: &mut Cx<'_, '_>) {
        cx.command(Command::OpenSlot(
            slots::MENU,
            Params::new().with(params::MENU, MENU.as_str()),
        ));
    }
}

/// Draws one row: a cover (or folder) on the left, then title, progress
/// and a note (author and format).
/// An entry of the page as drawn: the file or folder, its details, its
/// cover made small.
struct Shown<'a> {
    entry: &'a Entry,
    book: Option<&'a Book>,
    thumb: Option<&'a Thumbnail>,
}

/// Where a row of the list shows its cover.
fn row_thumb(style: &UiStyle, rect: Rect) -> Rect {
    let margin = style.margin as u16;
    let thumb_h = rect.height.saturating_sub(style.gap as u16 * 2);
    Rect::new(
        rect.x + margin,
        rect.y + (rect.height - thumb_h) / 2,
        thumb_h * 2 / 3,
        thumb_h,
    )
}

/// Where a cell of the grid shows its cover, above its title's lines.
fn cell_cover(env: &Env<'_>, style: &UiStyle, rect: Rect) -> Option<Rect> {
    let gap = style.gap as u16;
    let line = style.font(env)?.line_metrics(style.small).line_height as u16;
    let width = rect.width.saturating_sub(2 * gap);
    Some(Rect::new(
        rect.x + gap,
        rect.y + gap,
        width,
        rect.height.saturating_sub(2 * gap + 2 * line),
    ))
}

fn draw_row(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    style: &UiStyle,
    rect: Rect,
    focused: bool,
    shown: Shown<'_>,
) {
    let Shown {
        entry,
        book,
        thumb: small,
    } = shown;
    let fg = if focused { Color::WHITE } else { Color::BLACK };
    let thumb = row_thumb(style, rect);
    let text_x = thumb.x + thumb.width;
    let text = Rect::new(
        text_x,
        rect.y,
        rect.width.saturating_sub(text_x - rect.x),
        rect.height,
    );
    if entry.is_dir {
        draw_folder(canvas, style, thumb, fg);
        let note = env.i18n.tr("files.folder");
        ui::draw_noted_row(canvas, env, style, text, focused, &entry.name, "›", note);
        return;
    }
    // A zipped book is named by what is inside (`book.fb2.zip`: FB2).
    let name = entry.name.as_str();
    let unzipped = name
        .strip_suffix(".zip")
        .or_else(|| name.strip_suffix(".ZIP"))
        .filter(|inner| inner.contains('.'))
        .unwrap_or(name);
    let (stem, extension) = unzipped.rsplit_once('.').unwrap_or((unzipped, ""));
    let format = extension.to_uppercase();
    match (small, book.and_then(|b| b.cover.as_ref())) {
        (Some(small), _) => {
            let x = thumb.x + thumb.width.saturating_sub(small.width()) / 2;
            let y = thumb.y + thumb.height.saturating_sub(small.height()) / 2;
            small.draw(canvas, x, y);
        }
        (None, Some(cover)) => {
            let (w, h) = ui::fit_image(cover, thumb.width, thumb.height);
            let area = Rect::new(
                thumb.x + (thumb.width - w) / 2,
                thumb.y + (thumb.height - h) / 2,
                w,
                h,
            );
            ui::draw_image(canvas, env.storage, cover, area);
        }
        (None, None) => {
            let title = book.and_then(|b| b.title.as_deref()).unwrap_or(stem);
            draw_placeholder(canvas, env, style, thumb, title, &format, fg);
        }
    }
    let title = book.and_then(|b| b.title.as_deref()).unwrap_or(stem);
    let progress = match book.and_then(|b| b.progress) {
        Some(100) => env.i18n.tr("files.finished").to_string(),
        Some(p) if p > 0 => format!("{p} %"),
        _ => String::new(),
    };
    let note = match book.and_then(|b| b.author.as_deref()) {
        Some(author) => format!("{author} · {format}"),
        None => format,
    };
    ui::draw_noted_row(canvas, env, style, text, focused, title, &progress, &note);
}

/// The library's list, or its grid of covers ([`VIEW`]): as many columns
/// as covers about 3 cm wide fit, at least three.
fn list_view(env: &Env<'_>, style: &UiStyle, area: Rect) -> ListView {
    if env.settings.read(&VIEW) != VIEW_COVERS {
        return ListView::new(area, style.noted_row()).focus_on_keys(env);
    }
    // Covers line up with the margins, as text does.
    let inset = style.margin.saturating_sub(style.gap) as u16;
    let area = Rect::new(
        area.x + inset,
        area.y,
        area.width.saturating_sub(2 * inset),
        area.height,
    );
    let columns = (u32::from(area.width) / env.metrics.mm(30).max(1)).max(3);
    let cell = u32::from(area.width) / columns;
    let cover = cell.saturating_sub(2 * style.gap) * 4 / 3;
    let title = style.font(env).map_or(style.row / 2, |f| {
        f.line_metrics(style.small).line_height as u32
    });
    let row = (cover + title * 2 + style.gap * 2).min(u32::from(area.height));
    ListView::new(area, row).columns(columns).focus_on_keys(env)
}

/// One cover of the grid: the cover (or folder, or a stand-in) and the
/// title below it, shortened to its width.
fn draw_cell(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    style: &UiStyle,
    rect: Rect,
    focused: bool,
    shown: Shown<'_>,
) {
    let Shown {
        entry,
        book,
        thumb: small,
    } = shown;
    let fg = if focused { Color::WHITE } else { Color::BLACK };
    let gap = style.gap as u16;
    let (Some(font), Some(cover)) = (style.font(env), cell_cover(env, style, rect)) else {
        return;
    };
    let line = font.line_metrics(style.small).line_height as u16;
    let width = cover.width;
    let label_area =
        |n: u16| Rect::new(rect.x + gap, cover.y + cover.height + n * line, width, line);
    if entry.is_dir {
        draw_folder(canvas, style, cover, fg);
        let x = f32::from(rect.x + gap);
        ui::draw_label(
            canvas,
            &font,
            style.small,
            x,
            label_area(0),
            &entry.name,
            fg,
        );
        return;
    }
    let name = entry.name.as_str();
    let (stem, extension) = name.rsplit_once('.').unwrap_or((name, ""));
    let title = book.and_then(|b| b.title.as_deref()).unwrap_or(stem);
    match (small, book.and_then(|b| b.cover.as_ref())) {
        // On the line its title starts, at the bottom of its place.
        (Some(small), _) => {
            let y = cover.y + cover.height.saturating_sub(small.height());
            small.draw(canvas, cover.x, y);
        }
        (None, Some(image)) => {
            let (w, h) = ui::fit_image(image, cover.width, cover.height);
            let area = Rect::new(cover.x, cover.y + (cover.height - h), w, h);
            ui::draw_image(canvas, env.storage, image, area);
        }
        (None, None) => draw_placeholder(
            canvas,
            env,
            style,
            cover,
            title,
            &extension.to_uppercase(),
            fg,
        ),
    }
    // The title, then how far it is read.
    let x = f32::from(rect.x + gap);
    ui::draw_label(canvas, &font, style.small, x, label_area(0), title, fg);
    let progress = match book.and_then(|b| b.progress) {
        Some(100) => env.i18n.tr("files.finished").to_string(),
        Some(p) if p > 0 => format!("{p} %"),
        _ => book.and_then(|b| b.author.clone()).unwrap_or_default(),
    };
    let gray = if focused {
        Color::gray(215)
    } else {
        style.faint(80)
    };
    ui::draw_label(
        canvas,
        &font,
        style.small,
        x,
        label_area(1),
        &progress,
        gray,
    );
}

/// A simple folder outline with a tab.
fn draw_folder(canvas: &mut Canvas<'_>, style: &UiStyle, area: Rect, color: Color) {
    let line = style.line.max(2);
    let (x, w) = (i32::from(area.x), u32::from(area.width));
    let h = w * 3 / 4;
    let y = i32::from(area.y) + (i32::from(area.height) - h as i32) / 2;
    let tab = h / 5;
    canvas.stroke_rect(x, y + tab as i32, w, h - tab, line, color);
    canvas.fill_rect(x, y, w * 2 / 5, tab + line, color);
}

/// A book without a cover: an outline with the title's first letter,
/// large, and the format below it.
fn draw_placeholder(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    style: &UiStyle,
    area: Rect,
    title: &str,
    format: &str,
    color: Color,
) {
    canvas.stroke(area, style.line.max(2), color);
    let Some(font) = style.font(env) else {
        return;
    };
    let center = |text: &str, size: f32| {
        f32::from(area.x) + (f32::from(area.width) - font.measure(size, text)) / 2.0
    };
    let initial = title
        .chars()
        .find(|c| c.is_alphanumeric())
        .map(|c| c.to_uppercase().collect::<String>());
    let size = style.title * 1.3;
    let (upper, lower) = (area.height * 3 / 5, area.height * 2 / 5);
    match initial {
        Some(initial) if font.line_metrics(size).line_height < f32::from(upper) => {
            let top = Rect::new(area.x, area.y, area.width, upper);
            ui::draw_label(
                canvas,
                &font,
                size,
                center(&initial, size),
                top,
                &initial,
                color,
            );
            let bottom = Rect::new(area.x, area.y + upper, area.width, lower);
            ui::draw_label(
                canvas,
                &font,
                style.small,
                center(format, style.small),
                bottom,
                format,
                color,
            );
        }
        _ => {
            let x = center(format, style.small);
            ui::draw_label(canvas, &font, style.small, x, area, format, color);
        }
    }
}

impl Screen for Browser {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let len = self.len;
        let title = if self.dir == self.root {
            env.i18n.tr("files.title").to_string()
        } else {
            self.dir.rsplit('/').next().unwrap_or_default().to_string()
        };
        // The order, which the menu changes (also by tapping the header).
        let order = env.i18n.tr(env.settings.read(&SORT));
        let note = format!("{order} ›");
        self.header.draw(
            canvas,
            env,
            &style,
            &title,
            Some(&note),
            self.list.paging(len),
        );

        if self.len == 0 {
            let message = format!(
                "{}\n{}",
                env.i18n
                    .format("files.empty", &[("dir", Arg::Str(&self.dir))]),
                env.i18n.tr("files.hint")
            );
            ui::draw_message(canvas, env, &style, self.list.area, &message);
            return;
        }
        let (first, page, thumbs) = (self.first, &self.page, &self.thumbs);
        let grid = self.list.column_count() > 1;
        self.list
            .draw(canvas, &style, len, |canvas, index, rect, focused| {
                let slot = index.checked_sub(first);
                if let Some((entry, book)) = slot.and_then(|i| page.get(i)) {
                    let thumb = slot.and_then(|i| thumbs.get(i)).and_then(Option::as_ref);
                    let shown = Shown {
                        entry,
                        book: book.as_ref(),
                        thumb,
                    };
                    if grid {
                        draw_cell(canvas, env, &style, rect, focused, shown);
                    } else {
                        draw_row(canvas, env, &style, rect, focused, shown);
                    }
                }
            });
    }

    fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        if !core::mem::take(&mut self.fresh) {
            // Back from a book: its progress and place in the order changed.
            let focus = self.list.focus;
            self.load(&env);
            self.list.focus = focus.min(self.len.saturating_sub(1));
            self.show_focus(&env);
            cx.request_redraw();
        }
        cx.wake_at(env.now);
    }

    /// Reads the details of one visible book that has none yet.
    fn on_timer(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let rows = self.list.rows();
        let pending = self
            .page
            .iter()
            .position(|(entry, book)| !entry.is_dir && !book.as_ref().is_some_and(|b| b.indexed));
        let Some(slot) = pending else {
            return;
        };
        let path = self.path_of(&self.page[slot].0);
        self.page[slot].1 = Some(library::index(&env, &path));
        if let Some(thumb) = self.thumbs.get_mut(slot) {
            *thumb = None;
        }
        let made = self.thumb(&env, slot);
        if let Some(thumb) = self.thumbs.get_mut(slot) {
            *thumb = made;
        }
        let index = self.first + slot;
        if index / rows == self.list.page() {
            cx.request_redraw_area(self.list.row_rect(index % rows));
            cx.request_refresh(RefreshMode::Partial);
        }
        cx.wake_at(env.now.add_millis(STEP_DELAY_MS));
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let rows = self.list.rows() as isize;
        let a = action;
        // In a grid, up and down go a row of covers.
        let row = self.list.column_count() as isize;
        let delta = if *a == core_actions::UP {
            Some(-row)
        } else if *a == core_actions::DOWN {
            Some(row)
        } else if *a == core_actions::LEFT || *a == core_actions::PREVIOUS {
            Some(-1)
        } else if *a == core_actions::RIGHT || *a == core_actions::NEXT {
            Some(1)
        } else if *a == core_actions::NEXT_PAGE {
            Some(rows)
        } else if *a == core_actions::PREV_PAGE {
            Some(-rows)
        } else {
            None
        };
        if let Some(delta) = delta {
            self.step(delta, row, cx);
        } else if *a == core_actions::CONFIRM && self.header.note_focused {
            Self::open_menu(cx);
        } else if *a == core_actions::CONFIRM {
            self.open(self.list.focus, cx);
        } else if *a == core_actions::MENU {
            // Without touch, a book's options (holding it needs a finger);
            // they lead on to the library's menu.
            if cx.env().device.input.touch || !self.offer(self.list.focus, cx) {
                Self::open_menu(cx);
            }
        } else if *a == core_actions::BACK {
            return if self.up(cx) {
                Handled::Yes
            } else {
                Handled::No
            };
        } else {
            return Handled::No;
        }
        Handled::Yes
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        if gesture.kind == GestureKind::LongPress
            && let Some(index) = self.list.hit(gesture.start, self.len)
        {
            self.list.focus = index;
            return if self.offer(index, cx) {
                Handled::Yes
            } else {
                Handled::No
            };
        }
        if gesture.kind != GestureKind::Tap {
            return Handled::No;
        }
        let paging = self.list.paging(self.len);
        let tapped = self
            .header
            .tap(cx.env(), &self.style, gesture.start, paging);
        if let Some(action) = tapped {
            cx.command(Command::RunAction(action));
            return Handled::Yes;
        }
        if self.header.area.contains(gesture.start) {
            Self::open_menu(cx);
            return Handled::Yes;
        }
        match self.list.hit(gesture.start, self.len) {
            Some(index) => {
                self.list.focus = index;
                self.open(index, cx);
                Handled::Yes
            }
            None => Handled::No,
        }
    }

    fn on_event(&mut self, event: &Id, params: &Params, cx: &mut Cx<'_, '_>) {
        if let Some(picked) = CHOSEN.read(event, params) {
            self.chosen(&picked.option, &picked.path, cx);
            return;
        }
        if *event == events::LIBRARY_CHANGED {
            // Books came or went: shown as they are now.
            let env = *cx.env();
            let focus = self.list.focus;
            self.load(&env);
            self.list.focus = focus.min(self.len.saturating_sub(1));
            self.show_focus(&env);
            cx.request_redraw();
            cx.request_refresh(RefreshMode::Partial);
            return;
        }
        if *event == events::SETTING_CHANGED {
            // The UI size, the order or the library folder may have
            // changed; a new library folder is shown from its top.
            let env = *cx.env();
            let root = env.settings.read(&ROOT).trim_matches('/');
            let dir = if root == self.root {
                self.dir.clone()
            } else {
                root.to_string()
            };
            let focus = self.list.focus;
            *self = Self::new(&env, &Params::new().with(params::PATH, dir));
            self.fresh = false;
            self.list.focus = focus.min(self.len.saturating_sub(1));
            self.show_focus(&env);
            cx.request_redraw();
            cx.wake_at(env.now);
        }
    }
}

#[cfg(test)]
mod tests {
    use quireink_i18n::{Catalog, check};

    #[test]
    fn translations_are_complete() {
        let en = Catalog::parse(include_str!("../i18n/en.ini")).unwrap();
        let de = Catalog::parse(include_str!("../i18n/de.ini")).unwrap();
        let issues = check::compare(&en, &de);
        assert!(issues.is_empty(), "{issues:?}");
    }
}
