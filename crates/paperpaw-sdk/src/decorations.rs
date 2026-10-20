//! Decorations of text: marks any plugin puts on a book's words, drawn
//! the same by every reader (ADR 0021).
//!
//! - A [`Decoration`] says how text is marked: a background, the text's
//!   ink, a line under it (solid, dotted, dashed, wavy, double) in a tone
//!   and a thickness, a frame, struck through. Tones ([`Tone`]) are named;
//!   panels show them as well as they can (colours where there is colour,
//!   their lightness elsewhere, backgrounds dithered where there are few
//!   grays). [`draw_behind`] and [`draw_over`] draw one around the text.
//! - A *role* names what a mark means (`vocabulary.learning`); its look is
//!   settings, one per aspect (see [`add_role`]), in the group
//!   [`groups::MARKS`]: the plugin gives
//!   the default, themes and users the rest. [`Decoration::of_role`] reads
//!   it.
//! - A *decorator* ([`DecoratorSpec`], `Registrar::add_decorator`) gets the
//!   words of a page ([`PageText`]) and gives back ranges with their roles
//!   ([`Decorated`]). Readers ask the decorators ([`Decorators`], in
//!   `Env::decorators`) once a page is laid out, keep the answer with the
//!   page, and ask again when a decorator's [`signature`](Decorators::signature)
//!   changes. Where marks meet on a word, their decorations are
//!   [`combined`](Decoration::combined), the earlier decorator winning an
//!   aspect both set.
//! - A decorator that [takes taps](DecoratorSpec::taps) hears
//!   [`events::DECORATION_TAPPED`](crate::builtin::events::DECORATION_TAPPED)
//!   when one of its words is tapped, or chosen with the buttons.
//!
//! What changes widths (bold, a larger size) is a text filter instead
//! (see [`filters`](crate::filters)): it needs the page laid out again.

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::format;
use alloc::vec::Vec;
use core::fmt;

use crate::builtin::groups;
use crate::document::Locator;
use crate::setting::Setting;
use crate::{
    Canvas, Choices, Color, Env, Id, Rect, RegisterError, Registrar, SettingExtra, SettingKind,
    SettingSpec, SettingValue,
};

/// A named tone for backgrounds, ink and lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Tone {
    LightGray,
    Gray,
    DarkGray,
    Black,
    White,
    Yellow,
    Green,
    Blue,
    Pink,
    Orange,
    Red,
}

impl Tone {
    /// Every tone, in the order settings offer them.
    pub const ALL: &[Tone] = &[
        Tone::LightGray,
        Tone::Gray,
        Tone::DarkGray,
        Tone::Black,
        Tone::White,
        Tone::Yellow,
        Tone::Green,
        Tone::Blue,
        Tone::Pink,
        Tone::Orange,
        Tone::Red,
    ];

    /// The colour drawn: as it is on colour panels, by its lightness on
    /// the others (a yellow is a light gray there).
    pub fn color(self) -> Color {
        match self {
            Tone::LightGray => Color::gray(210),
            Tone::Gray => Color::gray(150),
            Tone::DarkGray => Color::gray(90),
            Tone::Black => Color::BLACK,
            Tone::White => Color::WHITE,
            Tone::Yellow => Color::rgb(255, 230, 80),
            Tone::Green => Color::rgb(120, 200, 120),
            Tone::Blue => Color::rgb(90, 160, 240),
            Tone::Pink => Color::rgb(240, 120, 170),
            Tone::Orange => Color::rgb(255, 170, 60),
            Tone::Red => Color::rgb(220, 50, 50),
        }
    }

