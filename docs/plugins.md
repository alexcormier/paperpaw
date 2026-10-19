# Writing plugins

Everything PaperPaw shows and does comes from plugins: the home screen,
the reader, the library, the menus, the settings, even the keyboard. The
kernel only hosts them. A plugin you write can do anything these do, and
replace any of them.

There are two kinds:

- **Data plugins**: a folder with a `plugin.ini` and data files, no code.
  They bring translations (also whole new languages), key maps, new
  defaults for any setting, and fonts. They work on every device as they
  are; see [Plugins without code](#plugins-without-code).
- **Code plugins**: Rust crates against the SDK (`paperpaw-sdk`). They
  bring screens, actions, settings, formats, widgets, services, anything.
  Today they are compiled into a build of PaperPaw; plugins loaded at run
  time (WebAssembly, with the same API) are planned.

This guide walks through a code plugin, then lists what plugins can
contribute. Working with an AI assistant: the skill in
[`.claude/skills/paperpaw-plugin-development`](../.claude/skills/paperpaw-plugin-development/SKILL.md)
tells it how to write a plugin for PaperPaw as it is (without changing
PaperPaw), try it in the emulator and on an X4, and measure it; copy the
folder into a plugin's own project (or one's own skills) to use it
there. The SDK's API documentation (`cargo doc -p paperpaw-sdk
--open`) has every detail.

## The sample plugin

[`samples/hello-plugin`](../samples/hello-plugin/src/lib.rs) is a small
plugin to start from. It adds *Hello* to the tools menu, which opens a
screen greeting you by a name set in the settings, with the number of
books opened since the start. Try it in the emulator:

```sh
cargo run -p paperpaw-emulator --features hello -- --profile boox-go-7 --set hello.name=Ada
```

Open the main menu (a tap at the bottom of the home screen), then
*Tools*, then *Hello*.

### The plugin

A plugin is a type implementing [`Plugin`]: a manifest and a `register`
function. `register` runs once at start-up and adds the plugin's
contributions through a [`Registrar`]:

```rust
pub const PLUGIN: Id = Id::new_static("hello");
pub const OPEN: Id = Id::new_static("hello.open");
pub const SCREEN: Id = Id::new_static("hello.screen");

pub struct Hello;

