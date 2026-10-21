//! The sizes and looks every part of the UI is drawn with ([`UiStyle`]),
//! the focus, separators and rounded fills.

#[allow(clippy::wildcard_imports)]
use super::*;

/// Sizes of the standard UI elements, in pixels: made for the device and
/// the user's settings with [`UiStyle::new`], or [`UiStyle::PLAIN`] where
/// there is no environment (tests, pictures made beforehand).
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct UiStyle {
    /// Body text size.
    pub text: f32,
    /// Secondary text (status lines, hints).
    pub small: f32,
    /// Titles (e.g. of dialogs and widgets).
    pub title: f32,
    /// The titles of headers (core.ui-title), larger than [`title`] in
    /// some themes.
    ///
    /// [`title`]: UiStyle::title
    pub head: f32,
    /// Height of a list row.
    pub row: u32,
    /// Height of a header bar.
    pub header: u32,
    /// Horizontal margin of content.
    pub margin: u32,
    /// Standard gap between elements.
    pub gap: u32,
    /// Width of lines and borders.
    pub line: u32,
    /// Headers on a dark bar, else on a light one with a line below.
    pub dark_header: bool,
    /// Radius of the corners of highlights, buttons and boxes (0: square).
    pub corner: u32,
    /// Gray text is drawn black (see [`UiStyle::faint`]).
    pub strong: bool,
    /// Buttons, switches, the focus and other small shapes as pills,
    /// their ends round (see [`UiStyle::radius`]), with round or soft
    /// corners; else with [`corner`].
    ///
    /// [`corner`]: UiStyle::corner
    pub pill: bool,
    /// How the focus of rows and items is drawn.
    pub focus: Focus,
    /// The lines between rows, below light headers and above notes.
    pub separators: Separators,
    /// Headers without a bar or a line: the title on the page itself.
    pub plain_header: bool,
    /// The entries of a list's sections on rounded cards.
    pub cards: bool,
    /// The grays the panel shows (2 for black and white alone).
    pub grays: u16,
    /// The least size of what a finger sets or presses, about 7 mm, on
    /// touch screens; 0 with buttons alone (see [`UiStyle::touch_row`]).
    pub finger: u32,
    /// The look a plugin gave the parts (see [`look`](super::look)).
    pub look: super::look::Look,
    /// The grays of the interface (see [`UiStyle::ink`]).
    pub palette: Palette,
}

/// What a colour is for ([`UiStyle::ink`]): the grays of the interface,
/// each set by the palette (setting `core.ui-palette`), so a theme
/// changes them all at once and plugins draw in the same grays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Ink {
    /// Text and what is drawn in its colour.
    Text,
    /// What the interface is drawn on.
    Paper,
    /// Notes below the titles of rows, what an empty screen explains.
    Note,
    /// Headings of sections, a widget's lesser text.
    Muted,
    /// The least of text, e.g. what an empty screen says.
    Faint,
    /// What cannot be used now, e.g. an arrow at the last page.
    Disabled,
    /// The line below a light header or above the row of button hints.
    Rule,
    /// The lines between rows.
    Line,
    /// The fainter lines between rows that start at the text.
    LineFaint,
    /// Gray text on a dark focus.
    OnFocusMuted,
    /// Lesser text on a dark card.
    OnDarkMuted,
    /// Cards, e.g. the frames of widgets.
    Card,
    /// Cards on panels of black and white alone, drawn as a pattern.
    CardPattern,
    /// The cards a list's sections are on.
    Section,
}

impl Ink {
    /// Every role, in the order of [`Palette`]'s values.
    pub const ALL: [Ink; 14] = [
        Ink::Text,
        Ink::Paper,
        Ink::Note,
        Ink::Muted,
        Ink::Faint,
        Ink::Disabled,
        Ink::Rule,
        Ink::Line,
        Ink::LineFaint,
        Ink::OnFocusMuted,
        Ink::OnDarkMuted,
        Ink::Card,
        Ink::CardPattern,
        Ink::Section,
    ];