    /// Its value in settings (also the translation key of its name).
    pub const fn id(self) -> &'static str {
        match self {
            Tone::LightGray => "core.tone.light-gray",
            Tone::Gray => "core.tone.gray",
            Tone::DarkGray => "core.tone.dark-gray",
            Tone::Black => "core.tone.black",
            Tone::White => "core.tone.white",
            Tone::Yellow => "core.tone.yellow",
            Tone::Green => "core.tone.green",
            Tone::Blue => "core.tone.blue",
            Tone::Pink => "core.tone.pink",
            Tone::Orange => "core.tone.orange",
            Tone::Red => "core.tone.red",
        }
    }

    /// The tone of a setting's value.
    pub fn from_id(id: &str) -> Option<Tone> {
        Tone::ALL.iter().copied().find(|tone| tone.id() == id)
    }
}

/// The kind of a line under text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum LineKind {
    Solid,
    Dotted,
    Dashed,
    Wavy,
    Double,
}

impl LineKind {
    /// Every kind, in the order settings offer them.
    pub const ALL: &[LineKind] = &[
        LineKind::Solid,
        LineKind::Dotted,
        LineKind::Dashed,
        LineKind::Wavy,
        LineKind::Double,
    ];

    /// Its value in settings (also the translation key of its name).
    pub const fn id(self) -> &'static str {
        match self {
            LineKind::Solid => "core.line.solid",
            LineKind::Dotted => "core.line.dotted",
            LineKind::Dashed => "core.line.dashed",
            LineKind::Wavy => "core.line.wavy",
            LineKind::Double => "core.line.double",
        }
    }

    pub fn from_id(id: &str) -> Option<LineKind> {
        LineKind::ALL.iter().copied().find(|kind| kind.id() == id)
    }
}

/// The value of settings for no background, no line.
pub const NONE: &str = "core.tone.none";
/// The value of the ink setting for the text as it is.
pub const AS_IS: &str = "core.tone.as-is";
/// The thickness of lines and frames where none is set, in tenths of a
/// millimetre.
pub const THICKNESS: u8 = 4;

/// How text is marked (see the [module documentation](self)).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub struct Decoration {
    /// Behind the text.
    pub background: Option<Tone>,
    /// The text itself; `None` as it is.
    pub ink: Option<Tone>,
    /// A line under the text.
    pub line: Option<LineKind>,
    /// The line's (and the frame's) tone; `None` the ink's, else black.
    pub line_tone: Option<Tone>,
    /// Of lines and frames, in tenths of a millimetre; 0 for
    /// [`THICKNESS`].
    pub thickness: u8,
    /// A frame around the text.
    pub frame: bool,
    /// A line through the text.
    pub struck: bool,
}

impl Decoration {
    /// No mark at all.
    pub const NONE: Decoration = Decoration {
        background: None,
        ink: None,
        line: None,
        line_tone: None,
        thickness: 0,
        frame: false,
        struck: false,
    };

    /// This one with `tone` behind the text.
    pub const fn background(mut self, tone: Tone) -> Self {
        self.background = Some(tone);
        self
    }

    /// This one with the text in `tone`.
    pub const fn ink(mut self, tone: Tone) -> Self {
        self.ink = Some(tone);
        self
    }

    /// This one with a line of `kind` under the text.
    pub const fn line(mut self, kind: LineKind) -> Self {
        self.line = Some(kind);
        self
    }

    /// This one with lines and frames in `tone`.
    pub const fn line_tone(mut self, tone: Tone) -> Self {
        self.line_tone = Some(tone);
        self
    }

    /// This one with lines and frames `tenths` of a millimetre thick.
    pub const fn thickness(mut self, tenths: u8) -> Self {
        self.thickness = tenths;
        self
    }

    /// This one with a frame.
    pub const fn frame(mut self) -> Self {
        self.frame = true;
        self
    }

    /// This one struck through.
    pub const fn struck(mut self) -> Self {
        self.struck = true;
        self
    }

    /// Whether it marks nothing.
    pub fn is_none(&self) -> bool {
        self.background.is_none()
            && self.ink.is_none()
            && self.line.is_none()
            && !self.frame
            && !self.struck
    }

