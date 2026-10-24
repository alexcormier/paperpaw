//! The paged reader.
//!
//! - Shows any document a format plugin can open, page by page, in the
//!   reading font role, with configurable size, spacing, margins,
//!   justification and paragraph indents.
//! - Saves the reading position per book and reopens the last book at
//!   start-up (setting `reader.open-last`).
//! - Builds a page index in the background, so page numbers and total pages
//!   become available without slowing down page turns.
//! - Follows links: a tap, or with buttons *Links* in the menu, which
//!   moves from link to link (on over the pages) until one is confirmed.
//!   *Back* returns to where a link was followed from.
//! - Emits the standard events `core.book-opened` and `core.page-turned`,
//!   so other plugins (home screen, statistics, sync, …) follow the reading.
//! - Offers what every reader offers (see `paperpaw_sdk::builtin`): its
//!   menus (`menus::READER`, `READER_MORE`, `READER_PANEL`, `SELECTION`),
//!   the events `core.goto`, `core.text-chosen` and `core.notes-exported`,
//!   the font size `core.font-size`, the status lines' group, key bindings
//!   on the slot's layer (`[keymap.core.reader]`). Other plugins know no
//!   more of it.
//!
//! It is bound to the `core.reader` slot; any other plugin can replace it.

// What goes wrong with the user's data is told, never dropped unseen:
// a result left unread says why (see CONTRIBUTING.md).
#![deny(clippy::let_underscore_must_use)]
#![no_std]

extern crate alloc;

mod aloud;
mod annotations;
mod footnote;
mod screen;
mod search;
mod selection;
mod skim;
mod status;
mod toc;

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::string::String;

use paperpaw_sdk::builtin::{
    actions as core_actions, events, groups, menus, params, settings as core_settings, slots,
};
use paperpaw_sdk::event::Event;
use paperpaw_sdk::help::{self, Zone};
use paperpaw_sdk::replies::Typed;
use paperpaw_sdk::{
    Command, Commands, Env, Id, Manifest, Params, Plugin, Priority, RegisterError, Registrar,
    ScreenSpec, SettingSpec,
};

pub const PLUGIN: Id = Id::new_static("reader");
pub const SCREEN: Id = Id::new_static("reader.screen");
/// The reader as a reading app (see `paperpaw_sdk::readers`); its name is
/// the translation of its id.
pub const APP: Id = Id::new_static("reader.app");
/// The table of contents screen; parameters `path`, `section`, `offset`.
pub const TOC_SCREEN: Id = Id::new_static("reader.toc");
/// The bookmarks and highlights of a book; parameter `path`.
pub const ANNOTATIONS_SCREEN: Id = Id::new_static("reader.annotations");
/// The actions offered for selected text, as a menu: plugins add their own
/// with `Registrar::add_menu_entry`, users reorder them in `[menus]`. The
/// standard one, so plugins need not know this reader.
pub const SELECTION_MENU: Id = menus::SELECTION;
/// Search results for a book; parameters `path` and `text`.
pub const SEARCH_SCREEN: Id = Id::new_static("reader.search-results");
/// The keyboard's replies: what to search for, which page to go to.
const SEARCH_ENTERED: Event<Typed> = Event::new("reader.search-entered");
const PAGE_ENTERED: Event<Typed> = Event::new("reader.page-entered");
const PERCENT_ENTERED: Event<Typed> = Event::new("reader.percent-entered");
/// The keyboard's reply with a note on a highlight.
const NOTE_ENTERED: Event<Typed> = Event::new("reader.note-entered");
/// Font and layout, shown as a panel ([`actions::QUICK_SETTINGS`]), and
/// the submenu "More" of [`MENU`]: menus like any other, so plugins add
/// entries and users redefine them in `[menus]` or the menu editor. The
/// standard ones.
pub const LAYOUT_MENU: Id = menus::READER_PANEL;
pub const MORE_MENU: Id = menus::READER_MORE;
/// The reply when a page has been read aloud; parameter `path`.
const SPOKEN: paperpaw_sdk::event::Event<paperpaw_sdk::replies::Spoken> =
    paperpaw_sdk::event::Event::new("reader.spoken");