impl Plugin for Hello {
    fn manifest(&self) -> Manifest {
        Manifest {
            id: PLUGIN,
            name: Cow::Borrowed("Hello"),
            description: Cow::Borrowed("A sample plugin to start from."),
            version: Version::new(0, 1, 0),
            sdk: SDK_VERSION,
        }
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_action(ActionSpec::new(OPEN, "hello.title").handler(|_, out| {
            out.push(Command::OpenScreen(SCREEN, Params::new()));
        }))?;
        r.add_menu_entry(MenuEntry { menu: menus::TOOLS, item: OPEN, order: 90 })?;
        r.add_icon(OPEN, Cow::Borrowed("star"))?;
        // … the screen, a setting, an event, translations
        Ok(())
    }
}
```

Every id starts with the plugin's id (`hello.open`); `core` is the
kernel's. Titles are translation keys, translated where they are shown.

Icons are named as in [Tabler Icons](https://tabler.io/icons), any of
them (`paperpaw_sdk::icons` knows every name). PaperPaw's icon font holds
the ones it uses itself; for another, bring its drawing as an icon pack
(see below), else none shows.

### Reading and changing: `Env` and commands

Plugins never change the kernel's state directly. They read it through
[`Env`]: the device, the settings, the translations, the fonts, the
storage, the time, the battery, the passwords and keys. They change it
with [`Command`]s: open a screen or slot, go back, change a setting,
emit an event, run an action, show a short message, make an HTTP
request, keep a password, and so on. The kernel runs the commands after
the plugin's code returns.

This keeps plugins simple to test and the API message-based, so the same
model works for sandboxed plugins later.

### Screens

A screen implements [`Screen`]. `draw` paints it; `on_action` reacts to
actions (from buttons, or gestures bound to them); `on_gesture` to taps,
long presses and swipes; `on_event` to events while it is open:

```rust
impl Screen for HelloScreen {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let title = env.i18n.tr("hello.title");
        self.header.draw(canvas, env, &self.style, title, None, None);
        // … the greeting, centred below
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        if *action == actions::CONFIRM || *action == actions::BACK {
            cx.command(Command::Back);
            return Handled::Yes;
        }
        Handled::No
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        // A tap on the header's back arrow, or anywhere else, closes it.
    }
}
```

Screens are made by a factory each time they open
(`r.add_screen(ScreenSpec { id, title, factory })`). Return
`Handled::No` for what a screen does not handle: the kernel then runs
the action's own handler (e.g. the menu button opens the menu).

Some rules every screen should follow:

- **Touch and buttons alike.** Some devices have only buttons (the
  Xteink X4), some mostly touch (the Boox). Everything must be easy with
  either: buttons move a focus and confirm, taps act at once. Show the
  focus only once buttons are used (`ListView::focus_on_keys`).
- **`draw` must not change state.** On small devices a frame is drawn in
  bands, so `draw` may run several times for one frame. It can skip what
  lies outside `canvas.clip()`. Small files and sizes it reads come from
  the card once per frame (the kernel keeps them while it draws), but
  work on what was read (parsing, sorting, decoding) is done again for
  every band: keep it in the screen, made when it opens or its data
  changes.
- **Ask for redraws.** After a change, `cx.request_redraw()` (or a part
  of the screen) and `cx.request_refresh(mode)`: `Fast` for a moving
  focus, `Partial` for new content, `Full` to clear ghosting.
- **Overlays** are screens with a `region`. One that takes the reader's
  attention (a menu, a question) washes out what lies below it
  (`dims_below`); one looked through (a keyboard over the field it
  types into, a notice) does not. A screen whose content is best in
  grays (a picture) says so (`grays`): panels with few grays then show
  it in more, as slowly as a full refresh.
- **Use the SDK's UI parts** (`paperpaw_sdk::ui`): headers, rows, lists,
  switches, numbers with − and +, messages, icons. They follow the user's
  appearance settings and line up with the rest of PaperPaw.

Screens say what they show, for reading the interface aloud,
automation and tests: the `ui` helpers (list rows, buttons, switches,
headers, labels) tell what they draw by themselves. A screen that draws
in its own way tells it with `canvas.describe(role, text, area,
focused, on)`.

### Settings

A setting is a constant with its id and the type of its value
(`Setting<bool>`, `Setting<i64>` or `Setting<Text>` for choices,
languages, lists and text):

```rust
pub const NAME: Setting<Text> = Setting::new("hello.name");

r.add_setting(
    SettingSpec::text(&NAME, "hello.setting.name", "")
        .group(groups::OTHER)
        .description("hello.setting.name.help"),
)?;

let name: &str = env.settings.read(&NAME);
out.push(Command::set(&NAME, "Ada".into()));
```

`SettingSpec::switch`, `number`, `choice`, `language`, `list` and
`text` make the others, their defaults of the setting's type. The
settings screens list every setting by its group, with its description
below it; the kernel saves changes to `paperpaw.ini`. Settings can be
numbers with units and steps, switches, choices, languages or text;
`advanced()` hides rarely needed ones, `scoped()` lets one be set per
book, `editor(screen)` opens a screen of your own instead. Settings
known only at run time (one per widget, say) are read by their ids,
`env.settings.text("…")` and so on. Passwords and keys do not belong in
settings: see [Passwords and keys](#passwords-and-keys).

Each setting built this way takes about 200 bytes while the device
runs. A plugin with many settings keeps them in the program instead, in
a `static` list (on the X4 it stays in flash, next to no memory):

```rust
const GROUP: Id = Id::new_static("hello.clock");
static SETTINGS: [SettingSpec; 2] = [
    SettingSpec::fixed_switch("hello.clock.date", "hello.setting.date", true, GROUP),
    SettingSpec::fixed_choice("hello.clock.face", "hello.setting.face",
        &["hello.face.plain", "hello.face.ticks"], "hello.face.ticks", GROUP)
        .described("hello.setting.face.help"),
];