    /// Whether anything is drawn over the text (lines, a frame).
    fn has_strokes(&self) -> bool {
        self.line.is_some() || self.frame || self.struck
    }

    /// This one with what `later` adds: each aspect this one leaves open
    /// taken from `later`; frames and strikes from either; the thickness
    /// goes with the strokes it came with.
    pub fn combined(self, later: Decoration) -> Decoration {
        Decoration {
            background: self.background.or(later.background),
            ink: self.ink.or(later.ink),
            line: self.line.or(later.line),
            line_tone: self.line_tone.or(later.line_tone),
            thickness: if self.has_strokes() || !later.has_strokes() {
                self.thickness
            } else {
                later.thickness
            },
            frame: self.frame || later.frame,
            struck: self.struck || later.struck,
        }
    }

    /// The look of `role`, as its settings say (see [`add_role`]); no mark
    /// for a role nobody added.
    pub fn of_role(env: &Env<'_>, role: &Id) -> Decoration {
        let settings = env.settings;
        let tone = |aspect: &str| Tone::from_id(settings.text(&format!("{role}.{aspect}")));
        Decoration {
            background: tone("background"),
            ink: tone("ink"),
            line: LineKind::from_id(settings.text(&format!("{role}.line"))),
            line_tone: tone("line-tone"),
            thickness: settings
                .int(&format!("{role}.thickness"))
                .clamp(0, i64::from(u8::MAX)) as u8,
            frame: settings.bool(&format!("{role}.frame")),
            struck: settings.bool(&format!("{role}.struck")),
        }
    }

    /// The colour of the text, where it is not as it is.
    pub fn ink_color(&self) -> Option<Color> {
        self.ink.map(Tone::color)
    }

    fn stroke_color(&self) -> Color {
        self.line_tone
            .or(self.ink)
            .map_or(Color::BLACK, Tone::color)
    }
}

/// All marks of `decorations` combined, the earlier winning (see
/// [`Decoration::combined`]).
pub fn combine(decorations: impl IntoIterator<Item = Decoration>) -> Decoration {
    decorations
        .into_iter()
        .fold(Decoration::NONE, Decoration::combined)
}

/// The options of a tone setting: `none` (no tone, or the text as it
/// is), then every tone.
const fn tone_options(none: &'static str) -> [&'static str; Tone::ALL.len() + 1] {
    let mut options = [none; Tone::ALL.len() + 1];
    let mut i = 0;
    while i < Tone::ALL.len() {
        options[i + 1] = Tone::ALL[i].id();
        i += 1;
    }
    options
}

/// The options of a role's background, and of its ink and line tone.
static TONES: [&str; Tone::ALL.len() + 1] = tone_options(NONE);
static INKS: [&str; Tone::ALL.len() + 1] = tone_options(AS_IS);

/// The options of a role's line: none, then every kind.
static LINES: [&str; LineKind::ALL.len() + 1] = {
    let mut options = [NONE; LineKind::ALL.len() + 1];
    let mut i = 0;
    while i < LineKind::ALL.len() {
        options[i + 1] = LineKind::ALL[i].id();
        i += 1;
    }
    options
};

const fn tone_value(tone: Option<Tone>, none: &'static str) -> SettingValue {
    SettingValue::Text(Cow::Borrowed(match tone {
        Some(tone) => tone.id(),
        None => none,
    }))
}

const fn line_value(line: Option<LineKind>) -> SettingValue {
    SettingValue::Text(Cow::Borrowed(match line {
        Some(kind) => kind.id(),
        None => NONE,
    }))
}

const fn default_thickness(default: Decoration) -> i64 {
    if default.thickness == 0 {
        THICKNESS as i64
    } else {
        default.thickness as i64
    }
}

/// The aspects of a role, each a setting `<role>.<aspect>`.
const ASPECTS: [&str; 7] = [
    "background",
    "ink",
    "line",
    "line-tone",
    "thickness",
    "frame",
    "struck",
];