    /// Its name in the palette, e.g. `muted`.
    pub const fn name(self) -> &'static str {
        match self {
            Ink::Text => "text",
            Ink::Paper => "paper",
            Ink::Note => "note",
            Ink::Muted => "muted",
            Ink::Faint => "faint",
            Ink::Disabled => "disabled",
            Ink::Rule => "rule",
            Ink::Line => "line",
            Ink::LineFaint => "line-faint",
            Ink::OnFocusMuted => "on-focus-muted",
            Ink::OnDarkMuted => "on-dark-muted",
            Ink::Card => "card",
            Ink::CardPattern => "card-pattern",
            Ink::Section => "section",
        }
    }

    /// Text, which strong contrast draws black.
    const fn is_text(self) -> bool {
        matches!(self, Ink::Note | Ink::Muted | Ink::Faint)
    }
}

/// The gray of each [`Ink`] (0 black, 255 white), read from the setting
/// `core.ui-palette`: names and values separated by spaces, e.g.
/// `muted=60 line=170`; what it leaves out has its default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette([u8; 14]);

impl Palette {
    /// The interface's own grays.
    pub const DEFAULT: Palette = Palette([
        0, 255, 60, 90, 120, 150, 150, 190, 210, 215, 200, 225, 240, 238,
    ]);

    /// The default with what `text` sets; names it does not know and bad
    /// values are left out.
    pub fn parse(text: &str) -> Palette {
        let mut palette = Palette::DEFAULT;
        for pair in text.split_whitespace() {
            let Some((name, value)) = pair.split_once('=') else {
                continue;
            };
            let (Some(at), Ok(value)) = (
                Ink::ALL.iter().position(|ink| ink.name() == name),
                value.parse::<u8>(),
            ) else {
                continue;
            };
            palette.0[at] = value;
        }
        palette
    }

    /// The gray of `ink`.
    pub fn gray(&self, ink: Ink) -> u8 {
        let at = Ink::ALL.iter().position(|i| *i == ink).unwrap_or(0);
        self.0[at]
    }
}

impl Default for Palette {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// What a text is (see [`UiStyle::size`]): its size follows from the
/// style's, so text of one kind is as large on every screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum TextRole {
    /// Smaller than small, e.g. under an icon.
    Caption,
    /// Hints, notes, status lines.
    Small,
    /// Most text.
    Body,
    /// Titles of dialogs and widgets.
    Title,
    /// The titles of headers.
    Head,
    /// Large, e.g. a widget's main line.
    Headline,
    /// Larger, e.g. a clock.
    Display,
    /// Large numbers standing alone, e.g. a page count.
    Numeral,
}

/// How the focus of rows and items is drawn ([`draw_focus`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Focus {
    /// A black bar, what is on it white.
    Fill,
    /// A thick round frame, what is in it black.
    Ring,
    /// A light gray bar, what is on it black; a frame on panels of black
    /// and white alone.
    Soft,
}

/// The lines between rows ([`draw_separator`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Separators {
    /// Light lines within the margins.
    Line,
    /// Fainter lines, from where the text starts.
    Inset,
    /// None: room alone parts the rows.
    None,
}

impl UiStyle {
    /// Fixed sizes for text 20 pixels high, square corners, lines between
    /// rows, the focus a dark bar, two grays: what no device or setting
    /// changes. Its fields can be changed as any style's.
    pub const PLAIN: UiStyle = UiStyle {
        text: 20.0,
        small: 15.0,
        title: 23.0,
        head: 23.0,
        row: 60,
        header: 64,
        margin: 12,
        gap: 8,
        line: 2,
        dark_header: false,
        corner: 0,
        strong: false,
        pill: false,
        focus: Focus::Fill,
        separators: Separators::Line,
        plain_header: false,
        cards: false,
        grays: 2,
        finger: 0,
        look: super::look::Look::none(),
        palette: Palette::DEFAULT,
    };