r.add_settings(&SETTINGS)?;
```

Such settings are read like any other; those that need a unit, a step,
a section or an editor screen are made as above.

### Events

```rust
r.on(events::BOOK_OPENED, move |env, reading, out| {
    opened.set(opened.get() + 1);
    // reading.path, reading.title, reading.progress, reading.at() …
})?;
```

The kernel and plugins emit events; any plugin can listen. An `Event<P>`
names an event and what it carries, a struct made with `event_data!`,
so emitting (`EVENT.emit(data)`) and hearing (`r.on`) are checked as
the plugin is built; screens get every event too and read one with
`EVENT.read(event, params)` in `Screen::on_event`. Standard ones are in
`builtin::events`: started, a setting changed, a book opened, a page
turned, a book moved, books came into the library or left it, its
bookmarks or highlights changed, a word looked up, a place in the open
book to show, text chosen, notes exported, the device suspends or
resumes. Plugins define their own the same way (in their namespace),
and `Command::Schedule` emits
one at a time of day. Underneath, what events carry travels as named
strings (`Params`): events known only at run time are heard by id with
`r.subscribe`.

Answers come as events too, typed in `replies`: a plugin names the
event it wants, e.g. `const FETCHED: Event<HttpReply> =
Event::new("news.fetched")`, and passes it with the request. To ask
the reader, `ask::text("Server", &ENTERED).initial(url).command()`
opens the keyboard and brings back a `Typed`, `ask::folder` a
`Chosen`, and `choice::open` a `Picked`. What the plugin added
(`.with(key, value)`) comes back as the answer's `context`, to tell
its questions apart.

What is known of books is a service, not a screen's: `library` has a
book's details (title, author, series, tags, cover, progress, and
fields plugins add with `library::set_field`), the books read last and
the favourites, so any plugin can show or use them. Views of many books
show covers made small and kept on the storage (`library::cover`, a few
kilobytes each, not the picture) and take a book's details with the
size a listing told (`library::listing`, `library::book_sized`): on a
card, each file looked up again is a search through its folder.

The bookmarks and highlights of books are shared: `annotations` reads
and keeps them, and whoever changes them emits
`annotations::changed(book)`, so the reader shows them anew and syncs
pass them on (the `koreader` plugin, for example).

State a plugin keeps between events lives in an `Rc` it hands to its
handlers when it registers.

### Translations

```ini
[meta]
language = en
name = English
plural = one-other

[messages]
hello.title = Hello
# {name} is the name set, {count} the books opened since the start.
hello.greeting.one = Hello, {name}! You opened {count} book.
hello.greeting.other = Hello, {name}! You opened {count} books.
```

Every text a plugin shows comes from catalogs like this, added with
`r.add_translations(…)`; English first, then any other language. `tr`
translates, `format` fills `{placeholders}`, `plural` picks the form for
a number. A test that all languages have the same keys belongs in every
plugin (see the sample).

### Testing

Plugins are tested with the real kernel and an in-memory storage, as the
sample's [test](../samples/hello-plugin/tests/hello.rs) does: build a
kernel with the plugin, send it input, look at the open screen, the
settings, the files, the HTTP requests it makes (`Kernel::take_io`)
and answer them (`Kernel::http_response`). The emulator then shows how it
looks and how much memory it needs on each device:

```sh
cargo run -p paperpaw-emulator -- --profile xteink-x4 --headless --script "confirm" --screenshot out.png
```

## What plugins contribute

| Contribution | Registrar | What it is |
|---|---|---|
| Actions | `add_action` | Something to do, bound to buttons and gestures, listed in menus |
| Screens | `add_screen` | Full screens and overlays (menus, dialogs, panels) |
| Slots | `add_slot`, `bind_slot` | Roles a screen fills: home, reader, library, menu, settings, keyboard, dictionary, … |
| Menu entries and sections | `add_menu_entry`, `add_menu_section` | Actions, settings and submenus in any menu |
| Icons | `add_icon` | The icon of an action, setting group or menu |
| Settings and groups | `add_setting`, `add_setting_group` | Shown and saved by the kernel |
| Setting defaults | `set_setting_default` | New defaults for other plugins' settings (themes) |
| Event handlers | `subscribe` | React to events |
| Key maps | `add_keymap`, `add_keymap_preset` | What buttons, taps and swipes do |
| Formats | `add_format` | Open a kind of file as a document to read; big pictures may be decoded when their page shows (`Document::prepare_image`) |
| Fonts | `add_font`, `add_font_file`, `bind_font` | Fonts and the roles they fill |
| Hyphenation | `add_hyphenation` | Patterns for a language |
| Text filters | `add_text_filter` | Change a book's text before it is laid out |
| Decorators | `add_decorator` | Mark words on book pages (a background, a line, a frame), drawn the same by every reader |
| Input filters | `add_input_filter` | See every button and finger first: pass it, take it, or run an action instead |
| Refresh filters | `add_refresh_filter` | Change how the panel is refreshed, e.g. in full on a screen of your own |
| Status items and lines | `add_status_item`, `add_status_line` | What status lines show |
| Widgets | `add_widget` | Blocks for the home screen and the sleep screen |
| Setup steps | `add_setup_step` | A page of the first-run setup: a setting to choose or a screen of the plugin's, where the device needs it |
| Services | `add_service` | Anything one plugin offers others, found with `env.services.get` |
| Contributions | `contribute` | Additions to a point another plugin defined |
| Translations | `add_translations` | Catalogs for any language |
| Book data | `add_book_data` | Where the plugin keeps something for each book (named by `library::key`): moved and removed with the book, cleaned up when it is gone |

Slots are how a plugin replaces another: bind your screen to
`core.home` and it is the home screen; the user's
`[slots]` configuration has the final word. Built-in slots are in
`builtin::slots`, built-in menus in `builtin::menus`: the main menu
(`core.main`), its tools (`core.tools`) and the settings' pages
(`core.settings-pages`).

### What every reader offers

A plugin that works with books never names the reader plugin: it uses
what `builtin` defines for every reader, so a reader of one's own (bound
to `core.reader`) replaces the shipped one in full.

| What | Standard | |
|---|---|---|
| Show a place in the open book, a part marked | `events::GOTO` (`Goto`) | readers take it; tables of contents, bookmarks, search and syncs send it |
| Text chosen, and the action chosen for it | `menus::SELECTION`, `events::TEXT_CHOSEN` | add an action to the menu, hear the event |
| Notes exported as Markdown | `events::NOTES_EXPORTED` | for notes apps and syncs |
| The reader's menus | `menus::READER`, `READER_MORE`, `READER_PANEL` | add entries there |
| Font size | `settings::FONT_SIZE` | the kernel's setting, which readers read |
| Status lines' settings | `groups::STATUS_LINES` | the group readers put them in |
| Reading aloud's settings | `setting_sections::ALOUD` | the section readers and shells put them in |
| Key bindings while reading | the layer `[keymap.core.reader]` | the slot's layer, whichever screen fills it |

A reader in turn sends the reading events (`BOOK_OPENED`, `PAGE_TURNED`,
`BOOK_CLOSED`, `BOOK_FINISHED`), takes `GOTO`, and offers its actions
in these menus.

### Services and points of your own

A plugin can offer others more than the kernel knows of. A service is
any value published under an id in its namespace, usually a trait
object whose trait the plugin's crate exports; others use it when it is
there and do without it when not:

```rust
// The publishing plugin.
r.add_service(WEATHER, Box::new(Box::new(Forecast::new()) as Box<dyn Weather>))?;