/// Adds role `role`: its settings, one per aspect, in the group
/// [`groups::MARKS`] under a section `title` (the role's name, a
/// translation key), with `default` as their defaults. The settings are
/// `<role>.background`, `.ink`, `.line`, `.line-tone`, `.thickness`,
/// `.frame` and `.struck`; themes set their defaults like any setting's.
/// A role known when the plugin is written takes less memory with
/// [`role_settings!`](crate::role_settings) instead.
pub fn add_role(
    r: &mut Registrar,
    role: &Id,
    title: impl Into<Cow<'static, str>>,
    default: Decoration,
) -> Result<(), RegisterError> {
    let title: Cow<'static, str> = title.into();
    let id = |aspect: &str| {
        Id::parse(&format!("{role}.{aspect}")).map_err(|_| RegisterError::Invalid {
            message: Cow::Borrowed("a role's id makes no setting ids"),
        })
    };
    let choice = |aspect, title, options: &'static [&'static str], value| {
        Ok::<_, RegisterError>(SettingSpec::new(
            id(aspect)?,
            title,
            SettingKind::Choice(Choices::Fixed(options)),
            value,
        ))
    };
    let specs = [
        choice(
            ASPECTS[0],
            "core.mark.background",
            &TONES,
            tone_value(default.background, NONE),
        )?,
        choice(
            ASPECTS[1],
            "core.mark.ink",
            &INKS,
            tone_value(default.ink, AS_IS),
        )?,
        choice(
            ASPECTS[2],
            "core.mark.line",
            &LINES,
            line_value(default.line),
        )?,
        choice(
            ASPECTS[3],
            "core.mark.line-tone",
            &INKS,
            tone_value(default.line_tone, AS_IS),
        )?,
        SettingSpec::new(
            id(ASPECTS[4])?,
            "core.mark.thickness",
            SettingKind::Int { min: 1, max: 20 },
            SettingValue::Int(default_thickness(default)),
        )
        .unit("core.unit.tenth-mm"),
        SettingSpec::new(
            id(ASPECTS[5])?,
            "core.mark.frame",
            SettingKind::Bool,
            SettingValue::Bool(default.frame),
        ),
        SettingSpec::new(
            id(ASPECTS[6])?,
            "core.mark.struck",
            SettingKind::Bool,
            SettingValue::Bool(default.struck),
        ),
    ];
    for (spec, aspect) in specs.into_iter().zip(ASPECTS) {
        let help = format!("core.mark.{aspect}.help");
        r.add_setting(
            spec.group(groups::MARKS)
                .section(title.clone())
                .description(help),
        )?;
    }
    Ok(())
}

