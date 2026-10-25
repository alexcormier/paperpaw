//! The library: the books under the library folder, as a calm list.
//!
//! It opens with the book read last as a card, so going on reading is one
//! confirm or one tap; below it every book with its cover, title, author
//! and how far it is read. What is shown (all books, those being read,
//! unread or finished ones, the shelves) is chosen above the books: from
//! one row on small screens, from chips on larger ones. Series, folders
//! (collections), favourites, authors and tags open as lists of their
//! own; "Find" picks a book by a letter without a keyboard. See `list`.
//!
//! The library folder is a setting; its editor (`files.choose-folder`)
//! browses the device's folders, so no path has to be typed.
//!
//! Works with buttons alone and with touch alike. Bound to
//! `core.library`, and to `core.home` only as a fallback when no home
//! screen plugin is installed.

// What goes wrong with the user's data is told, never dropped unseen:
// a result left unread says why (see CONTRIBUTING.md).
#![deny(clippy::let_underscore_must_use)]
#![no_std]

extern crate alloc;

mod folders;
mod info;
mod list;
mod shelves;

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::string::{String, ToString};

use paperpaw_sdk::ask;
use paperpaw_sdk::builtin::{actions as core_actions, events, groups, params, slots};
use paperpaw_sdk::event::Event;
use paperpaw_sdk::files;
use paperpaw_sdk::replies::Typed;
use paperpaw_sdk::setting::{Setting, Text};
use paperpaw_sdk::ui::{self, UiStyle};
use paperpaw_sdk::{
    Color, Command, Commands, Cx, Env, Id, Manifest, MenuEntry, Params, Plugin, Priority, Rect,
    RegisterError, Registrar, ScreenSpec, SettingSpec,
};

pub const PLUGIN: Id = Id::new_static("files");
pub const SCREEN: Id = Id::new_static("files.browser");
/// The library folder, relative to the storage root.
pub const ROOT: Setting<Text> = paperpaw_sdk::library::FOLDER;
/// The order of books: [`SORT_RECENT`] or [`SORT_NAME`].
pub const SORT: Setting<Text> = Setting::new("files.sort");
/// Recently read books first, then the others by title.
pub const SORT_RECENT: &str = "files.sort.recent";
/// By title.
pub const SORT_NAME: &str = "files.sort.name";
/// The folder finished books are moved to (by the book's options).
pub const ARCHIVE: Setting<Text> = Setting::new("files.archive");
/// How books are shown: as a list ([`VIEW_LIST`]) or a grid of covers
/// ([`VIEW_COVERS`]).
pub const VIEW: Setting<Text> = Setting::new("files.view");
pub const VIEW_LIST: &str = "files.view.list";
pub const VIEW_COVERS: &str = "files.view.covers";
/// How the library's covers show: in black and white ([`COVERS_MONO`]),
/// which a focus moves over fast, or in grays ([`COVERS_GRAY`]), finer
/// where the panel shows grays with a refresh of its own (the X4s: the
/// first step after such a page refreshes it whole).
pub const COVERS: Setting<Text> = Setting::new("files.covers");
pub const COVERS_MONO: &str = "files.covers.mono";
pub const COVERS_GRAY: &str = "files.covers.gray";

/// Whether the library's covers show in grays ([`COVERS`]).
pub(crate) fn covers_in_grays(env: &Env<'_>) -> bool {
    env.settings().read(&COVERS) == COVERS_GRAY
}
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
/// Book information; parameter `path`.
pub const INFO_SCREEN: Id = Id::new_static("files.info");
/// The submenu with the views above ("Shelves").
pub const SHELVES_MENU: Id = Id::new_static("files.shelves");
const SHELF_SCREEN: Id = Id::new_static("files.shelf");

/// The library plugin.
#[derive(Debug, Default)]
pub struct Files;

/// Folders chosen with the folder chooser, whichever plugin fills it.
static FOLDER: paperpaw_sdk::SettingExtra =
    paperpaw_sdk::SettingExtra::new().editor(paperpaw_sdk::builtin::screens::FOLDER_EDITOR);