    pub fn new(env: &Env<'_>) -> Self {
        let m = env.metrics;
        let base = env.settings.read(&settings::UI_SIZE).clamp(6, 40) as f32;
        let text = m.pt(base);
        let line_height = env
            .fonts
            .role(fonts::UI.as_str())
            .map_or(text * 1.25, |f| f.line_metrics(text).line_height);
        let s = env.settings;
        let rows = s.read(&settings::UI_ROW).clamp(120, 300) as f32 / 100.0;
        Self {
            text,
            small: m.pt(base * 0.75),
            title: m.pt(base * 1.15),
            head: m.pt(base * s.read(&settings::UI_TITLE).clamp(100, 200) as f32 / 100.0),
            row: (line_height * rows) as u32,
            header: (line_height * (rows + 0.1)) as u32,
            margin: m.mm(s.read(&settings::UI_MARGIN).clamp(0, 15) as u32),
            gap: m.mm(2),
            line: m
                .tenth_mm(s.read(&settings::UI_LINES).clamp(1, 10) as u32)
                .max(1),
            dark_header: s.text(settings::UI_HEADER.as_str()) == settings::HEADER_DARK,
            corner: match settings::CORNERS
                .iter()
                .position(|c| *c == s.text(settings::UI_CORNERS.as_str()))
            {
                Some(1) => m.tenth_mm(8),
                Some(2) => m.mm(2),
                Some(3) => m.tenth_mm(35),
                _ => 0,
            },
            strong: s.text(settings::UI_CONTRAST.as_str()) == settings::CONTRAST_STRONG,
            pill: settings::CORNERS[2..].contains(&s.text(settings::UI_CORNERS.as_str())),
            focus: match s.text(settings::UI_FOCUS.as_str()) {
                settings::FOCUS_RING => Focus::Ring,
                settings::FOCUS_SOFT => Focus::Soft,
                _ => Focus::Fill,
            },
            separators: match s.text(settings::UI_SEPARATORS.as_str()) {
                settings::SEPARATORS_INSET => Separators::Inset,
                settings::SEPARATORS_NONE => Separators::None,
                _ => Separators::Line,
            },
            plain_header: s.text(settings::UI_HEADER.as_str()) == settings::HEADER_PLAIN,
            cards: s.text(settings::UI_GROUPS.as_str()) == settings::GROUPS_CARDS,
            grays: env.device.display.format.gray_levels().unwrap_or(256),
            finger: if env.device.input.touch { m.mm(7) } else { 0 },
            look: env.look,
            palette: Palette::parse(s.text(settings::UI_PALETTE.as_str())),
        }
    }

    /// The radius of the corners of a shape `height` pixels high: half
    /// its height for pills, else the style's corners.
    /// Taller shapes (a row with a note) keep the corners, at most half a
    /// margin round: the room between their edge and their text, so the
    /// curve never comes near what is in them.
    pub fn radius(&self, height: u32) -> u32 {
        if self.pill && height <= self.row * 5 / 4 {
            height / 2
        } else if height > self.row * 5 / 4 {
            self.corner.min(self.margin / 2)
        } else {
            self.corner
        }
    }

    /// Whether the focus is drawn dark, so what is on it is drawn white.
    pub fn dark_focus(&self) -> bool {
        self.focus == Focus::Fill
    }

    /// The ink of what is drawn on a row or item, `focused` or not:
    /// white on a dark focus, else black.
    pub fn focus_ink(&self, focused: bool) -> Color {
        if focused && self.dark_focus() {
            Color::WHITE
        } else {
            Color::BLACK
        }
    }

    /// The paper of a row or item, `focused` or not: what its content may
    /// be cleared to (black on a dark focus).
    pub fn focus_paper(&self, focused: bool) -> Color {
        if focused && self.dark_focus() {
            Color::BLACK
        } else {
            Color::WHITE
        }
    }