/// The settings of a role (see [`add_role`]) kept in the program, for
/// [`Registrar::add_settings`]: made by [`role_settings!`](crate::role_settings).
#[doc(hidden)]
pub const fn fixed_role(
    ids: [&'static str; 7],
    section: &'static SettingExtra,
    thickness: &'static SettingExtra,
    default: Decoration,
) -> [SettingSpec; 7] {
    let choice = SettingSpec::fixed;
    let tones = SettingKind::Choice(Choices::Fixed(&TONES));
    let inks = SettingKind::Choice(Choices::Fixed(&INKS));
    let lines = SettingKind::Choice(Choices::Fixed(&LINES));
    [
        choice(
            ids[0],
            "core.mark.background",
            tones,
            tone_value(default.background, NONE),
            groups::MARKS,
        )
        .with_extra(section)
        .described("core.mark.background.help"),
        choice(
            ids[1],
            "core.mark.ink",
            inks,
            tone_value(default.ink, AS_IS),
            groups::MARKS,
        )
        .with_extra(section)
        .described("core.mark.ink.help"),
        choice(
            ids[2],
            "core.mark.line",
            lines,
            line_value(default.line),
            groups::MARKS,
        )
        .with_extra(section)
        .described("core.mark.line.help"),
        choice(
            ids[3],
            "core.mark.line-tone",
            SettingKind::Choice(Choices::Fixed(&INKS)),
            tone_value(default.line_tone, AS_IS),
            groups::MARKS,
        )
        .with_extra(section)
        .described("core.mark.line-tone.help"),
        SettingSpec::fixed_number(
            ids[4],
            "core.mark.thickness",
            1,
            20,
            default_thickness(default),
            groups::MARKS,
        )
        .with_extra(thickness)
        .described("core.mark.thickness.help"),
        SettingSpec::fixed_switch(ids[5], "core.mark.frame", default.frame, groups::MARKS)
            .with_extra(section)
            .described("core.mark.frame.help"),
        SettingSpec::fixed_switch(ids[6], "core.mark.struck", default.struck, groups::MARKS)
            .with_extra(section)
            .described("core.mark.struck.help"),
    ]
}

/// The settings of a role, as [`add_role`] adds them, kept in the
/// program: a `static` list for [`Registrar::add_settings`], which takes
/// next to no memory. The role and its title (a translation key) are
/// written out:
///
/// ```
/// use paperpaw_sdk::SettingSpec;
/// use paperpaw_sdk::decorations::{Decoration, LineKind};
/// static LEARNING: [SettingSpec; 7] = paperpaw_sdk::role_settings!(
///     "my.learning",
///     "my.role.learning",
///     Decoration::NONE.line(LineKind::Dotted)
/// );
/// // In `register`: r.add_settings(&LEARNING)?;
/// ```
#[macro_export]
macro_rules! role_settings {
    ($role:literal, $title:literal, $default:expr $(,)?) => {{
        static SECTION: $crate::SettingExtra = $crate::SettingExtra::new().section($title);
        static THICKNESS: $crate::SettingExtra = $crate::SettingExtra::new()
            .section($title)
            .unit("core.unit.tenth-mm");
        $crate::decorations::fixed_role(
            [
                concat!($role, ".background"),
                concat!($role, ".ink"),
                concat!($role, ".line"),
                concat!($role, ".line-tone"),
                concat!($role, ".thickness"),
                concat!($role, ".frame"),
                concat!($role, ".struck"),
            ],
            &SECTION,
            &THICKNESS,
            $default,
        )
    }};
}

/// The boxes of words as one per line: `words` are the marked words'
/// places in reading order with their index on the page; words next to
/// each other on a line become one box.
pub fn join_boxes(words: impl IntoIterator<Item = (usize, Rect)>) -> Vec<Rect> {
    let mut boxes: Vec<Rect> = Vec::new();
    let mut previous: Option<usize> = None;
    for (index, area) in words {
        match boxes.last_mut() {
            Some(last) if previous == Some(index.wrapping_sub(1)) && last.y == area.y => {
                last.width = area.right().saturating_sub(u32::from(last.x)) as u16;
            }
            _ => boxes.push(area),
        }
        previous = Some(index);
    }
    boxes
}

/// What lies behind the text of `boxes` (one per line): the background.
pub fn draw_behind(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    boxes: &[Rect],
    decoration: &Decoration,
) {
    let Some(tone) = decoration.background else {
        return;
    };
    // A little wider than the words, so a marked word does not touch the
    // edge of its mark.
    let pad = env.metrics.tenth_mm(5).max(1) as i32;
    for b in boxes {
        canvas.fill_shaded(padded(*b, pad), tone.color());
    }
}

/// `b` widened by `pad` on either side, cut at the left edge of the canvas
/// (and the largest width a `Rect` holds) without moving its right side.
fn padded(b: Rect, pad: i32) -> Rect {
    let left = (i32::from(b.x) - pad).max(0);
    let right = (i32::from(b.x) + i32::from(b.width) + pad).min(i32::from(u16::MAX));
    Rect::new(left as u16, b.y, (right - left).max(0) as u16, b.height)
}

/// What is drawn over the text of `boxes` (one per line): the line under
/// it, the frame, the line through it.
pub fn draw_over(canvas: &mut Canvas<'_>, env: &Env<'_>, boxes: &[Rect], decoration: &Decoration) {
    if !decoration.has_strokes() {
        return;
    }
    let tenths = if decoration.thickness == 0 {
        THICKNESS
    } else {
        decoration.thickness
    };
    let t = env.metrics.tenth_mm(u32::from(tenths)).max(1);
    let color = decoration.stroke_color();
    for b in boxes {
        let (x, w) = (i32::from(b.x), u32::from(b.width));
        let bottom = b.bottom() as i32;
        if let Some(kind) = decoration.line {
            draw_line(canvas, kind, x, w, bottom, t, color);
        }
        if decoration.frame {
            let pad = env.metrics.tenth_mm(5).max(1) as i32;
            canvas.stroke_rect(
                x - pad,
                i32::from(b.y),
                w + 2 * pad as u32,
                u32::from(b.height),
                t,
                color,
            );
        }
        if decoration.struck {
            // Through the middle of the small letters.
            let y = i32::from(b.y) + i32::from(b.height) * 3 / 5 - t as i32 / 2;
            canvas.fill_rect(x, y, w, t, color);
        }
    }
}

/// A line of `kind` under text from `x`, `width` wide, its lowest pixels
/// at `bottom - 1`, `t` thick.
fn draw_line(
    canvas: &mut Canvas<'_>,
    kind: LineKind,
    x: i32,
    width: u32,
    bottom: i32,
    t: u32,
    color: Color,
) {
    let ti = t as i32;
    let end = x + width as i32;
    match kind {
        LineKind::Solid => canvas.fill_rect(x, bottom - ti, width, t, color),
        LineKind::Dotted | LineKind::Dashed => {
            let (on, step) = if kind == LineKind::Dotted {
                (ti, 2 * ti)
            } else {
                (3 * ti, 5 * ti)
            };
            let mut at = x;
            while at < end {
                let w = on.min(end - at) as u32;
                canvas.fill_rect(at, bottom - ti, w, t, color);
                at += step.max(1);
            }
        }
        LineKind::Wavy => {
            // Up and down by `t` over a period of four strokes.
            let amplitude = ti.max(1);
            let period = (4 * ti).max(4);
            for column in x..end {
                let phase = (column - x) % period;
                let rise = if phase < period / 2 {
                    phase
                } else {
                    period - phase
                };
                let dy = rise * 2 * amplitude / period;
                canvas.fill_rect(column, bottom - ti - dy, 1, t, color);
            }
        }
        LineKind::Double => {
            canvas.fill_rect(x, bottom - ti, width, t, color);
            canvas.fill_rect(x, bottom - 3 * ti, width, t, color);
        }
    }
}

/// A word of a page given to decorators: its text and where it is in the
/// book. A word split over two lines (hyphenated) is two page words, each
/// with the whole word as its text and its own part's place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageWord<'a> {
    pub text: &'a str,
    pub start: Locator,
    /// Just after its last character.
    pub end: Locator,
}

