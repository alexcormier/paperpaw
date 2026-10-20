//! Ids of the contributions the kernel itself provides (namespace `core`).
//!
//! Plugins use these to bind screens to the standard slots, to handle the
//! standard navigation actions and to read the global settings.

/// The kernel's own plugin id; it cannot be switched off.
pub const CORE_PLUGIN: crate::Id = crate::Id::new_static("core");

/// Standard actions. Navigation actions have no global handler; screens
/// interpret them. `back`, `home` and `toggle-night-mode` have global
/// handlers, which screens may still override by handling the action.
pub mod actions {
    use crate::Id;

    /// Does nothing: bound to an input, it stops the input from doing
    /// anything there.
    pub const NOTHING: Id = Id::new_static("core.nothing");

    pub const NEXT_PAGE: Id = Id::new_static("core.next-page");
    /// "Forward" in the widest sense: next list entry, increase a value
    /// being edited; screens without such a meaning treat it as the next
    /// page. Lets devices with very few buttons operate everything.
    pub const NEXT: Id = Id::new_static("core.next");
    /// The opposite of [`NEXT`].
    pub const PREVIOUS: Id = Id::new_static("core.previous");
    pub const PREV_PAGE: Id = Id::new_static("core.prev-page");
    pub const UP: Id = Id::new_static("core.up");
    pub const DOWN: Id = Id::new_static("core.down");
    pub const LEFT: Id = Id::new_static("core.left");
    pub const RIGHT: Id = Id::new_static("core.right");
    pub const CONFIRM: Id = Id::new_static("core.confirm");
    pub const MENU: Id = Id::new_static("core.menu");
    pub const BACK: Id = Id::new_static("core.back");
    pub const HOME: Id = Id::new_static("core.home");
    pub const TOGGLE_NIGHT_MODE: Id = Id::new_static("core.toggle-night-mode");
    /// Opens the library slot.
    pub const LIBRARY: Id = Id::new_static("core.library");
    /// Opens the settings slot.
    pub const SETTINGS: Id = Id::new_static("core.settings");
    /// Saves what the screen shows as a picture, in `Screenshots` beside
    /// the library ([`Command::Screenshot`](crate::Command::Screenshot)).
    pub const SCREENSHOT: Id = Id::new_static("core.screenshot");
    /// Removes what plugins kept for books no longer on the storage
    /// ([`Registrar::add_book_data`](crate::Registrar::add_book_data)),
    /// e.g. deleted from a computer.
    pub const CLEAN_UP: Id = Id::new_static("core.clean-up");
    /// Opens the tools menu ([`menus::TOOLS`](super::menus::TOOLS)).
    pub const TOOLS: Id = Id::new_static("core.tools");
    /// Puts the device to sleep: shows the sleep screen (slot
    /// [`SLEEP`](super::slots::SLEEP)), then the shell sleeps until it is
    /// woken (see [`Capabilities::sleep`](crate::Capabilities)). Where the system sleeps on
    /// its own (Android), nothing sends it.
    pub const SLEEP: Id = Id::new_static("core.sleep");
    /// Opens the quick panel from the top of the screen: the menu
    /// [`QUICK`](super::menus::QUICK) as a panel of controls (night mode,
    /// light, turning the screen, …). A swipe down from the top edge
    /// opens it.
    pub const QUICK_PANEL: Id = Id::new_static("core.quick-panel");
    /// Refreshes the whole screen with a flash, clearing what e-ink left
    /// of earlier pictures (ghosting).
    pub const REFRESH_SCREEN: Id = Id::new_static("core.refresh-screen");
}

/// Standard slots.
pub mod slots {
    use crate::Id;

    /// The start screen. The default is a dashboard, but any screen can be
    /// bound here: a file browser, a bookshelf, a single "continue" button.
    pub const HOME: Id = Id::new_static("core.home");
    /// Shows a book. Opened with the parameter [`PATH`](super::params::PATH).
    pub const READER: Id = Id::new_static("core.reader");
    /// Browses the library.
    pub const LIBRARY: Id = Id::new_static("core.library");
    /// Shows a menu (an overlay). Opened with the parameter
    /// [`MENU`](super::params::MENU).
    pub const MENU: Id = Id::new_static("core.menu");
    /// Shows and edits settings; with the parameter
    /// [`ID`](super::params::ID), the settings of that group, e.g. a
    /// widget's own (see [`widgets`](crate::widgets)).
    pub const SETTINGS: Id = Id::new_static("core.settings");
    /// The first-run setup (see [`setup`](crate::setup)): shown once on
    /// the first start, and again when asked.
    pub const SETUP: Id = Id::new_static("core.setup");
    /// Looks up a word (an overlay): opened with the parameters
    /// [`WORD`](super::params::WORD) and, if known,
    /// [`LANGUAGE`](super::params::LANGUAGE). The standard dictionary
    /// binds here; translators or encyclopedias can too.
    pub const LOOKUP: Id = Id::new_static("core.lookup");
    /// Looks a word up online (an overlay), with the same parameters as
    /// [`LOOKUP`]: an encyclopedia, an online dictionary, a translator.
    /// Dictionaries offer it beside their own entries when a screen is
    /// bound here ([`Env::has_slot`](crate::Env::has_slot)) and there is
    /// a network.
    pub const LOOKUP_ONLINE: Id = Id::new_static("core.lookup-online");
    /// Shows a menu as a panel of controls for quick changes (an overlay):
    /// parameter [`MENU`](super::params::MENU). The reader opens its
    /// font and layout menu this way.
    pub const PANEL: Id = Id::new_static("core.panel");
    /// Asks for text (usually an overlay with an on-screen keyboard).
    /// Parameters: [`TITLE`](super::params::TITLE) (shown above the
    /// field, already translated), [`TEXT`](super::params::TEXT) (the
    /// initial text), [`MODE`](super::params::MODE) and
    /// [`REPLY`](super::params::REPLY). When the text is confirmed the
    /// screen closes and emits the event named by `reply` with all of its
    /// parameters, `text` set to what was entered, so the asking plugin
    /// can pass along anything it needs to know (e.g. which setting). Bind
    /// another screen here for a different keyboard, handwriting or voice.
    pub const KEYBOARD: Id = Id::new_static("core.keyboard");
    /// Asks for text typed on another device, e.g. a phone that opens an
    /// address shown as a QR code: handy for passwords and keys. Same
    /// parameters as [`KEYBOARD`]; the text comes back as the event named
    /// by [`REPLY`](super::params::REPLY). The keyboard offers it where a
    /// screen is bound here and there is a network.
    pub const REMOTE_KEYBOARD: Id = Id::new_static("core.remote-keyboard");
    /// Shows a web page: parameter [`URL`](super::params::URL), and
    /// [`ID`](super::params::ID) different for every request (opening a
    /// slot with the parameters it is open with only brings it forward).
    /// The reader opens links out of a book this way, if a browser is
    /// bound here ([`Env::has_slot`](crate::Env::has_slot)).
    pub const BROWSER: Id = Id::new_static("core.browser");
    /// Chooses a folder, browsing the device's folders (with a way to make
    /// a new one). Parameters: [`ID`](super::params::ID), a setting the
    /// folder is written to, or [`REPLY`](super::params::REPLY), an event
    /// emitted with all of the parameters and [`PATH`](super::params::PATH)
    /// set to the folder; [`TEXT`](super::params::TEXT), where to start.
    pub const CHOOSE_FOLDER: Id = Id::new_static("core.choose-folder");
    /// Shows what is known of a book: parameter [`PATH`](super::params::PATH).
    pub const BOOK_INFO: Id = Id::new_static("core.book-info");
    /// Shows a short message ([`Command::Notify`](crate::Command::Notify))
    /// over whatever is open, for a few seconds: parameter
    /// [`TEXT`](super::params::TEXT). It gets no input and is closed by
    /// the kernel; bind another screen here for messages that look
    /// different. Its [`region`](crate::Screen::region) is where it shows.
    pub const NOTICE: Id = Id::new_static("core.notice");
    /// Translates text (an overlay): parameters
    /// [`TEXT`](super::params::TEXT) and, if known,
    /// [`LANGUAGE`](super::params::LANGUAGE) (the text's). The reader
    /// offers it for selected text when a screen is bound here and there
    /// is a network; the `translate` plugin binds DeepL, other services
    /// can take its place.
    pub const TRANSLATE: Id = Id::new_static("core.translate");
    /// Shown while the device sleeps (see
    /// [`actions::SLEEP`](super::actions::SLEEP)): a cover, a picture, a
    /// message. It closes itself when the device wakes
    /// ([`events::RESUME`](super::events::RESUME)).
    pub const SLEEP: Id = Id::new_static("core.sleep");
}