/// Skimming through the book, an overlay (see `skim`); it asks the
/// reader to move with `reader.skim` and hears where the book is with
/// `reader.skimmed`.
pub const SKIM_SCREEN: Id = Id::new_static("reader.skim-screen");
/// A footnote over the page (see `footnote`).
pub const FOOTNOTE_SCREEN: Id = Id::new_static("reader.footnote");
/// A sample of text set as the reading settings say, the preview above
/// them on their settings page (a widget, see `screen::Sample`).
pub const SAMPLE: Id = Id::new_static("reader.sample");

/// Settings of the reader.
pub mod settings {

    use paperpaw_sdk::setting::{Setting, Text};

    /// The standard type of book pages, registered by the kernel (each
    /// book may have its own).
    pub use paperpaw_sdk::builtin::settings::{
        FONT_SIZE, FONT_WEIGHT, FOOTNOTES, FOOTNOTES_JUMP, FOOTNOTES_POPUP, HYPHENATE, INDENT,
        JUSTIFY, LINE_SPACING, MARGIN, MARGIN_ENDS, PARAGRAPH_SPACING, PUBLISHER_STYLES,
        WORD_SPACING,
    };
    /// Book pages with smooth letters, their edges in grays, where the
    /// panel shows black and white but can show grays with a refresh of
    /// its own (the X4): slower and costing more battery.
    pub const SMOOTH_TEXT: Setting<bool> = Setting::new("reader.smooth-text");
    /// Reopen the last book at start-up.
    pub const OPEN_LAST: Setting<bool> = Setting::new("reader.open-last");
    /// A tap on the top right corner sets or removes a bookmark.
    pub const CORNER_BOOKMARK: Setting<bool> = Setting::new("reader.corner-bookmark");
    /// Holding a page button goes a chapter on or back; off, the page
    /// buttons turn pages as they are pressed rather than let go.
    pub const HOLD_CHAPTERS: Setting<bool> = Setting::new("reader.hold-chapters");
    /// How fast books are read aloud, in percent of the voice's speed.
    pub const ALOUD_SPEED: Setting<i64> = Setting::new("reader.aloud-speed");
    /// The sentence read aloud is underlined.
    pub const ALOUD_HIGHLIGHT: Setting<bool> = Setting::new("reader.aloud-highlight");
    /// Reading aloud turns the pages; else it stops at the page's end.
    pub const ALOUD_TURN: Setting<bool> = Setting::new("reader.aloud-turn-pages");
    /// Reading aloud stops by itself after a while (a sleep timer).
    pub const ALOUD_TIMER: Setting<Text> = Setting::new("reader.aloud-timer");
    /// Seconds between pages turned by themselves
    /// ([`actions::AUTO_TURN`](super::actions::AUTO_TURN)).
    pub const AUTO_TURN_SECONDS: Setting<i64> = Setting::new("reader.auto-turn-seconds");
}

/// The reader's menu (`core.menu` on the reader screen). Any plugin can
/// add entries; users can redefine it in `[menus]`. The standard one.
pub const MENU: Id = menus::READER;

/// Actions of the reader.
pub mod actions {
    use paperpaw_sdk::Id;