/// The words of a page, for decorators.
#[derive(Debug, Clone, Copy)]
pub struct PageText<'a> {
    /// The book.
    pub path: &'a str,
    /// Its language, e.g. `de` (empty where unknown).
    pub language: &'a str,
    /// In reading order.
    pub words: &'a [PageWord<'a>],
}

/// A range a decorator marks, with its role.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decorated {
    pub start: Locator,
    /// Just after its last character.
    pub end: Locator,
    pub role: Id,
}

impl Decorated {
    /// The range of `words` (from the first's start to the last's end) in
    /// `role`.
    pub fn words(words: &[PageWord<'_>], role: &Id) -> Option<Decorated> {
        Some(Decorated {
            start: words.first()?.start,
            end: words.last()?.end,
            role: role.clone(),
        })
    }
}

/// What a decorator does: a page's words in, the ranges it marks out.
pub type DecorateFn = Box<dyn Fn(&Env<'_>, &PageText<'_>) -> Vec<Decorated>>;
/// What a decorator's marks depend on besides the page (its data, its
/// settings): readers ask again when it changes.
pub type DecoratorSignatureFn = Box<dyn Fn(&Env<'_>) -> u64>;

/// A decorator contribution (see the [module documentation](self)).
#[non_exhaustive]
pub struct DecoratorSpec {
    pub id: Id,
    /// Translation key of its name.
    pub title: Cow<'static, str>,
    /// Decorators run by order, smallest first; the earlier wins an
    /// aspect two set on the same word.
    pub order: i16,
    /// A setting that switches it on and off, if any.
    pub switch: Option<Id>,
    /// It hears taps on what it marks
    /// ([`events::DECORATION_TAPPED`](crate::builtin::events::DECORATION_TAPPED)).
    pub taps: bool,
    pub signature: DecoratorSignatureFn,
    pub decorate: DecorateFn,
}

impl DecoratorSpec {
    /// Decorator `id`, named `title`, marking with `decorate`; order 0,
    /// always on, no taps, no signature.
    pub fn new(
        id: Id,
        title: impl Into<Cow<'static, str>>,
        decorate: impl Fn(&Env<'_>, &PageText<'_>) -> Vec<Decorated> + 'static,
    ) -> Self {
        Self {
            id,
            title: title.into(),
            order: 0,
            switch: None,
            taps: false,
            signature: Box::new(|_| 0),
            decorate: Box::new(decorate),
        }
    }

    pub fn order(mut self, order: i16) -> Self {
        self.order = order;
        self
    }

    /// Runs only while `switch` is on.
    pub fn switch(mut self, switch: &Setting<bool>) -> Self {
        self.switch = Some(switch.into());
        self
    }

    /// Hears taps on what it marks.
    pub fn taps(mut self) -> Self {
        self.taps = true;
        self
    }

    /// What its marks depend on besides the page. Readers ask for it
    /// whenever they draw a page (on some devices once for every band of
    /// it): it says it from memory, e.g. a count of the changes to its
    /// data, without reading the storage.
    pub fn signature(mut self, signature: impl Fn(&Env<'_>) -> u64 + 'static) -> Self {
        self.signature = Box::new(signature);
        self
    }
}

impl fmt::Debug for DecoratorSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DecoratorSpec")
            .field("id", &self.id)
            .field("order", &self.order)
            .field("taps", &self.taps)
            .finish_non_exhaustive()
    }
}

/// A range of a page that a decorator marked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mark {
    /// The decorator.
    pub decorator: Id,
    pub role: Id,
    pub start: Locator,
    pub end: Locator,
    /// Its decorator hears taps on it.
    pub taps: bool,
}