/// Standard menus. Plugins add entries with
/// [`Registrar::add_menu_entry`](crate::Registrar::add_menu_entry); users can
/// redefine any menu in `[menus]`.
///
/// A menu named like a setting with choices (e.g. `core.reading-font`)
/// holds more ways to choose: where its options are listed, its entries
/// follow them (e.g. *More fonts*, to download one).
pub mod menus {
    use crate::Id;

    /// The menu shown when a screen has no menu of its own: short and
    /// always the same (home, library, tools, night mode, settings).
    pub const MAIN: Id = Id::new_static("core.main");
    /// The tools plugins bring (statistics, catalogs, files, transfers,
    /// …): plugins add their entries here rather than to [`MAIN`], so the
    /// main menu stays short however many there are. Also the home
    /// screen's grid of tools.
    pub const TOOLS: Id = Id::new_static("core.tools");
    /// Extra pages on the main settings page, below the setting groups
    /// (e.g. the plugin manager). Entries are actions.
    pub const SETTINGS: Id = Id::new_static("core.settings-pages");
    /// The quick panel ([`actions::QUICK_PANEL`](super::actions::QUICK_PANEL)):
    /// switches and controls for quick changes, from every plugin.
    pub const QUICK: Id = Id::new_static("core.quick");
    /// The reader's menu, while a book is open (whichever plugin is the
    /// reader, [`slots::READER`](super::slots::READER)): plugins add what
    /// belongs to reading.
    pub const READER: Id = Id::new_static("core.reader-menu");
    /// The reader's submenu "More": what is used less often (statistics,
    /// syncing a book, profiles).
    pub const READER_MORE: Id = Id::new_static("core.reader-more");
    /// The reader's panel of font and layout, shown over the page; plugins
    /// add controls such as the light.
    pub const READER_PANEL: Id = Id::new_static("core.reader-panel");
    /// What can be done with text chosen in a book (look it up, share,
    /// translate): plugins add their actions; when one is chosen, the
    /// reader sends [`events::TEXT_CHOSEN`](super::events::TEXT_CHOSEN),
    /// then runs it.
    pub const SELECTION: Id = Id::new_static("core.selection-menu");
}

/// The roles of decorations every reader draws (see
/// [`decorations`](crate::decorations)): the kernel adds them, so their
/// looks are settings like any role's.
pub mod roles {
    use crate::Id;

    /// One's own highlights.
    pub const HIGHLIGHT: Id = Id::new_static("core.highlight");
    /// Links in a book.
    pub const LINK: Id = Id::new_static("core.link");
    /// A search hit gone to.
    pub const SEARCH_HIT: Id = Id::new_static("core.search-hit");
    /// The sentence being read aloud.
    pub const ALOUD: Id = Id::new_static("core.aloud");
}

/// Standard parameter names.
pub mod params {
    /// A storage path, e.g. of the book to open.
    pub const PATH: &str = "path";
    /// The menu to show in the menu slot.
    pub const MENU: &str = "menu";
    /// An id, e.g. of the setting in `core.setting-changed`.
    pub const ID: &str = "id";
    /// A title, e.g. of a book or of what a keyboard asks for.
    pub const TITLE: &str = "title";
    /// Text, e.g. entered on the keyboard.
    pub const TEXT: &str = "text";
    /// What kind of text a keyboard asks for: `text` (the default),
    /// `number` or `password` (shown as dots).
    pub const MODE: &str = "mode";
    /// The event a screen emits with its result, e.g. the keyboard's text.
    pub const REPLY: &str = "reply";
    /// Why something failed, e.g. joining a network
    /// ([`events::WIFI_FAILED`](super::events::WIFI_FAILED)).
    pub const REASON: &str = "reason";
    /// A position in a document: section and offset (see
    /// `document::Locator`).
    pub const SECTION: &str = "section";
    pub const OFFSET: &str = "offset";
    /// Reading progress in percent, e.g. `12.5`.
    pub const PROGRESS: &str = "progress";
    /// The page (from 1) and the number of pages, once a reader has
    /// counted them.
    pub const PAGE: &str = "page";
    pub const PAGES: &str = "pages";
    /// A word, e.g. to look up.
    pub const WORD: &str = "word";
    /// A BCP 47 language tag, e.g. of the book a word is from.
    pub const LANGUAGE: &str = "language";
    /// A web address, e.g. for [`slots::BROWSER`](super::slots::BROWSER).
    pub const URL: &str = "url";
    /// An action to run, e.g. when the title of a menu bar is tapped.
    pub const ACTION: &str = "action";
    /// Entries a menu leaves out this time, as ids separated by commas:
    /// what does not apply where it is opened (e.g. searching a comic).
    pub const HIDE: &str = "hide";
    /// Where something was before, e.g. a moved book
    /// ([`events::BOOK_MOVED`](super::events::BOOK_MOVED)).
    pub const FROM: &str = "from";
}

