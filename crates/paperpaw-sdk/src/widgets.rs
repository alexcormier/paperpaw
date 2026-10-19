//! Widgets: small boxes plugins contribute for screens that show them,
//! such as the home screen's pages (a clock, the book being read, today's
//! reading, a grid of tools). Any plugin adds widgets with
//! [`Registrar::add_widget`](crate::Registrar::add_widget); screens let
//! the user choose which to show and where.
//!
//! A screen that shows a widget makes it ([`Widgets::make`]) and keeps
//! it: the widget reads what it shows when the screen updates it (when
//! it shows, about each minute, after events), and draws from that, so
//! drawing stays quick ([`Widget`]). Widgets with nothing to read, such
//! as a clock, are a pair of functions ([`WidgetSpec::drawn`]).
//!
//! A widget may have targets: places that do something when tapped, which
//! buttons reach one after the other too (see [`WidgetTarget`]).
//!
//! A widget may have settings of its own, e.g. where its text sits
//! ([`align_setting`]): they go in a settings group with the widget's id
//! ([`settings_group`]). The settings list leaves such groups out; screens
//! that edit widgets open them (slot
//! [`SETTINGS`](crate::builtin::slots::SETTINGS) with parameter
//! [`ID`](crate::builtin::params::ID)).
//!
//! The kernel brings two widgets for room between the others, which a list
//! may hold more than once ([`repeatable`]): [`SPACE`], a gap of a set
//! height, and [`FILL`], the room the others leave. Screens that place
//! widgets one below the other give each fill its share of that room
//! ([`fill_height`]); elsewhere it is not there.
//!
//! Screens that show widgets in a grid, as the home screen does, use
//! [`grid`]: its layout as data, and a view that edits it in place.
//!
//! Widgets share a look the reader shapes for each ([`Frame`], [`Size`],
//! [`draw_caption`], see [`look`]), and say which kind they are
//! ([`categories`]), so that screens offering many of them group them.

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

use paperpaw_graphics::Canvas;
use paperpaw_platform::Rect;

use crate::env::Env;
use crate::id::Id;
use crate::settings::{SettingGroupSpec, SettingKind, SettingSpec, SettingValue};

pub mod grid;
pub mod look;

pub use look::{
    Frame, Look, Size, caption_height, choice, draw_caption, draw_frame, draw_message,
    draw_wrapped, frame, frame_setting, frame_spec, line_height, part_setting, part_spec,
    setting_id, shows, size, size_setting, size_spec,
};

/// The kinds of widgets ([`WidgetSpec::category`]), each the translation
/// key of its name; screens that offer widgets group them so.
pub mod categories {
    /// Clocks, the date, calendars.
    pub const TIME: &str = "core.widgets.time";
    /// The book being read and others: covers, progress, shelves.
    pub const BOOKS: &str = "core.widgets.books";
    /// Reading itself: time read, goals, streaks.
    pub const READING: &str = "core.widgets.reading";
    /// Words and texts: quotes, highlights, vocabulary.
    pub const WORDS: &str = "core.widgets.words";
    /// One's own: notes, to-dos, pictures.
    pub const PERSONAL: &str = "core.widgets.personal";
    /// From outside: news, weather.
    pub const WORLD: &str = "core.widgets.world";
    /// The device: battery, status.
    pub const DEVICE: &str = "core.widgets.device";
    /// Room between the others.
    pub const LAYOUT: &str = "core.widgets.layout";
    /// Any other.
    pub const OTHER: &str = "core.widgets.other";

    /// Every kind, in the order screens show them.
    pub const ALL: [&str; 9] = [
        TIME, BOOKS, READING, WORDS, PERSONAL, WORLD, DEVICE, LAYOUT, OTHER,
    ];
}

/// A widget as a screen shows it: made for that screen by its spec
/// ([`WidgetSpec`]), it keeps what it shows.
///
/// What it shows is read in [`update`](Self::update), when the screen
/// asks: when it shows the widget, about each minute, and after events
/// that may change it (e.g. the library or a setting changed). Drawing
/// then only draws: a frame may be drawn in bands, so
/// [`draw`](Self::draw) runs several times for it and reads nothing from
/// the storage (but pictures, which are drawn from their files).
pub trait Widget {
    /// Reads what it shows anew; `true` if that changed since it was
    /// last read, so that the screen draws it again. Widgets with nothing
    /// to read keep the default: nothing changes.
    fn update(&mut self, env: &Env<'_>) -> bool {
        let _ = env;
        false
    }

    /// Whether what it shows changes by itself (the time, the battery):
    /// it is read anew each minute and when the device's status changes.
    /// Widgets that show what changes only after events (books, settings)
    /// say `false`, and are read when shown and after those events only:
    /// reading them each minute would cost time and battery for nothing.
    fn live(&self) -> bool {
        true
    }