// Any other plugin, wherever it has an `Env`.
if let Some(weather) = env.services.get::<Box<dyn Weather>>(WEATHER.as_str()) {
    let today = weather.today();
}
```

A point is an id a plugin names, with what a contribution there is in
its documentation (for example one line per entry); others add to it
with `r.contribute(point, text)`, and the plugin reads
`env.services.contributions(point)`, each with the plugin it came from.

### Marks in the text

A plugin marks words on book pages with a decorator (ADR 0021): it gets
the words of a page with their places in the book and gives back ranges,
each with a *role*. The reader draws them; the plugin knows neither the
reader nor the panel.

```rust
use paperpaw_sdk::decorations::{self, Decorated, Decoration, DecoratorSpec, LineKind};

const LEARNING: Id = Id::new_static("vocabulary.learning");

// The role: its look is settings (Settings, Marks in the text), the
// plugin gives the default; themes and users change it.
decorations::add_role(r, &LEARNING, "vocabulary.role.learning",
    Decoration::NONE.line(LineKind::Dotted))?;
r.add_decorator(
    DecoratorSpec::new(Id::new_static("vocabulary.words"), "vocabulary.decorator",
        |env, page| {
            page.words
                .iter()
                .filter(|w| is_learnt(env, w.text))
                .filter_map(|w| Decorated::words(core::slice::from_ref(w), &LEARNING))
                .collect()
        })
    .switch(&MARK)          // a bool setting turns it on and off
    .taps()                 // taps on its marks come back
    .signature(|env| words_changed(env)),
)?;
r.on(events::DECORATION_TAPPED, |env, tapped, out| { /* tapped.text */ })?;
```

- A `Decoration` is a background, the text's ink, a line under the text
  (solid, dotted, dashed, wavy, double) with its tone and thickness, a
  frame, struck through; tones are named, and each panel shows them as
  well as it can (colours, or their lightness, dithered where there are
  few grays).
- Each aspect of a role is a setting of its own (`<role>.background`,
  `.ink`, `.line`, `.line-tone`, `.thickness`, `.frame`, `.struck`), so
  profiles, rules and themes set them like any other.
- Decorators are asked once a page is laid out, and again when their
  signature changes (their data, their settings): keep it cheap, and
  never work on a whole book.
- Where marks meet on a word, the earlier decorator (by `order`) wins an
  aspect both set; the rest combine.
- The reader's own marks are roles too: `roles::HIGHLIGHT`, `LINK`,
  `SEARCH_HIT`, `ALOUD`. A reader of one's own draws all marks with
  `decorations::draw_behind` and `draw_over`.
- What changes widths (bold, a larger size) is a text filter instead: it
  lays the page out again.

### Widgets

A widget is a [`Widget`] a screen makes and keeps
(`env.widgets.make`): it reads what it shows in `update`, which the
screen calls when it shows the widget, about each minute and after
events, and only draws in `draw` (which runs for every band of a
frame). Register it with `WidgetSpec::new(id, title, factory)`; a widget
with nothing to read, such as a clock, is a pair of functions
(`WidgetSpec::drawn` with `Drawn::new(height, draw)`, and `shows` to be
drawn again when what it shows changes). A widget whose content changes
only after events (books, settings) says so (`live` false) and is not
read again each minute; one that can be drawn smaller says how small
(`min_height`), so that a grid fits it where its full height has no
room. `widgets::grid` puts widgets
and tools in a grid as the home screen does, edited in place
(`GridView`). Say which kind a widget is
(`.category(widgets::categories::TIME)`, or `BOOKS`, `READING`, `WORDS`,
`PERSONAL`, `WORLD`, `DEVICE`): the home screen offers widgets by their
kind, each drawn as it would show.

### Widgets with settings

A widget may have settings of its own, such as where its text sits. Put
them in a settings group with the widget's id
(`widgets::settings_group`); the settings list leaves such groups out,
and the home screen's editor offers them as the widget's *Settings*. For
the alignment there is a ready setting: register
`widgets::align_setting(&id, Align::Left)` and place each text with
`widgets::align(env, id).x(area, width)`.

Widgets of all plugins share a look the reader shapes for each, so that
they sit well together on a page: give yours a frame and draw in it.

```rust
const ID: Id = Id::new_static("my.card");
static SETTINGS: [SettingSpec; 4] = [
    widgets::align_spec("my.card.align", ID, Align::Left),
    widgets::frame_spec("my.card.frame", ID, Frame::None), // none, line, light or dark card
    widgets::size_spec("my.card.size", ID, Size::Medium, true), // true: also "as large as it fits"
    widgets::part_spec("my.card.author", ID, "my.show-author", true),
];
r.add_setting_group(widgets::settings_group(&ID, "my.card"))?;
r.add_settings(&SETTINGS)?;