    pub const FONT_LARGER: Id = Id::new_static("reader.font-larger");
    pub const FONT_SMALLER: Id = Id::new_static("reader.font-smaller");
    pub const TOGGLE_JUSTIFY: Id = Id::new_static("reader.toggle-justify");
    /// Opens the table of contents.
    pub const TOC: Id = Id::new_static("reader.show-toc");
    /// Selects text with the buttons to look it up or highlight it
    /// (touch: long-press a word).
    pub const SELECT: Id = Id::new_static("reader.select-text");
    /// Chooses a link on the page with the buttons and follows it
    /// (touch: tap the link).
    pub const LINKS: Id = Id::new_static("reader.links");
    /// Shows the next of the status lines below the text (above, if there
    /// is none below), as a tap on it does.
    pub const NEXT_STATUS_LINE: Id = Id::new_static("reader.next-status-line");
    /// Shows the next of the status lines above the text.
    pub const NEXT_TOP_STATUS_LINE: Id = Id::new_static("reader.next-top-status-line");
    /// Sets or removes the bookmark of the page.
    pub const TOGGLE_BOOKMARK: Id = Id::new_static("reader.toggle-bookmark");
    /// Lists the book's bookmarks and highlights.
    pub const ANNOTATIONS: Id = Id::new_static("reader.show-annotations");
    /// Searches the book (asks for the text on the keyboard).
    pub const SEARCH: Id = Id::new_static("reader.search");
    /// Opens font and layout as a panel of controls.
    pub const QUICK_SETTINGS: Id = Id::new_static("reader.quick-settings");
    /// Goes to a page number (asks for it on the keyboard).
    pub const GO_TO_PAGE: Id = Id::new_static("reader.go-to-page");
    /// Goes to a percentage of the book (asks for it on the keyboard).
    pub const GO_TO_PERCENT: Id = Id::new_static("reader.go-to-percent");
    /// Reads the book aloud from the page shown, turning the pages, until
    /// a tap or a button (where the device can speak).
    pub const READ_ALOUD: Id = Id::new_static("reader.read-aloud");
    /// Items of the selection menu that the reader handles itself.
    pub const LOOK_UP: Id = Id::new_static("reader.selection.look-up");
    /// Highlights the selected text, or removes highlights in it.
    pub const HIGHLIGHT: Id = Id::new_static("reader.selection.highlight");
    /// Skims through the book: a bar with its chapters, chapter by
    /// chapter, back to where one was (an overlay at the bottom).
    pub const SKIM_BOOK: Id = Id::new_static("reader.skim-book");
    /// Turns the pages by themselves every few seconds, or stops it.
    pub const AUTO_TURN: Id = Id::new_static("reader.auto-turn");
    /// Goes to the start of the next chapter.
    pub const NEXT_CHAPTER: Id = Id::new_static("reader.next-chapter");
    /// Goes to the start of this chapter, or of the one before where the
    /// page is its first.
    pub const PREV_CHAPTER: Id = Id::new_static("reader.prev-chapter");
    /// Shows the book's information (slot `core.book-info`).
    pub const BOOK_INFO: Id = Id::new_static("reader.book-info");
    /// A note on the selected text (highlighted with it).
    pub const NOTE: Id = Id::new_static("reader.selection.note");
    /// Reads aloud from the selected text on.
    pub const READ_FROM: Id = Id::new_static("reader.selection.read-aloud");
    /// Copies the selected text to the clipboard.
    pub const COPY: Id = Id::new_static("reader.selection.copy");
    /// Translates the selected text (slot `core.translate`), where a
    /// translator is installed and there is a network.
    pub const TRANSLATE: Id = Id::new_static("reader.selection.translate");
    /// Moves the end of the selection with the buttons.
    pub const EXTEND: Id = Id::new_static("reader.selection.extend");
    pub const CANCEL: Id = Id::new_static("reader.selection.cancel");
    /// Shows all actions for the selection as a list, when the bar is too
    /// small for them.
    pub const MORE: Id = Id::new_static("reader.selection.more");
}

/// The reader plugin.
#[derive(Debug, Default)]
pub struct Reader;

/// The parts of the reading settings, with units and steps; those set by
/// eye with sliders.
mod extras {
    use paperpaw_sdk::SettingExtra;
    use paperpaw_sdk::builtin::group_sections::{ALOUD, BEHAVIOUR, FONT};