    /// The height it needs at `width`, in pixels (0: nothing to show).
    fn height(&self, env: &Env<'_>, width: u16) -> u16;

    /// The least height it can do with at `width`, drawn smaller (e.g.
    /// covers in a row): where its full [`height`](Self::height) has no
    /// room, it is placed in less, down to this. Widgets that cannot
    /// shrink keep the default, their full height.
    fn min_height(&self, env: &Env<'_>, width: u16) -> u16 {
        self.height(env, width)
    }

    /// Draws it into `area` (cleared before), from what it read.
    fn draw(&self, canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect);

    /// The places in it that do something, drawn in `area`, in reading
    /// order; without them it only shows something.
    fn targets(&self, env: &Env<'_>, area: Rect) -> Vec<WidgetTarget> {
        let _ = (env, area);
        Vec::new()
    }
}

/// Makes a widget for a screen that shows it.
pub type WidgetFactory = Box<dyn Fn(&Env<'_>) -> Box<dyn Widget>>;

/// A place in a widget that does something: a tap on it, or confirm
/// once buttons moved the focus there, runs `command`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WidgetTarget {
    pub area: Rect,
    pub command: crate::Command,
}

/// A widget contribution: its id, its name and how it is made.
#[non_exhaustive]
pub struct WidgetSpec {
    pub id: Id,
    /// Translation key of its name, e.g. "Clock".
    pub title: Cow<'static, str>,
    pub factory: WidgetFactory,
    /// Which kind it is, one of [`categories`] (or a translation key of
    /// a plugin's own).
    pub category: Cow<'static, str>,
}

impl WidgetSpec {
    /// A widget of a type of its own ([`Widget`]), made by `factory`.
    pub fn new(
        id: Id,
        title: impl Into<Cow<'static, str>>,
        factory: impl Fn(&Env<'_>) -> Box<dyn Widget> + 'static,
    ) -> Self {
        Self {
            id,
            title: title.into(),
            factory: Box::new(factory),
            category: Cow::Borrowed(categories::OTHER),
        }
    }

    /// Of the kind `category`, one of [`categories`].
    pub fn category(self, category: impl Into<Cow<'static, str>>) -> Self {
        Self {
            category: category.into(),
            ..self
        }
    }

    /// A widget with nothing of its own to read: it draws from the
    /// settings, the clock and the like (see [`Drawn`]).
    pub fn drawn(id: Id, title: impl Into<Cow<'static, str>>, drawn: Drawn) -> Self {
        Self::new(id, title, move |_| {
            Box::new(DrawnWidget { drawn, shown: None })
        })
    }
}

impl fmt::Debug for WidgetSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WidgetSpec")
            .field("id", &self.id)
            .field("title", &self.title)
            .field("category", &self.category)
            .finish_non_exhaustive()
    }
}