    /// The colour of gray text (notes) on a row or item, `focused` or not
    /// (light on a dark focus).
    pub fn focus_faint(&self, focused: bool, gray: u8) -> Color {
        if focused && self.dark_focus() {
            self.ink(Ink::OnFocusMuted)
        } else {
            self.faint(gray)
        }
    }

    /// A row or bar of `height` for buttons: on touch screens as much
    /// higher as its buttons ([`button_height`]) grow to be as large as a
    /// fingertip ([`UiStyle::finger`]), so that the room above and below
    /// them stays what it is in a row of `height`; as it is with buttons
    /// alone. Every row or bar that holds buttons is made with it.
    pub fn touch_row(&self, height: u32) -> u32 {
        height.max(self.finger + (height - height * 7 / 10))
    }

    /// The colour of `ink` in the palette; gray text (notes, headings,
    /// what cannot be used) black for strong contrast.
    pub fn ink(&self, ink: Ink) -> Color {
        if self.strong && ink.is_text() {
            Color::BLACK
        } else {
            Color::gray(self.palette.gray(ink))
        }
    }

    /// The size of text of `role`.
    pub fn size(&self, role: TextRole) -> f32 {
        match role {
            TextRole::Caption => self.small * 0.8,
            TextRole::Small => self.small,
            TextRole::Body => self.text,
            TextRole::Title => self.title,
            TextRole::Head => self.head,
            TextRole::Headline => self.title * 1.6,
            TextRole::Display => self.title * 2.0,
            TextRole::Numeral => self.title * 3.0,
        }
    }

    /// The colour of gray text such as hints and notes: `gray`, or black
    /// for strong contrast.
    pub fn faint(&self, gray: u8) -> Color {
        if self.strong {
            Color::BLACK
        } else {
            Color::gray(gray)
        }
    }

    /// A header's background and text: black and white, or the other way
    /// round for light headers.
    pub fn header_colors(&self) -> (Color, Color) {
        if self.dark_header {
            (Color::BLACK, Color::WHITE)
        } else {
            (Color::WHITE, Color::BLACK)
        }
    }

    /// Draws a header's background in `area`: dark, or light with a thin
    /// line below (as the separators are; none for plain headers).
    pub(super) fn header_background(&self, canvas: &mut Canvas<'_>, area: Rect) {
        let (paper, ink) = self.header_colors();
        canvas.fill(area, paper);
        if self.dark_header || self.plain_header {
            return;
        }
        let line = self.line.max(1) as u16;
        let y = (area.y + area.height).saturating_sub(line);
        match self.separators {
            Separators::Line => canvas.fill(Rect::new(area.x, y, area.width, line), ink),
            Separators::Inset => {
                let margin = self.margin as u16;
                let width = area.width.saturating_sub(2 * margin);
                canvas.fill_tint(
                    Rect::new(area.x + margin, y, width, line),
                    self.ink(Ink::Rule),
                );
            }
            Separators::None => {}
        }
    }

    /// The part of a header's `area` its title and signs are centred in:
    /// all of it, but the line below a light header.
    pub(super) fn header_band(&self, area: Rect) -> Rect {
        let lined = !self.dark_header && !self.plain_header && self.separators != Separators::None;
        let line = if lined { self.line.max(1) as u16 } else { 0 };
        Rect::new(area.x, area.y, area.width, area.height.saturating_sub(line))
    }

    /// The UI font.
    pub fn font<'a>(&self, env: &Env<'a>) -> Option<FontHandle<'a>> {
        env.fonts.role(fonts::UI.as_str())
    }

    /// Height of a row with a note: the title line, then up to
    /// [`NOTE_LINES`] lines of small text (see [`draw_noted_row`]).
    pub fn noted_row(&self) -> u32 {
        self.noted_row_of(NOTE_LINES)
    }

    /// Height of a row with a note of `lines` lines (e.g. 1 for denser
    /// lists of many entries): a title line, then the note.
    pub fn noted_row_of(&self, lines: usize) -> u32 {
        let notes = (lines as f32 * self.small * NOTE_LINE_HEIGHT) as u32;
        self.title_band() + notes + self.gap * 3 / 2
    }

    /// Height of the title line of a noted row.
    pub(super) fn title_band(&self) -> u32 {
        self.row * 3 / 4
    }
}