    pub(crate) static IN_FONT: SettingExtra = SettingExtra::new().section(FONT);
    pub(crate) static IN_BEHAVIOUR: SettingExtra = SettingExtra::new().section(BEHAVIOUR);
    pub(crate) static AUTO_TURN: SettingExtra = SettingExtra::new()
        .unit("core.unit.seconds")
        .step(5)
        .section(BEHAVIOUR);
    pub(crate) static ALOUD_SPEED: SettingExtra = SettingExtra::new()
        .unit("core.unit.percent")
        .step(10)
        .section(ALOUD)
        .slider();
    pub(crate) static IN_ALOUD: SettingExtra = SettingExtra::new().section(ALOUD);
}

/// The reader's own settings: smooth letters, what it does, and reading
/// aloud (the type of pages is the kernel's, see `settings`).
static SETTINGS: [SettingSpec; 9] = [
    SettingSpec::switch(
        settings::SMOOTH_TEXT.static_str(),
        "reader.setting.smooth-text",
        false,
        groups::READING,
    )
    .with_extra(&extras::IN_FONT)
    .description("reader.setting.smooth-text.help"),
    SettingSpec::switch(
        settings::OPEN_LAST.static_str(),
        "reader.setting.open-last",
        true,
        groups::READING,
    )
    .advanced()
    .with_extra(&extras::IN_BEHAVIOUR)
    .description("reader.setting.open-last.help"),
    SettingSpec::switch(
        settings::CORNER_BOOKMARK.static_str(),
        "reader.setting.corner-bookmark",
        true,
        groups::CONTROLS,
    )
    .advanced()
    .description("reader.setting.corner-bookmark.help"),
    SettingSpec::switch(
        settings::HOLD_CHAPTERS.static_str(),
        "reader.setting.hold-chapters",
        true,
        groups::CONTROLS,
    )
    .advanced()
    .description("reader.setting.hold-chapters.help"),
    SettingSpec::number(
        settings::AUTO_TURN_SECONDS.static_str(),
        "reader.setting.auto-turn-seconds",
        5,
        600,
        30,
        groups::READING,
    )
    .advanced()
    .with_extra(&extras::AUTO_TURN)
    .description("reader.setting.auto-turn-seconds.help"),
    SettingSpec::number(
        settings::ALOUD_SPEED.static_str(),
        "reader.setting.aloud-speed",
        50,
        300,
        100,
        groups::READING,
    )
    .with_extra(&extras::ALOUD_SPEED)
    .description("reader.setting.aloud-speed.help"),
    SettingSpec::switch(
        settings::ALOUD_HIGHLIGHT.static_str(),
        "reader.setting.aloud-highlight",
        true,
        groups::READING,
    )
    .with_extra(&extras::IN_ALOUD)
    .description("reader.setting.aloud-highlight.help"),
    SettingSpec::switch(
        settings::ALOUD_TURN.static_str(),
        "reader.setting.aloud-turn-pages",
        true,
        groups::READING,
    )
    .with_extra(&extras::IN_ALOUD)
    .description("reader.setting.aloud-turn-pages.help"),
    SettingSpec::choice(
        settings::ALOUD_TIMER.static_str(),
        "reader.setting.aloud-timer",
        &aloud::TIMER_CHOICES,
        aloud::TIMER_CHOICES[0],
        groups::READING,
    )
    .with_extra(&extras::IN_ALOUD)
    .description("reader.setting.aloud-timer.help"),
];

/// How it works, by language (see `Manifest::how`).
static HOW: [(&str, &str); 2] = [
    ("en", include_str!("../how/en.txt")),
    ("de", include_str!("../how/de.txt")),
];