/// A widget that only draws, as functions ([`WidgetSpec::drawn`]): for
/// widgets with nothing to read from the storage, such as a clock.
#[derive(Clone, Copy)]
pub struct Drawn {
    /// The height it needs for a width.
    pub height: fn(&Env<'_>, u16) -> u16,
    /// Draws it into its area.
    pub draw: fn(&mut Canvas<'_>, &Env<'_>, Rect),
    /// What it shows now, as text (e.g. the time): when that changes, it
    /// is drawn again. Without it, it is drawn when its screen is.
    pub shows: Option<fn(&Env<'_>) -> String>,
}

impl Drawn {
    /// A widget `height` high, drawn by `draw`.
    pub fn new(
        height: fn(&Env<'_>, u16) -> u16,
        draw: fn(&mut Canvas<'_>, &Env<'_>, Rect),
    ) -> Self {
        Self {
            height,
            draw,
            shows: None,
        }
    }

    /// Drawn again when what `shows` says changes.
    pub fn shows(self, shows: fn(&Env<'_>) -> String) -> Self {
        Self {
            shows: Some(shows),
            ..self
        }
    }
}

impl fmt::Debug for Drawn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Drawn").finish_non_exhaustive()
    }
}

/// A [`Drawn`] widget on a screen.
struct DrawnWidget {
    drawn: Drawn,
    shown: Option<String>,
}

impl Widget for DrawnWidget {
    fn update(&mut self, env: &Env<'_>) -> bool {
        let Some(shows) = self.drawn.shows else {
            return false;
        };
        let now = Some(shows(env));
        let changed = self.shown.is_some() && now != self.shown;
        self.shown = now;
        changed
    }

    fn height(&self, env: &Env<'_>, width: u16) -> u16 {
        (self.drawn.height)(env, width)
    }

    fn draw(&self, canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect) {
        (self.drawn.draw)(canvas, env, area);
    }
}

/// Says that what widget `id` shows changed (e.g. its own data, after a
/// tap on it): screens showing it read it anew
/// ([`WIDGET_CHANGED`](crate::builtin::events::WIDGET_CHANGED)).
pub fn changed(id: &Id) -> crate::Command {
    use crate::builtin::events;
    events::WIDGET_CHANGED.emit(events::WidgetChanged { id: id.clone() })
}

/// The registered widgets.
#[derive(Debug, Default)]
pub struct Widgets {
    widgets: Vec<WidgetSpec>,
}

impl Widgets {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, widget: WidgetSpec) {
        self.widgets.push(widget);
    }

    pub fn shrink_to_fit(&mut self) {
        self.widgets.shrink_to_fit();
    }

    /// Widget `id`, also of a placement of it (`widgets.clock@2`).
    fn get(&self, id: &str) -> Option<&WidgetSpec> {
        let id = base(id);
        self.widgets.iter().find(|w| w.id.as_str() == id)
    }

    /// Every widget: its id and the translation key of its name.
    pub fn iter(&self) -> impl Iterator<Item = (&Id, &str)> {
        self.widgets.iter().map(|w| (&w.id, w.title.as_ref()))
    }

    /// The kind of widget `id`, one of [`categories`] (or a plugin's own).
    pub fn category(&self, id: &str) -> Option<&str> {
        self.get(id).map(|w| w.category.as_ref())
    }

    /// The translation key of the name of widget `id`.
    pub fn title(&self, id: &str) -> Option<&str> {
        self.get(id).map(|w| w.title.as_ref())
    }

    /// Whether widget `id` exists.
    pub fn has(&self, id: &str) -> bool {
        self.get(id).is_some()
    }

    /// Widget `id` for a screen, what it shows read; `None` if there is
    /// none. Screens keep it while they show it and
    /// [`update`](Widget::update) it.
    ///
    /// For a placement of a widget (`widgets.clock@2`, see
    /// [`Id::placement`]), the widget reads, measures and draws itself in
    /// the placement's own values of its settings
    /// ([`Settings::with_placement`](crate::Settings::with_placement)).
    pub fn make(&self, id: &str, env: &Env<'_>) -> Option<Box<dyn Widget>> {
        let spec = self.get(id)?;
        let mut widget = if id.contains('@') {
            let inner = env
                .settings
                .with_placement(Some(id), || (spec.factory)(env));
            Box::new(Placed {
                placement: String::from(id),
                inner,
            })
        } else {
            (spec.factory)(env)
        };
        widget.update(env);
        Some(widget)
    }
}

/// The widget a placement shows: `widgets.clock` for `widgets.clock@2`.
pub fn base(id: &str) -> &str {
    id.split_once('@').map_or(id, |(base, _)| base)
}

/// A widget in a placement of its own, in the placement's values.
struct Placed {
    placement: String,
    inner: Box<dyn Widget>,
}

impl Placed {
    fn with<R>(&self, env: &Env<'_>, f: impl FnOnce() -> R) -> R {
        env.settings.with_placement(Some(&self.placement), f)
    }
}

impl Widget for Placed {
    fn update(&mut self, env: &Env<'_>) -> bool {
        let placement = core::mem::take(&mut self.placement);
        let changed = env
            .settings
            .with_placement(Some(&placement), || self.inner.update(env));
        self.placement = placement;
        changed
    }

    fn live(&self) -> bool {
        self.inner.live()
    }

    fn height(&self, env: &Env<'_>, width: u16) -> u16 {
        self.with(env, || self.inner.height(env, width))
    }

    fn min_height(&self, env: &Env<'_>, width: u16) -> u16 {
        self.with(env, || self.inner.min_height(env, width))
    }

    fn draw(&self, canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect) {
        self.with(env, || self.inner.draw(canvas, env, area));
    }

    fn targets(&self, env: &Env<'_>, area: Rect) -> Vec<WidgetTarget> {
        self.with(env, || self.inner.targets(env, area))
    }
}

/// A gap of the height [`SPACE_SIZE`] says.
pub const SPACE: &str = "core.space";
/// The room the other widgets leave, shared among the fills: its own
/// height is 0, screens that place widgets give it the rest.
pub const FILL: &str = "core.fill";
/// The height of [`SPACE`]: one of [`SPACE_SIZES`].
pub const SPACE_SIZE: crate::setting::Setting<crate::setting::Text> =
    crate::setting::Setting::new("core.space.size");
/// The values of [`SPACE_SIZE`]: small, medium, large.
pub const SPACE_SIZES: [&str; 3] = ["core.space.small", "core.space.medium", "core.space.large"];

/// Whether a list of widgets may hold `id` more than once.
pub fn repeatable(id: &str) -> bool {
    id == SPACE || id == FILL
}

/// The height of each of `fills` fills where widgets `used` pixels high
/// are placed in `room` pixels.
pub fn fill_height(room: u32, used: u32, fills: usize) -> u16 {
    if fills == 0 {
        return 0;
    }
    (room.saturating_sub(used) / fills as u32).min(u32::from(u16::MAX)) as u16
}

/// The values of a widget's alignment setting ([`align_setting`]).
pub const ALIGNMENTS: [&str; 3] = ["core.align.left", "core.align.center", "core.align.right"];

/// Where a widget's text sits in its place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
}