/// Where the focus of a row or an item in `rect` is drawn: a little in
/// from the sides, so it sits within the screen's margins as a rounded
/// bar, and off the lines between rows.
pub fn focus_rect(style: &UiStyle, rect: Rect) -> Rect {
    let side = (style.margin / 2) as u16;
    let edge = (style.line as u16).min(rect.height / 8);
    rect.inset(side.min(rect.width / 8), edge)
}

/// Draws the focus of the row or item in `rect` (see [`focus_rect`]):
/// black, for content drawn white on it. Every list draws its focus this
/// way, so it looks the same everywhere.
pub fn draw_focus(canvas: &mut Canvas<'_>, style: &UiStyle, rect: Rect) {
    let r = focus_rect(style, rect);
    if style
        .look
        .focus(canvas, style, super::look::FocusPart { rect: r })
    {
        return;
    }
    // Items taller than wide (a cover) keep the corners.
    let radius = if r.width < r.height {
        style.corner
    } else {
        style.radius(u32::from(r.height))
    };
    let ring = (style.line * 2).max(3);
    match style.focus {
        Focus::Fill => canvas.fill_round(r, radius, Color::BLACK),
        Focus::Ring => canvas.stroke_round(r, ring, radius, Color::BLACK),
        Focus::Soft if style.grays > 2 => {
            // As light as the panel shows with black text reading well.
            let gray = if style.grays >= 16 { 215 } else { 170 };
            canvas.fill_round(r, radius, Color::gray(gray));
        }
        Focus::Soft => canvas.stroke_round(r, ring, radius, Color::BLACK),
    }
}

/// Draws the line below the row in `rect` as the style's separators
/// are: `indent` is where the row's text starts after the margin (e.g.
/// right of an icon), where faint lines start.
pub fn draw_separator(canvas: &mut Canvas<'_>, style: &UiStyle, rect: Rect, indent: u16) {
    let part = super::look::SeparatorPart { row: rect, indent };
    if style.look.separator(canvas, style, part) {
        return;
    }
    let margin = style.margin as u16;
    let line = style.line as u16;
    let y = rect.y + rect.height - line;
    match style.separators {
        Separators::Line => canvas.fill_tint(
            Rect::new(
                rect.x + margin,
                y,
                rect.width.saturating_sub(2 * margin),
                line,
            ),
            style.ink(Ink::Line),
        ),
        Separators::Inset => {
            let start = margin + indent;
            let thin = (line / 2).max(1);
            canvas.fill_tint(
                Rect::new(
                    rect.x + start,
                    y,
                    rect.width.saturating_sub(start + margin),
                    thin,
                ),
                style.ink(Ink::LineFaint),
            );
        }
        Separators::None => {}
    }
}

/// Fills a rectangle with the style's corners.
pub fn fill_round(canvas: &mut Canvas<'_>, style: &UiStyle, area: Rect, color: Color) {
    canvas.fill_round(area, style.corner, color);
}

/// Draws a box's frame, `thickness` wide, with the style's corners.
pub fn stroke_round(
    canvas: &mut Canvas<'_>,
    style: &UiStyle,
    area: Rect,
    thickness: u32,
    color: Color,
) {
    canvas.stroke_round(area, thickness, style.corner, color);
}

/// Fills a rectangle.
#[deprecated(since = "0.4.0", note = "use `Canvas::fill`")]
pub fn fill(canvas: &mut Canvas<'_>, area: Rect, color: Color) {
    canvas.fill(area, color);
}