/// Events emitted by the kernel.
pub mod events {
    use alloc::string::String;

    use crate::Id;
    use crate::document::Locator;
    use crate::event::Event;

    crate::event_data! {
        /// Where a book is being read ([`BOOK_OPENED`], [`PAGE_TURNED`],
        /// [`BOOK_CLOSED`], [`BOOK_FINISHED`]).
        pub struct Reading {
            /// The book.
            pub path: String,
            pub title: String,
            /// How far in it, in percent.
            pub progress: f32,
            /// The page's start ([`Reading::at`]).
            pub section: u32,
            pub offset: u32,
            /// The page and the pages, once the reader counted them.
            pub page: Option<u32>,
            pub pages: Option<u32>,
            /// The title of the chapter the page is in: empty where the
            /// book has no table of contents or the page comes before
            /// its first entry.
            pub chapter: String,
            /// The pages after this one in the chapter (0 on its last
            /// page), once the reader counted the pages and where the
            /// chapter is known.
            pub chapter_left: Option<u32>,
        }
    }

    impl Reading {
        /// The page's start.
        pub fn at(&self) -> Locator {
            Locator::new(self.section, self.offset)
        }
    }

    crate::event_data! {
        /// Which setting changed ([`SETTING_CHANGED`]).
        pub struct SettingChanged {
            pub id: Id,
        }
    }

    crate::event_data! {
        /// A book that was deleted ([`BOOK_REMOVED`]).
        pub struct BookRemoved {
            /// Its path, where it was.
            pub path: String,
        }
    }

    crate::event_data! {
        /// A book that was moved or renamed ([`BOOK_MOVED`]).
        pub struct BookMoved {
            /// Its old path.
            pub from: String,
            /// Its new one.
            pub path: String,
        }
    }

    crate::event_data! {
        /// The book whose annotations changed ([`ANNOTATIONS_CHANGED`]).
        pub struct AnnotationsChanged {
            pub path: String,
        }
    }

    crate::event_data! {
        /// A word looked up in a book ([`WORD_LOOKED_UP`]).
        pub struct WordLookedUp {
            pub word: String,
            /// The book's language, e.g. `de`.
            pub language: String,
            /// The sentence the word is in.
            pub text: String,
            /// The book.
            pub path: String,
            pub title: String,
            /// Where the word is.
            pub section: u32,
            pub offset: u32,
        }
    }

    crate::event_data! {
        /// A place in a book to show ([`GOTO`]): where it begins, and
        /// where a part to mark ends (e.g. a search hit), if any.
        pub struct Goto {
            /// The book.
            pub path: String,
            pub section: u32,
            pub offset: u32,
            pub end_section: Option<u32>,
            pub end_offset: Option<u32>,
        }
    }

    impl Goto {
        /// Place `at` in the book at `path`, nothing marked.
        pub fn new(path: impl Into<String>, at: Locator) -> Self {
            Self {
                path: path.into(),
                section: at.section,
                offset: at.offset,
                end_section: None,
                end_offset: None,
            }
        }

        /// The same place, with the text up to `end` marked.
        pub fn marking(mut self, end: Locator) -> Self {
            self.end_section = Some(end.section);
            self.end_offset = Some(end.offset);
            self
        }

        /// Where it begins.
        pub fn at(&self) -> Locator {
            Locator::new(self.section, self.offset)
        }

        /// Where the part to mark ends, if one is.
        pub fn end(&self) -> Option<Locator> {
            Some(Locator::new(self.end_section?, self.end_offset?))
        }
    }

    crate::event_data! {
        /// Text chosen in a book, and the action chosen for it
        /// ([`TEXT_CHOSEN`]).
        pub struct TextChosen {
            /// The action, from [`menus::SELECTION`](super::menus::SELECTION).
            pub action: Id,
            pub text: String,
            /// The book's language, e.g. `de`.
            pub language: String,
            /// The book, and where the text begins and ends.
            pub path: String,
            pub section: u32,
            pub offset: u32,
            pub end_section: u32,
            pub end_offset: u32,
        }
    }

    impl TextChosen {
        /// Where the text begins.
        pub fn start(&self) -> Locator {
            Locator::new(self.section, self.offset)
        }

        /// Just after its end.
        pub fn end(&self) -> Locator {
            Locator::new(self.end_section, self.end_offset)
        }
    }

    crate::event_data! {
        /// A decorated word that was tapped, or chosen with the buttons
        /// ([`DECORATION_TAPPED`]): the decorator's mark it is in.
        pub struct DecorationTapped {
            /// The decorator, and the role of its mark.
            pub decorator: Id,
            pub role: Id,
            /// The words of the mark, as on the page.
            pub text: String,
            /// The book, and where the mark begins and ends.
            pub path: String,
            pub section: u32,
            pub offset: u32,
            pub end_section: u32,
            pub end_offset: u32,
        }
    }

    impl DecorationTapped {
        /// Where the mark begins.
        pub fn start(&self) -> Locator {
            Locator::new(self.section, self.offset)
        }

        /// Just after its end.
        pub fn end(&self) -> Locator {
            Locator::new(self.end_section, self.end_offset)
        }
    }

    crate::event_data! {
        /// A book's notes, exported ([`NOTES_EXPORTED`]).
        pub struct NotesExported {
            /// The file written (Markdown).
            pub path: String,
            /// The book's title.
            pub title: String,
        }
    }

    crate::event_data! {
        /// The menu that was redefined or reset ([`MENU_CHANGED`]).
        pub struct MenuChanged {
            pub id: Id,
        }
    }

    crate::event_data! {
        /// The widget whose content changed ([`WIDGET_CHANGED`]).
        pub struct WidgetChanged {
            pub id: Id,
        }
    }

    crate::event_data! {
        /// A network that could not be joined ([`WIFI_FAILED`]).
        pub struct WifiFailed {
            pub ssid: String,
            pub reason: crate::wifi::JoinFailure,
        }
    }