impl Align {
    /// The alignment a value of [`ALIGNMENTS`] names; left for others.
    pub fn of(value: &str) -> Self {
        match ALIGNMENTS.iter().position(|a| *a == value) {
            Some(1) => Align::Center,
            Some(2) => Align::Right,
            _ => Align::Left,
        }
    }

    /// Its value in [`ALIGNMENTS`].
    pub const fn value(self) -> &'static str {
        ALIGNMENTS[self as usize]
    }

    /// Where something `width` wide starts in `area`.
    pub fn x(self, area: Rect, width: f32) -> f32 {
        let left = f32::from(area.x);
        let room = (f32::from(area.width) - width).max(0.0);
        match self {
            Align::Left => left,
            Align::Center => left + room / 2.0,
            Align::Right => left + room,
        }
    }
}

/// The settings group of `widget`'s own settings, named as the widget.
pub fn settings_group(widget: &Id, title: impl Into<Cow<'static, str>>) -> SettingGroupSpec {
    SettingGroupSpec {
        id: widget.clone(),
        title: title.into(),
        order: 0,
        page: None,
    }
}

/// The setting of where `widget`'s text sits, `<widget>.align`, in the
/// widget's settings group; read it with [`align`].
pub fn align_setting(widget: &Id, default: Align) -> SettingSpec {
    let id = Id::parse(&alloc::format!("{widget}.align")).unwrap_or_else(|_| widget.clone());
    SettingSpec::new(
        id,
        "core.setting.align",
        SettingKind::Choice(crate::settings::Choices::Fixed(&ALIGNMENTS)),
        SettingValue::Text(Cow::Borrowed(default.value())),
    )
    .group(widget.clone())
}

/// [`align_setting`] kept in the program
/// ([`SettingSpec::fixed`](crate::SettingSpec::fixed)), for a `static`
/// list of the widget's settings: `id` must be `<widget>.align`.
pub const fn align_spec(id: &'static str, widget: Id, default: Align) -> SettingSpec {
    SettingSpec::fixed_choice(
        id,
        "core.setting.align",
        &ALIGNMENTS,
        default.value(),
        widget,
    )
}

/// Whether widget `id` has settings of its own (a settings group with its
/// id holding settings, see [`settings_group`]).
pub fn has_settings(env: &Env<'_>, id: &str) -> bool {
    let id = base(id);
    env.settings.groups().any(|group| group.id.as_str() == id)
        && env
            .settings
            .specs()
            .any(|spec| env.settings.group_of(spec).as_str() == id)
}

/// Where `widget`'s text sits, by its [`align_setting`]; left without one.
pub fn align(env: &Env<'_>, widget: &str) -> Align {
    Align::of(env.settings.text(&alloc::format!("{widget}.align")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alignments_place_text_in_the_area() {
        let area = Rect::new(10, 0, 100, 20);
        assert_eq!(Align::Left.x(area, 40.0), 10.0);
        assert_eq!(Align::Center.x(area, 40.0), 40.0);
        assert_eq!(Align::Right.x(area, 40.0), 70.0);
        // Too wide: from the left edge.
        assert_eq!(Align::Right.x(area, 140.0), 10.0);
    }

    #[test]
    fn alignments_round_trip() {
        for align in [Align::Left, Align::Center, Align::Right] {
            assert_eq!(Align::of(align.value()), align);
        }
        assert_eq!(Align::of("sideways"), Align::Left);
    }

    #[test]
    fn fills_share_the_room_left() {
        assert_eq!(fill_height(1000, 400, 2), 300);
        assert_eq!(fill_height(1000, 1200, 1), 0);
        assert_eq!(fill_height(1000, 400, 0), 0);
        assert!(repeatable(FILL) && repeatable(SPACE) && !repeatable("home.clock"));
    }

    #[test]
    fn the_setting_belongs_to_the_widget() {
        let widget = Id::new_static("home.clock");
        let spec = align_setting(&widget, Align::Center);
        assert_eq!(spec.id.as_str(), "home.clock.align");
        assert_eq!(spec.group, widget);
        assert_eq!(
            spec.default,
            SettingValue::Text(Cow::Borrowed("core.align.center"))
        );
    }
}