impl Mark {
    /// Whether it covers some of the text from `start` to `end`.
    pub fn overlaps(&self, start: Locator, end: Locator) -> bool {
        self.start < end && start < self.end
    }
}

/// The registered decorators, by order.
#[derive(Debug, Default)]
pub struct Decorators {
    list: Vec<DecoratorSpec>,
}

impl Decorators {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds `decorator` after those of a smaller or the same order.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn add(&mut self, decorator: DecoratorSpec) {
        let at = self
            .list
            .iter()
            .position(|d| d.order > decorator.order)
            .unwrap_or(self.list.len());
        self.list.insert(at, decorator);
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    /// The decorator `id`, e.g. for its name.
    pub fn get(&self, id: &Id) -> Option<&DecoratorSpec> {
        self.list.iter().find(|d| d.id == *id)
    }

    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn shrink_to_fit(&mut self) {
        self.list.shrink_to_fit();
    }

    fn on(&self, env: &Env<'_>) -> impl Iterator<Item = &DecoratorSpec> {
        self.list.iter().filter(move |d| {
            d.switch
                .as_ref()
                .is_none_or(|s| env.settings.bool(s.as_str()))
        })
    }

    /// What the marks of all decorators depend on besides the page: which
    /// are on and their signatures. Readers ask them again when it
    /// changes.
    pub fn signature(&self, env: &Env<'_>) -> u64 {
        // FNV-1a as `paperpaw_digest::fnv1a64` computes it, fed piece by
        // piece: readers ask on every band, so nothing is allocated.
        const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
        const PRIME: u64 = 0x0000_0100_0000_01b3;
        let feed = |hash: u64, bytes: &[u8]| {
            bytes
                .iter()
                .fold(hash, |h, &b| (h ^ u64::from(b)).wrapping_mul(PRIME))
        };
        self.list.iter().fold(OFFSET, |hash, decorator| {
            let on = decorator
                .switch
                .as_ref()
                .is_none_or(|s| env.settings.bool(s.as_str()));
            let hash = feed(hash, decorator.id.as_str().as_bytes());
            let hash = feed(hash, &[u8::from(on)]);
            if on {
                feed(hash, &(decorator.signature)(env).to_le_bytes())
            } else {
                hash
            }
        })
    }