/// Where the archive is, and how the library shows the books (the
/// library folder is the kernel's, [`ROOT`]).
static SETTINGS: [SettingSpec; 4] = [
    SettingSpec::text(
        ARCHIVE.static_str(),
        "files.setting.archive",
        "books/Archive",
        groups::LIBRARY,
    )
    .with_extra(&FOLDER)
    .kept_on_reset()
    .advanced()
    .description("files.setting.archive.help"),
    SettingSpec::choice(
        VIEW.static_str(),
        "files.setting.view",
        &[VIEW_LIST, VIEW_COVERS],
        VIEW_LIST,
        groups::LIBRARY,
    )
    .description("files.setting.view.help"),
    SettingSpec::choice(
        COVERS.static_str(),
        "files.setting.covers",
        &[COVERS_MONO, COVERS_GRAY],
        COVERS_MONO,
        groups::LIBRARY,
    )
    .description("files.setting.covers.help"),
    SettingSpec::choice(
        SORT.static_str(),
        "files.setting.sort",
        &[SORT_RECENT, SORT_NAME],
        SORT_RECENT,
        groups::LIBRARY,
    )
    .description("files.setting.sort.help"),
];

/// How it works, by language (see `Manifest::how`).
static HOW: [(&str, &str); 2] = [
    ("en", include_str!("../how/en.txt")),
    ("de", include_str!("../how/de.txt")),
];