    /// After the home screen has been opened at start-up. Plugins use it to
    /// e.g. reopen the last book.
    pub const STARTED: Event<()> = Event::new("core.started");
    /// After a setting changed: which ([`SettingChanged`]).
    pub const SETTING_CHANGED: Event<SettingChanged> = Event::new("core.setting-changed");
    /// A book was opened for reading ([`Reading`]). Every reader plugin
    /// emits it, so home screens, statistics or sync plugins work with any
    /// reader. Not for documents in hidden folders
    /// ([`files::is_hidden`](crate::files::is_hidden)), e.g. web pages:
    /// they are no books.
    pub const BOOK_OPENED: Event<Reading> = Event::new("core.book-opened");
    /// The reader moved to another page ([`Reading`]).
    pub const PAGE_TURNED: Event<Reading> = Event::new("core.page-turned");
    /// The book read was closed (the reader left, or another book
    /// opened): where it was left ([`Reading`]).
    pub const BOOK_CLOSED: Event<Reading> = Event::new("core.book-closed");
    /// The last page of a book was reached, once each time it is opened
    /// ([`Reading`]).
    pub const BOOK_FINISHED: Event<Reading> = Event::new("core.book-finished");
    /// A book file was moved or renamed ([`BookMoved`]). Plugins that keep
    /// data by a book's path move it along (progress, notes, statistics).
    pub const BOOK_MOVED: Event<BookMoved> = Event::new("core.book-moved");
    /// A book file was deleted ([`BookRemoved`]): what plugins keep for it
    /// goes ([`Registrar::add_book_data`](crate::Registrar::add_book_data)
    /// folders by the kernel; other data by the plugin). Whoever deletes a
    /// book emits it.
    pub const BOOK_REMOVED: Event<BookRemoved> = Event::new("core.book-removed");
    /// Books came into the library or left it (received, downloaded,
    /// copied, deleted): views of the library read it again. Plugins that
    /// change the library emit it ([`library::changed`](crate::library::changed));
    /// a move is [`BOOK_MOVED`].
    pub const LIBRARY_CHANGED: Event<()> = Event::new("core.library-changed");
    /// The values shown without being kept changed, or ended (see
    /// [`Command::PreviewSettings`](crate::Command::PreviewSettings)).
    pub const PREVIEW_CHANGED: Event<()> = Event::new("core.preview-changed");
    /// The bookmarks or highlights of a book changed
    /// ([`annotations`](crate::annotations), [`AnnotationsChanged`]).
    /// Readers show them anew, syncs pass them on.
    pub const ANNOTATIONS_CHANGED: Event<AnnotationsChanged> =
        Event::new("core.annotations-changed");
    /// Asks the reader to show a place in the book it has open ([`Goto`]);
    /// any reader takes it. Sent by the table of contents, bookmarks,
    /// search, syncs of the reading position.
    pub const GOTO: Event<Goto> = Event::new("core.goto");
    /// A book's notes were exported as a file ([`NotesExported`]), for
    /// plugins that take them further (a notes app, a sync). Readers send
    /// it; its form is Markdown.
    pub const NOTES_EXPORTED: Event<NotesExported> = Event::new("core.notes-exported");
    /// An action was chosen for text chosen in a book ([`TextChosen`]),
    /// just before it runs: the plugin whose action it is reacts (e.g. a
    /// "Share" action needs no more than a menu entry in
    /// [`menus::SELECTION`](super::menus::SELECTION) and this event).
    pub const TEXT_CHOSEN: Event<TextChosen> = Event::new("core.text-chosen");
    /// A word a decorator marked was tapped, or chosen with the buttons
    /// ([`DecorationTapped`]); sent for decorators that take taps (see
    /// [`decorations`](crate::decorations)), whose plugin answers.
    pub const DECORATION_TAPPED: Event<DecorationTapped> = Event::new("core.decoration-tapped");
    /// A word of a book was looked up ([`WordLookedUp`]).
    pub const WORD_LOOKED_UP: Event<WordLookedUp> = Event::new("core.word-looked-up");
    /// What a widget shows changed by other means than the events screens
    /// already follow, e.g. a tap on it turned to another quote or ticked
    /// an item off ([`WidgetChanged`]): screens showing it read it anew
    /// ([`Widget::update`](crate::widgets::Widget::update)). See
    /// [`widgets::changed`](crate::widgets::changed).
    pub const WIDGET_CHANGED: Event<WidgetChanged> = Event::new("core.widget-changed");
    /// A data plugin was loaded or removed while running
    /// ([`Command::LoadPlugin`](crate::Command::LoadPlugin),
    /// [`Command::RemovePlugin`](crate::Command::RemovePlugin)): lists of
    /// plugins, fonts and presets read them anew.
    pub const PLUGINS_CHANGED: Event<()> = Event::new("core.plugins-changed");
    /// A menu was redefined or reset ([`MenuChanged`]).
    pub const MENU_CHANGED: Event<MenuChanged> = Event::new("core.menu-changed");
    /// The device goes to sleep or the app to the background: a moment to
    /// save or send what is pending.
    pub const SUSPEND: Event<()> = Event::new("core.suspend");
    /// Back from [`SUSPEND`].
    pub const RESUME: Event<()> = Event::new("core.resume");
    /// The battery or the time zone changed; status lines redraw.
    pub const STATUS_CHANGED: Event<()> = Event::new("core.status-changed");
    /// The battery was measured again ([`Env::status`](crate::Env::status)),
    /// about once a minute where the device measures it: for plugins that
    /// record its use. Nothing need be drawn anew.
    pub const BATTERY_MEASURED: Event<()> = Event::new("core.battery-measured");
    /// The Wi-Fi state changed ([`Env::wifi`](crate::Env), see
    /// [`wifi`](crate::wifi)).
    pub const WIFI_CHANGED: Event<()> = Event::new("core.wifi-changed");
    /// Networks were found ([`Wifi::networks`](crate::wifi::Wifi)).
    pub const WIFI_SCANNED: Event<()> = Event::new("core.wifi-scanned");
    /// Joining a network failed ([`WifiFailed`]).
    pub const WIFI_FAILED: Event<WifiFailed> = Event::new("core.wifi-failed");
    /// A plugin's request, server or connection waits for the network,
    /// where PaperPaw manages Wi-Fi and is in none: e.g. to join one.
    pub const NETWORK_WANTED: Event<()> = Event::new("core.network-wanted");
    /// Nothing uses the network any more (no request, server or
    /// connection), where PaperPaw manages Wi-Fi: e.g. to switch the radio
    /// off a little later.
    pub const NETWORK_IDLE: Event<()> = Event::new("core.network-idle");
    /// An install of a firmware update went on, ended or failed
    /// ([`Firmware::state`](crate::firmware::Firmware), see
    /// [`firmware`](crate::firmware)).
    pub const FIRMWARE_CHANGED: Event<()> = Event::new("core.firmware-changed");
    /// The power level or mode changed ([`power`](crate::power)), e.g. by
    /// the battery in Automatic.
    pub const POWER_CHANGED: Event<()> = Event::new("core.power-changed");
    /// A plugin's declared work was taken out because it can never run
    /// (its network switched off, not declared in its manifest), sent to
    /// that plugin; work only held back stays queued (see
    /// [`work`](crate::work)).
    pub const WORK_REFUSED: Event<crate::work::WorkRefused> = Event::new("core.work-refused");
    /// What held work or requests back no longer does (the mode changed,
    /// the reader switched a plugin's network on, went online): a plugin
    /// told [`Retry::WhenAllowed`](crate::work::Retry::WhenAllowed) may
    /// try again.
    pub const WORK_ALLOWED: Event<()> = Event::new("core.work-allowed");
    /// A plugin's declared work changed its state or its progress (at most
    /// once a second), sent to that plugin, e.g. for its page; the work's
    /// id comes as [`params::ID`](super::params::ID).
    pub const WORK_CHANGED: Event<()> = Event::new("core.work-changed");
}