fn draw(&self, canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect) {
    let look = widgets::draw_frame(canvas, env, ID.as_str(), area);
    let rest = widgets::draw_caption(canvas, env, ID.as_str(), &look, "Today");
    let size = widgets::size(env, ID.as_str()).text(&UiStyle::new(env));
    // ... the content in `rest`, in `look.ink` (and `look.faint` for
    // what matters less), never in a fixed black: on a dark card it is
    // light.
}
```

A widget may be placed more than once, each placement with its own
values of the widget's settings: the home screen places a clock as
`widgets.clock@1` and `widgets.clock@2`, whose frames are the settings
`widgets.clock@1.frame` and `widgets.clock@2.frame` (saved like any
other, the widget's own `widgets.clock.frame` where one sets none).
`env.widgets.make` gives a placement's widget, which reads its settings
by their usual ids and gets that placement's values
(`Settings::with_placement`); a widget need do nothing for it. Screens
of their own that place widgets use `Id::placement` and forget a
placement's values when it goes (`Settings::placement_values`).

`widgets::shows(env, id, "author")` reads a part's switch,
`widgets::choice` makes other choices of the widget's own, and
`widgets::draw_message` says kindly what is not there yet. A widget's
`height` counts its frame (`widgets::look::look` gives the area inside
it without drawing). For round things there are `Canvas::fill_circle`,
`stroke_circle`, `fill_ring` (arcs, e.g. a progress ring) and
`draw_hand`, with angles as on a clock face (`paperpaw_sdk::round`);
light grays are drawn with `fill_shaded` or `fill_round_shaded`, which
dither them on black and white panels.

Screens that place widgets one below the other should know the kernel's
two widgets for room between them: `widgets::SPACE` is an ordinary
widget, a gap; `widgets::FILL` has no height of its own and takes its
share of the room the others leave (`widgets::fill_height`). Both may
be in a list more than once (`widgets::repeatable`).

### Presets and home screen layouts

A preset is a ready-made set of setting values chosen as a whole
(`paperpaw_sdk::presets`), such as a layout of the home screen. It has a
kind, named by the plugin that offers such presets: the home screen
offers those of `home.layout` in its chooser (in the first-run setup,
and under *Add* while editing it), shows each on itself before it is
kept, and applies the one chosen. Add presets kept in the program with
`Registrar::add_presets` (a `static` list of `Preset::fixed`), for your
own kind or another plugin's:

```rust
static LAYOUTS: [Preset; 1] = [Preset::fixed(
    "my.nook",
    "home.layout",
    "my.nook",      // translation keys of its name
    "my.nook.help", // and of a sentence on it
    "home.page-1 = widgets.word-clock@1 0/0 4x2, home.continue@1 0/2 4x2
home.rows = 6",
)];