impl Plugin for Files {
    fn manifest(&self) -> Manifest {
        Manifest::new(PLUGIN, "File browser", paperpaw_sdk::package_version!())
            .description("Lists the books and folders on the device and opens them.")
            .how(&HOW)
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    #[expect(
        clippy::too_many_lines,
        reason = "longer than the limit from before it was set; split it when it is next changed"
    )]
    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_screen(ScreenSpec::new(
            SCREEN,
            "files.title",
            Box::new(list::screen),
        ))?;
        list::register(r)?;
        r.add_screen(ScreenSpec::new(
            CHOOSE_FOLDER,
            "files.choose.title",
            Box::new(|env, params| Box::new(folders::FolderChooser::new(env, params))),
        ))?;
        r.add_screen(ScreenSpec::new(
            INFO_SCREEN,
            "files.info",
            Box::new(|env, params| Box::new(info::InfoScreen::new(env, params))),
        ))?;
        r.add_screen(ScreenSpec::new(
            SHELF_SCREEN,
            "files.title",
            Box::new(|env, params| Box::new(shelves::ShelfScreen::new(env, params))),
        ))?;
        for (action, mode, title, icon) in [
            (BY_AUTHOR, shelves::MODE_AUTHORS, "files.by-author", "user"),
            (
                BY_SERIES,
                shelves::MODE_SERIES,
                "files.by-series",
                "list-details",
            ),
            (BY_TAG, shelves::MODE_TAGS, "files.by-tag", "tag"),
        ] {
            r.add_action(
                paperpaw_sdk::ActionSpec::new(action.clone(), title).handler(
                    move |_: &Env<'_>, _, out: &mut Commands| {
                        out.push(Command::OpenScreen(
                            SHELF_SCREEN,
                            Params::new().with(params::MODE, mode),
                        ));
                    },
                ),
            )?;
            r.add_icon(action, Cow::Borrowed(icon))?;
        }
        r.add_action(
            paperpaw_sdk::ActionSpec::new(FAVOURITES, "files.favourites").handler(
                |_: &Env<'_>, _, out: &mut Commands| {
                    out.push(list::shelf::open(list::shelf::Kind::Favourites, ""));
                },
            ),
        )?;
        r.add_icon(FAVOURITES, Cow::Borrowed("star"))?;
        r.bind_slot(slots::CHOOSE_FOLDER, CHOOSE_FOLDER, Priority::DEFAULT)?;
        r.bind_slot(slots::BOOK_INFO, INFO_SCREEN, Priority::DEFAULT)?;
        r.bind_slot(slots::HOME, SCREEN, Priority::FALLBACK)?;
        r.bind_slot(slots::LIBRARY, SCREEN, Priority::DEFAULT)?;
        paperpaw_sdk::help::add(
            r,
            SCREEN.as_str(),
            &[
                paperpaw_sdk::help::HelpHint::touch("files.help.open"),
                paperpaw_sdk::help::HelpHint::touch("files.help.hold"),
                paperpaw_sdk::help::HelpHint::touch("files.help.pages"),
                paperpaw_sdk::help::HelpHint::buttons("files.help.choose"),
                paperpaw_sdk::help::HelpHint::buttons("files.help.more"),
            ],
        )?;
        r.add_settings(&SETTINGS)?;
        // Gray covers or black and white only where the panel is black and
        // white; a gray one shows them in grays as fast.
        r.set_menu_need(COVERS.id().clone(), paperpaw_sdk::MenuNeed::BlackAndWhite)?;
        r.add_action(
            paperpaw_sdk::ActionSpec::new(SEARCH, "files.search").handler(
                |env: &Env<'_>, _, out: &mut Commands| {
                    out.push(
                        ask::text(env.i18n().tr("files.search.prompt"), &SEARCH_ENTERED).command(),
                    );
                },
            ),
        )?;
        r.add_icon(SEARCH, Cow::Borrowed("search"))?;
        // The menu's settings, with icons as its other rows.
        for (setting, icon) in [
            (Id::from(SORT), "sort-ascending"),
            (VIEW.into(), "layout-grid"),
            (ROOT.into(), "folder"),
        ] {
            r.add_icon(setting, Cow::Borrowed(icon))?;
        }
        r.on(SEARCH_ENTERED, |_, typed, out| {
            let text = typed.text.trim();
            if !text.is_empty() {
                out.push(list::shelf::open(list::shelf::Kind::Search, text));
            }
        })?;
        // The shelves as a submenu: an action named like its menu.
        r.add_action(
            paperpaw_sdk::ActionSpec::new(SHELVES_MENU, "files.shelves").handler(
                |_: &Env<'_>, _, out: &mut Commands| {
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
            r.add_menu_entry(MenuEntry::new(SHELVES_MENU, item).order(order))?;
        }
        // Only what is about the library: night mode and home are the
        // device's (in the main menu and the quick panel).
        for (item, order) in [
            (Id::from(SORT), 10),
            (VIEW.into(), 11),
            (list::FIND, 20),
            (SEARCH, 21),
            (SHELVES_MENU, 30),
            (ROOT.into(), 80),
            (core_actions::SETTINGS, 90),
        ] {
            r.add_menu_entry(MenuEntry::new(MENU, item).order(order))?;
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

/// The corners of covers, `area` the place of one: the style's, but as
/// small as a book's, so a narrow cover stays a box.
pub(crate) fn cover_radius(style: &UiStyle, area: Rect) -> u32 {
    u32::from(style.corner()).min(u32::from(area.width.min(area.height)) / 8)
}

/// The corners of covers' pictures: as [`cover_radius`] where corners
/// are round (at least the gap), square for smaller ones, as before.
pub(crate) fn picture_radius(style: &UiStyle, area: Rect) -> u32 {
    if u32::from(style.corner()) >= u32::from(style.gap()) {
        cover_radius(style, area)
    } else {
        0
    }
}

/// A book's file name without its extension, and its format as the
/// extension says: a zipped book by what is inside (`book.fb2.zip` is
/// `book` in FB2).
pub(crate) fn stem_and_format(name: &str) -> (&str, String) {
    let unzipped = match name.len().checked_sub(4).and_then(|at| name.get(at..)) {
        Some(zip) if zip.eq_ignore_ascii_case(".zip") => {
            let inner = &name[..name.len() - 4];
            if inner.contains('.') { inner } else { name }
        }
        _ => name,
    };
    let (stem, extension) = unzipped.rsplit_once('.').unwrap_or((unzipped, ""));
    (stem, extension.to_uppercase())
}

/// A book without a cover: an outline with the title's first letter,
/// large, and the format below it.
pub(crate) fn draw_placeholder(
    ui: &mut ui::Ui<'_>,
    area: Rect,
    title: &str,
    format: &str,
    color: Color,
) {
    let style = ui.style();
    let radius = cover_radius(style, area);
    ui.canvas()
        .stroke_round(area, ui::button_border(style), radius, color);
    let Some(font) = ui.font() else {
        return;
    };
    let center = |text: &str, size: f32| {
        f32::from(area.x) + (f32::from(area.width) - font.measure(size, text)) / 2.0
    };
    // The first letter in capitals, written on the stack: a letter's
    // capital is at most three letters of four bytes.
    let mut letters = [0u8; 12];
    let initial = title.chars().find(|c| c.is_alphanumeric()).map(|c| {
        let mut len = 0;
        for upper in c.to_uppercase() {
            if len + upper.len_utf8() <= letters.len() {
                len += upper.encode_utf8(&mut letters[len..]).len();
            }
        }
        core::str::from_utf8(&letters[..len]).unwrap_or_default()
    });
    let (upper, lower) = (area.height * 3 / 5, area.height * 2 / 5);
    // As large as titles and a bit, or as fits in the upper part.
    let mut size = style.title() * 1.3;
    let height = font.line_metrics(size).line_height;
    if height > f32::from(upper) {
        size *= f32::from(upper) / height;
    }
    match initial {
        Some(initial) if size >= style.text() => {
            let top = Rect::new(area.x, area.y, area.width, upper);
            ui::Label::new(initial)
                .px(size)
                .color(color)
                .draw_from(ui, center(initial, size), top);
            let bottom = Rect::new(area.x, area.y + upper, area.width, lower);
            ui::Label::new(format)
                .px(style.small())
                .color(color)
                .draw_from(ui, center(format, style.small()), bottom);
        }
        _ => {
            let x = center(format, style.small());
            ui::Label::new(format)
                .px(style.small())
                .color(color)
                .draw_from(ui, x, area);
        }
    }
}

/// Whether the book at `path` may move to the archive: there is one, and
/// the book is not in it.
pub(crate) fn can_archive(env: &Env<'_>, path: &str) -> bool {
    let archive = env.settings().read(&ARCHIVE).trim_matches('/');
    !archive.is_empty() && !files::same_path(env.storage(), files::parent(path), archive)
}

/// Moves the book at `path` to the archive folder, and says how it went.
pub(crate) fn archive(env: &Env<'_>, path: &str, cx: &mut Cx<'_, '_>) {
    let archive = env
        .settings()
        .text(ARCHIVE.as_str())
        .trim_matches('/')
        .to_string();
    #[expect(
        clippy::let_underscore_must_use,
        reason = "made where missing; moving the book into it tells if it is not there"
    )]
    let _ = env.storage().create_dir(&archive);
    let target = files::free_path(env.storage(), &archive, files::name(path));
    match files::move_to(env.storage(), path, &target) {
        Ok(()) => {
            cx.command(events::BOOK_MOVED.emit(events::BookMoved::new(path.to_string(), target)));
            cx.command(Command::Notify(env.i18n().tr("files.archived").to_string()));
        }
        Err(_) => cx.command(Command::Notify(
            env.i18n().tr("files.archive-failed").to_string(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use paperpaw_i18n::{Catalog, check};

    #[test]
    fn translations_are_complete() {
        let en = Catalog::parse(include_str!("../i18n/en.ini")).unwrap();
        let de = Catalog::parse(include_str!("../i18n/de.ini")).unwrap();
        let issues = check::compare(&en, &de);
        assert!(issues.is_empty(), "{issues:?}");
    }
}