/// Status items of the kernel (see [`status`](crate::status)).
pub mod status_items {
    use crate::Id;

    /// The time, as set in `core.clock-format`.
    pub const CLOCK: Id = Id::new_static("core.status.clock");
    /// The battery's charge.
    pub const BATTERY: Id = Id::new_static("core.status.battery");
    /// The battery's charge, only while it is low
    /// ([`settings::BATTERY_LOW`](super::settings::BATTERY_LOW)): a calm
    /// line while there is enough.
    pub const BATTERY_LOW: Id = Id::new_static("core.status.battery-low");
    /// Today in words ([`time::date_text`](crate::time::date_text)).
    pub const DATE: Id = Id::new_static("core.status.date");
    /// Today in short words, e.g. "Thu 15 Oct"
    /// ([`time::short_date_text`](crate::time::short_date_text)), for
    /// narrow places.
    pub const DATE_SHORT: Id = Id::new_static("core.status.date-short");
}

/// Standard screens.
pub mod screens {
    use crate::Id;

    /// Shown when a slot has no screen bound to it.
    pub const PLACEHOLDER: Id = Id::new_static("core.placeholder");
    /// Chooses the status items a setting lists (see
    /// [`ui::items`](crate::ui::items)): name it as the setting's editor.
    pub const STATUS_ITEMS: Id = Id::new_static("core.status-items");
    /// Chooses the status lines a setting lists.
    pub const STATUS_LINES: Id = Id::new_static("core.status-lines");
}

/// Global settings.
pub mod settings {
    use crate::setting::{Setting, Text};

    /// UI language tag, e.g. `en` or `de`.
    pub const LANGUAGE: Setting<Text> = Setting::new("core.language");
    /// Developer options: the settings of
    /// [`groups::DEVELOPER`](super::groups::DEVELOPER) show, and plugins may
    /// show what only developers need (e.g. what each plugin took).
    pub const DEVELOPER: Setting<bool> = Setting::new("core.developer");
    /// Draw everything inverted.
    pub const NIGHT_MODE: Setting<bool> = Setting::new("core.night-mode");
    /// In night mode, pictures inverted too (else as they are, not as
    /// negatives).
    pub const NIGHT_PICTURES: Setting<bool> = Setting::new("core.night-pictures");
    /// How full text is on panels of black and white alone
    /// ([`MenuNeed::BlackAndWhite`](crate::MenuNeed)): from what part
    /// covered a pixel at the edge of a letter is drawn, one of
    /// [`MONO_TEXT_LEVELS`] (half, three eighths, a quarter).
    pub const MONO_TEXT: Setting<Text> = Setting::new("core.mono-text");
    pub const MONO_TEXT_LEVELS: &[&str] = &[
        "core.mono-text.light",
        "core.mono-text.normal",
        "core.mono-text.full",
    ];
    /// How far in from the top and the bottom edge a swipe begins to count
    /// as one in from the edge, in millimetres (0: none).
    pub const EDGE_ENDS: Setting<i64> = Setting::new("core.edge-ends");
    /// The same at the left and right edges.
    pub const EDGE_SIDES: Setting<i64> = Setting::new("core.edge-sides");
    /// Say what the buttons move to (a row, a button), where the device
    /// speaks: for using PaperPaw without seeing the screen.
    pub const SPEAK_FOCUS: Setting<bool> = Setting::new("core.speak-focus");
    /// Partial refreshes between two full refreshes; `0` disables full
    /// refreshes for ghosting removal.
    pub const FULL_REFRESH_INTERVAL: Setting<i64> = Setting::new("core.full-refresh-interval");
    /// Whether another screen opening flashes the panel (a full refresh,
    /// no traces of the last one, but slow on some panels); else it counts
    /// as a page turn towards [`FULL_REFRESH_INTERVAL`].
    pub const FLASH_SCREENS: Setting<bool> = Setting::new("core.flash-screens");
    /// Minutes without use after which the device goes to sleep by itself
    /// (`0`: never), where PaperPaw puts it to sleep
    /// ([`Capabilities::sleep`](crate::Capabilities)). Turning pages, also by themselves,
    /// counts as use; reading aloud, downloads and servers keep it awake.
    pub const SLEEP_AFTER: Setting<i64> = Setting::new("core.sleep-after");
    /// The charge in percent at and below which the battery counts as low
    /// (the status item [`BATTERY_LOW`](super::status_items::BATTERY_LOW)).
    pub const BATTERY_LOW: Setting<i64> = Setting::new("core.battery-low");
    /// How the screen is turned: one of [`ROTATIONS`], where the shell
    /// can turn it (see [`MenuNeed::Rotation`](crate::MenuNeed)); each
    /// book may have its own ([`SCOPE_ONLY`]).
    pub const ROTATION: Setting<Text> = Setting::new("core.rotation");
    /// Whether a row below the screen shows what the buttons under it do,
    /// where a device has buttons in a row there (see
    /// [`MenuNeed::ButtonRow`](crate::MenuNeed)).
    pub const BUTTON_HINTS: Setting<bool> = Setting::new("core.button-hints");
    /// The page buttons the other way round while reading (on the screen
    /// bound to [`slots::READER`](super::slots::READER)): page forward
    /// goes back and page back forward (the kernel swaps them before key
    /// maps and input filters see them).
    pub const SWAP_PAGE_BUTTONS: Setting<bool> = Setting::new("core.swap-page-buttons");
    /// The values of [`ROTATION`]: upright, on its side (landscape),
    /// upside down, on its other side.
    pub const ROTATIONS: &[&str] = &[
        "core.rotation.upright",
        "core.rotation.landscape",
        "core.rotation.upside-down",
        "core.rotation.landscape-flipped",
    ];
    /// The colour black is shown in: one of [`TEXT_COLORS`], where the
    /// shell tints the screen (see [`MenuNeed::Colors`](crate::MenuNeed)).
    /// Everything is drawn in grays; the shell shows black as this colour,
    /// white as [`PAPER_COLOR`] and the grays between them.
    pub const TEXT_COLOR: Setting<Text> = Setting::new("core.text-color");
    /// The colour white is shown in: one of [`PAPER_COLORS`].
    pub const PAPER_COLOR: Setting<Text> = Setting::new("core.paper-color");
    /// The own text colour, used when [`TEXT_COLOR`] is [`OWN_TEXT`]: a
    /// colour code such as `#5b4636` (see [`parse_rgb`]).
    pub const TEXT_COLOR_OWN: Setting<Text> = Setting::new("core.text-color-own");
    /// The own paper colour, used when [`PAPER_COLOR`] is [`OWN_PAPER`].
    pub const PAPER_COLOR_OWN: Setting<Text> = Setting::new("core.paper-color-own");
    /// The value of [`TEXT_COLOR`] that takes [`TEXT_COLOR_OWN`].
    pub const OWN_TEXT: &str = "core.color.own";
    /// The value of [`PAPER_COLOR`] that takes [`PAPER_COLOR_OWN`].
    pub const OWN_PAPER: &str = "core.paper.own";
    /// The values of [`TEXT_COLOR`]: dark colours, then one's own.
    pub const TEXT_COLORS: &[&str] = &[
        "core.color.black",
        "core.color.dark-gray",
        "core.color.gray",
        "core.color.brown",
        "core.color.dark-blue",
        "core.color.dark-green",
        "core.color.burgundy",
        "core.color.violet",
        OWN_TEXT,
    ];
    /// The values of [`PAPER_COLOR`]: light colours, then one's own.
    pub const PAPER_COLORS: &[&str] = &[
        "core.paper.white",
        "core.paper.cream",
        "core.paper.sepia",
        "core.paper.sand",
        "core.paper.light-gray",
        "core.paper.mint",
        "core.paper.light-blue",
        "core.paper.rose",
        OWN_PAPER,
    ];