r.add_presets(&LAYOUTS)?;
```

A layout sets the pages (`home.page-1` to `home.page-4`, as
`widgets::grid` writes them) and whatever of the home screen's look it
wants (`home.columns`, `home.rows`, `home.status-bar`,
`home.footer.dots` ...); what it leaves out goes back to its default.
Widgets placed as `id@1` have settings of their own in it
(`home.recent@1.count = 8`), so a layout gives them their looks without
touching the widget's other placements. To offer presets of a kind of
your own, list them (`env.settings.presets().of_kind(kind)`), work out
what one changes (`Preset::changes` with the settings it covers), show
it (`Command::PreviewSettings`: the values show on every screen without
being kept, until the screen that asked closes) and keep it
(`presets::commands`).

### Passwords and keys

Plugins keep passwords, API keys and tokens with
`Command::SetSecret(SecretChange::keep(id, label, value))` and read them
with `env.secrets.get(id)`. They are all listed in *Settings, Passwords
and keys*, where the user changes or removes them. Ask for one on the
keyboard in password mode (`params::MODE` `password`); the keyboard
offers to type it on a phone. They are kept apart from the settings and
not readable at a glance, but not encrypted: the devices have no safe
place for a key.

### The network

`Command::Http` makes a request where the device has a network
(`env.caps.network`); the answer comes as the request's reply event, an
`HttpReply` with the status and the body, or saved to a file (`save_to`)
for large ones.
Read large JSON answers as they stream with `paperpaw_json::stream`
(the crate `paperpaw-json`), so they
need little memory. `Command::Serve` serves HTTP to the local network,
e.g. for transfers from a computer or typing on a phone; what happens
to it comes as `ServerNotice`s, and to sockets (`Command::Connect`,
`Command::Broadcast`) as `SocketNotice`s.

### Firmware updates

Where PaperPaw is the device's firmware (the X4), `env.firmware` says
which version runs and how an install goes; elsewhere it is `None`.
`Command::Firmware(FirmwareCommand::Install { path })` installs a
signed update file, `FirmwareCommand::Restart` restarts into it once
`FirmwareState::Installed`; `events::FIRMWARE_CHANGED` comes on the
way. `firmware::Update::read` tells what a file holds before. The
`update` plugin does all this for update files on the card; a plugin
that fetches updates from somewhere else only needs to put the file
there.

## Memory

The smallest device, the Xteink X4, has 256 KiB of heap for everything.
A plugin that should run there:

- keeps little in memory: lists in files, read as they are needed;
- reads files a piece at a time (`Storage::read_at`), never whole books;
- writes downloads straight to storage (`HttpRequest::save_to`);
- reserves large buffers with `try_reserve` and copes when that fails;
- checks `env.device.memory` (`MemoryClass::Small`) to do less there.

The emulator's X4 profile enforces the budget, and `--heap-report` shows
where memory goes.

## Plugins without code

A data plugin is a folder:

```text
plugins/large-print/
  plugin.ini     [plugin] id, name, description, version, formats
  i18n/*.ini     translation catalogs (also whole new languages)
  keymap.ini     key bindings
  settings.ini   new defaults for any plugin's settings ([reader] font-size = 14)
  fonts/*.ttf    fonts to read with (also .otf, .ttc), by family
  fonts/*.ppfp   packs with letters other fonts lack, named in plugin.ini:
                 [extends] cyrillic.ppfp = fonts-default.literata
  fonts/…        a family's faces, named in plugin.ini: [family] name,
                 regular, bold, italic, bold-italic (packs, or .ttf/.otf
                 where scalable fonts are drawn)
  styles.css     a stylesheet for books, after a book's own (EPUB)
  presets.ini    ready-made sets of settings, e.g. home screen layouts
```

`formats` says which version of these files' formats the plugin is made
for (1 now, and if it is left out); a PaperPaw that reads an older one
leaves the plugin out and says why, rather than reading it wrongly.

Packs for other scripts (`tools/script-pack.py` builds Cyrillic and
Greek for Literata) are read a glyph at a time, so devices with little
memory can use them ([ADR 0018](adr/0018-font-packs-for-other-scripts.md)).
Icon packs are such packs for the icon font: `tools/icons/pack.py` builds
one with the Tabler icons named
(`[extends] icons.ppfp = fonts-default.icons`).

A family of its own needs a name and a regular face; its faces are
packs, drawn on every device and read a glyph at a time, or font files,
drawn where scalable fonts are, a variable font at its style's weight:

```ini
[family]
name = Fredoka
regular = regular.ppfp
bold = bold.ppfp
```

A logo of its own takes the place of PaperPaw's (on the sleep screen,
the About page and the logo widget): a picture in the folder, named in
`settings.ini`:

```ini
[core]
logo = plugins/my-brand/logo.png
```

The fonts browser (`plugins/fonts-browser`) downloads fonts as such
plugins from paperpaw.org: font files for devices with scalable fonts
and the memory for them, packs for the others. The store
(`plugins/store`, *More plugins*) lists every other data plugin of the
plugin library made for the device and installs, updates and removes it
([ADR 0023](adr/0023-plugin-store.md)). Both build on the SDK's
`install` module: an `Installer` downloads a `Package`'s files one after
another into `plugins/.<id>.part`, checks each by its size and SHA-1,
then has the kernel install it (`Command::InstallPlugin`) in place of
every version of it before, what was chosen from it still chosen; the
job is kept on the storage, so it goes on with no screen open.

Copy it into the `plugins` folder of a device (or of the emulator's
library) and restart, or have a plugin load it at once
(`Command::LoadPlugin` with its folder; `Command::RemovePlugin` removes
one, its fonts and presets at once, the rest after the next start). A plugin's `styles.css` comes after a book's own
stylesheets and before one's own (`styles.css` beside the books, then
the one named like a book), so the more particular wins; plugins with
code add theirs with `Registrar::add_stylesheet`.

A `presets.ini` holds presets (see above), a section each: its `kind`,
its `name` and `description` (translation keys from its `i18n`, or the
text itself), then the settings it sets by their whole id:

```ini
[nook]
kind = home.layout
name = reading-nook.nook
description = reading-nook.nook.help
home.page-1 = widgets.word-clock@1 0/0 4x2, home.continue@1 0/2 4x2, home.recent@1 0/4 4x2
home.rows = 6
```

So anyone can make a home screen layout and share it: it is offered
with the home screen's own. [`samples/plugins`](../samples/plugins) has
three: large print, an airy theme and a layout (reading nook).

[`Plugin`]: ../crates/paperpaw-sdk/src/plugin.rs
[`Registrar`]: ../crates/paperpaw-sdk/src/plugin.rs
[`Env`]: ../crates/paperpaw-sdk/src/env.rs
[`Command`]: ../crates/paperpaw-sdk/src/command.rs
[`Screen`]: ../crates/paperpaw-sdk/src/screen.rs
[`Widget`]: ../crates/paperpaw-sdk/src/widgets.rs

Other protocols go over sockets ([ADR 0014](adr/0014-sockets-for-plugins.md)):
`Command::Connect` opens a TCP connection, `Command::SocketSend` sends,
and `Command::SocketRead` asks for what comes next, as text
(`SocketRead::Text`) or as a file of a known length written to storage as
it comes (`SocketRead::File`), so files of any size never need to fit in
memory. `Command::Broadcast` finds services in the local network by a
UDP datagram and their answers. Everything comes back as events
(`socket_params`); a socket closes when the screen showing as it was
opened is closed.