impl Plugin for Reader {
    fn manifest(&self) -> Manifest {
        Manifest::new(PLUGIN, "Reader", paperpaw_sdk::package_version!())
            .description("Shows books page by page, with table of contents and reading settings.")
            .how(&HOW)
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    #[expect(
        clippy::too_many_lines,
        reason = "longer than the limit from before it was set; split it when it is next changed"
    )]
    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        // Each book's position, page index and own settings, moved and
        // removed with it.
        r.add_book_data(
            alloc::format!("{}/books", Env::plugin_dir(PLUGIN.as_str())),
            "",
        )?;
        // The page takes all of the screen; the buttons' row comes on a press.
        r.set_screen_hints(SCREEN, paperpaw_sdk::HintVisibility::OnPress)?;
        r.add_screen(ScreenSpec::new(
            SCREEN,
            "reader.title",
            Box::new(|env, params| Box::new(screen::ReaderScreen::open(env, params))),
        ))?;
        r.bind_slot(slots::READER, SCREEN, Priority::DEFAULT)?;
        // The first time: the tap zones (as the standard key map has them),
        // or the buttons; once the book shows (see `Screen::on_show`).
        help::wait(r, SCREEN.as_str())?;
        help::add(
            r,
            SCREEN.as_str(),
            &[
                help::HelpHint::touch("reader.help.previous").zone(Zone::new(0, 120, 333, 760)),
                help::HelpHint::touch("reader.help.menu").zone(Zone::new(333, 120, 334, 760)),
                help::HelpHint::touch("reader.help.next").zone(Zone::new(667, 120, 333, 760)),
                help::HelpHint::touch("reader.help.quick"),
                help::HelpHint::touch("reader.help.night"),
                help::HelpHint::buttons("reader.help.turn"),
                help::HelpHint::buttons("reader.help.select"),
                help::HelpHint::buttons("reader.help.back"),
                help::HelpHint::buttons("reader.help.panel"),
            ],
        )?;
        r.add_reader(paperpaw_sdk::readers::ReaderSpec::new(APP, SCREEN))?;
        r.add_screen(ScreenSpec::new(
            TOC_SCREEN,
            "reader.toc",
            Box::new(|env, params| Box::new(toc::TocScreen::open(env, params))),
        ))?;
        // Its other screens fill the standard slots, which others may.
        r.bind_slot(slots::TOC, TOC_SCREEN, Priority::DEFAULT)?;
        r.bind_slot(slots::ANNOTATIONS, ANNOTATIONS_SCREEN, Priority::DEFAULT)?;
        r.bind_slot(slots::FOOTNOTE, FOOTNOTE_SCREEN, Priority::DEFAULT)?;
        r.bind_slot(slots::SEARCH, SEARCH_SCREEN, Priority::DEFAULT)?;
        r.bind_slot(slots::SKIM, SKIM_SCREEN, Priority::DEFAULT)?;
        r.add_action(paperpaw_sdk::ActionSpec::new(actions::TOC, "reader.toc"))?;
        r.add_screen(ScreenSpec::new(
            ANNOTATIONS_SCREEN,
            "reader.annotations",
            Box::new(|env, params| Box::new(annotations::AnnotationsScreen::open(env, params))),
        ))?;
        status::register(r)?;
        r.add_screen(ScreenSpec::new(
            SKIM_SCREEN,
            "reader.skim",
            Box::new(|env, params| Box::new(skim::SkimScreen::new(env, params))),
        ))?;
        r.add_screen(ScreenSpec::new(
            FOOTNOTE_SCREEN,
            "reader.title",
            Box::new(|env, params| Box::new(footnote::FootnoteScreen::new(env, params))),
        ))?;
        r.add_screen(ScreenSpec::new(
            SEARCH_SCREEN,
            "reader.search",
            Box::new(|env, params| Box::new(search::SearchScreen::open(env, params))),
        ))?;
        for (id, title) in [
            (actions::SEARCH, "reader.search"),
            (actions::GO_TO_PAGE, "reader.go-to-page"),
            (actions::GO_TO_PERCENT, "reader.go-to-percent"),
            (actions::READ_ALOUD, "reader.read-aloud"),
            (actions::SELECT, "reader.select"),
            (actions::LINKS, "reader.links"),
            (actions::NEXT_STATUS_LINE, "reader.next-status-line"),
            (actions::NEXT_TOP_STATUS_LINE, "reader.next-top-status-line"),
            (actions::TOGGLE_BOOKMARK, "reader.toggle-bookmark"),
            (actions::ANNOTATIONS, "reader.annotations"),
            (actions::LOOK_UP, "reader.select.look-up"),
            (actions::HIGHLIGHT, "reader.select.highlight"),
            (actions::COPY, "reader.select.copy"),
            (actions::TRANSLATE, "reader.select.translate"),
            (actions::READ_FROM, "reader.select.read-aloud"),
            (actions::NOTE, "reader.select.note"),
            (actions::BOOK_INFO, "reader.book-info"),
            (actions::AUTO_TURN, "reader.auto-turn"),
            (actions::SKIM_BOOK, "reader.skim"),
            (actions::NEXT_CHAPTER, "reader.next-chapter"),
            (actions::PREV_CHAPTER, "reader.prev-chapter"),
            (actions::EXTEND, "reader.select.extend"),
            (actions::CANCEL, "reader.select.cancel"),
            (actions::MORE, "reader.select.more"),
        ] {
            r.add_action(paperpaw_sdk::ActionSpec::new(id, title))?;
        }
        // The bar for selected text; plugins add to it.
        for (item, order) in [
            (actions::LOOK_UP, 10),
            (actions::HIGHLIGHT, 20),
            (actions::NOTE, 22),
            (actions::READ_FROM, 25),
            (actions::COPY, 30),
            (actions::TRANSLATE, 35),
            (actions::EXTEND, 80),
            (actions::CANCEL, 90),
        ] {
            r.add_menu_entry(paperpaw_sdk::MenuEntry::new(SELECTION_MENU, item).order(order))?;
        }

        r.add_settings(&SETTINGS)?;
        // A sample of text above the reading settings, set as they say.
        r.add_widget(
            paperpaw_sdk::widgets::WidgetSpec::new(SAMPLE, "reader.sample", |_| {
                Box::new(screen::Sample::new())
            })
            .previews(groups::READING),
        )?;

        let font_step = |step: i64| -> paperpaw_sdk::ActionHandler {
            Box::new(
                move |env: &Env<'_>, _: &paperpaw_sdk::Params, out: &mut Commands| {
                    // Within the sizes the reading font has (see the
                    // setting), to the next that looks different.
                    let Some(spec) = env.settings().spec(settings::FONT_SIZE.as_str()) else {
                        return;
                    };
                    let size = env.settings().read(&settings::FONT_SIZE);
                    let next = match spec.kind {
                        paperpaw_sdk::SettingKind::Int { min, max } => {
                            (size + step).clamp(min, max.max(min))
                        }
                        _ => size + step,
                    };
                    let next = paperpaw_sdk::ui::shown_step(env, spec, size, next);
                    out.push(Command::set(&settings::FONT_SIZE, next));
                },
            )
        };
        let handlers: [(Id, &'static str, paperpaw_sdk::ActionHandler); 3] = [
            (
                actions::FONT_LARGER,
                "reader.action.font-larger",
                font_step(1),
            ),
            (
                actions::FONT_SMALLER,
                "reader.action.font-smaller",
                font_step(-1),
            ),
            (
                actions::TOGGLE_JUSTIFY,
                "reader.action.toggle-justify",
                Box::new(
                    |env: &Env<'_>, _: &paperpaw_sdk::Params, out: &mut Commands| {
                        let justify = env.settings().read(&settings::JUSTIFY);
                        out.push(Command::set(&settings::JUSTIFY, !justify));
                    },
                ),
            ),
        ];
        for (id, title, handler) in handlers {
            r.add_action(paperpaw_sdk::ActionSpec::new(id, title).handler(handler))?;
        }
        // Font and layout as a panel of controls: changes show at once.
        r.add_action(
            paperpaw_sdk::ActionSpec::new(actions::QUICK_SETTINGS, "reader.layout").handler(
                |_: &Env<'_>, _, out: &mut Commands| {
                    out.push(Command::OpenSlot(
                        slots::PANEL,
                        Params::new().with(params::MENU, String::from(LAYOUT_MENU.as_str())),
                    ));
                },
            ),
        )?;
        // A tap at the bottom of a page opens it. Swipes in from the sides
        // turn pages, as swipes do, rather than going back; two fingers
        // apart or together make the text larger or smaller. Holding
        // Select (which opens the menu) marks the page, as the corner does
        // by touch.
        r.add_keymap(Cow::Borrowed(
            "[keymap.core.reader]\ntap.bottom = reader.quick-settings\n\
             edge.left = core.prev-page\nedge.right = core.next-page\n\
             pinch.out = reader.font-larger\npinch.in = reader.font-smaller\n\
             long.confirm = reader.toggle-bookmark\n",
        ))?;
        // Holding a page button goes a chapter on or back, while the
        // switch is on: off, the binding is gone, and a page turns as its
        // button goes down rather than when it is let go.
        r.add_keymap_when(
            &settings::HOLD_CHAPTERS,
            Cow::Borrowed(
                "[keymap.core.reader]\nlong.page-forward = reader.next-chapter\n\
                 long.page-back = reader.prev-chapter\n",
            ),
        )?;
        // A short menu; settings in it show their state and change in
        // place.
        let entries = [
            // First, focused as the menu opens: with buttons, the font and
            // layout are two presses away.
            (MENU, actions::QUICK_SETTINGS, 5),
            (MENU, actions::TOC, 10),
            (MENU, actions::SEARCH, 20),
            (MENU, actions::READ_ALOUD, 30),
            (MENU, actions::ANNOTATIONS, 40),
            (MENU, core_settings::NIGHT_MODE.into(), 60),
            // Beside the night mode, to be found; held Select and the
            // corner are quicker.
            (MENU, actions::TOGGLE_BOOKMARK, 65),
            (MENU, MORE_MENU, 70),
            // Last, so that one press back from the first entry reaches
            // it: home from a book without a further list.
            (MENU, core_actions::HOME, 80),
            (LAYOUT_MENU, core_settings::SCOPE_ONLY.into(), 1),
            (LAYOUT_MENU, core_settings::READING_FONT.into(), 5),
            (LAYOUT_MENU, settings::FONT_SIZE.into(), 10),
            (LAYOUT_MENU, settings::FONT_WEIGHT.into(), 15),
            (LAYOUT_MENU, settings::LINE_SPACING.into(), 20),
            (LAYOUT_MENU, settings::MARGIN.into(), 30),
            (LAYOUT_MENU, settings::JUSTIFY.into(), 40),
            (LAYOUT_MENU, settings::HYPHENATE.into(), 50),
            (LAYOUT_MENU, settings::PUBLISHER_STYLES.into(), 60),
            (MORE_MENU, actions::SKIM_BOOK, 15),
            (MORE_MENU, actions::GO_TO_PAGE, 16),
            (MORE_MENU, actions::GO_TO_PERCENT, 17),
            (MORE_MENU, actions::AUTO_TURN, 17),
            (MORE_MENU, actions::SELECT, 20),
            (MORE_MENU, actions::LINKS, 25),
            (MORE_MENU, actions::NEXT_STATUS_LINE, 30),
            (MORE_MENU, core_actions::SETTINGS, 50),
        ];
        for (menu, item, order) in entries {
            r.add_menu_entry(paperpaw_sdk::MenuEntry::new(menu, item).order(order))?;
        }
        // Short menus: reading aloud where the device speaks; choosing
        // text and links and switching status lines (a tap on one by
        // touch) with the buttons where there is no touch screen.
        for item in [
            actions::READ_ALOUD,
            actions::READ_FROM,
            settings::ALOUD_SPEED.into(),
            settings::ALOUD_HIGHLIGHT.into(),
            settings::ALOUD_TURN.into(),
            settings::ALOUD_TIMER.into(),
        ] {
            r.set_menu_need(item, paperpaw_sdk::MenuNeed::Speech)?;
        }
        for item in [actions::SELECT, actions::LINKS, actions::NEXT_STATUS_LINE] {
            r.set_menu_need(item, paperpaw_sdk::MenuNeed::NoTouch)?;
        }
        r.set_menu_need(settings::SMOOTH_TEXT, paperpaw_sdk::MenuNeed::BlackAndWhite)?;
        r.set_menu_need(settings::CORNER_BOOKMARK, paperpaw_sdk::MenuNeed::Touch)?;
        for (item, icon) in [
            (actions::TOC, "list"),
            (actions::LINKS, "link"),
            (actions::SEARCH, "search"),
            (actions::GO_TO_PAGE, "hash"),
            (actions::GO_TO_PERCENT, "arrow-bar-to-right"),
            (actions::ANNOTATIONS, "bookmarks"),
            (actions::QUICK_SETTINGS, "typography"),
            (actions::TOGGLE_BOOKMARK, "bookmark"),
            (actions::SELECT, "cursor-text"),
            (actions::READ_ALOUD, "volume"),
            (actions::AUTO_TURN, "clock-play"),
            (actions::SKIM_BOOK, "arrows-horizontal"),
            (actions::NEXT_STATUS_LINE, "layout-bottombar"),
        ] {
            r.add_icon(item, Cow::Borrowed(icon))?;
        }

        r.on(events::STARTED, |env, (), out| reopen_last_book(env, out))?;
        r.on(events::BOOK_MOVED, |env, moved, _| book_moved(env, moved))?;
        // The first-run setup: how large the text is.
        r.add_setup_step(
            paperpaw_sdk::setup::SetupStepSpec::new(
                Id::new_static("reader.setup.font-size"),
                "",
                paperpaw_sdk::setup::SetupPage::Setting(settings::FONT_SIZE.into()),
            )
            .order(30),
        )?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/en.ini")))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/de.ini")))?;
        Ok(())
    }
}