    /// The marks of `page` from every decorator that is on, by order.
    pub fn decorate(&self, env: &Env<'_>, page: &PageText<'_>) -> Vec<Mark> {
        let mut marks = Vec::new();
        for decorator in self.on(env) {
            for range in (decorator.decorate)(env, page) {
                if range.start < range.end {
                    marks.push(Mark {
                        decorator: decorator.id.clone(),
                        role: range.role,
                        start: range.start,
                        end: range.end,
                        taps: decorator.taps,
                    });
                }
            }
        }
        marks
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backgrounds_keep_their_padding_at_the_edges() {
        // Inside: as wide as the word and a padding on either side.
        assert_eq!(padded(Rect::new(10, 5, 20, 8), 3), Rect::new(7, 5, 26, 8));
        // At the left edge: cut there, the right side where it belongs.
        assert_eq!(padded(Rect::new(1, 5, 20, 8), 3), Rect::new(0, 5, 24, 8));
        // At the end of what a `Rect` holds: no wrap.
        let far = padded(Rect::new(u16::MAX - 4, 0, 4, 8), 3);
        assert_eq!(far.x + far.width, u16::MAX);
    }

    #[test]
    fn tones_and_lines_by_their_ids() {
        for &tone in Tone::ALL {
            assert_eq!(Tone::from_id(tone.id()), Some(tone));
        }
        for &kind in LineKind::ALL {
            assert_eq!(LineKind::from_id(kind.id()), Some(kind));
        }
        assert_eq!(Tone::from_id(NONE), None);
        assert_eq!(Tone::from_id(AS_IS), None);
    }

    #[test]
    fn the_earlier_wins_what_both_set() {
        let first = Decoration::NONE.background(Tone::Yellow).thickness(2);
        let second = Decoration::NONE
            .background(Tone::Gray)
            .line(LineKind::Wavy)
            .thickness(8)
            .frame();
        let both = first.combined(second);
        assert_eq!(both.background, Some(Tone::Yellow));
        assert_eq!(both.line, Some(LineKind::Wavy));
        assert!(both.frame);
        // The thickness goes with the strokes, which came from the second.
        assert_eq!(both.thickness, 8);
        assert_eq!(combine([Decoration::NONE, second]), second);
        assert!(combine([]).is_none());
    }

    #[test]
    fn words_next_to_each_other_join() {
        let boxes = join_boxes([
            (0, Rect::new(0, 0, 10, 5)),
            (1, Rect::new(12, 0, 10, 5)),
            (3, Rect::new(40, 0, 10, 5)),
            (4, Rect::new(0, 8, 10, 5)),
        ]);
        assert_eq!(
            boxes,
            [
                Rect::new(0, 0, 22, 5),
                Rect::new(40, 0, 10, 5),
                Rect::new(0, 8, 10, 5)
            ]
        );
    }
}
