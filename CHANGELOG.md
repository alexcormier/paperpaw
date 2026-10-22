# Changelog

What changes from one release of PaperPaw to the next. Versions follow
[Semantic Versioning](https://semver.org); before 1.0, a minor version may
change the plugin SDK.

## [Unreleased]

- No building block shortens a label any more, even at the largest
  text: header bars set a long title on two lines, smaller where needed;
  value, noted and switch rows wrap their titles and grow
  (`ui::value_row_height`, `ui::noted_row_height`,
  `ui::switch_row_height`), the value whole beside the wrapped title or,
  where the title's words do not fit beside it, on a line of its own
  below; sliders do the same (`ui::slider::row_height_for`,
  `ui::slider::title_extra`, `ui::slider::band_below`,
  `SliderView::room`). Buttons set a long word on more lines
  (`ui::button_height_for`), and rows of buttons go into more rows where
  their words do not fit side by side (`ui::button_cells`,
  `ui::button_cells_height`, `Segments::fitted`, `Segments::height_for`).
  `ui::draw_address` wraps its hint and last line whole (smaller where its
  area is short; `ui::address_height`; `ADDRESS_HINT_LINES` is gone),
  `ui::OfflineOffer` keeps its words whole with its buttons between the
  margins (`OfflineOffer::height`), `ui::draw_empty` wraps a long title,
  and notes, messages and empty states say how high they need to be
  (`ui::note_line_height`, `ui::message_height`, `ui::empty_height`). The
  button hints below the screen set a long word smaller, then on two
  lines. `paperpaw-dev ui-kit` checks every page of the kit strictly.
  At the largest text sizes, where a row cannot hold a label on two lines
  even in smaller letters, rows (and every list with them) grow to two
  lines of small text; controls keep their size (`UiStyle::control_row`,
  which buttons, switches, ticks and pickers are sized from). At the
  default and smaller sizes nothing changes. Value rows too short for two
  lines set title and value smaller first. Header bars without arrows
  leave out the page number where the title is not whole beside it. The
  reading statistics go to one column, paged like a list, where their
  tiles do not fit side by side.
- `ask::confirm` asks yes or no on the screen of the new slot
  `core.confirm` (the kernel's `ui::confirm::Confirm`, a `ui::Dialog` in
  the middle of the screen, unless a plugin binds another): the question,
  an optional text, two buttons (*Yes* and *No* unless given words of
  their own), the focus on no unless `focus_yes`. The answer is a
  `replies::Confirmed` with `yes` and the question's context; *Back* or a
  tap outside answers nothing. Removing a font and resetting the keys ask
  this way.
- More building blocks for plugin screens: `ui::Chips` (filters or tags
  as wide as their words, one chosen; the library's filters use them),
  `ui::draw_text_field` with `ui::edit_text` (a one-line text in a row,
  typed on the keyboard's screen), `ui::draw_check_row` and
  `ui::draw_check` (lists where several may be ticked; looks may paint
  the box, `Painters::check`), `ui::TextView` (text longer than its room,
  scrolled by buttons, swipes and taps) and `ui::DatePicker` /
  `ui::TimePicker` (fields stepped with the buttons or by touch, months
  and leap years right). All are in `paperpaw-dev ui-kit`. No block
  shortens a title or a button's word: text fields put their box below a
  long title (`ui::text_field_height`), ticked rows and dialogs wrap
  their titles (`ui::check_row_height`), a dialog's buttons go one below
  the other where their words do not fit side by side, chips go onto a
  second row (`Chips::height_for`), and value rows set a long title on two
  small lines before they shorten it. `paperpaw-dev ui-kit` checks the
  kit in long German and the pseudo-language, a shortened label an error;
  a value shortened in a field is no longer reported. Dialogs that do
  not fit even in smaller sizes scroll their title and text above their
  buttons; header bars leave out their page count before shortening
  their title. `paperpaw-dev`: `--set` now applies to every command (it
  reached `perf` alone), `--labels-whole` makes any shortened label an
  error.
- Touch lock (in the screen lock plugin): *Lock touch* in the quick
  panel and the reader's menu, on touch screens. While it is on, taps,
  swipes and the pen do nothing on every screen and the buttons still
  work, e.g. to read with a hand on the screen; a small lock in the status
  lines says so. To unlock: tap twice with one finger and keep it down a
  moment (a hand resting on the screen does not do that, and a second
  finger starts it over), or on the X4 Pro hold the power button, or
  switch it off from a button bound to *Lock or unlock touch*. Off by
  default.
- Building blocks for plugin screens, so positions are not computed by
  hand: `ui::Stack` (parts one below the other, from the top and the
  bottom), `ui::columns` (equal columns that end flush with their area),
  `ui::Grid` (cells, what a tap hits, the focus moved by the buttons),
  `ui::draw_card` and `ui::card_inside` (a group on a card or in a frame,
  painted as the look paints widget frames), `ui::Segments` (a row of
  options, one chosen, also as tabs) and `ui::Dialog` (a box with a title,
  text and buttons, one space all round). The quick panel, the setup
  guide and the hints use them. `paperpaw-dev ui-kit` draws every
  building block on every device, in every look, light and dark.
- How it works: every plugin of PaperPaw says what it does, where it is
  and how it is used, read from the plugin manager. The vocabulary
  review has its first-time hints. Hyphenation has English texts.
- *Pictures in grays* (*Settings*, *Display*; on by default, on panels
  of black and white alone): a screen with pictures, such as the
  library's covers, the home screen's or a sleep screen's picture, is
  shown in four grays when it opens or changes whole, as pages with
  pictures already are. What changes on it after (the focus moving) is
  shown in black and white. The kernel counts the pictures of the
  screen drawn aside, where nothing is drawn or read. Covers kept small
  on such panels (thumbnails) keep four grays beside black and white
  for it (2.7 KB for a cover of 70 by 100 pixels instead of 0.9 KB), and
  those kept on the card are made anew. The emulator shows frames in
  grays as the X4 does.
- SDK: `Screen::pictures_in_grays` (yes by default) lets a screen say
  no to being shown in grays for its pictures (the reader decides
  itself, with `Screen::grays`); the setting is
  `settings::PICTURES_IN_GRAYS`.
- Pictures on panels with few grays (the X4s in black and white, four
  grays) are dithered by error diffusion (after Atkinson's, seven eighths
  of the error passed on) instead of a 4×4 ordered pattern: line drawings
  such as the cats stay clean, without the grid that made them look
  pixelated, and covers and photos keep their tones. A picture is
  diffused from its first row each time it is drawn, so bands and parts
  drawn again (a notice closing over it) show the same pixels. Each band
  takes it up where the band above left it, so its rows are still read
  once a frame; it takes two rows of error while it is drawn, nothing
  after the frame. Covers and
  pictures kept small in black and white (thumbnails) are diffused when
  they are made, and those kept on the card are made anew. Flat shades
  of the interface, and pictures with transparency, keep the calm
  ordered pattern.
- Vocabulary: flash cards with spaced repetition (FSRS, Anki's
  scheduler), in *Tools*, *Flash cards*. A review shows each due card's
  front, then its back and example and four answers (*Again*, *Hard*,
  *Good*, *Easy*) with when the card would come again; with the buttons
  alone the focus starts on *Good*, by touch a tap answers. New cards a
  day (20) and the retention wanted (90 %) can be set. *Edit with your
  phone* serves a page on the local network while it is open (a QR code,
  a key of its own each time): decks and cards are made, changed and
  deleted there, a list pasted as CSV or TSV, or an Anki deck (`.apkg`,
  exported for older versions) read in the phone's browser. A widget says
  how many cards are due. *A flash card due* can be the sleep screen: the
  front of the next card, answered on waking, as the review opens with
  it; *Review on waking* (off at first) opens the review on waking with
  any sleep screen. With *Words looked up become cards* (off at first)
  each word looked up goes into the deck *Looked up*, its sentence the
  example, when the flash cards open next. The sleep screen now closes
  itself on waking wherever it is, so what opened as the device woke
  stays. Only the due cards' state is held during a review; decks
  are text files in the plugin's folder, versioned.
- SDK: `paperpaw_sdk::served`, for pages served to a phone or computer
  on the local network: the key in the address (`key`), a `Guard` that
  counts wrong keys and turns requests away for a while after a few,
  `param` (a query's parameter, decoded) and `json_string`; and
  `ui::draw_address`, the address as a QR code and as text with a hint.
  The transfer plugin uses them.
- X4 Pro: the battery no longer shows 0 % after it was cut off (run flat
  or taken out). Its fuel gauge then starts asleep and forgets the
  battery's profile; PaperPaw gives it the profile again at the start
  and wakes it, as the stock firmware did.
- Hints: the first time a screen opens, a short note over it says what
  its controls do, the parts a tap goes to framed, for touch or for
  buttons as the device has them. A tap or any button closes it for
  good; *No more hints* switches them all off, as does a setting under
  System, where *Show hints again* has them shown once more. The home
  screen's gesture tips are now its hints; the reader, the library, the
  quick panel have theirs. SDK: `help` (`help::add` for a screen's
  hints, `help::hints` to read them, `help::wait` and `help::ready` for
  a screen that prepares first), the `help` plugin shows them.
- How it works: a plugin's own longer text (plain text files named by
  its manifest, a data plugin's `<id>.plugin.how`; `description` in the
  store's catalogue), to read paged from the plugin manager (hold a
  plugin) and the store's page of a plugin. `paperpaw-dev` warns where a
  plugin with code has none or a short one (`lint.how-it-works`). SDK:
  `Manifest::how`, `PluginInfo::how`, `readers::show_text`.
- Reading goal, a plugin of its own (in the store on the X4s): the daily
  goal of the reading statistics (one goal for both; 20 minutes where
  this plugin is), and a cat whose mood follows today's reading: sad
  while nothing was read, so-so after a little, content from half the
  goal, happy once it is reached (both shares are settings). The widget
  *Reading cat* shows it with the minutes read and the goal, on the home
  screen or the sleep screen's widgets; a tap opens the goal, or, where
  the statistics are off or missing, offers to switch them on or get
  them from the store. *Today's
  reading cat* is a sleep screen too. One's own pictures can take the
  cats' place (a folder with `sad`, `meh`, `content` and `happy`), also
  a data plugin's. The minutes come from the reading statistics. SDK:
  `events::READ_TODAY` (`ReadToday`: the day and the seconds read), which
  the statistics send as they record reading.
- The sleep screen shows a sleeping cat by default: PaperPaw's own
  drawing, in the middle of the screen with even margins, in grays where
  the screen can. It also shows where the cover or the pictures chosen
  are missing. The cover, the pictures, the message and the others can
  still be chosen. SDK: `image::picture_from_bytes` decodes a picture a
  plugin carries in its code, as `picture_from_file` decodes a file.
- Wi-Fi transfer without a router (X4, X4 Pro): *Without a router* on
  the transfer screen opens the reader's own network and shows codes to
  join it and to open the transfer page there; closing the screen (or
  ten minutes without a request) closes the network.
- Wi-Fi set up with a phone (X4, X4 Pro): the reader opens a network of
  its own and shows two QR codes, to join it and to open a page there
  that lists the networks found and takes the password with the phone's
  keyboard; the reader then joins that network and keeps the password.
  A `wifi.ini` on the card (`ssid = …`, `password = …`) is read once at a
  start, kept with the passwords and removed. SDK: `WifiCommand::Host`,
  `WifiState::Hosting` (servers serve on the own network).
- X4 and X4 Pro save power (ADR 0025): the radio sleeps between the
  network's beacons while connected and no data moves; on battery the
  processor sleeps while the panel refreshes instead of watching it; the
  panel's power goes off after a refresh once nothing follows (soon on
  the X4, after 30 s on the X4 Pro, whose next refresh takes longer
  then); going to sleep lets requests under way finish first, 15 s at
  the longest, a press keeping it awake (`Kernel::holds_sleep`). A
  press is counted as soon as it holds (the device stays awake while it
  settles); *Button response* (Controls) set to *Faster* reads the
  buttons more often on battery and keeps the X4 Pro's screen powered,
  for a little more battery.

- In Offline mode the store, an OPDS catalog, Calibre and Wi-Fi
  transfer offer to go online once, inline ("Offline mode. Go online for
  it?" with *Go online* and *Not now*), instead of failing. SDK:
  `ui::OfflineOffer` (sends `Command::GoOnline`).
- Plugin authors: `paperpaw-dev flows` is the main test of a plugin's
  actions. Every step of a flow is measured on the X4's budgets, the
  costliest one named (`flows.heap`, `flows.frame-time`), and each flow
  is played again at the edges (`flows.varied`): every step repeated,
  *Back*, *Home* or sleep and waking in the middle, network answers that
  fail, come late, are empty or huge, the pseudo-language, long data and
  an almost full storage. `paperpaw-dev explore` uses every target of
  every screen once, each from a fresh start, as a cheap baseline.
  `--flow` takes a folder of flows; `package` warns of flows and test
  pictures in a data plugin's folder (`package.tests`). The templates
  for a plugin's repository have a flow and run them in CI.
- The on-screen keyboard on touch screens no longer takes *Home* (or any
  other action that is no move) as the press that shows its focus: *Home*
  leaves a text field at once. While a password is typed it still keeps
  every button but *Sleep*, refresh and night mode.
- The home screen's ready-made layouts are gone: no chooser, no row in
  *Add*, no step in the first-run setup; the home screen has one default
  layout. Breaking: presets of the kind `home.layout` (a data plugin's
  `presets.ini`) are no longer offered anywhere; they are left unread.
  Presets stay for other kinds (the power modes).
- X4, X4 Pro and X4 Classic: calm status lines by default. While
  reading, the time at the left edge and the page at the right (the mark
  of reading aloud beside the time while it reads); on the home screen,
  the time and the battery. Every item can still be chosen.
- The keyboard on narrow screens (the X4 and X4 Pro): a row too wide for
  keys at least 6 mm wide is split in two in its order ("qwert" above
  "yuiop"), so the keys are about twice as wide; by touch and with the
  buttons as before. Wide screens keep their rows.
- Settings, Plugins, *Screens*: for every place several plugins offer a
  screen for (the home screen, the library, the reader, the quick panel,
  the lock, the keyboard, the sleep screen, …) the plugin that shows it,
  *Automatic* by default; kept in `paperpaw.ini` (`[slots]`), a choice
  whose plugin is gone counts as *Automatic*. The reading app books open
  with is chosen there too. When a plugin comes that offers a screen for
  such a place, it asks once: use it now?
- SDK: `Env::slot_choice` (the user's choice for a slot), `Env::slot_title`
  and `Env::screen_title`.
- *Clean up storage* is on the System page of the settings, and runs by
  itself, silently, while the device charges: a day after the first
  start, then at most once a week (where there is a clock). A settings
  group's page lists the actions plugins add to the menu named by the
  group's id.

- Fewer columns or rows on the home screen no longer leave a page empty:
  an item whose cells lie outside the grid now is moved inside it as
  little as can be (narrower or lower where it was wider or higher than
  the grid), else to the first free cells, and what a page has no room
  for any more goes on to the next page (`widgets::grid::fit`). Before,
  such items were kept but not shown: with 3 columns instead of 4 the
  first page of the X4 Pro showed nothing.
- About PaperPaw is a settings page with rows (version, source, plugins
  built in, code plugins); the licences, a chapter each and in full, and
  the list of the plugins built in open in the reader. The reader offers
  *Back* always only on a plugin's pages opened with the SDK's new
  `readers::open_page` (the browser's), no more on every document a
  plugin keeps, and draws it as the interface's buttons.

- The quick panel leaves the screen's status line as it is: it has no
  line of its own at its top any more (its settings *Top line*, *Top
  left*, *Top centre*, *Top right* and *Text size at the top* are gone;
  saved values of them are left unread).
- The home screen has 3 columns by default (was 4), so the tools' names
  fit; a layout placing things 4 wide keeps 4.
- The front light and its warmth are no longer in the main menu: they
  are in the quick panel and the reader's panel.
- No title is cut for a control any more: menus lay out a setting's row
  as the settings pages do (its control below its title where both do
  not fit); a header's title gets smaller beside its arrows, down to the
  body's size, before it is shortened, and so does a row's title beside
  its value, down to the small size; the skim bar puts its buttons in two
  rows where their names do not fit in one.
- No paws beside symbols: the row of hints shows each button's label
  alone (no mark for holding, no paw while a press is worked on), and an
  empty screen its icon alone. SDK, Breaking: `ui::draw_button_hints`
  takes no `held` any more.
- Web emulator: plugins' requests get the page's recorded answers
  (method and address pattern to status, headers, body and a delay of
  the device's time; redirects followed through them), offline and the
  same each time; one that no answer matches fails as without a network,
  and each request is told to the page (`request`). Served on the
  computer itself, the frame may fetch the rest (`network` `"real"`);
  nothing goes through paperpaw.org. The frame serves the page that
  wrote to it first, and tells that page alone. Frame messages
  `fixtures` and `network`; `Emulator::set_fixtures`,
  `set_real_network`, `requests`, `outbox`, `fetched`. A shell can send
  the emulator's requests elsewhere (`http::Route`, `http::set_route`).

- The web emulator takes the remote control's commands, as
  `tools/x4-remote.py` sends them to an X4 and parsed the same
  (`paperpaw_x4::remote`): screens and slots opened by id with
  parameters, actions run, settings set and read, events sent, what is
  shown described, the card's files and the clock, answered as JSON
  (`Emulator::command`, `pp_command`, the frame's `command`, `describe`,
  `run`, `slot`, `show`, `open`, `set`, `emit`), and waits until a screen
  or a text shows (`Emulator::shows`, `until`, `expect`). The remote
  control has `slot` and `show` on the X4 too.

- The home screen's clock: *as large as fits* fills its place (a cell
  of the grid, or the width where it is listed without one), the date
  below it in proportion, and *small*, *medium* and *large* are clearly
  apart: the time a share of the screen's width (15, 21 and 32 %),
  wherever it is placed as far as the place holds it. Medium, as large
  as the clock was on every device, is now the default. On the X4 and
  X4 Pro every size above 48 pixels looked the same: their fonts are
  bitmap packs whose text sizes end there. The UI font's pack keeps the
  digits, the colon and the dash at sizes from 56 to 164 pixels too
  (about 150 KB of flash, no memory), and the clock draws its time from
  them; "AM" or "PM" stands beside the digits at the date's size. The
  spacing setting shows as it should: the clock fills what is left.
- SDK: `FontHandle::display` draws a text of a few characters drawn
  large alone (a time, a number) at a pack's large sizes made for those
  characters; any other text stays at the sizes that have every
  character, one size for the whole text (`FontHandle::size_for`). Font
  packs may hold strikes of a few characters (`BitmapFont::size_for`),
  and a character a strike lacks comes from the nearest that has it.
- Widgets: *as large as fits* is as large as the place holds for the
  library's numbers (its digits from the large sizes too), today's
  reading, the vocabulary's words and the greeting; it stopped at three
  times a title's size before.
- Library: a book is deleted from its sheet (*Delete …*, last of its
  actions). The sheet asks in place, the book still in sight and the
  focus on *Keep it*; Back keeps it too. Deleted, the book goes with what
  is kept for it (its place, bookmarks, notes, details, covers and what
  plugins keep), by touch and by buttons alike. SDK: `Library::delete`
  and `Library::can_delete` (`library::Deleted`), so a library of one's
  own deletes its own way; the kernel's removes the files and says each
  is gone (`BOOK_REMOVED`). Only books are deleted: of a format known
  (by extension, or by the first bytes of a file without one), never in
  PaperPaw's own folder (`library::is_book_path`); what is not a book is
  refused (`Deleted::refused`), what the storage cannot delete says why
  (`Deleted::failed`), and the sheet tells which. On a sheet of pages
  the answers sit at its bottom edge. Tasks kept follow their books:
  switching back no longer opens a book deleted (its screens and those
  over them go, the others stay), and a book moved opens where it is
  now.
- SDK: a setting's title is never cut for its control. Where it does not
  fit beside its `−` value `+`, its switch or its value, the control goes
  on a line of its own below the title, at the right, and a title too
  wide for a line wraps onto a second (`ui::RowLines`, `ui::row_lines`,
  `ui::setting_height`; drawn with `ui::draw_number_lines`,
  `ui::draw_stepper_lines`, `ui::draw_switch_lines`,
  `ui::draw_setting_row`, `ui::draw_lines_note`). It is decided once,
  when the list is made, for every value a row may show in place (a
  choice's widest option, the widest language, the wider of on and off,
  a number's least and greatest), so a value changed in place never
  makes a row overlap the next; `RowLines::at` moves it to the row's
  place, `ui::value_stepper` tells where its − and + are drawn (for the
  hit areas). The settings use it:
  on the X4 Pro *Saving below (%)* and *Offline below (%)* showed as
  *Saving bel…*, as their buttons are a fingertip wide. Touch targets
  and the focus by the buttons stay as they were.
- Headers centre their titles by the capitals (the baseline to the top
  of an *H*) rather than by each title's own ink: a title with a *g* or
  *y* stood higher than one without and above the back arrow, by up to
  6 pixels on the Boox. Notes and page numbers in headers stand the same
  way. Titles of headers are 115 % of the text by default (was 130 %),
  as titles of dialogs are; `core.ui-title` still sets them, in steps of
  5 %.
- Updates wait for their plugins: when the update server says that code
  plugins in use have no build for a new PaperPaw yet (`waiting`), or
  plugins built in are not built for it yet, a search in the background
  does not offer it. The update page says *PaperPaw 0.5.0 waits for
  plugins* and which, with *Keep waiting*, *Update anyway* (the offer
  then, which says what stops working) and *Check again*; PaperPaw asks
  again once a day at a cheap moment and offers the update once it waits
  no more. Unseen for a week, the page opens once at a start.

- X4 Pro: dragging and sizing a widget by touch keeps up with the finger.
  The firmware queues touch readings while it refreshes the panel; a
  finger's moves while the last one still waits now take its place
  (`paperpaw_x4::changes::Queue`). Before, every reading was handled
  with a refresh of its own and a drag overflowed the queue, so the lift
  was lost: a 0.6 s drag kept the device busy for 7 s and the widget
  stayed where it was; now it is done 2.7 s after the finger came down,
  the widget where the finger left it. The remote control's `drag`
  plays a finger as the touch controller reads it.

- Dragging an item of a grid (moving or sizing it on the home screen)
  draws it again only when it goes to other cells: a finger trembling in
  a cell asked for a refresh each time, most of a second each on the
  X4 Pro, so the item fell seconds behind the finger.
- SDK: a header's note is a button only where the screen says it does
  something (`ui::Header::note_button`, or while it has the focus): then
  it is told as a button as high as the bar. Breaking: a plugin whose
  note opens something sets `note_button`; one that only tells
  something (*3 hits*) is said with the title and is no button.
- The library's pills are at least as wide as they are high, so *All*
  is a fingertip wide on the Boox. The kernel's screen that adds a
  widget to a list is called *Add a widget to a list*, so the places of
  the buttons tell it from the home screen's.
- SDK: any place of status items shows a whole status line of the
  reader's choice. The line of a place's items is the kernel's
  (`status_lines::ITEMS`); it answers to the reader's
  `reader.status-line.items` too (`StatusLineSpec::also`) and is listed
  once, the reader's choice of the battery's percentage kept, and the
  editor of status lines keeps it by its own id (`StatusItems::canonical`);
  `Place::with_line` names the setting that chooses the line, and
  `Place::draw`, `Place::targets` and `Place::signature` draw it, tap it
  and tell when it changed. The home screen's status line
  (`home.status.line`) and the quick panel's head (`menu.quick.line`)
  show the line chosen, their items by default. `StatusFrame::percent`
  says whether levels show their percentage.
- SDK: widgets' frames. A look restyles every widget's frame
  (`ui::look::Painters::frame`, `FramePart`, through `set_ui_look`);
  *Frames of widgets* (`core.widgets.frame`, `settings::WIDGET_FRAME`)
  gives every widget whose own frame is not chosen one of the line, card
  or dark card (each its own at first), where a widget has none of its
  own (kept, shown while chosen, or its placement's; `Settings::is_chosen`);
  presets of the kind
  `core.widget-look` (`presets::WIDGET_LOOK`, `widget_look_settings`,
  `widget_look_changes`) give widgets a look as a whole, chosen under
  *Widget look* at the top of the appearance settings, as themes are.
  `widgets::frame_height` and `widgets::free_placement` are the SDK's
  (five and two plugins had their own). An empty state (`ui::draw_empty`,
  e.g. the home screen's card before a book is read) gets smaller before
  its title or text is cut, and keeps less margin in a small area: on
  the X4 with wide margins the card said *Start readi…*.
- SDK: screens with a grid of widgets say more about it.
  `GridView::set_options` adds options of their own to an item's, after
  move, size, settings and remove, answered as `GridResponse::Option`
  (Breaking: the item options asked are their translation keys whole);
  `GridView::set_drop_check` refuses where an item is dragged or moved
  by the buttons (`grid::DropCheck`). The home screen has eight pages
  (four before), and actions any plugin adds to the menu `home.footer`
  show as icons beside the pencil, tapped or reached by the buttons.
- SDK: the editor of lists of widgets is the kernel's
  (`ui::widget_list`, screens `core.widget-list` and `core.add-widget`,
  `screens::WIDGET_LIST`, `screens::ADD_WIDGET`), bound to slot
  `core.edit-widgets`: the sleep screen's widgets are edited without the
  home screen, which no longer brings its own copy. Breaking: the home
  screen's screens `home.edit` and `home.add-to-list` are gone; open the
  slot `core.edit-widgets` instead. Plugins add kinds of widgets of their
  own with their icon (`Registrar::add_widget_category`,
  `widgets::WidgetCategory`); `Widgets::kinds` and
  `Widgets::category_icon` list them after the kernel's, as the home
  screen's gallery shows them.
- SDK: widgets take more than taps. `Widget::on_input` hears a swipe
  that started on it and the buttons' focus coming to its targets and
  leaving them (`widgets::WidgetInput`); `WidgetTarget::hold` runs
  something of its own on a long press (else the page is edited, as
  before); `Widget::draw_focus` draws the focus its own way; and
  `Widget::context` tells it where it shows (`widgets::WidgetContext`:
  its placement, so a widget placed twice tells its taps apart, the
  screen showing it, `GridView::set_host`, and its `SizeClass`). With the
  buttons, the page buttons are a swipe to the widget that has the focus
  (the screen's own page where it takes none); Left and Right always go
  on through the targets, out of a widget after its last, so one that
  pages without end never keeps the focus (the X4 has no Up and Down).
  Where its targets are the only ones on the page, the page buttons are
  the screen's and Left and Right page it at its first and last target;
  Up and Down go to the target above or below. After a swipe it took,
  its targets are asked anew: where it had the focus, the focus goes to
  its first target and it is told; elsewhere the focus stays where it
  was. Holding Select is an action of its own (`actions::LONG_CONFIRM`,
  *Menu or more*, the menu where no screen takes it): on a target with a
  long press of its own it runs that.
- SDK: holding Select sends `actions::LONG_CONFIRM` to the screen first
  (the key map's `long.confirm`); the menu opens only where the screen
  does not take it. A screen that took every action it does not know
  (such as the keyboard typing a password) no longer gets
  `actions::MENU` when Select is held: it takes `LONG_CONFIRM` as it
  took the menu. The keyboard does, for a letter's variants and to end
  the entry.
- SDK: a widget says how many cells of a grid it takes
  (`WidgetSpec::cells`, `widgets::CellsHint`: its size, its least, square
  or not): added, it gets them without being made to be measured;
  resized by touch or buttons, it keeps its least and stays square where
  it says. `widgets::SizeClass::of` tells a widget how large it is drawn
  (small, medium, large by its shorter side), to show more where it has
  the room.
- SDK: places of widgets. `widgets::WidgetPlace`
  (`Registrar::add_widget_place`; `PlaceKind::Stack` or `Grid`) names
  the settings that list a place's widgets: the home screen's pages and
  the sleep screen's list register theirs, and `Widgets::shown` tells a
  plugin whether any place shows its widget (the weather fetches only
  then, without reading every setting); a place that shows only in a
  mode of its screen says so (`WidgetPlace::shown_if`), as the sleep
  screen's two do. `widgets::Stack` shows widgets
  above each other, as the sleep screen does; `widgets::refresh` says
  what an event means for the widgets a screen shows (make anew, read
  anew, read one, the live ones), `widgets::next_minute` when to read
  the live ones. New slot `core.edit-widgets` (`slots::EDIT_WIDGETS`)
  edits a list of widgets; the home screen binds it, and the sleep
  screen's list opens it instead of a screen of the home screen's.
- SDK: pictures with transparency, drawn over what is drawn. PNG and
  GIF decoding keeps a picture's transparency (alpha, a palette's, a
  colour key of gray and RGB images) for sinks that want it
  (`ImageSink::wants_alpha`, `row_alpha`; `Downscaler` with two
  channels weights the gray by opacity), kept in files of gray and
  opacity (`image::AlphaFile`, `AlphaWriter`, `ImageFile::alpha`;
  `image::picture_with_alpha`, which says why a picture cannot be
  read). `image::Decoded` opens a decoded file of any kind from one
  read of its header. `ui::draw_image_over` blends any decoded picture
  over what is below, through its clear parts and as see-through as
  asked, dithered on panels of few grays and read a row at a time;
  `Canvas::draw_gray_alpha_row` does it for a row. Pictures drawn larger
  than decoded are interpolated (bilinear) instead of blocky. An overlay
  says what shows through it (`Screen::shows_through`,
  `ShowsThrough`): everything below it, or the full screen alone, menus
  left open not drawn; one covering the screen so is drawn in grays as
  full screens are. `StackEntry::guards` marks a screen guarding the
  device. `Registrar::add_choice` offers a value of one's own among
  another plugin's choices. Breaking: `ImageFile` has the field
  `alpha` and, as `AlphaFile`, is `#[non_exhaustive]` (made with
  `new`).
- Sleep screen: *The page with a picture*. The page being read stays
  and a picture or a widget goes over it, its clear parts showing the
  page: from the pictures folder or offered by any plugin as a preset of
  kind `sleep.overlay` (also in a data plugin's `presets.ini`). Where it
  goes (below on the right at first, clear of the status lines), how
  large it is (a third of the width), how much of the page shows through
  it and when it is used (always, while reading, or elsewhere; the cover
  otherwise, and always where a lock is open) are settings. The
  picture is decoded once while it stays the same; the picker says why
  the last one did not show. Other plugins add sleep modes of their
  own: a screen offered among the choices of `sleep.show`.
- The web emulator (`shells/web`): the standard app in a browser as
  WebAssembly, on an emulated X4, X4 Pro or Boox Go 7 drawn into a
  canvas, with buttons, keys and touch; a page hands in a data plugin's
  files, which show after a restart, and asks `paperpaw-dev`'s checks of
  them, through the emulator frame's messages
  ([docs/web-emulator.md](docs/web-emulator.md)). `paperpaw-emulator`
  builds without its window (feature `window`) and for `wasm32`: its
  storage a folder or kept in memory (`device::Library`,
  `Device::in_memory`), time and random numbers from the page where
  there is no operating system (`host`). `paperpaw-dev` checks data
  plugins handed in as files (`target::DataPlugin` holds them,
  `data::check`), and plugins with code added to the app (a module of
  their own around `paperpaw-web` as a library: `set_plugins`, called
  from its `pp_plugins`; `check` with `plugins` runs `lint`, `visual`,
  `reach` and `perf` on them), its libraries kept in memory where asked
  (`Context::in_memory`). The frame says when the device ran out of its
  memory (`out-of-memory`, once the module stopped, before it starts again;
  `budget::last_refused` and `budget::clear_last_refused` tell the size
  refused), names the build in `ready` and `version`, holds buttons
  for long presses (`hold`), and takes screenshots of another device,
  night mode or the sleep screen. Releases carry the module and its
  pages as `paperpaw-web-<version>.zip`.

- SDK: status items run an action of their own plugin when tapped
  (`StatusItemSpec::tap`, with the line's context; bindable to buttons
  too), say how much they matter (`StatusItemSpec::priority`,
  `status::PRIORITY`) and keep a least width
  (`StatusItemSpec::min_width`). Where a line has no room for all its
  items, the battery's percentage goes first, then the items that
  matter least (texts are shortened first), never cut.
  `StatusValue::Labelled` shows an icon with a text.
  `StatusLine::targets` and `StatusLineSpec::targets` say where a tap
  does something; the reader, the home screen's status line and the
  quick panel's head run it (Wi-Fi opens its page, `wifi.open`, the
  power mode its settings, `power.open`). Places where items show are
  `status::Place`s (`Registrar::add_status_place`; the reader's above
  and below the text, the home screen's, the quick panel's), and
  `StatusFrame::place` and `StatusFrame::font` tell a line where it is
  drawn. `Drawn::targets` gives a drawn widget places that do
  something, and `WidgetTarget::touch` one tapped only, which the
  buttons' focus leaves out. Breaking: `StatusLine` and
  `widgets::Drawn` are made with `new` (`StatusLine::percent`,
  `Drawn::shows`, `Drawn::targets`).
- Kernel: a plugin cannot overwrite another plugin's secret or guarded
  setting: not set, reset, nor typed for, nor ask for text for it from
  the keyboard, the clipboard or anywhere else; only the settings
  editor and the plugin itself can, and the keyboard answers them.
- Emulator: folders are listed by name on every system (Linux gave the
  order they are stored in), so books of the same title show in the
  same order on Linux, macOS and Windows, and the screenshots with them.
- Buttons alone: skimming shows where the focus is. The bar has it at
  first (left and right go along it, Confirm stays), shown as the style
  shows a focus, and up and down go from it to the buttons below and
  back to it. Every tool on the home screen and in the drawer is told
  as a button by its name, its name shown or not, the focus on one.
- Pictures over the whole screen (a cover while sleeping) are drawn in
  half the time: a row's dither worked out once rather than for every
  pixel, and a picture's columns once rather than for every row.
- Texts: the statistics' tiles fit their values at one size (the week's
  time was cut on the X4), the update page's explanation gets smaller
  before it is cut and then ends in "…", the picture widget says in
  fewer words where pictures go, the menu above the plugins' list has a
  name (*Plugins: above the list*), and screens of the same name are
  told apart by the kind of screen each is in the buttons' places
  (*Buttons and gestures (list)*) rather than by their id.
- Adding to a menu takes 17 KiB of heap on the X4 (it took 94 KiB): a
  few bytes a candidate, its name among all the others' in one text.
- The sample plugin *Large print* sets `core.font-size` and
  `core.line-spacing` (the reader's settings it named no longer exist,
  so its larger text never applied).
- SDK: `readers`, reading apps. Any plugin registers a screen as a
  reader of some formats (`Registrar::add_reader`, `ReaderSpec`); books
  open through one call (`readers::open`, `Command::OpenBook`) in the
  app chosen for the book, else its folder, else its format, else the
  default one, else the screen of `slots::READER`. The library, the
  file manager, the browser, OPDS, the home screen, widgets and the
  other plugins open books so. The reading settings offer the default
  app and one for each format (`settings::READING_APP`), where more than
  one app reads it; the file manager offers *Open with* for a book and
  *Reading app* for a folder (`readers::ask`), kept with the book or
  the folder (`readers::folder_moved`: they follow a folder moved or
  renamed). One reading app is open at a time; one that cannot open a
  book passes it to the next (`readers::cannot_open`); a choice of an
  app no longer installed is passed over without a word
  (`SettingExtra::keeps_unknown`, `SettingSpec::accepts`, `parse`).
  Shells open books with `Kernel::open_book`, the X4's remote control
  too.
- Book menu: *Open with …* (`actions::OPEN_WITH`), where there is
  something to choose, lists the reading apps that read the book, the
  other ways to open it plugins add (`menus::OPEN_WITH`, run after
  `events::BOOK_CHOSEN` as the book menu's are) and *Always open it with
  …*; the others open it this time only (`readers::ask_once`). The file
  manager's *Open with* is the same list. The library's sheet shows the
  book menu as its entries say they are now (`Menus::shown_items`, their
  label and badge). Lists of reading apps are asked on `slots::CHOICE`
  (`ask::choice`); `screens::READING_APPS` is gone, and a choice whose
  option is an action shows the action's title.
- SDK: buttons on touch screens are at least a fingertip (7 mm) high.
  Breaking: `ui::button_height(style, row)` takes the style. Rows and
  bars that hold buttons are made with `UiStyle::touch_row`, which grows them
  by as much as their buttons grow, so the room above and below them,
  the margin to the screen's sides and the gaps between them stay as
  they were. A bar of buttons along the top or bottom edge keeps the
  side margin from that edge too (`ui::edge_bar_height`,
  `ui::edge_bar_band`, `ui::edge_room`): the file manager's, the
  store's and the plugin manager's bars, the sync question, the
  reader's footnote, selection and skim bars, the keyboard and the home
  screen's layouts. `ui::setting_row(style, spec, row, noted)` gives a
  number's row its height, in a noted row its first line
  (`ui::number_title_area`). `ui::button_width(style, label)` says how
  wide a button must be for its word to stay whole, for buttons laid
  out as wide as their words (the reader's selection bar). The shipped
  screens with buttons follow; `paperpaw-dev` finds buttons without
  their room (`visual.button-room`), the tiles of a grid edge to edge
  left alone, a last row of one tile too.
- SDK: accessibility. `events::FOCUS_CHANGED` (`FocusChanged`: role,
  label, area, on) tells plugins where the focus of the buttons went,
  worked out only while one listens or the focus is spoken. The kernel
  describes screens on a framebuffer that keeps no pixels
  (`Framebuffer::blind`; 48 KB less on the X4). New roles `Check`,
  `Radio`, `Field`, `Image`, `Link`, `Dialog` and `Progress`
  (`semantics::role_name` names them); progress bars and the keyboard's
  field describe themselves so, the reader's menu bar its entries (the
  focused one too). `Screen::secret` (a PIN, a password being
  typed) keeps its focus from being told, as do screens that guard the
  device; `Kernel::is_secret` says so to the USB remote and the dev
  tools, which then leave out what the parts say. A password typed (an
  answer to a question asked with `password()`) reaches the plugin that
  asked alone, whichever keyboard took it, and while it is typed only
  that plugin's input filters see the keys (`InputFilters::filter_of`).
  A secret setting (`SettingExtra::secret`) is kept with the secrets,
  never in the settings: read it with `env.secrets.get(id)`; set or
  reset, its owner alone hears `SETTING_CHANGED` (with its id, not its
  value), and one kept in `paperpaw.ini` before moves to the secrets at
  the start. The USB remote sends no secret setting's value. Text typed
  for a widget's placement sets the placement's own value. Breaking: a
  plugin sees its own secrets alone, those in its namespace: `env.secrets` is a
  `secrets::Visible` view (`get`, `iter`, `is_empty` of the running
  plugin's, `listed` and `has` of every secret without its value, as
  the settings' list shows); the kernel tells it which plugin runs
  (`secrets::Running`, `EnvParts::running`). `wifi::known` takes the
  secrets to look at. On a blind framebuffer, and
  while the focus is worked out, no glyph or picture is made.
- SDK: `ui::setting_opens` says what choosing a setting opens (its
  editor, the keyboard for text, the list of a choice's values), the
  same on the settings pages, in menus, the quick panel and the setup;
  text typed comes back as `events::SETTING_TYPED`, which the kernel
  sets. A text setting in a menu did nothing before.
  `SettingExtra::shown_if(setting, value)` shows a setting only while
  another has a value (`Settings::shows`; screens listing settings make
  themselves anew when such a setting changes, `Settings::governs`);
  `SettingExtra::check(fn)`
  refuses values (`SettingError::Refused`), and the kernel tells the
  reader why.
- SDK: one choice dialog for every plugin: `ask::choice(title, &reply)`
  with `.about(context)` and `.options(labels, &[..])` asks on the
  screen of the new slot `core.choice` (the kernel's
  `screens::CHOICE` unless a plugin binds another), so a theme restyles
  all questions. The fifteen plugins' own choice screens are gone.
  `ui::choice::open` (a plugin's own choice screen) and
  `GridView::new(env, choice, chosen)` are deprecated:
  `GridView::asking(env, chosen)` needs no choice screen.
- SDK: themes are presets of the kind `core.theme` (`presets::THEME`,
  covering `presets::theme_settings`, the appearance group): the
  settings plugin lists them under *Theme* at the top of the appearance
  settings where any is installed, so two themes no longer fight; *None*
  gives the appearance settings their defaults again. No theme changes
  the size of the interface's text, its contrast or the logo, even where
  it names them (`presets::theme_changes`).
- SDK: semantic icons (`icons::BACK`, `PREV`, `NEXT`, `HOLD`, `BUSY`,
  `SHIFT`, `DELETE`, `SPACE`) take the icon a plugin names for them with
  `add_icon`, so a theme changes the headers' arrows, the hints' marks
  and the keyboard's keys; without one they look as before.
  `ui::draw_arrow`, `ui::draw_icon_at` and `ui::has_icon` draw and ask
  for them. An icon named `U+F8000` is that code point, for icons of
  one's own. `ui::HOLD_ICON` is deprecated (`icons::HOLD`).
- SDK: the interface's grays have roles (`ui::Ink`: text, notes,
  muted, faint, disabled, rules, lines, cards, …), `UiStyle::ink` gives
  each, from the palette a theme sets in `core.ui-palette` (e.g.
  `muted=60 line=170`; `ui::Palette`). `UiStyle::size(TextRole)` gives
  the sizes of text by kind (caption to numeral). The SDK's parts draw
  in them; two grays became one each (an empty screen's explanation is
  drawn as notes, the pager's last arrow as what cannot be used).
- SDK: a plugin gives the parts of every screen a look of its own:
  `Registrar::set_ui_look(&'static Painters, Priority)` with painters
  for switches, the focus, separators, buttons, headers, the row of
  button hints, progress bars and sliders (`ui::look`). The SDK still
  places the parts and describes them; `UiStyle::look` and `Env::look`
  carry the look in use (the highest priority, of equal ones the later).
- SDK: a setting's editor may be a slot (`SettingExtra::editor_slot`,
  `SettingSpec::editor_slot`), so a plugin filling it edits the setting
  too. Breaking: `SettingExtra::editor` is now an `Editor` (`Screen` or
  `Slot`), opened with `Editor::open`. New slot `core.choose-value`
  (`slots::CHOOSE_VALUE`) picks a choice setting's value; the settings
  plugin binds it, menus and the setup open it. The dictionary, sleep
  and picture widget choose their folders with `core.choose-folder`.
- SDK: `ask::text(..).live()` asks for answers on each change too (a
  list that filters as one types), each with `Typed::typing` set; the
  keyboard gives them. `params::FOCUS` opens slot `core.settings` on a
  setting's page with the focus on it. New: `params::LIVE`,
  `params::TYPING`. Breaking: `Typed` has the field `typing` and is
  `#[non_exhaustive]`, made with `Typed::new(text, context)` (and
  `while_typing`). The kernel sets a setting from `SETTING_TYPED` only
  once the reader is done typing.
- SDK: `Services::contributed_points` lists the points contributed to,
  declared or not. Breaking: `Services::add`,
  deprecated since 0.4.0, is gone.
- SDK: `time::iso8601` and `time::parse_iso8601` write and read
  ISO 8601 times (UTC with milliseconds; offsets and dates alone read
  too; years too far out and offsets past a day read as none),
  `time::duration_text` writes a length of time as `1 h 05 min`,
  `42 min` or `< 1 min` in the language's words. Joplin uses them.
- SDK: `menus::BOOK` (`core.book-menu`), what can be done with a book
  in any view of the library, with `events::BOOK_CHOSEN` (`BookChosen`:
  the action and the book) sent before the action runs: the library's
  book sheet lists it (on pages, where more come than the sheet holds),
  its book details are its first entry (`files.book-info`). `menus::APPS` (`core.apps`), what a launcher
  lists: the home screen's tools list it beside the tools.
- Library: the kernel keeps the catalog of books, the only one writing
  it, one change at a time, in the background and never while a book is
  open. It walks the library folder once the library first shows after
  a start (never at every start: on the X4 every wake is one), for
  another library folder and when books came or went without saying
  which, taking each book whose file did not change from the old
  catalog, read along with the walk, instead of reading its details, and
  listing large folders a piece a step: a walk takes a few kilobytes
  whatever the library's size, and one paused goes on where it was. On a
  device of little memory (the X4) it waits while the library is open,
  whose index of the books takes the room (catching up, which takes less,
  goes on beside it); with 2400 books the X4's heap peaks at 153 KiB
  while it walks. Books that came, moved or changed are caught up without
  a walk, at a calm moment when many come one after the other (a Calibre
  or catalog sync); books removed are left out at once. Views of the
  library no longer make it themselves, and show at once what changed (a
  book finished, a favourite, a book removed) without reading each book.
  What could not be read is never written: an error leaves the old
  catalog whole, and a status that cannot be read is not noted. Books
  Calibre, the transfer page or Wallabag delete are removed from the
  library with what plugins keep for them; books they and catalogs bring
  are announced. Book information is read out (its rows were drawn
  only).
- Library: badges plugins add show on the books' rows, the fields they
  keep show on a book's information, and actions they add to the book
  menu show on its sheet. Each size of cover (row, card, covers) is kept
  apart, so switching views no longer makes covers anew.
- SDK: the library as a service (`library::service`, `trait Library`):
  every view of the library, its shelves, its letters and the book
  widgets read the books through it (`each`, `records`, `version`,
  `index`, `is_current`, `refresh`, `kept_index`, `keep_index`), so a
  library of one's own, published as `library::service::SERVICE` (the
  kernel's services a plugin may stand in for are
  `services::REPLACEABLE`), feeds them all with its own books. With the
  catalog's state (`catalog::state`, `catalog::generation`), what was
  noted beside it (`catalog::Changes`), queries (`catalog::Query`,
  `query`, `found`, and `query_in`, `found_in` over any library's
  `catalog::Books`), a page's books with the log read once
  (`catalog::read_page`), and the index of every book the calm list uses
  (`library::index::Index`, 24 bytes a book, made from any library with
  `Index::collect`, its items with `Item::new`, of a `version`, kept in
  a file while a book is open). New: `events::BOOK_ADDED`,
  `BOOK_CHANGED` (bits of `library::what`), `BOOK_CHOSEN`,
  `CATALOG_CHANGED`, `CATALOG_WANTED` (`CatalogWanted`);
  `library::{added, changed_book, request_catalog, request_catalog_anew,
  cover_variant}`; `menus::BOOK`; `Registrar::add_book_field`
  (`library::BookField`) and `add_book_badge` (`library::BookBadge`,
  `Badge`), listed in `Env::book_fields` and `Env::book_badges`;
  `catalog::series_order` reads `2,5` as `2.5`; `Formats::is_empty`.
- SDK, breaking: the catalog is of format 2 (status, added, fields, the
  file's size; a header with its generation; format 1 is still read)
  and `Record` has `status`, `added`, `size` and fields; status changes
  and removals are noted beside it and folded in by the kernel. Making
  it is the kernel's (`catalog::Walk`, `catalog::Rewrite`,
  `catalog::note`, `catalog::forget`, `catalog::log_is_long`,
  `library::note_status`, with the SDK's `kernel` feature): views listen
  to `CATALOG_CHANGED` and ask with `library::request_catalog` instead of
  walking. `catalog::changes` gives `Changes`; `Index::load` takes the
  version it is to be of. `Env` and `EnvParts` have `book_fields` and
  `book_badges`.
- The library folder is the kernel's setting, `core.library-folder`
  (it was the library plugin's `files.root`), so a library of one's own
  and every plugin that puts books somewhere read the same; a folder set
  before is read under its old id. SDK: `builtin::settings::LIBRARY_FOLDER`
  (`library::FOLDER` names it) and `builtin::screens::FOLDER_EDITOR`, a
  text setting's editor that opens the folder chooser of whichever
  plugin fills its slot.
- Emulator: scripts measure the heap's peak of one step (`reset-peak`)
  and send an event as a plugin would (`emit:core.library-changed`).
- SDK: `menus::APPS` (`core.apps`), what there is to open besides the
  tools; the home screen's grid of tools shows it too, after the tools,
  and the tools' menu has it as a submenu (*Apps*) where there is
  something in it, so that it is reached with the buttons alone.
- Keyboard: other plugins add layouts through the point
  `keyboard.layouts` (`id; code; letter rows; variants`), chosen as the
  built-in ones, with variants of letters of their own besides the
  built-in ones. A layout has at most 6 rows of 16 keys, a code of 4
  characters and 16 variants a letter; invisible characters and ids
  named as other layouts are left out.
- About: other plugins add lines to the page through the point
  `about.lines`: plain text, up to 512 bytes a plugin, each line named
  after the plugin that added it.
- Docs: what the slots besides the reader's promise (home, menu, panel,
  settings, keyboard, lookup) and the shipped plugins' points.
- Wikipedia: a lookup provider that goes online. Once *Online* is
  chosen in the dictionary's panel, its summary shows there as an entry
  beside the dictionaries', and its line *Open Wikipedia* (or confirm,
  where the entry shows) opens the whole summary on its own screen
  without fetching it again; that screen stays bound to
  `slots::LOOKUP_ONLINE`. *Online* is offered once a word. SDK:
  `LookupEntry::more`, the screen showing more of an entry, opened with
  the entry's text (`LookupQuery::answer_entry`, `LookupAnswered::more`).
  The panel names each source right above its entries.
- SDK: menu entries placed beside others (`MenuPlace`,
  `Registrar::place_menu_entry`; several in one place keep the order
  they were registered in) or taken out by a plugin (`hide_menu_entry`;
  the menu editor tells which, `Menus::hidden_by`); what a plugin's own
  entry shows now (`set_item_state`, `ItemState`: a label, a switch,
  faint, hidden, a badge; `ItemState::text`) and a line under it
  (`describe_item`), shown by the shipped list, bar, panel and home
  screen tools alike. A faint entry is passed by the focus and does
  nothing (`ui::Rows::set_passed`, `ui::draw_faint_button`).
- Menus: a menu the user redefined shows the entries of plugins
  installed since, after the entry before them (`[menus-known]` keeps
  what plugins put there that the user left out; lists kept before stay
  as they are).
- SDK: `lookup`, lookup providers (`Registrar::add_lookup_provider`):
  where words are looked up, at once or later, answering the asker's
  reply event (`LookupQuery::answer`, by default
  `events::LOOKUP_ANSWERED`). The dictionary's panel lists every
  provider's entries, naming each dictionary or provider only where
  entries come from more than one; it asks those that go online only
  when *Online* is chosen, and offers the online lookup screen
  (Wikipedia) after them, waiting 20 seconds at most for those that
  answer later. The StarDict dictionaries are a provider too.
- SDK: `voices`, a plugin's voice (`Registrar::add_voice`) speaks text
  aloud in place of the system's for the languages it speaks, chosen
  with `settings::VOICE`; it answers as the system does
  (`Speech::answer`). A device without a voice of its own has speech
  with one, which speaks unchosen (the choice *Automatic*); the system
  or another voice speaking in its place stops it.
- SDK: formats tell files without an extension by their first bytes
  (`FormatSpec::sniff`, `Formats::for_file`, `supports_file`; the last 32
  files told are kept by a hash of their path and their size, and
  however many a folder has, none is left out): the
  library, the file manager and the browser find such books; EPUB (its
  stored `mimetype` entry) and XTC say so. Formats read their own
  settings (`OpenOptions::setting`, filled by `reading::open_options`),
  and say a document still coming is not ready
  (`DocumentError::NotReady`; the reader waits for
  `events::DOCUMENT_READY`).
- Documents: `Metadata::direction` (`PageProgression`): an EPUB whose
  pages go right to left says so, and the reader mirrors taps, swipes
  and swipes in from the edges (`Screen::mirrored`, kept by the
  kernel's key map). EPUBs are converted anew once (cache version 25).
- Code plugins fit the firmware again: the SDK's fingerprint no longer
  counts features that add functions and modules only (`kernel`,
  `unstable`), with which the firmware is built and plugins are not, so
  no plugin built by an author fitted any firmware. A code plugin built
  as a library for a computer (`plugin!`) loads on Linux: it defines the
  unwinder's name that `core` and `alloc` leave open, never called where
  a plugin cannot unwind.
- SDK: services and introspection. A service type names its
  contract in a key (`services::ServiceType`, `"weather.forecast/1"`),
  published with a version (`Registrar::provide`) and found by it, its
  size and alignment checked (`Services::get_typed`,
  `get_versioned`), so native plugins built apart find it safely.
  `Registrar::add_point` and `declare_event` declare points and events
  (`Services::points`, `events`, `ids`) for editors and authors.
  `Env::slot_screen` and `slot_screens` say which screen a slot shows
  and which are offered; `KeyBindings::bindings` lists a layer's
  bindings with their origin (`keys::BindingOrigin`).
- Data plugins (formats 3, `DATA_FORMATS` is 3): `menus.ini`
  adds entries to any menu, places one after another and takes entries
  out, and makes menus of one's own (`title`, `entries`); `actions.ini`
  makes actions of up to 16 steps (setting values, `open:<slot>`,
  `notify:<text>`, `emit:<event>`, other actions); `icons.ini` gives
  items icons; `points/<point>.txt` contributes lines to a point;
  `setup.ini` adds steps to the first-run setup; `[fonts]` in
  `plugin.ini` binds fonts to roles. They are read for plugins made for
  formats 3 alone (others are told what is left out). A data plugin's
  files take at most 96 KiB together besides its translations (512 KiB
  of their own), 24 KiB of it of formats 3 and 64 lines for points; the
  files it parses are read one at a time as it registers. An action
  emits only its own events and sets no guarded setting. Its menu
  entries are placed and taken out as code plugins' are
  (`Registrar::place_menu_entry`, `hide_menu_entry`).
- SDK: refreshes. A refresh filter sees why the panel is
  refreshed (`filters::RefreshCause`: a page turned, screens, a notice,
  a status line; told with `Cx::refresh_cause`), the slot, how many
  partial refreshes were made since the panel was cleaned
  (`since_full`), whether the kernel made it full for that (`periodic`,
  so a filter keeping its own count replaces the policy), grays and
  pictures. Breaking: filters answer with `RefreshVerdict` (`Keep`,
  `Mode`, `Grays`, `Defer`: refreshed with the next one, but never a
  screen opened or closed, nothing while a screen guards the device and
  not more than three times in a row) and `RefreshFilters::apply` gives
  `None` for a refresh deferred.
  `Cx::set_refresh` sets the mode exactly, over the event's requests;
  `ScreenSpec::refresh_on_open` sets it for a screen opening.
  `RefreshMode` has `Mono`, `Gray` and `Clear` (a transition through
  white), mapped by the shells; `RefreshMode::cleans` says whether a mode
  removes ghosting.
- SDK: actions with params, overrides and observers.
  `Command::RunActionWith(id, params)` runs an action with params, which
  a handler made with `ActionSpec::handler_with` gets (a rule applying
  the profile it names). `Registrar::override_action(action, handler,
  priority)` puts a handler in place of any action's own; the one of the
  highest priority runs, as for slots, and `Command::RunOriginal` runs
  the action's own from it (from nowhere else). `ActionSpec::sealed`
  keeps others' handlers out (the lock's `lock.now`; overriding one is
  an error). `Registrar::observe_actions` and
  `observe_events` (`ObserverSpec`) see every action run and every event
  emitted (a macro recorder, rules on any event); nothing runs for them
  where there are none. While a screen guards the device, only its
  plugin's handlers in place of others, its observers, its input and
  refresh filters run, presses are not counted, `Command::Input` from
  others is ignored, and nobody else changes settings, secrets,
  plugins, menus, key bindings or slots; what a handler asks shows above
  the guard only where the guard itself let the action pass; screens
  above it that it did not open (a sleep screen) get no input and close
  at the next input or as the device wakes. The events the guard or a
  screen it opened gives (the password typed) go to them and their
  plugins alone. A plugin changes its own secrets; another's, a
  setting that keeps the device safe (`SettingSpec::guarded`, the
  lock's) and the sleep and settings slots only the settings editor does
  (the plugin of the settings slot), never another on the reader's
  behalf; no other plugin's default, preset or power mode changes such
  a setting,
  and plugins are not loaded or installed under the guard. What an
  action or an observer asks that runs the same action again with the
  same params is dropped with a warning. `ActionSpec::describe` and `keywords` give lists
  and searches of actions their words; `Actions::runnable` tells actions
  that run by themselves from screen actions. `SettingChanged` carries
  the value before (`old`) where the kernel changed it and a plugin
  hears the event. Breaking:
  `Contribution::Action` holds a `Box<ActionSpec>`.
- SDK: inputs of more kinds. Key maps bind `double.<button>`
  (pressed twice), `double-tap.<zone>`, `tap2` (two fingers),
  `pinch.<out|in>`, a layer's own zones (`[zones.<layer>] <name> =
  <left> <top> <width> <height>` in percent, tapped as `tap.<name>`),
  plugins' `input.<id>` (`Registrar::add_input`, `keys::InputSpec`,
  listed in `Env::inputs` where the device can make them, with the
  gestures, `keys::InputKind`) and `gesture.<id>`
  (`Registrar::add_gesture`, a `keys::GestureRecognizer` that sees the
  fingers where gestures are the key map's, never under another
  plugin's guard, and may claim them with `keys::PointerVerdict`). The
  keys plugin lists them all to bind. A press or tap waits for a second only where
  something is bound to the double one, so pages still turn as fast as
  they are tapped. Screens get `GestureKind::DoubleTap` (as a tap where
  they decline it; only to screens that say `Screen::takes_double_taps`)
  and `GestureKind::TwoFingerTap`. `Command::Input`
  raises an input by its name; `Command::SetKeyLayer` (sent with
  `Cx::set_key_layer`) puts a layer in front of the slot's for the
  screen that sent it, while it is on top (a book's own bindings); none
  below a screen that guards the device, and none sent from anywhere
  but the screen itself. What is held is resolved for
  the screen it began on. Input filters may
  `InputVerdict::Cancel` an input (what began with it is forgotten) or
  `InputVerdict::Replace` it (`InputFilters::filter` says the input
  a verdict was made for); `InputVerdict` is now non-exhaustive.
  `Env::last_input` says whether buttons, a finger or a pen came last.
  `keys::inputs` lists every input a device makes, with `keys::ZONES`,
  `SWIPES`, `EDGES`, `PINCHES` and `POLICIES`. New settings
  `core.double-tap-ms`, `core.tap-slop`, `core.swipe-distance`, and
  `core.frontlight` with `core.frontlight-warmth` where the shell sets
  the light (`KernelBuilder::frontlight`, `Capabilities::frontlight`,
  `MenuNeed::Frontlight`).
- SDK: screens hosted in a screen of one's own, tasks with their own way
  back. `Env::make_screen` makes any screen plugins
  added, to host it in a part of a screen (tabs, a split view);
  `host::ScreenHost` holds it there and passes drawing, input in its
  part (in its coordinates), events, timers and its showing and closing
  on to it. `Command::SwitchTask` shows another task, a tab of tabs:
  the screens open are kept by their ids and params (not in memory)
  and each task keeps its own way back.
- SDK: bars that stay across the screens shown.
  `Registrar::add_chrome(ChromeSpec::new(id, Edge::Bottom, factory))`
  adds a bar along an edge (`size`, `shown` over which screens, `need`
  such as a touch screen, `priority` among bars on one edge): an
  ordinary screen drawn into its strip, which takes the gestures that
  begin in it, hears every event and stays as screens open and close;
  the screens get the rest of the screen. No bar shows while a screen
  guards the device. `Env::content` says the part of the screen a
  screen is made for, with its place; screens draw and get input from
  its corner (`Env::within`, `Env::device_within` make the environment
  for a part). `Canvas::offset` moves what a canvas draws, for a screen
  drawn into part of the screen. `MenuNeed::met` says whether a device
  has what a need asks for. Bars live as screens do (`on_show`,
  `on_timer`, `on_close`, made anew for another size); an overlay shares
  the bars of the full screen below it, a screen that guards the device
  has none. Breaking: what a screen asks to redraw
  (`Cx::request_redraw_area`, `Output::redraw_area`) and an overlay's
  `Screen::region` are now from the corner of the screen's own part of
  the screen (`Env::content`), not of the panel; without bars that is the
  same. `EnvParts` has no `screen_maker`: the kernel sets it with
  `Env::with_screen_maker` (feature `kernel`, as `ScreenMaker`).
- Android: the app is now `org.paperpaw.app` (was
  `io.github.hthoene.paperpaw`), named after the project's own domain
  before the first release in a store. It installs as a new app beside
  the old one; books and settings of the old one stay in its own
  folder.
- SDK: `layers`, page layers and frames. A layer
  (`Registrar::add_page_layer`) draws on every page a reader shows,
  behind the text or over it, with the page's lines and columns
  (`PageView`); it may reserve room beside the text and hear taps on it
  (`events::PAGE_LAYER_TAPPED`, also with the buttons). A frame
  (`Registrar::add_page_frame`) sets pages in columns, chosen with
  `settings::PAGE_FRAME` (each book may have its own); a screen in
  columns is one page of the page index. `reading::PageType` has the
  layers' `rooms` and the frame's `columns`, and lays a page out in them
  (`PageType::lay_out`), also the settings' preview.
- Settings: a choice with nothing to choose from (one value) is not
  listed.
- SDK: block filters (`TextFilterSpec::block`) know where a block is
  (`FilterContext`: the book, its language, where the block starts) and
  may leave it out (`Verdict::Drop`): the reader lays out, searches and
  shows notes without it. `reading::DroppedRuns` keeps where runs left
  out end, so they are read once; too many left out in a row are an
  error, not the end of the section. `FilteredDocument::kept_block_at`
  gives a block only where it is kept, and `reading::note_text` takes
  the filtered book: a note whose block is left out shows nothing.
- SDK: `reading::note_text`, the text a footnote shows;
  `layout::{Line, wrap_lines, wrap_lines_hyphenated}` (feature
  `unstable`).
- Layout: `Style::widows` and `Style::orphans` (`Style::lines_kept`):
  the fewest lines of a paragraph left alone at the top or bottom of a
  page; 1 by default, as before. A paragraph that goes on whole takes
  the heading before it along, also with 1 where its first line does
  not fit (`layout::VERSION` 6: page indexes are made anew). Standard
  reading settings `settings::WIDOWS` and `ORPHANS` (advanced, each
  book may have its own) set them for every reader.
- Text: `Line` is `#[non_exhaustive]`, made with `Line::new`.
- SDK: `reading` holds the reader's building blocks for readers of
  one's own: `FilteredDocument`, `PageIndex`, and with the feature
  `unstable` `PageType`, `ReadingFaces`, `EmbeddedFonts`, `StatusRoom`,
  `WordBox`, `word_boxes`, `draw_lines`, `draw_image`. The reader uses
  them; pages look as before.
- SDK: the reader's contents, search, marks, footnotes and skimming are
  standard slots (`slots::TOC`, `SEARCH`, `ANNOTATIONS`, `FOOTNOTE`,
  `SKIM`), and following a link and skimming standard events
  (`events::FOLLOW_LINK`, `SKIM`, `SKIMMED`): a plugin replaces one of
  them, or a reader of one's own uses the shipped ones.
- SDK: `positions`, where each book was left, shared like its
  annotations (`.paperpaw/positions`, moved and removed with the book):
  readers keep it, plugins read it without opening the book. The reader
  keeps its place there (a place kept before is read).
- SDK: `reading::open_options` and `reading::conversion`: a plugin
  opens a book as the reader does, so its places fit. KOReader's notes
  exchange and the progress sync did not, and with the book's own
  styles off or a stylesheet plugin on they converted the book a second
  time and could meet other places.
- SDK: the reading event of a book closed carries its place as an
  XPointer (`Reading::xpointer`); the progress sync sends it as it is.
- SDK: the type of book pages is standard, registered by the kernel as
  the font size is: `settings::FONT_WEIGHT`, `WORD_SPACING`,
  `LINE_SPACING`, `MARGIN`, `MARGIN_ENDS`, `JUSTIFY`, `HYPHENATE`,
  `INDENT`, `PARAGRAPH_SPACING`, `PUBLISHER_STYLES` and `FOOTNOTES`
  (`core.*`, were `reader.*`), so a reader of one's own reads them and
  profiles, rules and themes set them for every reader. What was set
  under the old ids is kept.
- SDK: `pattern`, a small part of regular expressions (classes, word
  edges, repeats, alternatives, groups, any case) without a library and
  with bounded work; `decorations::PatternDecorator` marks the words
  patterns find on a page, each in its role.
- Plugins without code mark words by patterns with a `marks.ini`: a
  section per kind of mark, its patterns and its look, set like any
  mark's in Settings, Marks in the text. Its files' formats are now
  version 2 (`DATA_FORMATS`).
- SDK: any plugin reads the words of the page shown
  (`reading::ask_page_words`, answered with `reading::PageWords` by the
  reader while a book is open), not only decorators.
- SDK: the screen stack, readable and steerable.
  `Env::stack` lists the open screens (`StackEntry`: screen, slot,
  overlay, key) and `Env::top` the one shown; plugins subscribed to
  `events::SCREEN_SHOWN` hear which comes on top. A screen's `Cx` knows
  it (`Cx::screen_id`, `Cx::slot`, `Cx::key`) and closes it wherever it
  is (`Cx::close`, `Command::Close`). New commands: `Command::Replace`
  (in place of the screen on top, in one step), `Command::BackTo` (back
  to a screen or a slot's), `Command::OpenInSlot` (a chosen screen for a
  slot) and `Command::BindSlot` (a slot bound for the user, kept in
  `paperpaw.ini`). Screens may keep themselves open when about to close
  (`Screen::on_close_request` with `CloseCause`), say what a tap outside
  them does (`Screen::outside`: close, ignore, pass through), let input
  they do not take pass to the screen below (`Screen::passes_through`)
  and say their own row of hints (`Screen::hints`).
- A row's title and value keep a margin between them, not two: on a
  screen as narrow as the X4's both stay whole where they fit (Joplin's
  "Synchronised with, Joplin Cloud" lost the end of both).
- List icons are centred by their drawing in their column, so icons
  with more room on one side line up with the others. SDK:
  `FontHandle::ink_across`, `ui::icon_ink`.
- Restarting no longer counts as going to sleep: after plugins were
  switched or an update installed, rules for "on sleep", sync on sleep,
  work waiting for sleep and the sleep counters no longer take the
  restart for a sleep, and the battery log ends the time awake there.
  The reading position, the reading statistics and what the reading
  widget shows are still saved before the restart, now also in the
  emulator and on Android.
- SDK: `events::STOPPING` (`core.stopping`): PaperPaw stops (the device
  restarts, or an app closes for good); plugins save what is pending, as
  at `SUSPEND`, but no `RESUME` follows and it is no sleep. Shells call
  `Kernel::stop` for it.
- The update's progress bar fills inside its rounded track at every
  value; its fill stuck out past the track's rounded ends. SDK:
  `ui::draw_progress`, a progress bar for any plugin.
- SDK, before 1.0: what plugins build or match can grow without breaking
  them. Structs plugins hand over (`ScreenSpec`, `MenuEntry`,
  `MenuSection`, `StatusItemSpec`, `StatusLineSpec`, `StatusFrame`,
  `WidgetTarget`, `Gesture`, `FormatSpec`, `SetupStepSpec`,
  `SettingGroupSpec`, `SlotSpec`, `Manifest`, the filter specs,
  `HyphenationSpec`, `install::{Package, PackageFile}`, `widgets::Look`,
  `library::{Book, Provided}`, `document::{Block, Metadata}`,
  `layout::{Style, Page, LayoutContext}`, `UiStyle`, `Speech`, `FontSpec`,
  `Cells`, `Output`, `filters::Refresh`) are `#[non_exhaustive]`, made
  with `new` from what they need and the rest set with a method of its
  name (`MenuEntry::new(menu, item).order(90)`); `UiStyle::PLAIN` is a
  style of fixed sizes, `Look::within` the same look elsewhere. Events of
  `event_data!` are made with `new`, optional fields in an `optional { }`
  block set by name; `Goto::new(path, at)` is `Goto::to`. Enums that will
  grow (`StatusValue`, `Hint`, `Handled`, `SettingValue`, `Role`,
  `RefreshMode`, `MemoryClass`, the firmware, Wi-Fi and install states
  and more) are `#[non_exhaustive]`; so are the struct variants of
  `Contribution` and six of `Command`, made with `Command::bind`,
  `serve`, `connect`, `socket_send`, `socket_read` and `broadcast`.
  Fields whose rule a constructor keeps are private: `ListView`'s,
  `Rows`' and `Sections`' area and focus (`set_focus` says what changed),
  `Slider`'s range and step. Lists that may grow are slices; the
  built-in settings are all typed (`Setting<T>`). `ui` is split into
  modules by theme, every path as before.
- SDK: the crates it passes on are named lists, not `*`, each with its
  own `api.txt` checked in CI; `layout` and `zip` need the feature
  `unstable` until 1.0; `json` and `digest` are passed on too.

- Reader: switching a book's own styles ("Publisher's formatting") keeps
  the place on the same text, with the book open or closed; before, the
  place moved, as the book converted without its styles has its text
  elsewhere.
- Security: while the device is locked, nothing shows above the lock
  screen but what it opens itself (its keyboard for a password) and the
  sleep screen it lets pass. A screen a rule, a timer or a plugin opens
  meanwhile waits below it, and notices wait until it is unlocked;
  before, such a screen showed above the lock and could be used without
  the code.
- SDK: `Screen::guards` says that a screen guards the device, as the lock
  screen does while locked: until it lets go, screens others open wait
  below it, others' `Command::Back` closes nothing and notices wait.
  Actions it lets pass, and screens it opens itself, show above it.
- The emulator's plugin servers and the serve crate's tests listen on
  this computer only (`Servers::listening_on`), so Windows no longer
  asks to let test programs into the network after every build.
- Reading statistics: a stay cut off by a power cut no longer garbles
  the stays recorded after it, day totals no longer overflow on huge
  merged logs, and one stay dated far ahead (a clock wrong once) no
  longer hides the calendar day's books and pages.
- Fonts: a character a font lacks is drawn as one of nearly the same
  look before falling back to `?`: the non-breaking hyphen as a hyphen,
  the long s as s, rare punctuation as its common cousin.
- Statistics files: a database written for KOReader that fails part way
  (a full card) leaves the one there before as it was.
- X4, X4 Pro and X4 Classic: a lean firmware. The 20 plugins the store
  offers as files (statistics, widgets, Calibre, KOReader sync, weather,
  news, rules and more) are no longer built in, nor the diagnostics
  screen (developer builds, and the store, have it): 21 files in the
  store. The X4's image shrinks from 6.59 to 5.43 MB, the room for
  plugin files grows, and the X4 starts with less heap. Android builds
  them in, switched off for a new reader until wanted; for one who ran
  PaperPaw before they stay as they were. Pebble stays built in, off.
- Updates: the code plugins built for an update are fetched once it is
  downloaded, as far as the room for code holds them, and installed by
  the updated firmware: as the update's first start loads no code
  plugins, the firmware starts once more when it keeps the update (after
  half a minute), and they run from then on, also offline. A fetch cut
  off by a restart goes on when the update page shows; what is left for
  an update installed meanwhile is let go. A plugin that comes back as a
  file is no longer named as left out.
- Plugin files ask the firmware for icons by name instead of carrying
  all their names (some 125 KB less per file that draws icons): the
  firmware's table for plugins, `paperpaw_native::Api`, has a new entry
  at its end, `icon`, which the SDK's `icons::char` calls in a plugin
  built with `--cfg paperpaw_plugin` (as the sample is).
- Files, widgets and vocabulary keep no writable statics, so they pack
  as plugin files.
- First-run setup: after Wi-Fi, "Add more" offers four plugins most
  readers add (reading statistics, home widgets, Calibre, KOReader sync),
  all off; those chosen are installed in the background from the store
  (switched on where they are built in) and said once when done. The
  store's queue installs plugins not there yet too.
- Releases publish PaperPaw's own store plugins as files for every
  device (tools/plugin-files), built first and then signed apart with
  the first-party plugin key (secret `PAPERPAW_PLUGIN_KEY`), with their
  folders and their entries for the plugin library; CI builds, signs
  (with a key of its own) and catalogues them for every device. Their
  version is the release's (`paperpaw_sdk::package_version!`).
- Emulator: `--free-space <bytes>` makes the emulated storage say it has
  that much free, whatever the computer's disk has, so screenshots of the
  store look the same everywhere (the snapshot tests use it).
- Plugin authors: `paperpaw-dev` (`tools/dev`) checks a plugin before it
  is published, on the emulated X4, X4 Pro, X4 Classic and Boox, in
  every language, light and dark, and look: contact sheets of every
  screen and of every widget in every size; text cut, shortened or
  broken, parts outside their cell or the screen, uneven margins, touch
  targets too small, focus not shown, contrast on black and white;
  missing translations, settings without help, dashes, and a
  pseudo-language that shows where longer text will not fit; screens
  reachable and closable with buttons alone; storage read while drawing,
  heap and frame time against the X4's budgets; scripted flows with
  assertions (recorded in the emulator with `--record`); snapshots with
  `--bless`; data plugins checked as the installer and the plugin
  library take them, `.ppn` files as the device does. Templates for a
  plugin's own repository (harness, snapshot test, CI job) are in
  `tools/dev/templates`; docs/plugins.md, "Checking your plugin".
- Emulator: usable as a library (`paperpaw_emulator`), with plugins
  added to the standard app, frames recorded with their storage calls,
  and `--record <file>` to write what is done in the window as a flow.
- SDK: what the drawing helpers describe carries the clip each part was
  drawn in; `Kernel::described` and `Kernel::described_active` give it
  as drawn.

- Store: easier to find one's way in a library of thousands of
  plugins. The start lists Search, the plugins installed (with how many
  have an update), the newest, and the groups with how many plugins
  each has; a group's list can be sorted by name, newest first or most
  recently updated first, and a long list goes to a page asked for (a
  long press of Select; on touch screens a tap on the page number or a
  long press on the arrows beside it). Rows are denser
  (name, size or an installed or update mark, one line of what it does):
  five a page on the X4 instead of three. *Update all* among the
  installed plugins updates them one after another (those waiting
  marked so); one stopped or set aside lets the next go on, and the rest
  wait through a restart or a lost connection until the store is opened
  again. The order of a long list says how to go to a page. A plugin's page
  says whether it is a code plugin (full access to the device) or data
  only, whether its build was checked against its source, since when
  its author publishes and how many plugins they have, how much room it
  needs and how much is left, and how much memory it takes where its
  author measured it; its screenshots (on the X4 only those small enough
  to decode) and its source as a QR code are a tap away. A code plugin
  that does not fit in the room left for code plugins is not offered,
  or refused once downloaded.
- SDK: `Storage::free_space` says how many bytes a storage has free
  where it can tell (the desktop and Android by the file system);
  storages that wrap another forward it. A memory card says the count it
  keeps at once, counts its FAT exactly a part at each ask (never all of
  it at once, as rust-fatfs does on a card not unmounted) and follows
  what is written and removed from then on.
  `native::CodeRoom` (with `CodeRoom::in_flash` and `in_psram`),
  `native::code_room` and `native::keep_code_room` say how much room
  code plugins have and take (the X4's flash region, the X4 Pro's PSRAM),
  as the shell found it at the start, also in safe mode;
  `native::installed_code_need` and `native::read_header` measure the
  code installed. `ui::draw_qr` draws a QR code (`paperpaw_sdk::qr`),
  `Header::on_page_number` tells a tap on a header's page number, and
  `UiStyle::noted_row_of` is the height of a row with a note of so many
  lines.
- Key bindings: a button bound on the keys page (also to Nothing) does
  what was chosen there at once, also where the emulator's own
  configuration binds it.
- Reader: a new text size (pinch, the slider, Larger and Smaller, a
  book's own size) lays the open book out again at once; before, it
  applied only when the book was opened again.
- Reader: "Next status line" is in the More menu of the reader on
  devices without a touch screen (by touch, a tap on the line switches
  it).
- Reader: skimming through the book starts without a focused button;
  Up and Down move to the buttons below the bar, Select presses the one
  moved to, and with none it stays where skimming went.
- Reader: where the position in a book, a bookmark, a highlight or a
  note cannot be saved (a full or failing card), a notice says so; for
  the position once a book.
- Reader: a page back where the pages are not counted yet goes about a
  page back and reaches the page left, instead of to the last page
  counted; a book whose saved place cannot be shown opens at its start.
- Reader: search and the contents open a book as the page shows it, so
  without the book's styles or with a stylesheet plugin a search no
  longer converts the whole book again; a search hit's row shows the
  word found on narrow screens.
- Reader: a bookmark keeps words broken across lines whole, and Up and
  Down move to the line above and below while a word is chosen.

- The library anew, as a calm list: the book being read first as a card
  (one press or tap to go on reading), then every book with its cover,
  its title on up to two lines, its author and how far it is read (a
  bar and the percentage, a check once finished, New for a book added
  in the last 30 days and never opened). What is shown is chosen above
  the books, from one row on small screens (All books, Reading, Unread,
  Finished, Next in your series, Shelves, Find a book) and from pills
  on larger ones. Folders no longer come first: they are collections
  on the shelves, with favourites, authors and series. A series lists
  its books by number with the next one marked; Next in your series
  lists the next book of every series begun. Find picks a book by a
  letter of its title or its author's surname, without a keyboard.
  Buttons work as in every list: left and right move a book, the page
  buttons turn a page. A book's sheet (hold it, or the menu button)
  shows it with its cover and progress: go on reading, its series,
  finished or unread, favourite, information, archive. The library's
  menu holds only the library's things.
- Library: books as covers (Show books as: Covers) in a grid from
  margin to margin, how far each is read on its cover, the books of a
  series as one stack with their count.
- Library: on the X4 and the X4 Pro more room: fewer rows a page, one
  line of details under a title, the filters behind one row.
- SDK: `library::Book::added` (the day the library first read a book),
  `library::{today, is_new, is_new_on, NEW_DAYS}`. A change for
  plugins: a `Book` written out field by field needs `added` too (or
  `..Book::default()`).
- Library: while a book is read, the library holds next to nothing (its
  list of books is kept in a file and read back when it shows again);
  coming back, the focus is on the book it was on and only what may
  have changed is read again. A book moved (to the archive, by the file
  manager) or deleted goes from the list at once and shows where it
  went; one gone unannounced is not opened but said to be gone. The
  archive's books are on the shelves, not among the others. Settings
  changed while the library shows (how books show, the order, the size
  of the interface) apply at once. On devices of little memory (the X4)
  it lists 2000 books at most and says how many there are beyond.
- SDK, power saving (ADR 0025): a plugin's manifest says what it uses
  (`Manifest::uses`, `Uses::NETWORK`): every plugin that reaches the
  network must declare it. Declared work (`work`: `Work`, `Moments`,
  `Command::Work`, `Command::CancelWork`; jobs in steps with `WorkSpec`,
  `Step`, `Registrar::add_work`, `events::WORK_CHANGED`; keys for
  several works of one event), power levels and modes (`power`:
  `PowerLevel`, `Power`, `Wear`; `Env::power`, `Env::work`), and one
  answer for what did not run (`Refusal` with a `Reason` and when to
  `Retry`; `events::WORK_REFUSED`, `WORK_ALLOWED`, `POWER_CHANGED`).
  Breaking: `Command::Schedule` is gone (work at a time, `Work::at`, or
  `EVENT.schedule`, replaces it); `HttpReply` has two more public
  fields, `refused` and `headers`, so replies built field by field need
  them; `Manifest` has `uses`; `PluginUsage` has `requests` and
  `refused`. `Env::reader` tells a plugin whether the reader waits.
- Kernel, declared work (ADR 0025): one queue for plugins' timers and
  work. Work runs at its time, at one of its moments (waking, a book
  opened or closed, going to sleep, the radio on anyway, charging, idle)
  or at its deadline, whichever comes first, as background work even
  when the reader's own doing brought it due; work that needs the
  network and is only nice to have runs only while the radio is on
  anyway, and nothing wakes the device for work that cannot run then.
  Declared again, work keeps its first time and a running job goes on;
  a timer with a period comes again. Jobs take one step a tick, never
  while input waits. A plugin may have 8 works on the X4 (32 elsewhere),
  declared under its own name only; a full queue says `WORK_ALLOWED`
  once there is room again.
- Network by the power mode and the manifest: requests, servers and
  sockets of a plugin whose manifest does not declare the network are
  refused (`Reason::NotDeclared`). In Offline only what the reader went
  online for goes out (`Command::GoOnline`, for the screen in front
  until it closes); anything else is refused (`Reason::Offline`) and
  may try again when allowed. `Command::CancelHttp` takes a plugin's
  requests back, each answered as cancelled.
- Quiet reading (ADR 0025): background work says nothing over reading.
  A notice from an event handler, an action run by a rule or a job,
  outside the reader's own doing and over another plugin's screen,
  becomes that plugin's note (`Env::work`, `WorkStates::note`), to show
  on its own page. What the reader does (a held button too), answers to
  their requests, Wi-Fi while they wait for the network and a plugin's
  own screen still say what they like. `Command::Announce` is for what
  must reach the reader anyway: a rule's message they set up, and what
  happened to the device (safe mode, plugins not installed, an update
  installed).
- Power modes (ADR 0025): Normal, Saving, Offline and Automatic, chosen
  in the quick panel (four segments where they fit) or under Settings,
  System, Power.
  Automatic goes by the battery: Saving below 20 %, Offline below 10 %,
  back 5 % above each, Normal while charging; it switches silently, and
  the status lines show a leaf while saving and Wi-Fi crossed out while
  offline, nothing in Normal. Saving makes declared work wait four times
  as long, sleeps after at most 5 minutes and keeps the front light at
  most at half; Offline keeps the radio off for background work, also
  while charging. A mode's values and limits lie over the reader's own
  (`Settings::wear`) and go when it ends; what the reader changes while
  it is worn is theirs until the next mode, also across a start. The
  modes are presets (`core.power-mode`), worn with `Command::Wear`;
  `Kernel::power` tells shells the level.
- Plugins on declared work: weather looks again only once the radio is
  on anyway (no hourly wake), four times as seldom while saving, not by
  itself in Offline; Readwise and Joplin send what waits at the next
  connection, at the latest after half an hour; OPDS sync, news and
  Wallabag fetches, Joplin's send and the update check, when a rule runs
  them, wait for a good moment (at the latest an hour), while the
  reader's own run acts at once (`Env::reader`, `Work::when_cheap`); an
  update a rule's check finds waits on the update's page instead of
  opening it over the book.
- Battery: *Network this time awake* lists each plugin's requests, those
  held back, its network setting where not the default and what its
  background work last said (`PluginUsage::requests`, `refused`).
- Progress sync (kosync) by the power mode: silently, when a book opens
  or closes, on waking and before sleep, as before; while reading with
  the radio off, the progress goes out when the radio is on anyway, at
  the latest after 30 minutes (two hours in Saving). Offline sends and
  fetches nothing; once allowed again, the books switched from go out,
  then the open one's latest is fetched before its own is sent.
- Network per plugin (ADR 0025): under Settings, System, *Network per
  plugin*, each plugin that uses the network has its own part: its
  *Network* on or off (off, its requests and network work are refused,
  `Reason::NetworkOff`), and *In saving modes*: follow the mode, as in
  Normal (its work keeps Normal's limits while saving, not in Offline),
  or only when asked (no background network at all).
- SDK, HTTP: `HttpRequest` is made with `HttpRequest::get` or `new` and
  methods (`header`, `body`, `body_from`, `save_to`, `with`), no longer
  field by field (`#[non_exhaustive]`). A request may set a `timeout`
  (by default 30 s, 10 minutes for a download to a file), the answer's
  headers it wants back (`want_headers`, read with `HttpReply::header`)
  and the longest body kept in memory (`max_body`); by default that is
  64 KiB on the X4 and the X4 Pro (it was 1 MiB everywhere) and 1 MiB
  elsewhere. Every shell keeps the time limit and passes the headers
  back.
- Themes: a theme's defaults (a data plugin's `settings.ini`, or the
  Pebble theme switched on) win over the device's own: a large-print
  theme now enlarges the interface on the X4 too, whose own default is
  small. What the user chose still wins over both.
- Screen: a full refresh a plugin's refresh filter asks for (a flash on
  some page turns) starts the count to the next periodic full refresh
  anew, where it came again as soon as if there had been none. How often
  the screen is refreshed in full may be a book's own ("Only this
  book"), e.g. more often for a comic.
- Plugins: a data plugin updated or removed while running takes its
  stylesheet for books and its key bindings with it; before, the old
  version's applied until a restart, beside the new one's.
- Plugins: an update from the store that would not load (a translation
  with a mistake, say) is refused before anything changes, so the
  version before stays; before, it was removed and the new one marked
  failed. A plugin offered under the name of one built in (such as
  the default fonts or Wi-Fi) is refused, and installing a plugin from
  the folder it is installed in no longer deletes it.
- Plugins: data plugins are read one at a time at start, each let go
  once registered, instead of all of them first; one switched off keeps
  only its name and description in memory. A language pack beside a
  few other data plugins no longer fills the X4's memory at every start.
- Passwords and keys: the file that holds them is read again before
  each change is saved. Where it cannot be read, it is left as it is and
  the device says so; once it can be read again (corrected on a
  computer), what it holds is used again and kept along with what was
  entered since. Before, one failed read at start kept every later
  password from being saved, without a word.
- Languages: the kernel's translations name the language's decimal mark
  (`.` in English, `,` in German), which every plugin can use for the
  numbers it shows.
- SDK: a time a screen asks for with `Cx::wake_at` is that screen's:
  it comes while the screen is on top, waits while another covers it
  (a menu, a keyboard, a quick panel) and is kept when the screen is
  made anew for another size; before, it came to whatever screen was on
  top and was forgotten whenever a screen opened or closed.
  `Screen::on_close` is also called on the old screen when a screen is
  made anew; the commands it gives then are not run.
- Data plugins: translations named in capitals (`DE.INI`, as a memory
  card or an old computer may name them) are read too.
- Clean up storage: what is kept for a book still on the memory card
  stays, also where no plugin opens its format now (switched off) and
  whatever the case of its folder's name (`Books` or `books`); where a
  folder cannot be read, nothing is removed and the device says so.
  Before, notes, positions and covers of such books were deleted.
- Plugins: a plugin being updated when the power went is no longer
  lost: the version set aside for the install is put back at the next
  start.
- Plugins: a plugin (also a data plugin folder) named `plugins`,
  `fonts`, `menus` or `slots` is refused with a warning: its settings in
  `paperpaw.ini` would have been read as that section's entries and
  dropped.
- Speech: with "Say what has the focus" on, moving the focus quickly
  says each row in place of the one before, where only the first was
  said until it was done.
- Screen: a menu or other screen opened again for something else (in
  its place) no longer leaves pieces of the one before on the screen.
  Turning the screen on the X4 makes the screens anew once, not twice,
  and turning it half way round brings the row of button hints back
  when it is upright again. Where memory is short when the screen is
  turned or a picture is shown in grays, the frame is drawn in smaller
  bands rather than not at all until a restart.
- Buttons: a button let go no longer stays held when its press came
  twice around a new screen showing (pressed again, or its release
  missed): the kernel took it for held, and a long press followed by
  itself (going home), or the next release was lost. The same goes for
  a finger put down again.
- Touch: two fingers resting on the screen no longer keep the device
  awake (the kernel asked over and over to be woken for a long press
  two fingers never make), and the finger left after lifting one of two
  makes no long press either. A long press works at the edges of the
  screen too, where swipes in from the edge begin: on the first line of
  a book (to select a word) and on the keys along the edges of the
  keyboard. Swipes in from the edges work as before.
- Screens: a time a screen asks for is its own: it comes to that screen
  once it is on top again, where before it came to whatever screen was
  on top (a menu over the reader got the reader's) and was forgotten
  whenever a screen opened or closed (the library's shelves stopped
  being made). A screen covered by a menu, a keyboard or the quick panel
  waits meanwhile: pages no longer turn by themselves and the clock no
  longer refreshes below them. A screen made anew (the screen turned)
  first keeps what it would lose, as when it closes: the reader its
  position, which went back a few pages.
- Settings: a `paperpaw.ini` that is there but cannot be read (saved in
  another encoding than UTF-8, larger than 64 KiB, or a read of the
  memory card failed) is no longer written over: PaperPaw starts with
  the defaults, says so, and leaves the file as it is, where before the
  next change of a setting, a key binding or a menu wrote a new file
  with only that change and every other setting was lost. The same goes
  for a book's own settings ("Only this book") and for the file of
  passwords and keys. A `paperpaw.ini` saved as "UTF-8 with BOM" is read
  whole (its first section was lost). Saving a book's own settings keeps
  what else its file holds, such as the settings of a plugin switched
  off.
- Settings: "Only this book" holds while the book is open, also after
  the screen was turned or the row of button hints changed in a menu
  over the book; before, the next change went to every book.
- Key bindings: binding a button in PaperPaw no longer drops the
  emulator's own bindings from its user configuration.
- Storage: whether names in another case are the same file (as on the
  memory card) reaches plugins through the kernel's own layers; it read
  "no" everywhere before. Where they are other files (a Linux disk), the
  kernel no longer takes `A.txt` for `a.txt` in what it remembers of a
  call's reads.
- Wi-Fi: once work that waited for a network fails because none came
  (away from every known network), nothing wants the radio any more,
  so it may go off: before, it stayed wanted and the Wi-Fi plugin
  looked for networks every few seconds until the device slept.
- SDK: `firmware::compare_versions` orders pre-releases as SemVer does,
  numbered parts as numbers (`0.5.0-rc.9` before `0.5.0-rc.10`), so an
  update between numbered pre-releases is offered.
- Fonts browser: lighter on the X4. Only the files this device installs
  are kept of the catalog, a font's page gets its font from the list
  instead of reading the catalog again, and the samples held share 48 KB
  there (the fonts past it are named in the menus' font); the list lets
  go of its samples while a font's page is open. The catalog is fetched
  once each time the list opens, no longer on every return to it. A
  catalog with more fonts or files than are read no longer mixes the
  last font kept with those after it, and a download a restart cut off
  no longer shows as going on forever.
- Store: a plugin of the library with the id of a plugin built into
  PaperPaw, or of the other kind (data for code, code for data), is
  neither listed nor installed (*... cannot replace*), so a library can
  no longer take the place of, say, the fonts or Wi-Fi. A plugin's page
  is fetched into a file of its own and taken only if it is that
  plugin's, and it keeps the query of the library's address (a key a
  library of one's own asks for).
- Plugin manager: a plugin installed or removed while its list is open
  (from the store opened from it, say) shows there at once; the list
  was the one of when it opened.
- Update: an update file on the card no newer than the version running
  says so (*an older version*, *again*) and installs as *Install older
  version* or *Install again*, with *Search online* beside it; a newer
  update the server names comes first. A version from the update server
  that is no version (with `/`, say) is no offer, so it can no longer
  name a file outside `updates`, and a download a restart cut off
  (`updates/.download`, `.download.part`) is removed at start.
- A new look, rounder and friendlier: soft corners, pills for buttons,
  switches, chips and the highlight, plain headers (the title on the
  page, a little larger, no bar or line), faint lines between rows from
  where their text starts, margins of 4 mm. Rows with
  a note keep smaller corners, so the highlight never comes near their
  text; the home screen's grid, its frames and the focus are round too.
  A paw: beside the icon of empty screens (empty shelves of the library
  now show an icon and a title too), as the busy mark, and on the X4 in
  the hint of the button whose press is being worked on.
- Appearance settings: the text size and contrast for everyone; with
  advanced settings the header (plain, light, dark), corners (square,
  slightly rounded, round, soft; round and soft make pills), the
  highlight (dark bar, frame, light bar), lines between rows (faint,
  lines, none), lists on cards, the size of headers' titles, margins,
  rows and lines. Pebble, a theme for touch screens with lists on
  rounded cards and a light highlight, comes switched off in the plugin
  manager; `samples/plugins` has two more themes, Paw and Calm ink. The
  classic look is the Airy sample's light headers and small corners.
- Library: with round or soft corners, covers have rounded corners too,
  at most as round as a book's. The grid of covers keeps its covers
  clear of a framed focus, its focus a box; titles on a light focus
  stay black.
- Headers: their title, note, page number and arrows are centred by
  their ink, no longer by their lines of text, which set them up to
  nine pixels off the middle; the gray arrow at the first or last page
  shows as gray on panels of black and white too.
- Status lines: where their items would not fit whole (wide margins,
  large text), the gaps between them narrow first, so the quick panel's
  date is no longer shortened.
- Setup: its margins are the menus' and a gap, no longer twice the
  menus', so wide margins leave its choices room.
- SDK: `UiStyle::{pill, focus, separators, plain_header, cards, grays,
  head}`, `UiStyle::radius`, `focus_ink`, `focus_paper`, `focus_faint`,
  `dark_focus`, `ui::draw_separator`, `ui::Focus`, `ui::Separators`,
  `ui::PAW_ICON`, `Thumbnail::draw_round`; settings `core.ui-focus`,
  `core.ui-separators`, `core.ui-groups`, `core.ui-title`,
  `core.header.plain`, `core.corners.soft`; graphics:
  `Canvas::fill_corners`, `round::corner_inset`. Plugins draw what is on
  a focused row in `focus_ink` rather than white, so every highlight
  works.
- Home screen widgets fit their place in every size, on the X4, the X4
  Pro and large screens alike: text that must stay on one line (times,
  dates, numbers, titles) is made smaller before it is shortened, longer
  text breaks between words and never inside one, and a part that does
  not fit (a caption, a note, a cover too small to tell) is left out
  whole rather than cut or drawn over its neighbours. Sizes are found as
  the font draws them, so the font packs of the X4s fit as well. Fixed
  on the way: the continue card ran out of its frame in low cells, the
  world clock's places overlapped, the status widget's battery ran past
  a small cell, the streak's second line was cut, the calendar's digits
  ran together, the year's note fell below its cell. Each widget has a
  spacing setting (automatic, none, small, normal, large; per
  placement): automatically, widgets without a frame line up with the
  page's margins as before, and framed ones keep a gap inside their
  frame.
- Home screen and widgets, as `paperpaw-dev` found them: with buttons
  alone, the tool the focus is on is told by its name (screen readers
  and checks follow it); a tool's name too long for its tile is made a
  little smaller, then put on two lines where the tile is high enough;
  every setting of a widget explains itself (frame, size, alignment and
  the parts shown in the SDK, the rest in the widgets); the streak keeps
  its marks inside a wide row; shortened texts in small cells give way
  to shorter forms (the countdown's "days", the year's "Day 287", a
  quote's author by the last name), titles grow smaller before they are
  shortened, and covers without a picture show whole words only.
- Home screen tools: an icon and its name are placed by their ink as
  one group, half a gap apart, so that on the X4s the name no longer
  sits as near the tool below as its own icon.
- SDK, widgets: a spacing setting for widgets (`widgets::Padding`,
  `padding_spec`, `padding_setting`, `padding`; automatic, none, small,
  normal, large), which `look` and `draw_frame` keep. Text that fits
  its place: `fit_line` and `draw_line` (one line, smaller down to a
  least size, then shortened, centred by its ink), `largest_size`
  (measured as drawn, so fonts of a few sizes fit too),
  `ink_baseline`, `Wrapped::fitting`, `Wrapped::{size, lines, whole}`.
  `Wrapped`, `draw_wrapped` and `draw_message` break text between
  words only, never inside one, and make it smaller, down to the small
  size, before they cut it.
- Books: no book can crash the X4 or exhaust its memory any more. A
  picture's long alternative text in CJK script crashed the EPUB
  converter, and with the last book reopened at start, possibly at
  every start. Converting a chapter now takes bounded memory however
  its markup is made (deeply nested or unclosed elements, many text
  nodes, pictures, names, links), and what it writes is bounded by its
  size; a chapter that cannot be converted is not tried again until the
  book is opened again. What is read of an EPUB's package, a
  FictionBook's description and a comic's page list is bounded too;
  KOReader positions, anchors and printed page numbers are found a line
  at a time and in few reads; a cut or damaged cache is converted again.
- Books: a book replaced by another of its name, even of the same size,
  is read anew, without the pictures and chapters of the old one;
  chapters converted with stylesheets one no longer has go.
- EPUB: stylesheets in Latin-1 keep their rules; quoted strings and
  comments in stylesheets no longer end a rule. Pictures that cannot be
  decoded are shown as their alternative text and not decoded again on
  every page.
- FictionBook: the contents are kept on the card and lead to their
  titles also after self-closing sections; named styles (`<style>`)
  keep their words; a missing or broken cover is not looked for again
  each time the library shows the book.
- Comic books: the first pages are kept (5,000 at most), where pages at
  the end were kept before, and the page list is made once.
- Markdown: files of very long lines or many brackets open quickly in
  little memory.
- Plain text books read UTF-16 files and keep stray Latin-1 letters in
  otherwise UTF-8 text, pages of blank lines no longer end a book
  early, and paging reads far less from the card.
- XTC books with damaged or oversized pages fail cleanly instead of
  crashing the X4 or filling the card, gray pages open much faster, and
  long chapter lists take little memory.
- EPUB: small capitals of a book's stylesheet apply.
- EPUB: a quoted string left open in a stylesheet ends at its line, so
  the rules after it apply, and a font name can no longer break the
  stylesheet's cache.
- EPUB: CDATA in a chapter is shown as written, its entities not decoded.
- FictionBook: a second picture of the same id that cannot be decoded no
  longer removes the first.
- Markdown: two spaces at a line's end break the line after it, not
  before; very long lines are no longer cut inside words.
- Plain text: old Mac line ends (a carriage return alone) part words.
- Plain text: a long paragraph starting at certain places no longer
  loses its first word when read from inside it.
- Memory cards: books and folders whose names begin with a letter
  beyond ASCII ("Ärger.epub") no longer crash the X4; names of up to 255
  letters can be written, a file named like another's temporary one is
  never replaced, and a write that fails on a full card leaves nothing.
- Desktop and Android: no call can remove or write over the storage's
  top folder through an empty path.
- Updates: numbered pre-releases compare by number, so `0.5.0-rc.10`
  is offered on `0.5.0-rc.9`.
- Library: going up to a folder that starts a page shows that page.
- HTTP: an answer's head over the limit is refused also when it comes
  at once; redirects from an address without a path keep their target.
- Servers: port 65535 can be listened on; a socket closed right after
  sending still delivers what was sent.
- Progress sync: a position held back while its book's progress was
  fetched is sent even when another book was opened meanwhile.
- Browser: a server's error page is shown again.
- Weather: a day whose date cannot be read no longer shifts the others,
  answers with more days than asked for keep the first ones, and values
  at their limits no longer overflow.
- OPDS: a cover of the catalog shown before no longer appears beside
  another entry.
- News: very long feed titles are cut like article titles.
- Lock: a damaged kept code no longer locks the device for good, and the
  wait no longer shows a second too many.
- Battery: hostile logs no longer overflow, a damaged line loses only
  itself, 3999 mV shows as 4.00 V, and the hours left fit on the X4.
- Joplin: its menu entries show icons again.
- Native code plugins as files (ADR 0024): a code plugin
  can be a file in its folder on the card (`plugins/<id>/code.ppn`),
  which the Xteink X4 and X4 Pro load at their start, without a new
  firmware: the X4 Pro runs it from PSRAM, the X4 writes it once into
  flash behind its firmware and runs it from there, for some hundred
  bytes of RAM. Its code is that of a plugin built in, with one line
  more (`paperpaw_sdk::plugin!`); `paperpaw-sign pack` makes the file
  and signs it as its author. A file fits the PaperPaw built by the same
  compiler from the same SDK (its fingerprint, shown under *About* and
  sent with the update check). Plugins the web flasher writes behind the
  firmware are installed at the first start, their authors' keys
  remembered as the store does; code signed by another author than the
  version installed is not, and the plugin manager says so. Only code
  installed runs: the store and the web flasher's install name a file
  by its digest (kept in `.paperpaw`), and one put into a plugin's
  folder otherwise, over the Wi-Fi transfer or on the card, is not
  loaded (`tools/x4-remote.py --trust-code` names an author's own build
  over USB). Safe mode:
  after two starts that did not get to their first frame, or with *Back*
  (X4) or *Home* (X4 Pro) held at power-on, PaperPaw starts without code
  plugins and switches off the one that crashed; the plugin manager says why a code
  plugin does not run. `samples/hello-native` is the sample plugin as a
  file, with a CI job for a plugin's own repository.
- SDK: `native` (the fingerprint, `plugin!`, `Env::native`,
  `check_file`, `Native::not_installed`: plugins a shell did not
  install at the start, and why; `remember_code` and `installed_code`:
  the code installed, by its digest), `PluginProblem::NotInstalled` and
  `Changed`, `PluginInfo::kind` and
  `PluginInfo::problem`;
  `Registrar::publish` publishes a service with its type's name, so
  plugins built apart find it (`Registrar::add_service` is deprecated);
  a float in an event's parameters is read without `core`'s large exact
  parser (16 KB less in a native plugin that takes reading events).
- Kernel: `KernelBuilder::code_plugins` with the shell's `CodeLoader`
  (which loads a `CodeFile`: the plugin's id from its `plugin.ini`, its
  path, the digest installed), `safe_mode` and
  `install_plugin` (plugins a shell put in folders, installed before
  plugins load); `Kernel::describe` describes a notice showing.
- Dictionary: a damaged dictionary is no longer indexed again with
  every word looked up (each time reading its whole index): it is
  left out until its files change. The prepared indexes of
  dictionaries no longer in the folder are removed. Where a word can
  be looked up online, the row of button hints says which button
  does it.
- File manager: a folder of any size is read a page at a time, as the
  library reads its folders: a folder of thousands of files no longer
  fills the X4's memory. Moving or renaming a folder tells only of the
  books in it moving (not of every file), so their progress and notes
  go along, and says so where a folder holds more books than are told
  at once; settings naming the folder or one in it (the library folder,
  the archive, the dictionaries) follow it. A name that differs in case
  only is taken as it is (it got "(2)" on the memory card), through a
  free name between, so nothing is lost on storages that ignore case;
  names the memory card or Windows refuse (`* ? " < > |`, a dot at the
  end) are refused. A file picked to be moved goes when its folder is
  deleted.
- Library: while a book or another full screen covers it, the library
  lets go of its covers and page and reads nothing when books arrive or
  a setting changes (a font size in the reader, say); it reads its
  folder once when it shows again, and then only the page with the
  book the focus was on, which keeps the focus (also after marking a
  book finished or archiving one). Covers that could not be made small
  show a stand-in instead of being read from the book in every band;
  zipped books are named alike in the list and the grid; sizes in the
  book information use the language's decimal mark. The folder chooser
  opens in the folder set (it always started at the top), and names of
  new folders leave out what the memory card and Windows refuse. The
  shelves (by author, series, tag) make their catalog once, again only
  after books came, went or moved, the library folder changed or after
  half an hour, and making it goes on where a book was opened
  meanwhile (it stayed at "Collecting the books" for good).
- Vocabulary: the list of words is never lost to a failed read or
  write. It is written anew beside itself and put in its place only
  once complete, so a full card or a read error partway keeps it as
  it was, and after a power cut at the wrong moment the new list is
  taken at the next start. Deleting removes the word chosen even where
  the list changed meanwhile or holds a line cut off by a power cut;
  the next word looked up after such a line starts a line of its own,
  and the list shows words looked up elsewhere when it shows again.
  Looking up the newest word again writes nothing. A word that cannot
  be kept or deleted says so. The export for Anki tells Anki its
  fields are separated by tabs and plain text, and quotes fields with
  quotes in them. Without a clock the word of the day is drawn at
  random (else the newest) instead of staying on the oldest word; with
  one it changes at local midnight.
- Widgets: no widget panics where it has little room any more: the
  year so far with lines of 0.8 mm or more (on every draw, so a home
  screen showing it restarted the X4), the year ring, the two-month
  calendar, the world clock and the battery ring in small cells. A
  test draws every widget of the pack in each of its styles and pairs
  of its settings in tiny and narrow cells with the thickest lines.
  Until the clock is set, the time widgets say so instead of drawing an
  empty frame or a face without hands. The countdown no longer reads
  "12-280" as 24 December.
- Widgets: the to-do list is written back only once it was read: a list
  that cannot be read (another encoding, too large, the card failing)
  is left as it is, and the widget and the list say so; items past the
  hundredth are kept; a failed save is reported. Items move down as
  well as up. A quote, picture or highlight set to change "each time it
  shows" counts its showings in memory instead of writing on every
  making of the widget, so it no longer moves on with unrelated
  changes. "Next up: the newest" finds new books in a library of any
  size (a set of 8 KiB, `seen.bits`, in place of the first 4,096
  books' hashes); a long series is counted whole and kept round the
  book being read. Settings that need it are explained.
- Home screen: a UI margin of 0 no longer breaks the grid (a panic in
  debug builds, taps missing their cells in release). Pages are kept by
  their own setting: editing page 3 from the settings while page 2 is
  empty edits page 3, and *Add a page* adds one that shows until it is
  removed. The layout chooser fits screens without touch and offers
  only layouts whose widgets and tools are there. Widgets are made once
  as the screen shows and anew only for changes of the home screen, the
  look or their own settings; other changes have them read anew.
- X4 Pro: the front light is set in single percents and dims evenly to
  the eye. Its percentages follow the lightness the eye sees (CIE L*)
  rather than the square of the light, on a PWM of 12 bits instead of
  10, so each step looks about as large as the next, also at the dim
  end where the eye is most sensitive: from 1 % to 10 % the light now
  has ten levels where it had two (5 % and 10 %, the second five times
  as bright as the first); 1 % is dimmer than the least light before,
  100 % as bright. `−` and `+` change it a
  percent, a slider moves it further; its colour stays in fives.
- Reading, after a comparison with CrossPoint,
  CrossInk and KOReader:
  - Holding a page button goes a chapter on or back (*Hold a page
    button for the next chapter* under Controls, advanced, turns it off,
    and a page then turns as its button goes down); *Next chapter* and
    *Previous chapter* can be put on any button or gesture. Going a
    chapter on goes to the first that begins on a later page, and
    skimming goes chapter by chapter also through parts.
  - The X4's front buttons Left and Right turn pages in a book, as they
    did nothing there; *Swap the page buttons* under Controls turns the
    page buttons the other way round while reading.
  - Holding *Select* while reading sets or removes the page's bookmark
    (one press with buttons instead of five); *Bookmark* sits in the
    reader's bar instead of under *More*, and *Font* comes first in it,
    focused as it opens (two presses instead of five).
  - *Go to a percentage* and *Go to page* in *More*.
  - Status items: the page in the chapter (5 / 25), the pages left in
    it, and how far into it the page is as a short bar.
  - *Margins at the top and bottom* (advanced), as the sides until
    changed; *Space between paragraphs* and *Indent paragraphs* as two
    switches (who had turned indents off turns the spacing on for the
    look of before); line spacing down to 80 %, closer than the font
    sets its lines.
  - *Turn the screen* may be a book's own: turned while reading with
    *Only this book* on, for that book alone.
  - Fixed: a page back after a jump (contents, skimming, a chapter)
    went to the page read before the jump.
- Settings: rarely changed reading settings (reopening the last book,
  footnotes, the seconds between pages turned by themselves, the status
  lines' distance from the edge, replaced words, the dictionary folder,
  keeping words looked up, the language to translate into, the corner
  bookmark) are at the advanced level; those about taps and swipes show
  only on touch screens; the full refresh every so many pages is basic.
- SDK: `MenuNeed::Touch` for menu entries and settings that need a
  touch screen; `settings::SWAP_PAGE_BUTTONS`;
  `Registrar::add_keymap_when` (`Contribution::KeymapWhen`) for key
  bindings that apply while a switch is on; `SettingExtra::follows` for a
  setting that has another's value until set itself;
  `StatusValue::Bar` for status items shown as a short bar.
- Quick panel and reader's panel: on touch screens their sliders (the
  front light and its colour, the text size, spacing and margins) are as
  on the settings pages, across the screen with their title and value
  above, and every slider is a fingertip high (7 mm from the panel's
  dpi) with a large knob. A finger that comes down anywhere on a
  slider's band and moves sideways moves the knob as far as it moves
  itself, from where it was, so it never jumps under the finger and the
  finger need not hide it; a tap on the track sets the value there, a
  tap on `−` or `+` (or beside them) one step. While it moves the value
  shows at once, the light follows it, only the slider is drawn again
  with a quick refresh, and nothing is read from the card; lifting the
  finger keeps the value and refreshes the panel cleanly. The rest of
  the track is a thin black line, which quick refreshes keep. The
  panels' buttons are a fingertip high on touch screens too, a little
  apart, and buttons share their rows evenly (four two and two, not
  three and one), so the X4 Pro's quick panel fits on one page; the
  quick panel may cover up to 95 % of the screen, as phones' quick
  settings do. With the buttons, a slider is chosen and then moved as on
  the settings pages. A focused row that opens a list (*Font*) is drawn
  as other rows are, its name no longer cut at the left. With buttons alone (the X4) the panels are as
  before. The home screen no longer builds its widgets anew while a
  value is shown above it, which read the card on every step. On Boox
  devices the light follows a slider while it moves. The emulator has
  the front light of the X4 Pro and the Boox profiles, and its scripts
  can put a finger down, move and lift it step by step (`down`, `move`,
  `up`).
  SDK: `UiStyle::finger` (a fingertip's size on touch screens, 0 with
  buttons alone) and `UiStyle::touch_row`; `ui::slider` bands are a
  fingertip high on touch screens, `SliderDrag::on_pointer` takes the
  slider's band, the style and its value and moves the knob relative to
  where the finger came down, `Slider::knob_x` says where the knob is.
- Memory card: a file renamed to another case of its name (`book.epub`
  to `Book.epub`) is no longer removed, and a folder is no longer moved
  into itself by another case of its name. A file open under one
  spelling of its path and written anew or removed under another is no
  longer read from, or written back over, clusters given to other
  files. Changes reach the card in the order they were made, each one
  when it is done (appends still when written back): cut off by an
  empty battery, the card holds a file's old content or its new one,
  whole, but for short moments the FAT library leaves open (a file
  removed has its clusters freed before its name is gone; a file moved
  to another folder is at neither place for a moment). A damaged folder
  whose clusters run in a loop is no longer read for ever.
- Going back to an older update no longer lets it write over the
  library's files a newer PaperPaw wrote: annotations, the books opened
  last, the favourites and a book's details are read through
  `data::Format::FIRST` and left as they are when they say a later
  version. SDK: `data::Format::FIRST`, and `StorageError` from
  `data::Error`.
- Emulator and Android: a file written whole is on the disk before it
  replaces the old one, so a power cut leaves the old or the new, and
  files of one name with other extensions (`a.x`, `a.y`) no longer
  share one temporary file.
- SDK: `Id::unplaced` gives an id without its placement
  (`widgets.clock@2.face` to `widgets.clock.face`) as an id; `placed`,
  which did the same under a name that said the opposite, stays until
  plugins have moved. `Id::placement` of a placed id is another
  placement of the same (`widgets.clock@2` and 3 make `widgets.clock@3`),
  not an invalid `widgets.clock@2@3`.
- Storage: paths with control characters, a dot or space at the end of
  a name, or a name Windows keeps for a device (`CON`, `nul.txt`,
  `COM1`) are refused on every storage, as FAT and Windows take them for
  another entry or a device. Names made from the web
  (`web::clean_name`) leave those out, `_CON.txt` for `CON.txt`, and are
  cut as FAT counts their length. `web::host` leaves out a user and
  password written before the host.
- Passwords and Wi-Fi: a network whose name holds a tab or a line break
  is joined again by its name (it was kept with a space for them).
  Passwords are left out where secrets and Wi-Fi commands are printed
  for debugging.
- Time zones: a rule of your own (`core.time-zone-rule`) may name the
  days summer time starts and ends by the day of the year (`J60`, `59`)
  as POSIX allows, not only by week and weekday; one that is not a rule
  falls back to the zone chosen from the list instead of the system's
  (UTC on the X4).
- Library: searching a large library no longer runs the X4 out of
  memory. A search holds the first 200 books by title (it held up to
  2,000, unsorted until all were found); `catalog::search_after` pages
  through the rest and `catalog::page` and `catalog::count` page any
  view of the catalog the same way, a page in memory at a time.
- SDK: `library::clean_up_unused` removes what is kept for books no
  longer on the storage without touching what any book there needs:
  the data of files of formats switched off stays, and so does the data
  of a book opened by another spelling of its folder (a card's `Books`
  read as the library folder `books`). Where a folder cannot be listed,
  nothing is removed.
- Library: a folder that has become empty no longer stops the library
  in the emulator (a page of an empty listing was counted below zero).
- Data files: a file marked `# format 0` is left as it is instead of
  stopping PaperPaw; versions start at 1.
- The order of a status line's items can be changed with buttons alone:
  *Menu* (a long press of *Confirm* on the X4) takes the focused item
  along, the direction and page buttons move it, *Confirm* or *Back*
  put it down. Items of plugins not there now keep their places when
  others are shown, hidden or moved.
- The choice overlay (what to do with a file, whether to delete it)
  puts options that do not fit on pages, with a pager below them, so
  every option can be reached by touch. On touch screens, a button
  pressed on such a list or on a status line's items first shows the
  focus instead of choosing a row nobody saw.
- Home: on a touch screen with buttons, a tap during a move by the
  buttons (e.g. on the pencil) keeps what they moved; the second of two
  equal items without room is no longer dropped from the page.
- Status lines draw nothing past their area: icons, gauges and bars
  that do not fit whole are left out, a battery without its percentage
  where only the battery fits. A word's background at the left edge of
  the page keeps its padding on the right.
- Sliders: the buttons come back from an end that is no whole step to
  the steps a drag gives (from 12 in fives to 10, not 7); a drag brought
  back to where it began keeps the value, also where steps are finer
  than pixels; a touch whose end got lost no longer holds a slider or a
  row's grip.
- A kept thumbnail whose header claims a picture larger than the box it
  was made for takes no memory for it (a file of 17 bytes could ask for
  8 MB). Empty
  screens in narrow areas leave out the paw (or the icon) that would
  not fit.
- SDK: `Settings::define` and `define_fixed` refuse a setting past the
  32,768th of its kind instead of mixing it up with another; changing a
  spec kept in the program with no place left for its copy says
  `SettingError::Full`, and `Settings::set_choices` says whether it
  did. `Thumbnail::kept` takes the box the thumbnail was made for and
  reads no header claiming more.
- Headers translate their page number; on and off in setting rows are
  the kernel's words (`core.on`, `core.off`), also where the settings
  plugin is switched off.
- SDK: `ListView::confirm` (the first press shows a hidden focus) and
  `ListView::set_len` (the focus kept in a shorter list).
  `Slider::stepped` goes to the next step a drag gives;
  `ui::next_value` steps from a value not among the options to the
  first (or last) option and stays within a number's range at its
  ends. Fields set past what the constructors allow no longer bring a
  list down (`ListView::row` 0, more columns than fit, a `Slider` with
  `max` below `min`), nor do `Rows::new` with marks and heights of
  different lengths. Steppers measure the room of their value by its
  width.
- Section headings in lists end at the right margin, shortened with
  "…" where too long, as the rows' titles do (a long one reached a pixel
  past its row).
- Headers: the back arrow's ink begins at the content's margin, where
  list icons begin, and the title where labels after icons do; the next
  page's arrow ends at the right margin (on the Boox the arrow sat 8 px
  left of the icons).
- SDK: helpers that plugins wrote each for themselves, now in one place.
  `Storage::ignores_case` says whether a storage takes `Books` for
  `books` (memory cards, Windows and macOS disks do), and
  `files::same_path` and `files::is_within` compare paths as it does;
  `files::has_extension` ignores case everywhere. Files made in parts
  (downloads, copies) go to `files::part_path` and are put in place
  when whole with `files::put_in_place` or `files::put_in_free_place`.
  `web::USER_AGENT` names PaperPaw with its version (`web::user_agent`
  as a header), `web::base_url` makes a server's address as entered a
  base URL. `time::clock_of` and `time::twelve_hours` write a time of
  day as the user wants it; the calendar is public: `time::is_leap`,
  `days_in_month`, `days_in_year`, `days_from_civil`,
  `civil_from_days`, `weekday`, `weekday_of`, `day_of_year`,
  `iso_week`. `numbers::decimal_mark` is the language's decimal mark,
  `numbers::fixed` writes numbers with it.
- Files: a copy cut short (a full card) no longer leaves a half file
  that looks whole: each file is copied to a part and put in place when
  whole. A folder is not moved or copied into itself by another case of
  its name, and finding a free name ends after 65,536 tries.
- Data that cannot be read is no longer written over: annotations, the
  books opened last, the favourites and a book's details were taken for
  empty when their file could not be read (a failing card, a file too
  large, text saved in another encoding), and the next change wrote a
  file with only that change. Now nothing is written then. A book's
  details keep their values to a kilobyte and 32 tags, so they are
  always read back (larger ones were written but never read, and the
  book was read anew in every list). A UTF-8 byte order mark, as
  Windows Notepad writes, is read past. SDK:
  `files::load_for_update`, `files::load_text_for_update`,
  `files::read_text`, `files::without_bom`,
  `annotations::load_for_update`; `annotations::add`,
  `annotations::save` and `library::save` say whether they kept what
  they were given; `annotations::moved` (unused) is gone;
  `StorageError::TooLarge` (what `read_file` says of a file over its
  limit, before `Unsupported`) and `StorageError::InvalidData`;
  `data::Format::load` tells a missing file from an unreadable one.
  Annotations are read line by line, and where they are only drawn,
  without their texts.
- Library: a folder whose name ends inside a character, such as one
  named with two or three Chinese characters, no longer stops the
  library (the X4 restarted whenever the folder's parent or a catalog
  shelf was shown).
- Security: a redirect no longer takes a request's credentials to
  another server, nor any request from `https` down to plain `http`.
  Every shell follows redirects by one rule (`paperpaw_http::follow`):
  to another scheme, host or port only headers that never carry
  credentials go along (`Accept`, `Range`, `User-Agent`, …; not
  `Authorization`, cookies or a service's key headers), a body never
  (the request fails), and from `https` to `http` not at all. Android
  follows redirects itself for this, and now tells plugins where every
  redirected answer came from (`final-url`), not only after a change
  of scheme. Addresses with control characters are refused, spaces and
  other bytes a request line cannot carry are percent-encoded, a
  redirect's `Location` resolves as RFC 3986 says (`?query`, `../`),
  and IPv6 hosts work (`http://[::1]:8080/`).
- HTTP answers for a file (`save_to`): only a successful one (2xx) is
  saved; an error's body (404, 500, also a 304) is its reason for the
  plugin and leaves the file as it was, and the X4 replaces the old file
  only once the new one is whole. A download longer than the limit
  fails instead of being saved cut short. A request saving to a file
  another is still saving to replaces that one (it fails at once,
  "replaced by a newer request", and its late answer is passed over)
  instead of both being mixed into the file. A body for the plugin
  moves through the kernel without copies (it was held up to three
  times), and the X4 sends a request's body without copying it first.
- Sockets: text read from a socket keeps a character whole where a read
  ends inside it (it comes with the next read), so non-ASCII text, e.g.
  Calibre's metadata, no longer turns into replacement characters at
  random and no longer upsets a protocol that counts bytes.
- Plugins' servers on the emulator and Android (Wi-Fi transfer, the
  phone keyboard) no longer let anyone in the network tie them up: a
  request's head is read by the same parser as on the X4 (up to 8 KiB,
  within 30 seconds in all; 431 or 400 otherwise, also for a
  `Content-Length` that is no number), a server serves 8 connections at
  a time and answers more with 503, an answer that comes before an
  upload is whole (507, no room) ends it at once, and stopping a server
  ends its connections and frees its port without polling. Request
  bodies a crash left in `.paperpaw/tmp` are removed when the next one
  comes.

- Damaged or crafted files can no longer restart the X4, freeze the
  reader or use up its memory; what cannot be read is refused or left
  out, and the rest carries on:
  - Pictures: a GIF picture without width or height, or larger than
    the limit, is refused; parts of a GIF outside its screen are no
    longer decoded. A small JPEG that claims to be huge, stops its data
    early or repeats its scans takes milliseconds instead of minutes;
    where its data ends, the rest is filled evenly. A PNG with a colour
    table of more than 256 colours opens.
  - Statistics sync: a damaged or crafted KOReader statistics file is
    rejected, and reading it takes little memory however large it is.
  - Dictionaries: a damaged StarDict dictionary is handled safely,
    compressed or not; one lookup keeps to 32 KiB of text, so very long
    definitions are shortened.
  - Catalogs and web services (OPDS, the plugin store, Joplin,
    Wallabag): a broken escape, a very long number or key, or very deep
    nesting in JSON no longer takes the device's memory; parts nested
    too deeply are skipped. Broken escaped characters show as a
    replacement mark instead of losing the entry.
  - Fonts: a damaged font pack or font leaves the character it cannot
    draw blank, and characters far larger than the text size are left
    blank; a negative character width no longer crashes hyphenated
    lines.
  - Books: a part of a chapter that never ends is reported as damaged,
    and a picture without a size is left out. Files inside an EPUB or
    CBZ are checked against their checksum and size instead of showing
    as garbled text or pictures. Text after a very long comment or tag
    no longer shows stray markup, a lone `<` (as in "a < b") no longer
    swallows the words after it, and text full of `&` no longer slows
    reading down. A hyphenation exception starting or ending with a
    hyphen no longer crashes the reader.
- Books: chapters and pictures whose links differ from the file names
  only in upper and lower case are found again, and archives with
  names that are not UTF-8 or with a comment at their end open.
- Progress sync: books are identified as KOReader does also on storage
  that reads less than asked at a time, so the same book is found.
- Settings and the files kept per book are no longer broken by a key
  or section name with characters the format does not allow. SDK:
  `config::set_all` sets many keys in one pass.
- Drawing: lines, rounded boxes, rings and clock hands with very large
  or far away coordinates no longer crash, hang or land in the wrong
  place.
- Fuzzing covers every parser: SQLite, StarDict, JSON, INI, fonts, ZIP,
  translation catalogs, HTTP request heads, FB2, HTML, Markdown, CBZ
  and XTC besides EPUB, TXT, XML, pictures and hyphenation; the picture
  target decodes in colour and at sizes the input chooses.
- Emulator profiles match the devices: the X4 Pro draws in black and
  white at 217 dpi with the 8 MB of PSRAM it has (a 10 MiB heap budget);
  a new `xteink-x4-classic` profile; the Xteink profiles show the busy
  mark, keep 12 KiB for glyphs and keep Wi-Fi on while charging, as the
  firmware does; the Boox Go 7 draws in 256 grays, has Android's back
  button and shows the text and paper colours of the settings, as the
  Android app does.
- Emulator: closing the window lets plugins save and send what is
  pending, as the device does before it sleeps. An emulated device
  asleep no longer keeps a processor core busy. A restart forgets the
  old kernel's requests, servers, speech and Wi-Fi, whose answers
  reached the new kernel's requests before; the pretend radio reports
  in the order of time (joining no longer waits for a scan).
- `paperpaw-sign key` makes the key file readable by its owner alone
  (mode 0600 on Linux and macOS) and checks that none is there in the
  same step as making it.
- The sample plugin (`samples/hello-plugin`) no longer underflows below
  a header taller than the screen.
- Android: a PDF page that fails no longer makes every later page of the
  book fail. In the background or with the screen off, PaperPaw neither
  runs timers (unless it reads aloud) nor draws. A restart while reading
  aloud no longer keeps the screen on. Only the rows that changed are
  copied to the screen, not all 2.1 million pixels of a Boox Go 7. The
  sample books are copied once, into a library folder that is not there
  yet: deleted, they stay deleted.
- The Android app installs on devices with 16 KB memory pages: its
  library is linked and aligned in the APK for them (`zipalign -P 16`,
  build tools 35 or later). `build-apk.sh` also builds in folders whose
  path has spaces.
- Android: holding a page key that is a volume key no longer brings up
  the system's volume panel. A crash writes its message, place and stack
  to the log (`adb logcat -s PaperPaw`). Reading aloud goes on after the
  activity is made anew in the same process, and changes such as a new
  system language no longer make it anew.
- Android 8 to 10: PaperPaw asks for the permission to write the shared
  storage besides the one to read it, so downloads, uploads through
  Transfer, renaming and deleting books work there; who allowed reading
  only is offered access again on the home screen. Passwords and keys
  (`.paperpaw/secrets.txt`) are kept in the app's private folder, which
  no other app reads (up to Android 9 apps with the storage permission
  read the app's shared folder); a file kept there before moves at the
  first start. The app is left out of Android's backups.
- Browser: an address typed with a port (`example.org:8080`,
  `localhost:8080`) is loaded over `https://`; it was taken for an
  address of another kind and failed. In the tools it now comes before
  Calibre instead of sharing its place.
- Security: Calibre is connected to only on a computer the reader chose.
  One found in the network is shown with its name and address and
  trusted once the reader connects to it (kept in
  `.paperpaw/plugins/calibre/host.txt`; the address set by hand is
  trusted as it is), so another device in the network can no longer
  answer first and pose as Calibre. Calibre writes only books of formats
  PaperPaw reads, replaces and deletes only books it sent itself, and
  leaves the reader's own files as they are (a book whose name is taken
  is not received, with a notice).
- Calibre: the books on the device are listed to Calibre a part at a
  time and without their cover thumbnails, and on the X4 a message from
  Calibre may be 64 KB at most (a megabyte elsewhere), so a large
  library or a hostile peer no longer exhausts its memory.
- Wi-Fi transfer: PaperPaw's settings file, `paperpaw.ini`, can be
  fetched but no longer deleted or replaced from the browser: it names
  the servers that get passwords and keys.
- Wi-Fi transfer: a folder is listed to the browser in parts of about
  8 KB, which the page puts together and sorts, so a folder of thousands
  of books no longer has to fit in the X4's memory at once.
- Wi-Fi transfer: where there is too little room for the QR code
  (large fonts, a short screen turned), the address is shown without it;
  the code's place was computed below zero. Typing on a phone: a text
  too long for the device is refused with a message on the phone, which
  keeps it; it arrived as an empty text, which cleared the field.
- Wi-Fi: the radio goes off again once what waited for the network has
  failed (no saved network in reach, or none joined in the time a
  request waits). It went on looking for networks every ten seconds,
  with a notice each time, until the device slept, as the kernel kept
  saying that work waits.
- Wi-Fi: only secured networks are joined by themselves, with their
  saved password. Anyone can set up an open network with any name, so
  open networks are joined only when chosen on the Wi-Fi page and are no
  longer saved, and an open network named as a saved secured one is not
  joined either.
- Diagnostics: the page shown stays within the test pages when they
  are made fewer (it showed "Page 11 of 3" and drew past its bar), a
  held finger starts the screen over as a held *Select* does, its page
  is told to screen readers.
- Reading profiles: a name in use (whatever its case) is refused when a
  profile is saved or renamed, where two profiles could share one and
  *Next profile* then went back and forth between them; renaming the
  profile applied last keeps it the last, so *Next profile* goes on from
  it. Where the profiles cannot be read, the list says so and saves
  nothing over them, and a profile not saved says so too.
- Battery use: its page turns pages (page buttons, up and down, the
  arrows in the header) where the recent times awake do not fit, where
  those below the bottom could not be seen. The log goes to the card
  every ten minutes and at sleep instead of every minute, sparing the
  card. A full log that cannot become the old one starts anew instead
  of growing without end. "1 refresh" in the singular.
- Setup: *Back* on the first page asks whether to skip the setup, going
  on first, where a stray press ended it for good; the *Skip setup*
  button still skips at once. Without the settings plugin a choice of
  many (the time zone) steps to the next option instead of doing
  nothing.
- Keyboard: it takes up to 2,000 characters, where long web addresses
  and keys were cut at 200 without a word; text past the limit is cut
  and the keyboard says so. The list of layouts finds the layouts the
  fonts draw once, not at every frame, and says *On* and *Off* with
  words of its own, also without the settings plugin.
- Menu editor: a menu is no longer offered for one of its own submenus
  (the main menu inside the tools), a circle that hid both menus from
  the editor; menus caught in one all the same are listed, and *Reset
  all* resets every changed menu, listed or not.
- Rules: those that repeat every so many minutes run without a clock
  too (they count from the start); a time of day is waited for again
  when the clock becomes known, is set or moves to another time zone,
  where before it waited for the next wake. Rules that undo each other
  through the settings they change stop after four in a row, where they
  ran on until the kernel cut them off, writing the settings each time.
  The rules are read once and kept, not on every event; where the file
  cannot be read, none apply and the list says so instead of saving
  over it, and a rule not saved (card full) says so too.
- Buttons and gestures: where there are buttons only (the X4), a change
  that leaves no button to select, to go back or to move on or back
  with, everywhere or in one place (also the lock screen), is refused
  with a word why; before, binding *Select* to *Nothing* left the device
  unusable for good. *Reset buttons and gestures*, the last row, takes
  every change back (*Reset all settings* leaves key bindings as they
  are). Screens of the same name (a tool's list and its editor) are each
  offered as a place, told apart by their id; before, only one was.
- Screen lock: a locked device goes to sleep again, by the power button
  and by itself; refreshing the screen and night mode pass the lock
  too, nothing else does. Unlocking after the device woke with the sleep
  screen on no longer leaves the sleep screen above everything, taking
  every button (only a restart helped): it closes once the lock does,
  and asleep it lets *Sleep* and *Home* through. While a password is
  typed on the keyboard, a swipe in from an edge or a button opens
  nothing over it, so the quick panel no longer gets past a password
  lock on touch screens. The wait after five wrong tries holds when the
  screen is turned and when PaperPaw starts anew (the count is kept in
  `.paperpaw/plugins/lock/tries.txt`). The kind of lock changed
  elsewhere than on its own page (a plain list of choices, *Reset all
  settings*) goes back to the kind whose code is kept; with its code
  removed while locked, the lock lets the device be used instead of
  asking for a code it cannot check. *Back* shows no "Delete" over a
  password, and the help says that on the X4 on battery every wake is a
  start.
- Rules: a rule can apply when the battery reaches a level (80 %, 90 %,
  full or another), once each time it gets there and not again while
  the level wavers around it; with *Only if … While charging* it says
  when to unplug, for devices that cannot stop charging by themselves.
  On the X4 the level comes from the battery's voltage,
  which reads high while charging, and the choice of level says so; the
  X4 Pro's fuel gauge reports it directly. In the rules file the
  trigger is `battery-at 80`.
- Updates on the X4 Pro and the X4 Classic: they check update files for
  their own chip, the ESP32-S3, where they checked for the X4's: an X4
  Pro refused its own updates and would have taken the X4's. Each also
  names its own firmware in its image (`paperpaw-xteink-pro`,
  `paperpaw-xteink-classic`; the X4's stays `paperpaw-xteink`), as the
  two share the chip but not the board. SDK: `firmware::CHIP_ESP32_S3`.
- Releases carry the firmware of the X4 Pro and the X4 Classic as they
  do the X4's: `paperpaw-x4-pro-<version>.bin` for the whole flash with
  its signature file (`.bin.sig`), the signed update
  (`-update.bin`), the plugins built in (`-plugins.txt`) and checksums,
  and the same for `paperpaw-x4-classic`; a release fails if an update
  is not for its device's chip. CI lints and builds all three. The
  firmware's toolchains are pinned, as native code plugins need the
  same compiler (ADR 0024): Rust 1.99.0 for the X4
  (`shells/xteink/rust-toolchain.toml`), espup's `esp` toolchain
  1.97.0.0 with the Xtensa GCC 14.2.0 for the ESP32-S3 devices
  (`shells/xteink/versions.env`). The firmware is a workspace of its
  own, so it also builds in a checkout inside another one.
- Releases name, for each device, the fingerprint a native code plugin
  must have to run on its firmware (`paperpaw-<device>-<version>-abi.txt`,
  16 hexadecimal digits), for the plugin library and authors. The
  firmware's ELF file names it, the one its loader compares, and
  `paperpaw-sign fingerprint` reads it; a release fails if the sample
  code plugin, built and packed as an author does, does not fit its
  firmware. CI checks the same for the X4, X4 Pro and X4 Classic, loads
  a plugin built in another workspace into a kernel, keeps the SDK free
  of mutable statics, and checks that the sample and its CI template
  name the firmware's compilers.
- Security: PaperPaw's own data (`.paperpaw`: passwords, trusted
  firmware keys, settings) can no longer be reached by another spelling
  of its name. On the X4's card a name stands for an entry by its long
  name alone, by its short name only where it has no long one, so
  `PAPERP~1` reaches nothing; the storage of the emulator and Android
  takes no short name the system keeps for another (NTFS and FAT keep
  them) and, on Windows, no name with a dot or space at the end.
- Security: the Wi-Fi transfer finds every path a browser sends part by
  part in the folders' listings: only entries listed by exactly that
  name, nothing hidden, by any spelling (case, short names, dots or
  spaces at the end). Its key, and the remote keyboard's, is drawn from
  the platform's random source (65 bits, 13 letters and digits) once the
  server runs; after five wrong keys no request is answered for a
  minute. Tests send every hostile path we know of to every request, on
  a card, in memory and on a storage as lax as Windows.
- Security: Calibre's paths to books and Wallabag's ids of articles are
  taken only as plain names: a peer posing as Calibre cannot write into
  `.paperpaw` (e.g. trust a firmware key of its own), a hostile Wallabag
  server cannot have a file deleted.
- SDK: `Env::random` gives random bytes from the platform's source
  (`random::Random`: `fill`, `token`), filled by each shell with
  `KernelBuilder::random` (the system's on Android and desktops, the
  chip's on the X4); `files::is_plain_name` and `files::plain_path`
  check names and paths that come from outside.
- Joplin: a book's highlights and notes, once
  exported, become a note in Joplin, one per book, in a notebook named
  in the settings. Exporting again updates the note, also where it was
  moved to in Joplin; a note whose text was edited in Joplin stays, and
  a new one is made beside it. The notes go where Joplin synchronises
  to (Joplin Cloud, Joplin Server, or a WebDAV folder such as
  Nextcloud), so they reach Joplin on every device with no computer
  running. *Joplin* in the tools sets it up, each row with what it is
  for; the password is kept with the passwords and keys. What cannot be
  sent waits and goes with the next export, by the action *Send to
  Joplin* (also for rules), or when Wi-Fi comes back; no connection, a
  wrong password or address, Joplin's end-to-end encryption and a sync
  target in another version of Joplin's format than PaperPaw writes (no
  notes are sent then) are said plainly. While an older Joplin holds an
  exclusive lock on the target to upgrade it, the notes wait; PaperPaw
  takes no lock of its own, as Joplin 3.7 takes none. Notes are read
  and written in pieces and sent from a file, Joplin's answers saved to
  files of which only the end is read: the X4's heap peaks no higher
  for it. Shipped switched off.
- SDK: `files::for_each_line` reads a text file line by line as it
  streams from the storage, so lists of any length take little memory;
  `LocalTime::to_seconds` is the way back from
  `LocalTime::from_seconds`; `web::http_date` reads the dates servers
  send (`Last-Modified`, WebDAV's `getlastmodified`).
- Settings: a live preview above the reading settings:
  two short paragraphs set exactly as book pages are (the same layout
  engine, type, margins at the sides, justification, indents and
  hyphenation, in the interface's language), following each value at
  once while a slider moves or the buttons change it, before it is
  kept. In the emulator's X4 profile an update takes a third to a half
  of the time one book page takes to lay out, and about 3 KB of the
  heap; only the preview and the row changed are drawn again, with a
  quick refresh, and the full picture with its grays comes once the
  value is chosen.
  SDK: a widget can preview a group of settings instead of being one
  for screens (`WidgetSpec::previews`, `Widgets::preview`); settings
  pages show it above the group, and `Widgets::iter` leaves such
  widgets out.
- Settings: on touch screens, numbers better set by eye are sliders on
  the settings pages: the text size, word and line
  spacing, the margins, the speed of reading aloud, the interface's
  size, row height and margins, the frontlight; with buttons alone they
  keep their compact rows with `−` and `+`. A number changed with the buttons now
  shows each value at once without keeping it; *Select* keeps it,
  *Back* goes back to the value before, and the row of hints says so
  (`−`, `+`, *Keep*, *Cancel*). By touch, a slider's knob follows the
  finger and the value is kept where it is lifted; `−` and `+` change it
  a step at once, as before.
- SDK: sliders as a building block for any plugin (`ui::slider`:
  `Slider`, `SliderDrag`, `SliderFocus`, `SliderParts`, `SliderStep`,
  `SliderView`, `band`, `band_height`, `key_step`, `marked`, `parts`,
  `row_height`, `shows`; what a slider draws goes in a `SliderView`, so
  it can grow without breaking plugins): a track filled up to the value with a knob
  there, between a `−` and a `+` flush with the margins, the title and
  the value with its unit above it as in other rows. By touch a finger
  takes the knob along in the slider's steps and lifting it chooses the
  value, with the buttons left and right move it a step; `shows` says
  whether a setting gets one here (a touch screen). Screen readers
  hear it as `semantics::Role::Slider`. The help texts
  of the margins and the word spacing say more plainly what they do, and
  a test checks that every slider shows its unit.
- Screens draw without reading the card. The X4
  draws a frame in five bands, and what a screen read or worked out as it
  drew, it did five times over, and again for every frame. Covers on the
  home screen, in the book widgets, on the book information and in OPDS
  catalogs are made small once and drawn from memory (the home screen
  read its cover's 65 KB from the card in every frame); the highlight,
  note, quote and word of the day widgets lay their text out where they
  are placed; the menu editor's lists are made when the menus change (a
  frame 30 times faster), and so are which profiles are in use; weather
  now lays itself out where placed, the keyboard's field and the
  translation's heading are measured once; the logo widget and the
  words marked in books read nothing while drawing. Screens below a full screen let go of what
  they keep to draw, so a book opened from the home screen has more
  memory, and a screen on top only for a moment (the home screen below a
  menu that opens another screen) reads nothing. A test opens every
  screen of the standard app as on an X4 and fails where drawing reads
  the card; the guide says the rule.
  SDK: `Widget::place` (where a widget is drawn, before it is),
  `widgets::Wrapped` (a text broken into lines once, drawn as often as
  needed), `Screen::on_hide`, `GridView::release`; `Screen::on_show`
  comes once the commands of a call are done. A decorator's signature is
  asked as pages are drawn: it says it from memory. Kernel:
  `Kernel::open_screen`, `Kernel::known_screens`.
- Emulator: `--trace` prints what drawing each frame asks of the
  storage, band by band, how long it took and what was asked before it;
  scripts open any screen with its parameters (`open:`) and draw the
  screen again (`redraw`).
- The Xteink X4 Classic ("X4 v2", an ESP32-S3 board with other pins than
  the X4's): the firmware builds for it with the feature `x4-classic`
  (`cargo +esp x4classic`; ADR 0013). Its panel's controller (SSD1677,
  UC8179 or UC8279) is found at start and keeps a frame in its 8 MB of
  PSRAM, as on the X4 Pro; its seven buttons are the X4's (with the row
  of hints below the screen), each on a pin of its own; SD card, fuel
  gauge and clock chip as on the X4 Pro. Its pins come from the FreeInk
  SDK's and Papyrix's notes; not yet tried on one. Device id
  `xteink-x4-classic`, `x4-classic` for plugin libraries.
  `paperpaw-x4`: `buttons::State::keys`, the buttons from a pin each.
- X4 and X4 Pro: the display controller is found at start, so devices
  from other batches show something: the first X4 Pros have an SSD1677,
  later ones a UC8179 or a UC8279 (ADR 0013). The firmware asks the panel
  on its lines before the SPI bus takes them, logs what it answered
  (`display: UC8179 (FLG 13, REV 00 00 01 FF FF, BUSY high at rest),
  driven as UC8179`) and drives it with its controller's driver; without
  a sure answer, with the one the device always had. Not yet tried on a
  device with another controller; on a UC8279, and on an X4 with an
  UltraChip, grays are shown in black and white for now.
  `paperpaw-x4`: `probe` (the controller asked), `panel` (`Panel`, the
  driver for the controller found, `Device`), `uc8179::Display` for the
  UC8279 too (`Chip`) and without a frame of its own (`Frame`, the bands
  written through windows of the controller's memory); its `new` takes
  the chip and where the frame is kept.
- X4: downloads into a file (updates, books, fonts) no longer run out of
  memory half way: pieces of an answer gathered before writing them to
  the card grew without bound when the connection handed over a lot at
  once, and the X4 started again (found with the crash file's calls;
  the out-of-memory crashes seen in testing were this). Downloads of
  10 MB one after another now go through, at the peak 222 KB of the heap's 262 KB.
- Code plugins in the store (ADR 0023, ADR 0024): listed where a build
  fits the device and its firmware, installed with their code checked
  whole and signed by their author. The store says once, before the
  first, what code plugins are (full access to the device, safe mode if
  one goes wrong); a plugin's page says it is one and which PaperPaw it
  is made for, and offers *Restart now* once it is installed. The
  author's key is remembered at the first install, and an update signed
  by someone else is installed only when the reader says so after a
  clear warning; code that is damaged, unsigned, built for another
  firmware or not by the author the library names is not installed.
  SDK: `install::Digest` (SHA-1 or SHA-256 for a package's files,
  `PackageFile::digest` in place of `sha1`), `Received::Complete` and
  `Installer::install` (a job complete waits until its owner installs
  it, in place of `Received::Installed`), `Installer::folder`,
  `native::author` (whether a signature's key is the plugin's author
  known on the device, the first, or another), `native::known_author`
  and `remember_author` (one file for every installer),
  `PluginInfo::update`
  (a code plugin updated while it runs: the new version at the next
  start). Kernel: `Kernel::describe` also tells the notice shown over
  the screens.
- A plugin store on the device (*More plugins*, in the tools and in the
  plugin manager, ADR 0023): the data plugins of
  paperpaw.org's plugin library made for this device, PaperPaw version
  and data format, by group as on the website, named and described in
  the language shown, with *Search* (by name and what a plugin does)
  and *Fonts*, which opens the fonts browser. A plugin's page tells what
  it does, who made it, its licence, version and size, and installs,
  updates or removes it; downloads go on when the page is closed. The
  library sends a list made for the device
  (`plugins.json?device=…&lang=…`, format 2), which the store reads as it
  streams into an index on the card: however many plugins it lists,
  only a page of them is in memory (a list of 1001 plugins takes about
  3 KB on the X4). A plugin's files come with its own page
  (`plugins/<id>.json`). The setting *Plugin library* (developers) names
  another library.
  SDK: `install` (`Installer`, `Package`, `PackageFile`, `Progress`,
  `Received`, `fits`, `is_plugin_name`, `is_safe_path`, `MAX_FILES`):
  installing data plugins from the internet, file by file, each
  checked, the job kept on the storage, answers to jobs stopped passed
  over; the fonts browser uses it too. `Command::InstallPlugin`: a data
  plugin downloaded takes the place of every version of it on the
  storage (loaded, switched off or refused at start), stays off where
  it was switched off, and keeps what was chosen from it (updating the
  font one reads with keeps it). `KernelBuilder::catalog_device` and
  `Env::catalog_device`: what plugin libraries call the device (`x4`,
  `x4-pro`, `android`; the shells say it). `ui::Sections`: entries under
  the headings of their sections on pages, in memory that does not grow
  with the entries. `Plugins::owner`: the plugin that added an action,
  a screen or a setting. `DATA_FORMATS`: the data plugins' formats this
  PaperPaw reads. The plugin manager lists the actions of the menu
  `plugin-manager.more` above the plugins.
- X4: removing or copying a large folder (e.g. one with many books, or
  their caches) no longer starts the device again half way: work on the
  card keeps the watchdogs from taking it for hung (it took 20 s before
  they did, removing the books' caches over a minute). It does so for at
  most 15 minutes in one step of the firmware's loop: work on the card
  that never ends (e.g. on a damaged FAT) still starts the X4 again
  rather than keep it on until its battery is empty.
- X4 Pro and X4 Classic: the fuel gauge is read and the clock chip set
  by the firmware's loop, no longer in the interrupt that reads the
  buttons, where each I²C reading held off every other interrupt (the
  radio's too) for a millisecond or two. On every X4 the battery is read
  when its status is due (once a minute), no longer every second.
- X4: more memory left while the radio is on (on USB power it stays on
  by default, and a book opening then came close to the heap's end): the
  radio keeps 5 receive buffers instead of 10 and takes at most 16 more
  each way instead of 32, about 6 KB less all the time and up to 11 KB
  less at the peaks, downloads as fast as before (6 MB in about 33 s).
  What each plugin took is kept once instead of twice.
  SDK: `Env::plugin_usage()` gives it, `Usage::plugins` is gone (it was a
  copy made on every call).
- X4 test builds, for finding what takes the memory: a panic keeps the
  calls that led to it (also a failed allocation), and
  `tools/x4-remote.py --crashes` names them from the firmware's symbols;
  `heap` says the most the heap held since start, the log says the heap
  after every frame, and failed requests are logged with their host
  (`wifi:status`, `wifi:scan`, `http:get:` as script steps).
- X4: losing the network's address (as on every joining again) lets go
  of what was open on it: plugins' servers no longer keep listening and
  holding their memory (some 12 KB a connection), nor requests theirs
  (some 17 KB), and name lookups no longer pile up (after four, every
  lookup said "no such host" until the radio went off). Requests made
  while a network is being joined wait for its address, as they were
  meant to, instead of failing at once.
- X4: the clock takes the time only from the server asked and only its
  answer to that request (the random number sent with it given back), no
  longer any answer that comes. `paperpaw-x4`: `sntp::Asked` (its
  `request` and `answer`) in place of `sntp::request` and `sntp::answer`.
- X4: two clients sending or reading a byte now and then no longer hold
  a plugin's server for good: a request's body and its answer must move
  16 KB every 30 s (as a request's head must come whole in 30 s), while
  a large upload or download that moves takes as long as it needs.
- X4 updates: an update whose signature does not hold can never start.
  A second install before restarting wrote into the partition the first
  update had chosen, so a file under PaperPaw's key with a wrong
  signature started at the next restart. An update installed and not
  started yet is now given up before another is written, and an image's
  first word, without which the bootloader starts nothing, is written
  only once its signature held.
- X4 updates: firmware signed with another key than PaperPaw's (one's
  own builds) installs only when the reader accepts that key, shown by
  its fingerprint, at each install; nothing on the card trusts a key any
  more, so `.paperpaw/firmware-keys.txt` is no longer read and can go.
  The shell installs an older firmware, or the same again, only when
  the reader chose it on the update page. SDK:
  `FirmwareCommand::Install` has `key` and `older`,
  `Update::may_install` is the shell's check,
  `Update::is_newer`, `InstallFailure::NotNewer`; `Update::signer`
  takes no list of keys; `TRUSTED_KEYS`, `trust`, `trusted_keys` and
  `Signer::Trusted` are gone (none was released).
- X4 updates: an update that fails early in its start is undone too. It
  goes on trial before the board is set up, and is taken as good after
  half a minute of running in all (it was at its first frame, so one
  that crashed opening the last book stayed); going to sleep or
  restarting before keeps it on trial at the next start, so one that
  crashes after a wake is undone as well. The start is watched from its
  first moment; a power cut while an update is chosen no longer starts
  it without a trial, and the image running is marked as good also when
  an update was chosen meanwhile (that one was marked instead).
- X4: what was written is on the card before every restart, also one a
  plugin asks for (a plugin switched just before was lost), and at most
  2 s after work, however often work comes.
- X4: put to sleep on USB power and then unplugged, it sleeps as on
  battery, where it stayed awake and drained the battery.
- X4: a display whose busy line sticks ends in an error after 10 s,
  where the watchdog started the X4 again and again; a long panic
  message is kept for `crashes.txt`, no longer lost when a character was
  cut in two; the remote control over USB (test builds) refuses lines
  too long rather than carry them out cut.
- `paperpaw-update`: `Plugins::record` gives `None` for a list longer
  than readers take, and the firmware's build stops there.
- The X4's remote control over USB (test builds) does much more, for
  developing on the device: the card (`ls`, `stat`, `md5`, `rm` also of
  folders, `mkdir`, `mv`, `sync`; `tools/x4-remote.py` pushes and pulls
  whole folders and checks every file it puts by its MD5), what runs
  (`version` with the plugin crates built in, `plugins`, `actions`,
  `settings`, `screens`, `usage`: what each plugin took), the clock
  (`time`, `time set`), the log's level (`log`), events (`emit`), touch
  (`touch`, and the script's `tap`, `swipe`, `tap-label`), `restart` and
  `panic` (the script waits until the X4 is back and says why it
  started, with the panic). Scripts wait for and check what the screen
  shows (`until:`, `expect:`, `expect-not:`) and end with exit code 1 when
  a check fails. Kernel: `Kernel::screens`, `actions`, `usage`,
  `send_event`.
- Updates from the internet (X4): *Search online* on the update page
  (Settings, *Update*) asks paperpaw.org for a newer PaperPaw, and
  *Download* fetches it into `updates` on the card, to install as one
  copied there; the action `update.check` searches in the background
  (e.g. from a rule) and offers what it finds once. Nothing is sent
  unless asked; the setting *Update server* (developers) names another
  server (ADR 0022).
- The firmware knows its plugins: every image lists the plugin crates
  built in, right after its application descriptor, so that updates and
  the web flasher can bring the same ones; *About PaperPaw* shows them.
  An update that would leave plugins out says which, and the reader
  chooses *Update without them* or *Not now*.
  SDK: `Firmware::plugins`, `Update::plugins`, `Update::leaves_out`,
  `firmware::Plugins` and `PluginCrate` (from `paperpaw-update`, which
  also reads and writes the list). `paperpaw-sign plugins` prints an
  image's list; releases carry it as `paperpaw-x4-<version>-plugins.txt`.
- PaperPaw's logo, the paw on a sheet with a dog-ear: above "Sleeping" on
  the sleep screen (also what the X4 shows as it starts), at the top of
  *About PaperPaw*, and as a widget to add to the home screen (*PaperPaw
  logo*, alone, its name beside it if wanted). The Android app's icon is
  the logo too.
  `paperpaw_sdk::logo` draws it in any size, as shapes, nothing kept in
  memory; the setting `core.logo` names a picture to show in its place
  (a data plugin sets it in its `settings.ini`, see docs/plugins.md).
- Markdown files show the pictures beside them (`![alt](picture.png)`,
  or from the card's root with `/`); before, their alternative text.

- QuireInk is now called PaperPaw, everywhere. Crates `paperpaw-*` (for
  `quireink-*`), so a plugin uses `paperpaw_sdk` (for `quireink_sdk`).
  On the card `paperpaw.ini` and the folder `.paperpaw` (for QuireInk's
  own, which PaperPaw takes over once as it starts:
  `data::take_over_old_name`, so settings, statistics and passwords stay);
  the Android app is `io.github.hthoene.paperpaw`, a new app beside
  the old one. Formats with new magics: font packs `.ppfp` (were `.qifp`),
  hyphenation `.pphp` (packs and patterns made before, `QIFP` and `QIHP`
  inside, are still read: PaperPaw's own format), cached pictures `PPGI`
  and `PPCI`, made again where they are kept. The USB remote answers
  `PP:` lines. The website is
  paperpaw.org, the repository github.com/hthoene/paperpaw.
- More fonts, from the internet: *More fonts* below the fonts to choose
  from (Settings, Reading, Font, and the status lines' font) lists the
  fonts of paperpaw.org's font library, each name set in its own font;
  one shows larger with a sentence in it, its licence and size, and is
  downloaded, read with at once (no restart), set for the status lines,
  updated or removed. Nothing of a font is built in: devices with
  scalable fonts and the memory for them get its font files, the others
  (the X4) font packs made for them, read a glyph at a time from the
  card. Each file is checked by its size and SHA-1; a download goes on
  when the screen is left. First fonts: Atkinson Hyperlegible Next,
  Crimson Pro, EB Garamond, Figtree, Fredoka and Nunito (all OFL, none
  with a reserved font name). The library's address is a developer
  setting (`fonts-browser.catalog`). SDK: a data plugin names a family
  of its own in `[family]` (`name`, `regular`, `bold`, `italic`,
  `bold-italic`), its faces packs or font files (`FontFace`,
  `Registrar::add_font_face`, `Fonts::add_face`; `Fonts::remove_plugin`,
  `Presets::remove_plugin`, `Plugins::remove`); data plugins loaded and
  removed while running (`Command::LoadPlugin`, `Command::RemovePlugin`,
  the event `events::PLUGINS_CHANGED`); a menu named like a setting with
  choices holds more ways to choose, listed after its options (the
  settings' chooser, `ui::two_options`).

- X4 Pro: pictures in four grays, as on the X4. `uc8179::Display` shows
  the frame in black and white first, then lightens the grays with the
  FreeInk SDK's gray waveforms; the next refresh drives every pixel
  back to black or white.
- PaperPaw runs on the Xteink X4 Pro (ESP32-S3): the X4's firmware built
  with the feature `x4-pro` (`cargo +esp x4pro`). Its UC8179 display,
  touch screen and home key, page and power buttons, SD card (SDMMC),
  fuel gauge, clock chip (the time is kept while it is off), front light
  with brightness and warmth (in the quick panel and the reader's
  panel), sleep and Wi-Fi. Its 8 MB of PSRAM is the heap. It stays on
  while flashed (its power held through resets). `paperpaw-x4` has the
  drivers: `uc8179`, `gt911`, `cw2017`, `bm8563`. Cards whose partition
  table says more than the card holds (as an X4 Pro's card as sold) are
  read up to the card's end.

- Ready-made home screen layouts to start from: *Calm* (as it comes),
  *Covers* (the books first), *At a glance* (daily goal, today
  and the streak), *Clock* (a large clock face) and *Minimal* (the book
  alone). Each is shown on the home screen itself before it is kept,
  with a sheet below to go from one to the next and *Use this layout*;
  back leaves everything as it was. They are offered in the first-run
  setup and under *Add* while editing the home screen (action
  `home.choose-layout`), and everything on them can be changed after.
  Anyone can share one: a data plugin with a `presets.ini` (sample
  `samples/plugins/reading-nook`). The home screen lays itself out once
  for many changes at a time. SDK: presets, ready-made sets of setting
  values of a kind (`presets`: `Preset`, `Presets`, `Preset::changes`,
  `presets::commands`, `presets::applied`; `Registrar::add_presets`
  and `add_preset`, `Settings::presets`); values shown without being
  kept (`Command::PreviewSettings`, `Settings::set_preview`,
  `Settings::previewing`, the event `events::PREVIEW_CHANGED`), ended
  when the screen that asked closes; `Settings::is_set`.

- A new look, calmer and more modern, on every screen: rounded corners
  by default, the focus a rounded bar within the margins, thinner lines,
  dark rounded notices, and Source Sans 3 as the menus' font (warmer than
  Atkinson Hyperlegible Next, which it replaces). Empty screens say what
  belongs there and how to fill it, with an icon: the home screen before
  the first book (*Start reading*), the library, vocabulary, notes,
  rules, profiles, news, an empty folder, and Wi-Fi transfer without
  Wi-Fi. Every setting has a help text; notes speak of buttons on devices
  without touch; errors say what to do. Icons were chosen again, one
  idea one icon. Books without a title show their file's name readably
  ("Alice In Wonderland" for `alice-in-wonderland.txt`). The quick panel
  shows a short date. SDK: `ui::draw_empty`, `ui::draw_focus`,
  `ui::focus_rect`, `ui::tr_input` (a `<key>.buttons` text on devices
  without touch), `Command::NoNetwork` (work waiting for a network fails
  at once), `status_items::DATE_SHORT` with `time::short_date_text`,
  `library::readable_name` and `Book::shown_title`.

- Settings take much less memory: the kernel's and every first-party
  plugin's are kept in the program, also those with a unit, a section or
  an editor of their own. In the emulator's X4 profile the home screen
  needs 109 KiB instead of 152 and starting peaks at 140 KiB instead of
  207. SDK: `SettingExtra` is made in the program (`SettingExtra::new`
  and its `const` builders `unit`, `section`, `editor`, `step`,
  `slider`, `secret`) and given to fixed settings with
  `SettingSpec::with_extra`; `SettingSpec::scoped` and `advanced` are
  `const`; `Setting::static_str` and `Id::static_str` name a setting in
  `static` lists; `role_settings!` makes a decoration role's settings in
  the program (`decorations::add_role` stays for roles known only at
  run time); `Tone::id` and `LineKind::id` are `const`.

- *Now reading* shows the chapter and what is left of it ("22 pages
  left · about 11 min"), each a part to show or not. Plugins know the
  chapter from the reading events: `events::Reading` has `chapter` (its
  title) and `chapter_left` (pages after this one in it, once the pages
  are counted); the reader fills them.
- The status lines while reading have a font of their own (*Status
  lines → Font*): as the menus (as before), as the book, or any font
  installed. The bar the reader's menu opens at the top (the book,
  the chapter, the entries) is in that font too. SDK: `fonts::STATUS` (readers draw their status lines in
  it), `settings::STATUS_FONT`, `Fonts::set_status`.
- The same widget can be on the home screen more than once, each in a
  look of its own: every widget added has its own settings (its frame,
  size, what it shows), set from its *Settings* in the editor. Pages made
  before keep their widgets; one gets settings of its own when they are
  first opened. SDK: placements (`Id::placement`, `Id::placed`; ids such
  as `widgets.clock@2` and settings such as `widgets.clock@2.face`, saved
  in `paperpaw.ini` like others), `Settings::with_placement`,
  `Settings::spec`, `Settings::placement_values`, `widgets::base`;
  `Widgets::make` gives a placement's widget in its own values.
- *First day of the week* (System): Monday, Sunday, or as the language
  has it (Monday in English and German). The statistics' weeks and every
  calendar follow it (a calendar widget may choose its own). SDK:
  `settings::WEEK_START`, `time::WEEK_STARTS`, `time::week_start`.
- Many new widgets for the home screen (C26), each with settings of its
  own: what it shows, its style, its size and alignment, and a frame
  (none, a line, a light card, a dark card with light text) that every
  widget has. A widget pack (`widgets`): an analogue clock, a word clock
  (as a sentence or a grid of letters, English and German), a clock in
  tiles, a world clock, a calendar, the year so far, a countdown, the
  moon's phase, the day and date, a quote of the day (shipped quotes or
  one's own), a picture from a folder, a note, a to-do list with its own
  screen, a greeting, a book's progress as a ring, the book being read
  with the time left, the next book to read, a series, favourites, a
  highlight of one's own, the library in numbers and the battery. The
  reading statistics add a daily goal ring, a reading calendar, books
  this year (with a yearly goal) and a streak; vocabulary adds *Words
  saved*; a weather plugin (`weather`, Open-Meteo, nothing sent before a
  place is chosen) adds the weather now and a forecast. The widgets
  there before (clock, date, status line, continue reading, recently
  read, read today, the last days, the word of the day, news) have the
  same settings. Adding a widget shows the kinds of widgets first, then
  each widget of a kind drawn as it would show.
- SDK for widgets: a shared look (`widgets::Frame`, `Size`, `Look`,
  `draw_frame`, `frame_setting`, `size_setting`, `part_setting`,
  `shows`, `choice`, `draw_caption`, `draw_message`, `draw_wrapped`,
  `line_height`, `look::look`), kinds (`WidgetSpec::category`,
  `widgets::categories`, `Widgets::category`), `events::WIDGET_CHANGED`
  (`widgets::changed`) for a widget whose content changed. Graphics:
  `Canvas::fill_circle`, `stroke_circle`, `fill_ring` (arcs, smooth
  where the panel has grays), `draw_hand`, `fill_round_shaded`,
  `paperpaw_sdk::round` (angles as on a clock face, whole numbers).
  `Kernel::widgets`.
- SDK: settings kept in the program (`SettingSpec::fixed`,
  `fixed_switch`, `fixed_number`, `fixed_choice`, `fixed_text`,
  `described`, `for_advanced`; `Registrar::add_settings`;
  `widgets::frame_spec`, `size_spec`, `part_spec`, `align_spec`) take
  next to no memory, where one built at start takes about 200 bytes. A
  choice's values are `Choices` (`SettingKind::Choice`,
  `SettingSpec::choice`, `Settings::set_choices`): lists in the program
  cost nothing.
- X4: highlights were lost each time their book was opened: renaming a
  file that is not there removed the file at the new place (the reader
  looks for highlights kept where they were before). Such a rename now
  changes nothing, and the reader moves old highlights only where there
  are any. What is written reaches the card two seconds after the last
  work, not only before sleep, so a reset, a flash or an empty battery
  loses nothing; files sent over USB are on the card before the X4 says
  so. paperpaw-fat: `SharedDisk`, `FatStorage::with_disk`.
- Decorators get a word split over two lines (hyphenated) whole, in
  both parts, so words being learnt are marked there too. The page is
  drawn again at once when how marks look or what decorators mark
  changes (a setting of *Marks in the text*, *Mark vocabulary*), not
  only at the next page.
- Marks in the text (ADR 0021): plugins mark words on book pages through
  decorators, drawn the same by every reader. How a mark looks is
  settings, one per aspect (background, text, a line under the text that
  is solid, dotted, dashed, wavy or double, its tone and thickness, a
  frame, struck through), under *Marks in the text*; highlights, links,
  search hits and the sentence read aloud are such roles too. Highlights
  are visible on black and white panels (dithered; they were white
  before). A tap on a mark whose plugin hears taps goes to it; with the
  buttons, *Links and marks* reaches them. SDK: `decorations`
  (`Decoration`, `Tone`, `LineKind`, `add_role`, `draw_behind`,
  `draw_over`, `DecoratorSpec`, `Decorators`), `Registrar::add_decorator`,
  `Env::decorators`, `events::DECORATION_TAPPED`, `groups::MARKS`,
  `roles`, `words::WordSet` (whole words and phrases, many at once,
  without regard to case); graphics: `Canvas::fill_shaded`. The setting
  `reader.highlight-style` is gone (the highlight role's settings).
- Vocabulary: the words looked up can be marked where they occur in
  books of their language (*Mark vocabulary* in the reader's *More*,
  off by default); a tap on one, or choosing it with the buttons, looks
  it up again.
- A reader can be replaced in full: plugins reach it only through what
  the SDK defines for every reader, no longer through the shipped
  reader's ids. SDK: `events::GOTO` (`Goto`), `events::TEXT_CHOSEN`
  (`TextChosen`), `events::NOTES_EXPORTED` (`NotesExported`),
  `menus::READER`, `READER_MORE`, `READER_PANEL`, `SELECTION`,
  `settings::FONT_SIZE` (registered by the kernel), `groups::STATUS_LINES`,
  `setting_sections::ALOUD`; key bindings while reading on the slot's
  layer `[keymap.core.reader]`. Changed ids: `reader.goto` is
  `core.goto`, `reader.notes-exported` is `core.notes-exported`,
  `reader.text-chosen` is `core.text-chosen`, `reader.font-size` is
  `core.font-size` (set again once), the menus `reader.menu`,
  `reader.more`, `reader.layout` and `reader.selection` are
  `core.reader-menu`, `core.reader-more`, `core.reader-panel` and
  `core.selection-menu`, key bindings in `[keymap.reader.screen]` go to
  `[keymap.core.reader]`.
- Reader: *Smooth text* (Font, off by default, where the screen has no
  grays): book pages are shown in four grays, the letters with soft
  edges, as pages with pictures are; on the X4 a page turns in some
  1.9 s (0.7 s without).
- Text on black and white panels (the X4) is fuller: a pixel at the edge
  of a letter is drawn from three eighths covered (was half), so thin
  strokes of small text keep their weight and no longer look pale. The
  setting *Text darkness* (Display, where the screen has no grays)
  chooses light (as before), normal or full. SDK: `settings::MONO_TEXT`,
  `MONO_TEXT_LEVELS`, `Capabilities::black_and_white`,
  `MenuNeed::BlackAndWhite`; graphics: `Framebuffer::set_mono_coverage`,
  `MONO_COVERAGE`.
- Home: the screen shows without the focus until the first press, also
  on devices without touch, so it is seen as it is first; the setting
  *Show the focus at once* brings the old way back. Opening the home
  screen is faster: the recent books look up only as many as fit across
  the screen (was all that were set, six by default), and on the X4 no
  busy mark is shown after a page in grays (it took a refresh of the
  whole screen). Titles below the covers take the width of their
  column. SDK: `GridView::set_focus_shown`.
- Covers made small (lists, grids) no longer count as large pictures
  that leave ghosts: a page of the library's grid refreshes quickly on
  the X4 (about 0.5 s, was a full refresh of 1.7 s); large pictures and
  the periodic full refresh stay as they were. Graphics:
  `Canvas::small_pictures`.
- Covers are decoded as large as the screen shows them in a grid (three
  eighths of its longer side, at least 300 pixels): on large screens
  they no longer stay smaller than the places of books without a cover.
  The library reads books' details again once for it. SDK:
  `OpenOptions::cover_fit`, `cover_fit`, `COVER_FIT`.
- Library: the order, the view and the library folder have icons in the
  library's menu, as its other rows.
- Covers made small are kept as the panel shows them: in black and white,
  dithered, on panels of black and white alone (a quarter of the memory:
  a grid of covers on the X4 takes some 20 KB, was 90), in grays or in
  colour elsewhere. SDK: `ui::Thumbnail::new` and `library::cover` take
  the panel's `PixelFormat`.
- Library: the covers made small are let go while a book opened from it
  is read (on the X4 a grid of them took some 40 KB), and made again
  from their small files when it shows again.
- Night mode leaves pictures as they are (covers and photos were shown
  as negatives); "Pictures in night mode inverted" (Display) inverts
  them too. Graphics: `Framebuffer::set_pictures_kept`.
- What is kept for a book (its converted chapters, position and page
  index, notes, details) moves with it and is removed with it, and
  "Clean up storage" (Settings) removes what is left of books no longer
  there. Moving a book no longer converts it anew. SDK:
  `Registrar::add_book_data`, `library::{key, BookData, move_data,
  remove_data, clean_up, book_keys, books_in, removed}`,
  `events::BOOK_REMOVED`, `actions::CLEAN_UP`; the file manager says
  which books it deleted.
- X4: pages with pictures and the sleep screen's cover or picture show
  in four grays (the panel's gray waveform, as long as a full refresh);
  lists stay black and white. The time and the battery do not refresh a
  picture shown so; the next page does. SDK: `Screen::grays`, kernel:
  `Effects::grays`, `Kernel::render_gray`.
- Books: pictures are decoded when their page shows, not with their
  chapter (`Document::prepare_image`, `image::dimensions`): on the X4 no
  step of the background pagination of an illustrated book takes
  seconds any more.
- Home on the X4: the recently read shelf shows below the current book
  (it had no room at its full height and was left out). Widgets can do
  with less (`Widget::min_height`); the grid places them lower where
  their full height has no room; the shelf's covers then shrink.
- X4: the display driver shows four grays (`write_gray_rows`,
  `refresh_gray`, with the stock firmware's gray waveform); over USB,
  `gray` shows bars, a gradient and a cover in them to judge the panel.
  Not yet used for pictures. Kernel: `draw_again`.
- Emulator: the X4 profile has the X4's 256 KiB of heap (192 KiB and the
  64 KiB the boot loader used), not 192.
- Menus, the quick panel and questions wash out what lies below them
  (lighter gray; on the X4 every other pixel paper), so they stand out
  and the lines they cut through read as background. Overlays looked
  through (the keyboard, notices, skimming) keep it. SDK:
  `Screen::dims_below`; graphics: `Canvas::wash`.
- X4: the lines between the rows of lists, the tracks of sliders (the
  quick panel, skimming in the reader) and the statistics' lines show:
  light gray, they were drawn white on its black and white panel; there
  they are now every other pixel black. SDK/graphics:
  `Canvas::fill_tint`.
- Settings: each page ends with "Reset to defaults", the main page with
  "Reset all settings" (the language stays); both ask first. SDK:
  `Command::ResetSetting`, `Settings::reset` (the value forgotten, so
  that the default, or outside a book's own settings the global value,
  applies again).
- The row of hints over the buttons marks with a small hand the buttons
  that do something more when held (e.g. Back: home, Select: options),
  without growing. SDK: `ui::draw_button_hints` takes what holding each
  button does, `ui::HOLD_ICON`; the icon font has `hand-click`.
- Library: buttons reach the order at the top (a stop of the focus
  between the last book and the first); Confirm there opens the
  library's menu, as a tap does. SDK: `Header::note_focused`.
- Home: the book widgets are no longer read again each minute (on the
  X4 that read the recent books' details from the card, about 570 ms a
  minute, also on battery); widgets say whether they change by
  themselves (`Widget::live`, `GridView::update_live`). Only as many
  recent books as the widgets show are read, each once.
- Library: a page of books takes about 150 ms of work on the X4 instead
  of 500 to 700. Covers are kept made small (`library::cover`, from
  `ui::Thumbnail::keep`/`kept`), read as 3 KB instead of 60 each; a
  listing's entries carry their sizes (`Entry::size`,
  `library::book_sized`), so a book is not looked up again for them.
- X4: blocks of the card read one after the other are read eight at a
  time (a folder searched takes a third of the time), and a folder is
  looked up from its kept parent rather than from the root.
- X4: the screen turns ("Turn the screen": upright, on either side,
  upside down). Turned, the kernel draws bands of the same memory (fewer
  rows where the screen is wider) and frees the old frame before making
  the new one.
- Library: the covers of a page are made small once when it is shown and
  drawn from memory; a step of the focus draws only the two entries it
  changes. On the X4 a step in the library draws in about 105 ms (was
  455), a page in about 205 ms (was 650). SDK: `ui::Thumbnail` (a
  picture made small and kept, 4 bits a pixel), `ListView::request`
  (only the items a focus step changes).
- X4: the folders looked up last are kept, so a file is no longer looked
  up by walking every folder from the card's root: preparing a book the
  first time takes about 2.4 s instead of 2.9.
- Menus draw only the two rows a step of the focus changes (on the X4
  a step in the main menu draws in about 75 ms, was 155); SDK:
  `Rows::request` does so for screens where the focus changes nothing
  else.
- X4: opening a book for the first time overflowed the stack or ran out
  of memory (the X4 started again on the book before). Fixed at the
  causes: inflate states and JPEG tables are no longer built on the
  stack, pictures are written with a buffer taken once, the icon names
  stay in flash. A first opening now leaves about 14 KB of the stack.
- Plugins name any of Tabler's icons: `paperpaw_sdk::icons` knows all
  5934 names (a table in flash, about 150 KB on the X4, no memory); the
  drawings of those the icon font lacks come from icon packs.
- X4: opening a book for the first time could run out of memory (a
  crash, "memory allocation failed") where a chapter has blocks of many
  links or other short pieces: where each piece starts is now written as
  it comes. The first opening of a book with a large cover peaks some
  5 KiB lower.
- Emulator: `--trace-alloc` says where each allocation comes from.
- The X4's whole flash image in a release is signed too, in a signature
  file of its own (`paperpaw-x4-<version>.bin.sig`), which the web
  flasher checks before it writes (`paperpaw-sign sign-apart`, `check`;
  `paperpaw_update::check_apart`).
- Icon packs: `tools/icons/pack.py` builds a data plugin with the
  drawings of any Tabler icons PaperPaw's icon font lacks, read a glyph
  at a time from the card; `paperpaw-fontgen --chars` takes code points
  (`U+EA01-EA0F`). An icon without a drawing now shows nothing rather
  than a missing character.
- SDK: a widget is a `Widget` a screen keeps: it reads what it shows in
  `update` (when shown, each minute, after events) and only draws in
  `draw`, which may run several times a frame. `WidgetSpec::new` takes
  its factory, `WidgetSpec::drawn` a pair of functions for widgets with
  nothing to read (`Drawn`, which replaces `signature` with `shows`);
  `Widgets::make` makes one. The home screen, the sleep screen and the
  widgets of the statistics, news and vocabulary plugins work so: on
  the X4 a step in the main menu over the home screen drew in about
  0.15 s instead of 0.5 s.
- Reading: *Home* is in the menu's bar, last: from the first entry one
  press back reaches it (it was under *More*, at the end of a list).
- Buttons pressed quickly while the device is busy wait at most a frame:
  five presses in a row on a slow screen no longer run five times one
  after the other.
- Screens below a menu or another overlay are drawn only where it leaves
  them to be seen: moving in the main menu over the home screen no
  longer draws the home screen's widgets again.
- SDK: `widgets::grid`, the home screen's grid for any plugin: a layout
  of widgets and tools in cells as text (`parse`, `write`, `place`,
  `add`), and `GridView`, which shows it on a screen of the plugin's own
  and edits it in place (dragging, sizing, options, with buttons too).
  The home screen uses it. Also `widgets::has_settings` and
  `Menus::title`.
- Home: below the page, the pencil and the dots can go, and status items
  of any plugin can come (Settings > Look). *Edit home screen* in the main
  menu edits it, with buttons only too, as a long press does.
- Home: the status line at the top shows the status items chosen for its
  left, centre and right (the status line widget's settings), any
  plugin's, as the reader's lines do.
- PaperPaw builds with Rust 1.88 and later (it said 1.85, which was too
  old); CI checks it. A short [security model](docs/security-model.md)
  says what is protected and against whom.
- X4: a crash is written to the card at the next start
  (`.paperpaw/crashes.txt`, its version and why), for bug reports.
- Data plugins say which version of the formats they are made for,
  `formats = 1` in `plugin.ini`; a PaperPaw that reads older ones leaves
  such a plugin out and says why. Every data file PaperPaw keeps has a
  file of its version in `testdata`, read by the tests.
- SDK: a setting's unit, editor, step, slider, section and whether it is
  a secret are read with `SettingSpec::extra()` (they are kept apart, so
  that the settings take less memory: some 8 KiB less on the X4);
  `SettingSpec` is `#[non_exhaustive]`, made with its constructors and
  builder methods as before.
- HTML, FictionBook, Markdown and comic books are plugins of their own
  (`format-html`, `format-fb2`, `format-markdown`, `format-cbz`), no
  longer part of the EPUB plugin: each can be switched off or replaced
  alone.
- Drawing, text and layout take their parts together: `Canvas::fill`,
  `stroke`, `fill_round` and `stroke_round` take a `Rect` (`ui::fill` is
  deprecated), `paperpaw_text::draw_text` a `TextStyle` (with `bolder`,
  in place of `draw_text_bolder`), and `layout_page` and `line_start` a
  `LayoutContext`.
- The keyboard's shift, delete and space keys and the grip that moves a
  row say what they are (screen readers, remote control); plugins draw
  such buttons with `ui::draw_icon_button`.
- SDK: lists move by the navigation actions with `ListView::navigate`
  (or `ui::list_step` for a focus of one's own), and
  `ListChange::request` asks for the redraw and refresh a change needs,
  the same in every list.
- Kernel: shells take everything they are to do at once,
  `Kernel::take_io()`, as `Io` (requests, servers, sockets, speech, the
  clipboard, the radio, the firmware), matched in full so that a shell
  cannot forget one; the eight `take_…` methods are gone. On the X4,
  plugins reading the clipboard or speaking no longer wait for ever.
  Inside, each of these is a part of its own.
- SDK: `ActionSpec::new(id, title)` with `.handler(…)` and `.hint(…)`;
  the struct is `#[non_exhaustive]`, so actions can learn more without
  breaking plugins. How the row of hints shows an action (its title, a
  word of its own, an icon or nothing) is the action's `ActionHint`,
  no longer decided by the kernel.
- X4 firmware: released firmware leaves out remote control over USB
  (the `remote` feature, for testing), 34 KB less. Inside, the main loop
  is a `Firmware` with a method for each step and the pins are in one
  board module; the USB protocol, SNTP and the frame kept for the panel
  are in `paperpaw-x4`, tested on a computer.
- Kernel: device defaults are typed, `setting_default(&SETTING, value)`;
  `setting_default_text` takes an id and text, for settings known only
  at run time.
- SDK: its compatibility is checked. CI fails on a change that would
  break plugins unless the version says the next release breaks
  (`cargo-semver-checks`), and `crates/paperpaw-sdk/api.txt` lists its
  public API, so every change to it shows (ADR 0020). Builds between
  releases are the next version with `-dev`, and an update to the
  release counts as newer.
- SDK: typed answers. The answer to an HTTP request is an `HttpReply`
  (status, body, headers and the request's own parameters as
  `context`), what happens to a server a `ServerNotice`, to a socket a
  `SocketNotice`, what the reader typed a `Typed`, the folder chosen a
  `Chosen`, the option picked a `choice::Picked`. `ask::text` and
  `ask::folder` ask for them; `HttpRequest::new` and `choice::open`
  take the `Event` that brings the answer.
- SDK: typed events. An `Event<P>` names an event and what it carries (a
  struct made with `event_data!`); `r.on(EVENT, |env, data, out| …)`
  hears it, `EVENT.emit(data)` emits it, `EVENT.read(event, params)`
  reads it on a screen. Every built-in event is one: the reading events
  carry `Reading` (path, title, progress, position, page), a move
  `BookMoved`, a changed setting `SettingChanged`, and so on.
  `r.subscribe` takes any id, for events known only at run time.
- SDK: typed settings. A `Setting<T>` (`bool`, `i64` or `Text`) names a
  setting and the type of its value; `env.settings.read(&SIZE)` reads
  it, `Command::set(&SIZE, 12)` changes it, `SettingSpec::switch`,
  `number`, `choice`, `language`, `list` and `text` declare it with a
  default of its type. Settings by their ids stay for those only known
  at run time. The library folder is `library::FOLDER` and
  `library::folder(env)` for every plugin, no longer `"files.root"`
  spelled out in nine places. Registrar methods that name an item take
  `impl Into<Id>`.
- SDK: its helpers are crates of their own, so a change to one does not
  change the SDK: `paperpaw-digest` (MD5, SHA-1, FNV-1a), `paperpaw-json`,
  `paperpaw-qr`; plugins that read SQLite use `paperpaw-sqlite`.
- SDK: `Registrar` is a struct now that collects `Contribution`s (a
  `#[non_exhaustive]` enum, the large kinds boxed); `add_action` and the
  rest are shorthands for `add`. Plugins take `&mut Registrar` instead of
  `&mut dyn Registrar`. Anything can see what a plugin adds without
  PaperPaw running (`Registrar::new`, `contributions()`); the kernel
  applies the list and checks key maps, translations and patterns then.
- SDK: `Env` is `#[non_exhaustive]` (new fields break no plugin). What
  the device and its shell can do is `env.caps` (`Capabilities`: touch,
  network, Wi-Fi, firmware, speech, the sleep screen, rotation, colours,
  the row of buttons, a system clipboard), replacing `env.network`,
  `env.speech` and `env.sleep`; menu entries' needs are checked against
  it (`Menus::set_capabilities`).
- SDK: PaperPaw's folder is one constant, `data::DIR`, with
  `data_path!` for constants; the about page is written in its plugin's
  folder.
- Versioned data files ([data-files.md](docs/data-files.md)): every file
  PaperPaw keeps on the card is listed as data, cache or another
  program's format. Data formats have a version (`paperpaw_sdk::data`,
  `# format 2` as the first line), are upgraded as they are read, and are
  never written over by an older PaperPaw (after going back to an older
  update). Files of the current formats are kept in `testdata` folders
  and read by the tests. Profiles are kept as a proper ini file now (names
  with spaces and brackets survive); older files are read and upgraded.
- `paperpaw-config`: changing a key there more than once (by hand)
  changes the entry that is read, the last; `quote()` is public and the
  reader and statistics use it (a book path with a line break broke the
  reader's file).
- One FNV-1a (`paperpaw_digest::fnv1a64`) instead of fifteen copies.
- Firmware updates on the X4 ([ADR 0019](docs/adr/0019-firmware-updates.md)):
  two app partitions (the web flasher sets them up once), an update file
  in the folder `updates` on the card installed from Settings, *Update*
  (or offered at start) in about 45 s, then a restart. A new firmware
  that does not get to its first frame is undone at the next start.
  Update files are signed (Ed25519, `paperpaw-update`, made with
  `paperpaw-sign`): PaperPaw's key is built in, one's own builds are
  installed once their key is trusted on the device. Releases carry the
  signed update file beside the image for the web flasher.
- HTTPS on the X4 ([ADR 0017](docs/adr/0017-tls-on-the-x4.md)): mbedTLS
  (TLS 1.2 and 1.3), the certificate checked against Mozilla's roots,
  which stay in flash, and against the host; refused certificates fail
  the request with the reason. One secure connection at a time (about
  40 KB); a handshake takes 1.3 to 2.7 s. Expired certificates and ones
  not valid yet are refused too.
- The X4 knows the time once online (SNTP, again every 12 hours) and
  shows it in the status line and on the home screen; the time zone is
  chosen under Settings, System (UTC on "Auto": the X4 has none of its
  own). Gone when it is switched off, until it is online again.
- Other scripts on the X4 ([ADR 0018](docs/adr/0018-font-packs-for-other-scripts.md)):
  font packs as data plugins on the card, read a glyph at a time, add the
  letters the built-in fonts lack; `tools/script-pack.py` builds Cyrillic
  and Greek (also polytonic) from Literata. With it, Russian, Ukrainian
  and Greek books and keyboards work on the X4, menus included. Fonts ask
  each other in turn for a character they lack.
- Wi-Fi transfer: an upload that fails says so on the device (*Not
  received*), instead of the last one received staying shown.
- `Kernel::open_slot` opens a slot's screen for a shell (a file to open);
  over USB the X4 opens a book with `open <path>`.
- The keyboard offers only layouts whose letters the device's fonts can
  draw: on the X4 the Russian, Ukrainian and Greek ones are left out
  until a font pack for their scripts comes (they showed question marks).
  `FontHandle::has_char` tells plugins whether a character can be drawn.
- Notes at the bottom of a screen (what a tap or a hold does) wrap to
  two lines instead of being cut (`ui::draw_note_line`, used by catalogs,
  rules and profiles); German labels that were cut are shorter (*Fortschritt
  abgleichen*, *Einstellungen suchen*). Every screen was looked at in
  German too (`tools/screens.py --lang de`).
- Keyboard: no key shows only "…" any more. Narrow buttons keep less
  room at their sides, a letter wider than its key (an `m` on the X4's
  German layout) is set smaller, *OK* is black and plain to see, the key
  to symbols says `123`, and the key to the next layout is wide enough
  for its code. Checked with every Latin layout. Hints in German say
  *Wählen*, which fits.
- Home screen, with buttons only (X4): items are moved up and down and
  made taller or lower with the page buttons, which the X4 has instead of
  up and down (before, only their width could be changed); where another
  item is in the way, a notice says so.
- Headers are light by default (a line below the title instead of a
  black bar), calmer and as the home screen and the setup look; dark
  headers remain a choice under Appearance.
- X4: texts showed question marks where the font packs lack a character
  (arrows in the battery view, names of languages in other scripts); a
  test now keeps every translation to the characters the packs carry.
- The emulator's scripts can run actions (`run:stats.open`), and
  `tools/screens.py` draws every screen for a look at all of them.
- A first-run setup (new plugin `setup`): on the first start a short
  guide through the language, the buttons, the text size, the time zone,
  Wi-Fi, the keyboard and the screen lock, a page each with a good
  default chosen, so *Next* is always right; skippable, and opened again
  from Settings (*Set up PaperPaw*). Plugins add steps of their own
  (`Registrar::add_setup_step`); steps a device does not need are left
  out.
- The row of hints under the screen says what the buttons do there: only
  *Back* where nothing else does anything (battery, a book's statistics,
  Wi-Fi transfer), *Menu* in a book, *Delete* while unlocking.
- A status item *Battery when low* that shows the battery only at or
  below a level (Settings, System, advanced; 20 % by default), for a
  calm status line while there is enough.
- In lists and menus, moving on past the last entry goes round to the
  first, and back from the first to the last (turning pages still stops
  at the end).
- A setting with two choices (e.g. the clock's 24 or 12 hours) switches
  at once in the settings and menus, instead of opening a list to pick
  the other one; with more choices (a plugin may add some) it opens the
  list as before.
- A press that starts slow work shows at once that it was taken: the
  pressed button's hint turns black (or an hourglass shows in a corner)
  while the work goes on, on the X4 at no cost to it. Only where that
  work took 0.4 s or more the last time; quick screens come up as before.
- The icon font has an hourglass; any of Tabler's icons can be added by
  name (`tools/icons/subset.py`).
- Pressing again while another screen comes up no longer runs those
  presses afterwards (on the X4 they could jump far ahead): presses made
  before the new screen shows are dropped. Turning pages quickly keeps
  every press. Shells say when a frame shows (`Kernel::shown`).
- Progress sync (kosync) no longer keeps the X4's radio on while
  reading: turning pages sends the position only while the radio is on
  anyway; it is sent when the book is closed and when the device sleeps.
- A Wi-Fi sign in the status lines (home screen, book, quick panel)
  while the device is in a network: the status item `wifi.status` of the
  `wifi` plugin, before the battery by default.
- Screens come up faster on the X4: what a screen reads while it is
  drawn in bands comes from the card once, not once per band (the home
  screen after a book: 0.3 s instead of 1.9 s to draw). Plugins get this
  without any change.
- The screen lock could not be switched off with buttons: the chosen row
  was white on white. Its kinds are a list as in menus now.
- Wi-Fi transfer and Calibre on the X4: plugins' servers and sockets work
  there now (books up from a browser at about 190 KB/s, from Calibre at
  about 110 KB/s, files down at about 140 KB/s). The transfer's address is
  shown whole, on two lines if need be.
- X4: joining a network with several access points (a mesh) takes the
  strongest one, not the first one heard; joining after a start no longer
  fails a few times first.
- Calibre: the button row says "Try again" where it does something; the
  screen no longer asks to press a button by a name the device has not.
- The X4 can be told over USB to run any action (`run calibre.open`), for
  testing a tool without finding it in the menus.
- X4: requests fail with a reason instead of restarting the X4 when memory
  is short or in pieces; `heap` over USB also says the largest block that
  could be had.
- The licence check covers the X4 firmware's dependencies too.
- Releases carry the X4's firmware as one image for its whole flash
  (`paperpaw-x4-<version>.bin`, with its SHA-256), which paperpaw.org
  offers for flashing in the browser.

- Wi-Fi on the X4 (new plugin `wifi`, under Settings, Device): the radio
  is off while reading and comes on by itself when something needs the
  network (a catalog, a sync), joins the strongest network joined before
  and goes off again a minute after the last use (a setting); on USB it
  stays on. Its page lists the networks in reach to join, with the
  password typed on the keyboard, and forgets saved ones (menu button or
  a long press). Saved networks are kept with the passwords and keys.
  Plain HTTP and, since, HTTPS.
- Network for plugins on devices where PaperPaw manages Wi-Fi
  (`paperpaw_sdk::wifi`, [ADR 0016](docs/adr/0016-network-and-wifi.md)):
  `Command::Wifi`, `Env::wifi`, and the events `core.wifi-changed`,
  `core.wifi-scanned`, `core.wifi-failed`, `core.network-wanted` and
  `core.network-idle`. Requests, servers and connections made without a
  network wait for one up to 30 seconds. Shells read a request's file to
  send themselves (`HttpCall::body_from`) and may write a download to its
  file as it comes (`HttpCall::save_to`, `HttpResponse::saved`).
- Settings: a group may bring its own page (`SettingGroupSpec::page`),
  and settings and menu entries may need a device where PaperPaw manages
  Wi-Fi (`MenuNeed::Wifi`).
- `paperpaw-http` (new crate): the HTTP/1.1 client protocol without I/O,
  for shells with their own network stack.
- The emulator's X4 profiles pretend to manage Wi-Fi, with made-up
  networks, to try the `wifi` plugin without a device.
- Translations take a quarter of the memory: a catalog keeps where each
  message's line starts (4 bytes, 16 before) and reads key and value from
  it when looked up; about 16 KiB less on the X4, as fast.
- X4: the stack no longer runs into the memory below it (the FAT cache
  and the inflate state of pictures went to the heap); `heap` over USB
  also says how much of the stack was never used.

- Developer options: an advanced setting under System
  (`core.developer`) shows a group of settings for developers
  (`groups::DEVELOPER`), in which any plugin and shell may put its own;
  the battery's page lists what each plugin took only there
  (`battery.by-plugin`), and the X4's light sleep on battery can be
  switched off to measure the difference (`x4.light-sleep`).
- X4: on battery the processor sleeps (light sleep, about 0.1 mA) while
  there is nothing to do, waking every 30 ms to read the buttons and for
  what is due; before, it spun at full speed (about 20 mA) all the time
  it was awake. On USB it stays awake for the computer (`idle sleep
  <seconds>` over USB tries the battery's way for a while, then starts
  again).
- X4: two watchdogs start it again if the firmware stops coming by: the
  RTC's after 20 s, counting in light sleep too (a sleep that does not
  end), and a timer group's after 30 s; before, a hang kept it on until
  the battery was empty. Where it was is kept through the reset (`boot`
  over USB says it, with the reason).
- Battery use (new plugin, under Tools): records the battery about once
  a minute while the device is awake, with what it did, and shows the
  charge now, this time awake (pages, refreshes), the use on battery an
  hour awake and for 100 pages, how long it lasts at that pace, the recent
  times awake, and which plugins were shown and kept the processor busy.
  The records are CSV files in `.paperpaw/plugins/battery` (`log.csv`,
  and `plugins.csv` every ten minutes awake), for analysis on a computer.
- What each plugin takes: the kernel measures the time each plugin's code
  runs (screens drawing and handling input, event and action handlers),
  the time its screens are shown with the refreshes then, and the storage
  it reads and writes; plugins see it in `Env::usage`
  (`paperpaw_sdk::usage`), with the refreshes, input and sleeps of the
  whole device. Shells give a clock for it (`KernelBuilder::clock`) and
  say how long the panel refreshed (`Kernel::panel_busy`).
- The battery's voltage where the device measures it
  (`Battery::millivolts`: the X4, Android), and an event each time the
  battery is measured (`events::BATTERY_MEASURED`); status lines redraw
  only when the percent or charging changes.
- X4 remote: `--get` copies a file off the card (e.g. the battery logs).
- A new icon: the capital Q of Besley in ink on e-paper, as on
  paperpaw.org (its source is `shells/android/icon.svg`).
- X4: SD cards with a partition table (MBR or GPT), as nearly all come,
  are read; before, only cards with the file system from the first block
  were. A card in exFAT is named as such in the log.
- X4: remote control over USB for testing on the device:
  `tools/x4-remote.py` presses its buttons, takes screenshots and tells
  what the screen shows, with the steps of the emulator's scripts.
  `Kernel::capture` draws the whole screen for a shell to send elsewhere.
- What the buttons do, at the bottom of the screen: where buttons lie in
  a row below it (the X4's four), a row above them names what each does
  on the screen shown: *Back*, *Select*, arrows, or a screen's own word.
  The reader keeps the whole page: there the row shows for a few seconds
  after a button of the row is pressed. Switched off under Controls ›
  *Show what the buttons do*. Screens say when they show it
  (`Registrar::set_screen_hints`, `Command::ShowHints`) and name their
  buttons (`Screen::hint`); shells give the row with
  `KernelBuilder::button_row`.
- X4: books and other files go onto the card over USB
  (`tools/x4-remote.py --put`); code may use all 8 MB of flash the chip
  maps, not 4 shared with the fonts; the reader counts pages one at a
  time in the background, so buttons answer within half a second.
- Reader: the status lines sit close to the edge of the screen (1.5 mm,
  under Status lines › *Distance from the edge*), and the text has the
  room their margin took: two more lines a page on the X4.
- X4: the buttons are read in a timer's interrupt every 5 ms, so a press
  counts while PaperPaw is busy (before, the buttons did nothing while
  the reader counted a book's pages); presses wait in a queue with the
  time they came.
- X4: a page turns in about 1.5 s instead of 4 to 6. It draws in black and
  white, what its panel shows (its four grays are still to come), in
  bands twice as tall; the frame sent is kept in runs of bytes and sent
  again after a fast refresh instead of drawn anew; gray formats blend
  glyph edges in gray, not through red, green and blue; the panel gets
  its bytes straight from the frame, in bigger pieces.
- Storage on SD cards (the X4): the 8 blocks used last stay in memory,
  and the 4 files read last stay open. The library opens in under 2 s
  instead of 24, a page is laid out in a quarter of a second.
- *Flash when screens change* (Display, advanced): off, another screen
  opens with a fast refresh and counts towards the full refresh after so
  many pages. Off on the X4, where a full refresh takes 3.5 s.
- Faster on small devices, measured on the X4 (a page now turns in about
  0.7 s): font packs find a glyph's size in whole numbers (the X4 has no
  floating point unit, and every character asked); the EPUB reader keeps
  the chapter it reads and finds the next block right after the last;
  the reader writes its position at most every 5 s (always when the book
  closes or the device sleeps) and remembers the page it looked up last;
  bands are cleared a byte at a time; glyphs of the usual weight skip
  the loop of bold ones; the X4 turns black and white bands eight by
  eight pixels at once for its panel. A long chapter of notes no longer
  runs out of memory while converted.
- X4: the display is driven with the stock firmware's sequences (as the
  FreeInk SDK records them): fast `FC`, which loads the temperature each
  time, half `D7`, full `F7`, the stock booster and border; after every
  refresh both memories of the controller hold the frame shown. Before,
  a fast refresh sometimes left a page's text on the panel: the page
  before (or after) stood under the new page number until the next full
  refresh, which looked like a page turned by itself.
- X4: the full refresh against ghosts takes the shorter waveform of a warm
  panel (1.6 s instead of 3.5); the slow one only after starting.
- Pictures of books are decoded no larger than the screen shows them,
  upright or turned (`OpenOptions::image_fit`, a `Fit` from the device;
  it was a longer side), in their proportions to the pixel: a cover on
  the X4 is kept at 480 × 676, not 1500 × 2114, and its page drawn in
  1 s instead of 3.4. A JPEG made smaller is made smaller as it is
  decoded, from the lower frequencies of its blocks, then averaged to its
  size; pictures are dithered in whole numbers. Sleep screen pictures are
  kept at the screen's size too.
- Storage on SD cards (the X4): files appended to stay open and are
  written back when they are wanted otherwise, when another file needs
  the room, or on `Storage::sync` (new; the kernel calls it before the
  device sleeps). A cover decoded in hundreds of pieces is no longer
  looked up and written back for each.
- X4: the first opening of a book with a large cover takes 5 s instead
  of 22 (and says so after 1 s), through the changes above and below.
- Pictures compressed in a book are decoded as they are inflated
  (`ZipArchive::reader`, an `EntryReader`), not inflated to a temporary
  file first; with memory short for both, as before. JPEGs decode a
  third faster: Huffman codes looked up by their first 8 bits, bits
  taken a few bytes at a time, the inverse DCT in 32-bit numbers (at
  half size with 6 multiplications a line instead of 16), and blocks of
  one level filled at once. Pictures are read and written several blocks
  of a card at a time (rows drawn in chunks of 8 KB).
- X4: the SPI bus uses DMA for transfers of 32 bytes and more.
- Storage on SD cards: whole blocks of a file pass by the blocks kept,
  several read or written in one command (`Blocks::read_blocks`,
  `write_blocks`), so a picture streamed through no longer pushes the FAT
  and the folders out; 24 blocks are kept instead of 8, and 16 files
  open instead of 8.
- The reader says when a book is opened for the first time and takes a
  moment ("This takes a moment"), then opens it: formats say whether a
  document opens at once (`FormatSpec::prepared`, `Formats::prepared`);
  EPUB does once its first chapter is converted.
- X4: the SD card is read and written by PaperPaw's own driver
  (`paperpaw_x4::card`), a command at a time with the bus held, not a
  bus transaction for every byte as `embedded-sdmmc` did.
- X4: pages with pictures are sent to the panel's second memory as they
  were kept, packed, in 80 ms, not drawn again in 2 s: the heap has the
  64 KB more the bootloader leaves free.
- X4 remote: `--profile` shows where the time goes (the functions the
  processor was in, every 5 ms), `--trace` what is read and written.
- A button with nothing bound to its long press counts as it goes down,
  not when it is let go: pages turn as the button is pressed.
- X4: a button counts once its reading held 15 ms, and a ladder goes from
  one button to another only through nothing pressed: let go, the lower
  page button passed the level of the upper one and turned a page back
  by itself.
- X4: the buttons are read with the ADC calibrated for its offset.
  Uncalibrated, every reading came out some 1600 too high: the lower page
  button turned back, and the two left front buttons did nothing.
- A screen with large pictures (over 15 % of it, such as a book's cover
  page) is left with a full refresh: e-paper keeps strong ghosts of them
  after a fast one. Covers in a list stay below that.
- X4: on USB power, sleep shows the sleep screen and keeps the USB
  connection, so a computer can still read the log and wake it.
- OPDS: a catalog's user name and password go only to the catalog's own
  server (the same scheme, host and port); covers, books and further
  lists elsewhere no longer receive them, and the password field warns
  when the address begins with `http://`. Feeds made to fill the memory
  (endless links, authors or entries) are cut to what is shown. Books
  synced before are recognised however many there are, so deleted books
  no longer come back; a record of them that cannot be read leaves the
  catalog alone for that sync. Two books downloaded one right after the
  other are both saved intact, and a stopped sync leaves no partly
  downloaded book behind. A list of catalogs that cannot be read (a card
  error, a file saved in another encoding) is no longer saved over:
  changes are refused with a message, so catalogs and their passwords
  stay.
- Progress sync (kosync): *Sync progress* fetches the position on the
  server before it sends its own, so a further position from another
  device, such as KOReader, is no longer overwritten. Nothing is sent
  for a book while its progress is being fetched or while *Go there?* is
  open; it is sent once no other device is further, or after you choose
  to stay. An answer that cannot be read no longer counts as "no
  progress". Changing the server, however it is changed, signs out, so
  the account's key never goes to another server; the server setting
  opens the account screen.
- Joplin: when the connection drops just after a note was written,
  sending again finds the note Joplin already has and updates it instead
  of making it a second time. Lock files named in upper case are seen.
- Readwise: highlights are read and sent a piece at a time from a file,
  so notes of any length go, also on the X4; very long highlights are cut
  to what Readwise takes. What cannot be sent (no connection, Readwise
  down) waits and goes with the next export or when Wi-Fi comes back.
- Wallabag: an answer that is not the article list (a maintenance page,
  an error, a list cut short) no longer deletes the articles on the
  device. An unread article that is no longer among the 30 newest stays;
  a book goes only when Wallabag says its article is archived or
  deleted. If the list of fetched articles cannot be read, a fetch stops
  and changes nothing. The user name and client ID settings have help
  texts.
- News: feeds of the same name get a book each, and removing one no
  longer deletes the other's (books from earlier versions are found and
  replaced at the next fetch). Removing a feed during a fetch, or two
  fetches at once, no longer puts one feed's articles into another's
  book. An answer that is no feed, such as a Wi-Fi sign-in page, keeps
  the last book instead of emptying it. A list of feeds that cannot be
  read is not overwritten.
- KOReader: a KOReader file that cannot be read, is not there yet (while
  Syncthing is still copying a book's `.sdr` folder) or is in an older
  form no longer deletes the bookmarks and highlights on the device, and
  annotations PaperPaw cannot read are no longer removed in KOReader:
  the exchange waits, changing nothing on either side. On the X4, books
  whose KOReader file is over 32 KiB are not exchanged, so opening them
  cannot run out of memory. While the clock is not set, new annotations
  are no longer sent dated 1970; they go once the time is known, and a
  note changed on one side only always reaches the other. When the card
  is too full to keep what came from KOReader, the exchange stops instead
  of later removing those annotations in KOReader.
- Statistics sync: a file in the WebDAV folder that cannot be read (a
  login page, a file cut short) no longer replaces the statistics there
  or here; the sync stops and says so. Changing the folder or the user no
  longer deletes statistics on this device, and statistics lost here
  come back from the folder instead of being deleted there too. The
  first upload to an empty folder no longer overwrites a file that
  appeared meanwhile, and passwords with spaces at either end work.
  Syncing large KOReader libraries takes much less memory and leaves no
  working files behind after an error; KOReader books without a
  fingerprint stay separate books.
- Reading statistics: a list of books that cannot be read is no longer
  overwritten when a new book is opened. Closing a book ends the reading
  of its last page, and time on the home screen no longer counts as
  reading. On an X4 without a set clock, pages read are kept and
  recorded as soon as the time is known, and the statistics say they
  need it instead of showing January 1970. Pages of long books are no
  longer merged before the reader has counted them; the time per day is
  worked out anew after a change of time zone; the *Time left* items no
  longer read the statistics while the page is drawn. Setting a daily
  goal adds the *Read today* widget only the first time, so one you
  removed stays away. When the card is full, reading is kept, written
  once there is room, and a notice says so. Every widget setting has a
  help text.
- To-do list: an open item beginning with "x " ("x ray at 3") is no
  longer read back as done and lost; it is kept with a date in front, as
  todo.txt has the day an item was made.
- Picture widget: its hint for a folder name of thousands of words is
  measured without overflow (a panic in debug builds).
- Text layout: a word moved to the next line, and text carried to a
  narrower line (after a hanging first line), is broken again where it
  does not fit, so no line comes out wider than its box, and lines laid
  out from a kept position are those laid out from the paragraph's
  start. A paragraph's last line no longer counts the spaces after it,
  so right-aligned and centred last lines end where they should.
- Fonts: family names in Mac Roman show their letters beyond ASCII
  ("Café" had a control character for the "é"); absurd text sizes and
  outlines far outside a glyph's box no longer overflow (a panic in
  debug builds) or take long to draw.
- Hyphenation: words from a pattern file's exception list keep the
  minimum letters before and after a hyphen, as in TeX.
- Documents: a position in a gap before a block's first checkpoint (a
  comment at its start) maps to the next character, no longer past it;
  `Block::replace` keeps styled runs apart where the replaced text
  reaches from one run into the next.
- Pictures: a JPEG whose data ends without its end marker is shown,
  baseline and progressive, instead of failing; a GIF stops offering
  rows once the sink refused one; decoded image files open on storage
  that reads in pieces; `PngWriter::new` refuses a width of `u32::MAX`
  instead of overflowing.
- JSON: `stream::Parser::finish` reports a number or literal that ends
  the text (`42` as the whole text was never reported); `read_file`
  calls it.
- SQLite: integers and reals beyond 2^53 are ordered exactly, as SQLite
  orders them in indexes; a schema too large for page 1 leaves no
  `.part` file behind; `columns` passes over commas in comments and
  quoted texts.
- Dictionaries: a short definition that decoding makes longer (brackets
  around a pronunciation) comes whole, no longer cut at its end.
- Graphics: shading on panels of 2 to 16 grays is reported changed, so
  it is refreshed; `round::point_at` rounds to the nearest pixel on
  every side of the centre; two half rings meet without a seam; rounded
  rectangles and pills reach their sides wherever the circle covers
  most of a pixel (`round::corner_inset` rounds to the nearest pixel),
  and a 2 by 2 square of radius 1 is drawn. `Rect::union` keeps the
  largest size instead of wrapping past 65535.

## [0.3.0] - 2026-10-16

- Screen lock (off by default): a PIN, a pattern or a password asked for
  when PaperPaw starts and when the device wakes up, set under System ›
  *Screen lock*. The code is kept only as a salted hash. Locked, nothing
  else can be used; after five wrong tries the next one waits half a
  minute. *Lock now* locks at once from any button, gesture or rule.
  It looks like a phone's: the time and the date above, round keys
  below, rings for the digits still to come.
- `Canvas::draw_line`.
- Plugin SDK: input filters (`Registrar::add_input_filter`) see every
  button and finger before screens and key maps do, and may take it or
  run an action instead; refresh filters (`Registrar::add_refresh_filter`)
  change how the panel is refreshed. `InputEvent` and `ButtonState` are
  part of the SDK.

- Plugin SDK: services, what one plugin offers others
  (`Registrar::add_service`, `Env::services`), and points plugins define
  for others to contribute to (`Registrar::contribute`); the event
  `core.library-changed` (`library::changed`), emitted when books come
  into the library or leave it (Wi-Fi transfer, catalogs, Calibre, the
  file manager), and the library shows them at once.
- Rules can have a condition: only between two times of day, only while
  reading or while not, only on battery or while charging, only while a
  switch is on or off (hold a rule, *Only if …*). They can also set a
  number (e.g. the light to 20) and show a message.

- Drag and drop in lists, as on phones: entries of menus (*Customise
  menus*), the widgets of the sleep screen and the items of status lines
  carry a grip (≡) at their end; drag it to move the row, a bar shows
  where it lands. The status line's chosen items now come first, in
  their order. Buttons move rows as before.
- Plugin SDK: `ui::Reorder` (rows moved by dragging a grip, for any
  `ListView`), and screens get a finger before the swipes in from the
  edges do (`Screen::on_pointer`), so grips and handles at the edges
  work.
- The quick panel has its name in *Customise menus*.

- Rules know many more moments: at a time of day on chosen days (every
  day, Monday to Friday, at weekends, one day), every so many minutes,
  when a book is closed or read to its end, when the device goes to
  sleep, when the battery falls below a percentage, when charging begins
  or ends, and when a switch is turned on or off.
- Plugin SDK: events `core.book-closed` and `core.book-finished` (the
  reader emits them), and `Screen::on_close`, called when a screen is
  closed.

- Gestures as on phones: up from the bottom edge goes home, down from the
  top edge opens the quick panel, in from a side goes back (while reading
  it turns the page). The quick panel holds night mode, the light,
  turning the screen, refreshing the screen, a screenshot, sleep and the
  settings, with the time, date and battery above (what shows there and
  how large is set under Appearance); plugins and the menu editor change
  what it holds. *Refresh the screen* (`core.refresh-screen`) clears
  e-ink ghosting with a full refresh, from the quick panel or any button
  or gesture it is bound to. Swipes in from the edges come before the screen sees them.
- More gestures as on phones: up on the home screen shows every tool at
  once (*All tools*, `home.all-tools`), down on it searches the library,
  and two fingers apart or together make the text larger or smaller
  while reading (`pinch.out`, `pinch.in`, in a section *Two fingers* of
  *Buttons and gestures*). The first home screen on a touch screen
  explains the gestures once (*Hint on the gestures* under Appearance
  shows it again).
- Gestures per tool: as standard, the tool's own (gestures go to it, a
  swipe in from an edge needs a second one: for games and drawing) or
  off. A tool sets its default (`gestures.policy` in its key map); the
  user changes it in *Buttons and gestures*.
- *Buttons and gestures* is easier to find one's way in: where a change
  applies (everywhere, home screen, reading, library, any tool), inputs
  in sections (edges, swipes, taps with a picture of the zone, held
  taps, buttons, held buttons), and the actions to choose from in groups
  with a search, *As standard* and *Nothing* first.

- The home screen is a grid, as on a phone: 4 columns by default (rows
  to fit the screen; both settings), each widget taking a rectangle of
  cells, each tool an icon in a cell of its own; empty cells stay empty.
  A long press or the pencil shows the grid: drag an item to move it, by
  its corner to size it, tap it for its options (move or size it with
  the buttons, its settings, remove it); *Add* adds widgets, tools and
  pages. The status line sits above the grid. Pages written before are
  placed in the grid by themselves.
- Widgets have more settings of their own: the clock's size (also as
  large as its cells allow), the date's size, whether the continue card
  shows its cover, its progress and a frame, and for the recently read
  books their heading, their titles, where the titles sit and how many
  books show. Tools with or without their names.

## [0.2.0] - 2026-10-16

- Books from a catalog (OPDS) or from Calibre keep the title, author,
  series and tags the source gives, where the book names none itself
  (text files, PDFs): `library::provide`.
- *Say what is chosen* (System, where the device speaks): says the row
  or button the buttons move to, with a switch's state, for using
  PaperPaw without looking at the screen.
- Screens say what they show: rows with their text and
  focus, buttons, switches and headings, told by the drawing helpers as
  they draw (`Canvas::describe`, `semantics::parts`,
  `Kernel::describe`). The emulator's scripts tap by name
  (`tap-on:Display`) and print a screen's parts (`describe`).
- The library is a service (`library` in the plugin SDK): what is known
  of books (title, author, series, tags, cover, progress, and fields
  plugins add), the books read last and the favourites, kept by the
  kernel from the reading events. The library, the home screen and the
  sleep screen show the same data instead of keeping copies of their
  own; what the library kept before is taken over.
- Bookmarks and highlights shared with KOReader (`koreader` plugin), for
  books read in both on one device or in one synced library folder:
  what KOReader has in `Book.sdr` comes over when a book is opened, and
  what is new here goes to KOReader the next time it opens the book;
  removing one, or a later note, goes across too. EPUB only.
- EPUB positions as KOReader writes them, down to the character: reading
  positions synced with KOReader land on the word, not the paragraph.
- Plugin SDK: bookmarks and highlights are kept by `annotations`, for
  every plugin, with when they were made and changed; event
  `core.annotations-changed`. `Document::xpointer` and
  `Document::locate_xpointer` are exact to the character.
- Colour pictures on colour screens (ADR 0015): covers, pictures in
  books, comics, OPDS covers and the sleep screen's pictures are shown
  in colour on phones and tablets and on colour e-ink (Boox models named
  "Color" or "C", with colours strengthened for the filter); text and
  the interface stay gray, monochrome e-ink is unchanged. JPEG (also
  progressive), PNG and GIF are decoded in colour where asked. Plugin
  SDK: `ImageSink::wants_color` and `row_rgb`, `ColorFile`,
  `ColorWriter`, `ImageFile`, `image::picture_from_file`,
  `OpenOptions::color`, `Formats::options`, `Canvas::draw_rgb_row`,
  `Framebuffer::set_saturation`, `ColorPanel::boost_percent`.
- Smaller text by default: menus 9 pt (was 11), status lines 6 pt
  (was 8). Both stay adjustable in the settings.
- Dictionary: the page of a long entry no longer covers *Online*.
- Text and paper colours on Android (Appearance): eight of each, or
  one's own colour code (e.g. #5b4636); night mode swaps them.
- Room between widgets: *Space*, a gap (small, medium or large), and
  *Free space*, which takes the room the others leave, e.g. to put the
  clock in the middle or the tools at the bottom. Both may be added more
  than once, on the home screen and the sleep screen.
- Appearance: *Corners* (square, slightly rounded, round) for
  highlighted rows, buttons and boxes, and *Contrast*: strong draws gray
  hints and notes in black, for screens with few grays. The Airy theme
  sample rounds its corners. Plugin SDK: `UiStyle` gains `corner` and
  `strong` (with `UiStyle::faint` for gray text), `ui::fill_round`,
  `ui::stroke_round`, and the canvas rounded rectangles.
- GIF pictures in books, comics and the sleep screen's own pictures: the
  first picture of the file (also of an animation, interlaced or with
  transparency), decoded to gray row by row like JPEG and PNG.
- Plugins can bring a stylesheet for books: a data plugin's
  `styles.css`, or `Registrar::add_stylesheet`, applied after a book's
  own and before one's own (EPUB). SDK: `Env::stylesheets`,
  `OpenOptions::css` (no longer `Copy`).
- Books' embedded fonts in WOFF are used too (WOFF 2 is not read).
- Calibre's wireless device connection (`calibre` plugin, in the tools):
  the device finds Calibre in the network (or connects to the address
  set), and Calibre sends books to the library, lists and deletes them
  as with a reader on a cable; a password set in Calibre is asked for
  once. SDK: `sha1`, and `json::raw_member`, `json::unquote`,
  `json::string_array`.
- Sockets for plugins (ADR 0014): TCP connections that read text or
  files of any size into storage, and UDP broadcasts to find services,
  in the emulator and on Android (`Command::Connect`, `SocketSend`,
  `SocketRead`, `Disconnect`, `Broadcast`).
- Small capitals (CSS `font-variant: small-caps`, `font-variant-caps`)
  in EPUB, HTML and the other converted formats: lower case letters as
  smaller capitals, the text itself unchanged (search, selection and
  notes as before). Books are prepared anew once. Document model:
  `TextStyle::SMALL_CAPS`; embedded font families at most 31 (was 63).
- Snapshot tests: screens of the X4 and the Boox are drawn headless and
  compared with reference pictures, so layout faults fail a test.
- The folder chooser says when it is still opening a folder (slow SD
  cards) and ignores taps until it shows it.
- Widgets have settings of their own, from the home screen's editor:
  where the status line, date, clock, today's reading and the word of
  the day sit (left, centre, right), and whether the clock shows the
  date. Plugins give their widgets settings the same way
  (`widgets::settings_group`, `widgets::align_setting`).

## [0.1.1] - 2026-10-15

The Android app for F-Droid.

- The app is built reproducibly: with the same tools (shells/android/
  versions.env) it comes out byte for byte the same anywhere, so F-Droid
  can publish it signed with PaperPaw's key. A dependency's macro that
  wrote its code in a random order is patched (patches/jni-macros).
- The app's library is stored in the APK and runs from there, without
  symbols: 6.8 MB to download, less space taken on the device.
- The licence names the copyright holder; README with screenshots.

## [0.1.0] - 2026-10-15

The first release: PaperPaw on Android e-readers (tested on a Boox Go 7)
and in the desktop emulator, with its plugin SDK. The firmware for the
Xteink X4 is built but has not run on a device yet.

### Reading

- EPUB 2 and 3 (with the book's styles, embedded fonts and pictures, or
  one's own stylesheets per folder or book), FB2, Markdown, plain text,
  comic archives (CBZ), Xteink's XTC and XTCH, and PDF on Android.
- Contents, search, bookmarks and highlights with notes, export of notes
  as Markdown, a dictionary (StarDict), Wikipedia and translation
  (DeepL), a vocabulary list of words looked up.
- Hyphenation (German, English), reading fonts with weight and size,
  margins, spacing, status lines to taste, reading aloud, pages turned
  by themselves, turning the screen.
- Reading statistics, shared with KOReader (also over WebDAV), and
  reading positions synced with KOReader's sync server.

### Books and more

- A home screen of widgets on pages (date, battery, continue reading,
  recently read, tools, statistics, headlines), editable by touch or
  buttons.
- The library by folders, authors, series and tags; a file manager;
  books from OPDS catalogs and WebDAV folders (Nextcloud), kept in sync.
- News feeds as books; Wallabag's read-later articles, highlights to
  Readwise and a browser, switched off until wanted; transfers from a
  computer or phone over Wi-Fi, typing on a phone.
- A sleep screen (cover, picture or widgets), screenshots.

### Making it one's own

- Every screen, menu, key and gesture can be changed, in the app or in
  `paperpaw.ini`; profiles, rules, themes, translations and fonts as
  plugins without code (data plugins).
- Settings with descriptions and a search, passwords and keys in one
  place, short menus by default, usable by touch and by buttons alike.
- English and German.

### For developers

- The plugin SDK (`paperpaw-sdk`) with a guide ([Writing
  plugins](docs/plugins.md)) and a sample plugin; plugins are tested
  against the real kernel.
- The desktop emulator with device profiles and memory budgets.
- The Xteink X4's firmware, its display, buttons, battery and SD card
  tested against simulated hardware ([ADR 0013](docs/adr/0013-x4-firmware.md)).

[0.3.0]: https://github.com/hthoene/paperpaw/releases/tag/v0.3.0
[0.2.0]: https://github.com/hthoene/paperpaw/releases/tag/v0.2.0
[0.1.1]: https://github.com/hthoene/paperpaw/releases/tag/v0.1.1
[0.1.0]: https://github.com/hthoene/paperpaw/releases/tag/v0.1.0