/// Where the reader remembers the last book.
fn state_path() -> String {
    alloc::format!("{}/state.ini", Env::plugin_dir(PLUGIN.as_str()))
}

/// A book was moved: the last book read follows it (its data folder
/// moves with it, as the kernel moves registered book data).
fn book_moved(env: &Env<'_>, moved: &events::BookMoved) {
    let (from, to) = (moved.from.as_str(), moved.path.as_str());
    if last_book(env).as_deref() == Some(from) {
        screen::remember_last(env.storage(), to);
    }
}

/// The book read last, as remembered.
fn last_book(env: &Env<'_>) -> Option<String> {
    let data = paperpaw_sdk::read_file(env.storage(), &state_path(), 4096).ok()?;
    last_in(core::str::from_utf8(&data).ok()?)
}

/// The book kept in `state.ini`.
fn last_in(text: &str) -> Option<String> {
    paperpaw_sdk::config::parse(text)
        .filter_map(Result::ok)
        .filter(|e| e.section == "reader" && e.key == "last")
        .last()
        .map(|e| String::from(e.value.as_str()))
}

fn reopen_last_book(env: &Env<'_>, out: &mut Commands) {
    if !env.settings().read(&settings::OPEN_LAST) {
        return;
    }
    if let Some(path) = last_book(env)
        && env.storage().size(&path).is_ok()
    {
        // In the reading app chosen for it.
        out.push(paperpaw_sdk::readers::open(path));
    }
}

#[cfg(test)]
mod tests {
    use paperpaw_i18n::{Catalog, check};

    /// The last book as version 1 wrote it stays readable (see
    /// `docs/data-files.md`).
    #[test]
    fn the_last_book_of_version_1_is_read() {
        assert_eq!(
            super::last_in(include_str!("../testdata/state.ini.v1")).as_deref(),
            Some("books/\"Quoted\" title.epub")
        );
    }

    #[test]
    fn translations_are_complete() {
        let en = Catalog::parse(include_str!("../i18n/en.ini")).unwrap();
        let de = Catalog::parse(include_str!("../i18n/de.ini")).unwrap();
        let issues = check::compare(&en, &de);
        assert!(issues.is_empty(), "{issues:?}");
    }
}