    /// The red, green and blue of a value of [`TEXT_COLOR`] or
    /// [`PAPER_COLOR`] other than one's own.
    pub fn rgb(value: &str) -> Option<[u8; 3]> {
        Some(match value {
            "core.color.black" => [0, 0, 0],
            "core.color.dark-gray" => [51, 51, 51],
            "core.color.gray" => [85, 85, 85],
            "core.color.brown" => [91, 70, 54],
            "core.color.dark-blue" => [28, 42, 68],
            "core.color.dark-green" => [30, 58, 42],
            "core.color.burgundy" => [94, 28, 36],
            "core.color.violet" => [58, 42, 82],
            "core.paper.white" => [255, 255, 255],
            "core.paper.cream" => [250, 246, 236],
            "core.paper.sepia" => [244, 236, 216],
            "core.paper.sand" => [233, 222, 196],
            "core.paper.light-gray" => [232, 232, 232],
            "core.paper.mint" => [226, 240, 230],
            "core.paper.light-blue" => [226, 236, 244],
            "core.paper.rose" => [246, 232, 232],
            _ => return None,
        })
    }

    /// The red, green and blue of a colour code: `#5b4636`, `5b4636` or
    /// the short `#543`; `None` for anything else.
    pub fn parse_rgb(code: &str) -> Option<[u8; 3]> {
        let hex = code.trim().trim_start_matches('#');
        let digit = |i: usize| u8::from_str_radix(hex.get(i..i + 1)?, 16).ok();
        match hex.len() {
            3 => Some([digit(0)? * 17, digit(1)? * 17, digit(2)? * 17]),
            6 => {
                let byte = |i: usize| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok();
                Some([byte(0)?, byte(2)?, byte(4)?])
            }
            _ => None,
        }
    }
    /// Margin of menus and lists, in millimetres.
    pub const UI_MARGIN: Setting<i64> = Setting::new("core.ui-margin");
    /// Height of list rows, in percent of a line of text.
    pub const UI_ROW: Setting<i64> = Setting::new("core.ui-row");
    /// Width of lines and borders, in tenths of a millimetre.
    pub const UI_LINES: Setting<i64> = Setting::new("core.ui-lines");
    /// Headers dark ([`HEADER_DARK`]), light ([`HEADER_LIGHT`]) or plain,
    /// without a bar or a line ([`HEADER_PLAIN`]).
    pub const UI_HEADER: Setting<Text> = Setting::new("core.ui-header");
    pub const HEADER_DARK: &str = "core.header.dark";
    pub const HEADER_LIGHT: &str = "core.header.light";
    pub const HEADER_PLAIN: &str = "core.header.plain";
    /// Corners of highlights, buttons and boxes: one of [`CORNERS`].
    pub const UI_CORNERS: Setting<Text> = Setting::new("core.ui-corners");
    /// The values of [`UI_CORNERS`]: square, slightly rounded, round,
    /// soft (rounder still). With round and soft ones, small shapes
    /// (buttons, switches, the focus) are pills.
    pub const CORNERS: &[&str] = &[
        "core.corners.square",
        "core.corners.small",
        "core.corners.round",
        "core.corners.soft",
    ];
    /// How the focus is drawn: a dark bar, a frame, a light bar (a frame
    /// on panels of black and white alone).
    pub const UI_FOCUS: Setting<Text> = Setting::new("core.ui-focus");
    pub const FOCUS_FILL: &str = "core.focus.fill";
    pub const FOCUS_RING: &str = "core.focus.ring";
    pub const FOCUS_SOFT: &str = "core.focus.soft";
    /// The lines between rows: light, faint from the text, none.
    pub const UI_SEPARATORS: Setting<Text> = Setting::new("core.ui-separators");
    pub const SEPARATORS_LINE: &str = "core.separators.line";
    pub const SEPARATORS_INSET: &str = "core.separators.inset";
    pub const SEPARATORS_NONE: &str = "core.separators.none";
    /// The sections of lists as they are or on rounded cards.
    pub const UI_GROUPS: Setting<Text> = Setting::new("core.ui-groups");
    pub const GROUPS_PLAIN: &str = "core.groups.plain";
    pub const GROUPS_CARDS: &str = "core.groups.cards";
    /// Size of headers' titles, in percent of the text's.
    pub const UI_TITLE: Setting<i64> = Setting::new("core.ui-title");
    /// Gray text such as hints and notes: as it is, or black for strong
    /// contrast ([`CONTRAST_STRONG`]), e.g. on screens with few grays.
    pub const UI_CONTRAST: Setting<Text> = Setting::new("core.ui-contrast");
    pub const CONTRAST_NORMAL: &str = "core.contrast.normal";
    pub const CONTRAST_STRONG: &str = "core.contrast.strong";
    /// How long short messages show, in seconds.
    pub const NOTICE_SECONDS: Setting<i64> = Setting::new("core.notice-seconds");
    /// Milliseconds a button or finger must be held for a long press.
    pub const LONG_PRESS_MS: Setting<i64> = Setting::new("core.long-press-ms");
    /// Base size of user interface text in points; UI metrics scale with it.
    pub const UI_SIZE: Setting<i64> = Setting::new("core.ui-size");
    /// The active key map preset (see [`keymap_presets`](super::keymap_presets)).
    pub const KEYMAP_PRESET: Setting<Text> = Setting::new("core.keymap-preset");
    /// Scoped settings changed now apply to the active scope only, e.g.
    /// the open book ("Only this book"); switching it off returns the
    /// scope to the global values.
    pub const SCOPE_ONLY: Setting<bool> = Setting::new("core.scope-only");
    /// The font family for reading: [`DEFAULT_FONT`] (the plugins'
    /// choice) or one of the registered families.
    pub const READING_FONT: Setting<Text> = Setting::new("core.reading-font");
    /// The size of a book's text, in points; each book may have its own
    /// ([`SCOPE_ONLY`]). Every reader reads it.
    pub const FONT_SIZE: Setting<i64> = Setting::new("core.font-size");
    /// The value of [`READING_FONT`] for the plugins' choice.
    pub const DEFAULT_FONT: &str = "core.font.default";
    /// The font of the status lines while reading
    /// ([`fonts::STATUS`](super::fonts::STATUS)): [`STATUS_FONT_UI`],
    /// [`STATUS_FONT_BOOK`] or a font family's name.
    pub const STATUS_FONT: Setting<Text> = Setting::new("core.status-font");
    /// [`STATUS_FONT`]: as the menus.
    pub const STATUS_FONT_UI: &str = "core.status-font.ui";
    /// [`STATUS_FONT`]: as the book.
    pub const STATUS_FONT_BOOK: &str = "core.status-font.book";
    /// `core.clock.24h` or `core.clock.12h`.
    pub const CLOCK_FORMAT: Setting<Text> = Setting::new("core.clock-format");
    /// The first day of the week, one of
    /// [`time::WEEK_STARTS`](crate::time::WEEK_STARTS); read it with
    /// [`time::week_start`](crate::time::week_start).
    pub const WEEK_START: Setting<Text> = Setting::new("core.week-start");
    /// The time zone: `core.tz.auto` (the system's) or one of
    /// [`time::TIME_ZONES`](crate::time::TIME_ZONES).
    pub const TIME_ZONE: Setting<Text> = Setting::new("core.time-zone");
    /// A POSIX `TZ` rule that overrides [`TIME_ZONE`] when not empty.
    pub const TIME_ZONE_RULE: Setting<Text> = Setting::new("core.time-zone-rule");
}

/// Key map presets: named sets of bindings users switch between, e.g. for
/// devices with only two buttons. Plugins add more with
/// [`Registrar::add_keymap_preset`](crate::Registrar::add_keymap_preset).
pub mod keymap_presets {
    use crate::Id;

    /// No preset: the plugins' default bindings.
    pub const STANDARD: Id = Id::new_static(STANDARD_NAME);
    /// [`STANDARD`] as a static string, e.g. for setting values.
    pub const STANDARD_NAME: &str = "core.standard";
    /// Everything with "forward" and "back": short press moves, long press
    /// on forward confirms (opens the menu while reading), long press on
    /// back goes back.
    pub const TWO_BUTTONS: Id = Id::new_static("core.two-buttons");
    /// Taps on the left turn forward, on the right back: for the left
    /// hand.
    pub const LEFT_HANDED: Id = Id::new_static("core.left-handed");
}

/// The user's configuration file on the device's storage. Settings changed
/// in the UI are written back to it; comments and order are preserved.
pub const CONFIG_FILE: &str = "paperpaw.ini";

/// The folder with data plugins (one subfolder each) on the storage,
/// unless the shell names others.
pub const PLUGIN_DIR: &str = "plugins";

/// Groups of the generated settings screen; plugins can add their own.
pub mod groups {
    use crate::Id;

    /// Book text: font size, spacing, hyphenation, …
    pub const READING: Id = Id::new_static("core.group.reading");
    /// Screen: night mode, menu size, refresh.
    pub const DISPLAY: Id = Id::new_static("core.group.display");
    /// Buttons and gestures.
    pub const CONTROLS: Id = Id::new_static("core.group.controls");
    /// Books and folders.
    pub const LIBRARY: Id = Id::new_static("core.group.library");
    /// Language and everything about the device.
    pub const SYSTEM: Id = Id::new_static("core.group.system");
    /// How menus and lists look: text size, margins, rows, lines, headers.
    /// Themes (data plugins) set its defaults.
    pub const LOOK: Id = Id::new_static("core.group.look");
    /// Marks in the text: the looks of the roles of decorations (see
    /// [`decorations`](crate::decorations)), a section per role.
    pub const MARKS: Id = Id::new_static("core.group.marks");
    /// The status lines above and below a book's text (what they show,
    /// their size); the reader puts its settings for them here.
    pub const STATUS_LINES: Id = Id::new_static("core.group.status");
    /// Settings whose plugin names no registered group.
    pub const OTHER: Id = Id::new_static("core.group.other");
    /// For developers: measuring, trying, finding faults. Its settings show
    /// only with [`settings::DEVELOPER`](super::settings::DEVELOPER) on.
    pub const DEVELOPER: Id = Id::new_static("core.group.developer");
}

/// Sections of the settings' main page, with the setting groups and the
/// pages in [`menus::SETTINGS`] placed by their order: reading 1 to 49,
/// the device 51 to 199, customising PaperPaw from 201.
pub mod settings_sections {
    use crate::Id;

    /// Text, status bars, library, statistics, synchronisation.
    pub const READING: Id = Id::new_static("core.settings-section.reading");
    /// Display, controls, language and the system.
    pub const DEVICE: Id = Id::new_static("core.settings-section.device");
    /// Menus, plugins, advanced settings.
    pub const CUSTOMISE: Id = Id::new_static("core.settings-section.customise");
}

/// Parts of setting groups shared by plugins, in the order shown: a
/// setting names one with
/// [`SettingSpec::section`](crate::SettingSpec::section). Other parts
/// (any translation key) follow these.
pub mod setting_sections {
    /// The reading font, its size and weight.
    pub const FONT: &str = "core.setting-section.font";
    /// How a page is set: spacing, margins, justification, hyphenation.
    pub const PAGE: &str = "core.setting-section.page";
    /// What happens when reading: books opened again, bookmarks.
    pub const BEHAVIOUR: &str = "core.setting-section.behaviour";
    /// Reading aloud: its speed, what is marked, turning the pages
    /// (shells add the voice).
    pub const ALOUD: &str = "core.setting-section.aloud";

    /// The standard parts in their order.
    pub const ORDER: &[&str] = &[FONT, PAGE, BEHAVIOUR, ALOUD];
}

/// Sections of the main menu ([`menus::MAIN`]): plugins put their
/// entries in one by their order (reading 1 to 29, tools 31 to 79,
/// system 81 to 99).
pub mod sections {
    use crate::Id;

    /// Home, library and the like.
    pub const READING: Id = Id::new_static("core.section.reading");
    /// What plugins add: files, statistics, transfers, catalogs.
    pub const TOOLS: Id = Id::new_static("core.section.tools");
    /// Settings and the device.
    pub const SYSTEM: Id = Id::new_static("core.section.system");
}

/// Font roles.
pub mod fonts {
    use crate::Id;

    /// Menus, labels and other user interface text.
    pub const UI: Id = Id::new_static("core.font-ui");
    /// Book text.
    pub const READING: Id = Id::new_static("core.font-reading");
    /// Bold book text; readers fall back to [`READING`] without it.
    pub const READING_BOLD: Id = Id::new_static("core.font-reading-bold");
    /// Italic book text; readers fall back to [`READING`] without it.
    pub const READING_ITALIC: Id = Id::new_static("core.font-reading-italic");
    /// Bold italic book text; readers fall back to [`READING_BOLD`].
    pub const READING_BOLD_ITALIC: Id = Id::new_static("core.font-reading-bold-italic");
    /// Icons (see [`icons`](crate::icons)); without a font here, menus
    /// show text only.
    pub const ICONS: Id = Id::new_static("core.font-icons");
    /// The status lines while reading: as the menus ([`UI`]), as the book
    /// ([`READING`]) or a family of their own, as
    /// [`settings::STATUS_FONT`](super::settings::STATUS_FONT) says.
    /// Readers draw their status lines in it.
    pub const STATUS: Id = Id::new_static("core.font-status");
}

/// The name of the key map layer that applies everywhere.
pub const GLOBAL_KEYMAP_LAYER: &str = "global";

#[cfg(test)]
mod tests {
    use super::settings::*;
    use crate::document::Locator;
    use crate::event::EventData;

    #[test]
    fn standard_events_carry_what_they_say() {
        use super::events::{Goto, TextChosen};
        let goto = Goto::new("books/a.epub", Locator::new(3, 120));
        assert_eq!(goto.end(), None);
        let back = Goto::from_params(&goto.to_params()).unwrap();
        assert_eq!((back.at(), back.end()), (Locator::new(3, 120), None));
        let marked = goto.marking(Locator::new(3, 140));
        let back = Goto::from_params(&marked.to_params()).unwrap();
        assert_eq!(back.end(), Some(Locator::new(3, 140)));
        let chosen = TextChosen {
            action: crate::Id::new_static("share.send"),
            text: "Alice".into(),
            language: "en".into(),
            path: "books/a.epub".into(),
            section: 1,
            offset: 10,
            end_section: 1,
            end_offset: 15,
        };
        let back = TextChosen::from_params(&chosen.to_params()).unwrap();
        assert_eq!(back, chosen);
        assert_eq!(
            (back.start(), back.end()),
            (Locator::new(1, 10), Locator::new(1, 15))
        );
    }

    #[test]
    fn reading_carries_the_chapter() {
        use super::events::Reading;
        let reading = Reading {
            path: "books/a.epub".into(),
            title: "Alice".into(),
            progress: 12.5,
            section: 3,
            offset: 120,
            page: Some(40),
            pages: Some(300),
            chapter: "Down the Rabbit-Hole".into(),
            chapter_left: Some(7),
        };
        let params = reading.to_params();
        assert_eq!(params.get("chapter_left"), Some("7"));
        assert_eq!(Reading::from_params(&params).unwrap(), reading);
        assert_eq!(reading.at(), Locator::new(3, 120));
        // A book without a table of contents, its pages not counted.
        let unknown = Reading {
            page: None,
            pages: None,
            chapter: "".into(),
            chapter_left: None,
            ..reading
        };
        let params = unknown.to_params();
        assert_eq!(params.get("chapter_left"), None, "absent, not empty");
        assert_eq!(Reading::from_params(&params).unwrap(), unknown);
    }

    #[test]
    fn every_colour_but_ones_own_is_known() {
        for value in TEXT_COLORS.iter().chain(PAPER_COLORS) {
            let own = *value == OWN_TEXT || *value == OWN_PAPER;
            assert_eq!(rgb(value).is_none(), own, "{value}");
        }
    }

    #[test]
    fn colour_codes() {
        assert_eq!(parse_rgb("#5b4636"), Some([0x5b, 0x46, 0x36]));
        assert_eq!(parse_rgb(" F4ECD8 "), Some([0xf4, 0xec, 0xd8]));
        assert_eq!(parse_rgb("#543"), Some([0x55, 0x44, 0x33]));
        for wrong in ["", "#", "#12345", "#ggg", "blue", "#1234567"] {
            assert_eq!(parse_rgb(wrong), None, "{wrong}");
        }
    }
}
