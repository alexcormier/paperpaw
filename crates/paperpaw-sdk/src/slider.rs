//! Sliders: numbers better set by eye than by their value, such as the
//! line spacing or a light's brightness. A track filled up to the value,
//! with a knob there, between a `−` and a `+`; its title and value (with
//! its unit) on the line above, as in other rows.
//!
//! Equally easy by touch and with buttons alone:
//!
//! - Touch: a finger that comes down on the track takes the knob along
//!   ([`SliderDrag`]); the value follows it in the slider's steps, and
//!   lifting the finger chooses it. A tap on the track chooses the value
//!   there, a tap on `−` or `+` moves it one step.
//! - Buttons: on a focused slider, *confirm* starts moving it; left and
//!   right (or up and down, or the page buttons) move it one step each
//!   ([`key_step`]), *confirm* chooses the value, *back* goes back to the
//!   one before. The `−` and `+` are filled while the buttons move it.
//!
//! While a value moves it is best shown without being kept
//! ([`Command::PreviewSettings`](crate::Command::PreviewSettings) for a
//! setting), so that what it changes shows at once; it is kept when
//! chosen.
//!
//! Sliders are for touch screens, where a finger sets a value by eye;
//! with buttons alone a number is changed a step at a time all the same,
//! and a compact row with `−` and `+` serves it better ([`shows`]).
//!
//! A screen keeps a [`Slider`] for each (from a setting with
//! [`Slider::of`]), draws rows of [`row_height`] with
//! [`Slider::draw_row`] (what it shows in a [`SliderView`]), or only the
//! track with [`Slider::draw_track`] where it places the buttons itself,
//! and passes pointer samples to [`SliderDrag::on_pointer`] from
//! [`Screen::on_pointer`](crate::Screen::on_pointer).

use crate::builtin::actions;
use crate::id::Id;
use crate::semantics::Role;
use crate::settings::{SettingKind, SettingSpec};
use crate::ui::{
    UiStyle, button_height, draw_button, draw_focused_steps, draw_note_below, draw_value_row,
    title_area,
};
use crate::{Canvas, Color, Env, PointerEvent, PointerPhase, Rect};

/// A number from `min` to `max` in steps of `step`, set with a slider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Slider {
    pub min: i64,
    /// At least `min`.
    pub max: i64,
    /// At least 1.
    pub step: i64,
}

/// How a slider shows the focus (see [`Slider::draw_row`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SliderFocus {
    /// Not focused: black on white.
    #[default]
    None,
    /// Focused, on the focus bar ([`draw_focus`](crate::ui::draw_focus)):
    /// white on black.
    Focused,
    /// Focused and moved by the buttons: its `−` and `+` filled.
    Editing,
}

/// What a slider shows and where (see [`Slider::draw_row`] and
/// [`Slider::draw_band`]): made with [`SliderView::new`] and the builder
/// methods, so that it can grow without breaking plugins.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct SliderView<'a> {
    /// The row (for [`Slider::draw_row`]) or the band
    /// ([`Slider::draw_band`]).
    pub area: Rect,
    pub value: i64,
    /// The value as shown, with its unit (e.g. "135 %").
    pub shown: &'a str,
    /// The title on the row's first line; empty for none.
    pub title: &'a str,
    /// A short help text below it; empty for none.
    pub note: &'a str,
    pub focus: SliderFocus,
}

impl<'a> SliderView<'a> {
    /// `value` in `area`, shown as `shown`; no title or note, not
    /// focused.
    pub fn new(area: Rect, value: i64, shown: &'a str) -> Self {
        Self {
            area,
            value,
            shown,
            title: "",
            note: "",
            focus: SliderFocus::None,
        }
    }

    pub fn title(self, title: &'a str) -> Self {
        Self { title, ..self }
    }

    pub fn note(self, note: &'a str) -> Self {
        Self { note, ..self }
    }

    pub fn focus(self, focus: SliderFocus) -> Self {
        Self { focus, ..self }
    }
}

/// Where the parts of a slider are in its band (see [`parts`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SliderParts {
    pub minus: Rect,
    pub track: Rect,
    pub plus: Rect,
}

impl Slider {
    /// From `min` to `max` (no less than `min`) in steps of 1.
    pub const fn new(min: i64, max: i64) -> Self {
        Self {
            min,
            max: if max < min { min } else { max },
            step: 1,
        }
    }

    /// In steps of `step` (at least 1) from `min`.
    pub const fn step(self, step: i64) -> Self {
        Self {
            step: if step < 1 { 1 } else { step },
            ..self
        }
    }

    /// The slider of a number setting: its range and step; `None` for
    /// settings of other kinds.
    pub fn of(spec: &SettingSpec) -> Option<Self> {
        match spec.kind {
            SettingKind::Int { min, max } => Some(Self::new(min, max).step(spec.extra().step)),
            _ => None,
        }
    }

    /// `value` within the range.
    pub fn clamp(&self, value: i64) -> i64 {
        value.clamp(self.min, self.max)
    }

    /// `value` moved by `steps` steps, within the range.
    pub fn stepped(&self, value: i64, steps: i64) -> i64 {
        self.clamp(value.saturating_add(steps.saturating_mul(self.step)))
    }

    /// How far along the track `value` is, from 0 to 1.
    pub fn share(&self, value: i64) -> f32 {
        let range = (self.max - self.min).max(1) as f32;
        ((self.clamp(value) - self.min) as f32 / range).clamp(0.0, 1.0)
    }

    /// The value for a finger at `x` on `track` (where the knob's middle
    /// would be): the nearest step, the ends past the track's ends.
    pub fn value_at(&self, track: Rect, x: i32) -> i64 {
        let knob = i32::from(knob_size(track));
        let left = i32::from(track.x) + knob / 2;
        let width = (i32::from(track.width) - knob).max(1);
        let share = ((x - left) as f32 / width as f32).clamp(0.0, 1.0);
        if share >= 1.0 {
            return self.max;
        }
        let steps = ((self.max - self.min) as f32 * share / self.step as f32 + 0.5) as i64;
        self.clamp(self.min + steps * self.step)
    }

    /// Draws the track in `track`: a thin line filled with `ink` up to
    /// `value`, lighter after it, and the knob at the value (round where
    /// the interface has round corners). For screens that place a
    /// slider's buttons themselves, as panels do; [`Slider::draw_row`]
    /// draws all of it.
    pub fn draw_track(
        &self,
        canvas: &mut Canvas<'_>,
        style: &UiStyle,
        track: Rect,
        value: i64,
        ink: Color,
    ) {
        let knob = knob_size(track);
        let line = ((style.line * 2).max(2) as u16).min(knob / 2).max(1);
        let mid = track.y + track.height / 2;
        let left = track.x + knob / 2;
        let width = track.width.saturating_sub(knob);
        let filled = ((f32::from(width) * self.share(value)) + 0.5) as u16;
        // After the value thinner and lighter: gray on white (dithered on
        // panels of black and white alone), a thin line on the focus.
        let rest = Rect::new(
            left + filled,
            mid.saturating_sub(line / 2),
            width - filled,
            line,
        );
        if ink == Color::BLACK {
            canvas.fill_tint(rest, Color::gray(170));
        } else {
            let thin = (line / 2).max(1);
            let rest = Rect::new(rest.x, mid.saturating_sub(thin / 2), rest.width, thin);
            canvas.fill(rest, Color::gray(200));
        }
        canvas.fill(
            Rect::new(left, mid.saturating_sub(line), filled, line * 2),
            ink,
        );
        let (x, y) = (
            i32::from(left + filled) - i32::from(knob / 2),
            i32::from(mid) - i32::from(knob / 2),
        );
        let size = u32::from(knob);
        if style.corner > 0 {
            canvas.fill_round_rect(x, y, size, size, size / 2, ink);
        } else {
            canvas.fill_rect(x, y, size, size, ink);
        }
    }

    /// Draws a slider's band, `view.area` (see [`band`]): `−`, the track
    /// at the value and `+`, as its focus says; the value as shown tells
    /// screen readers what it is set to.
    pub fn draw_band(
        &self,
        canvas: &mut Canvas<'_>,
        env: &Env<'_>,
        style: &UiStyle,
        view: &SliderView<'_>,
    ) {
        let band = view.area;
        let focus = view.focus;
        let SliderParts { minus, track, plus } = parts(style, band);
        let ink = match focus {
            SliderFocus::None => {
                draw_button(canvas, env, style, minus, "−", false, false);
                draw_button(canvas, env, style, plus, "+", false, false);
                Color::BLACK
            }
            SliderFocus::Focused | SliderFocus::Editing => {
                if let Some(font) = style.font(env) {
                    let editing = focus == SliderFocus::Editing;
                    draw_focused_steps(canvas, style, &font, band, (minus, plus), editing);
                }
                Color::WHITE
            }
        };
        canvas.describe(
            Role::Slider,
            view.shown,
            track,
            focus != SliderFocus::None,
            None,
        );
        self.draw_track(canvas, style, track, view.value, ink);
    }

    /// Draws a row of [`row_height`] with a slider, `view.area`: the
    /// title and the value on its first line, as other rows have them,
    /// the slider's band below, and the note below that (if the view has
    /// one; the row is then as high as `row_height(style, true)` says). A
    /// focused row is drawn white, for the focus bar below it.
    pub fn draw_row(
        &self,
        canvas: &mut Canvas<'_>,
        env: &Env<'_>,
        style: &UiStyle,
        view: &SliderView<'_>,
    ) {
        let row = view.area;
        let focused = view.focus != SliderFocus::None;
        let title = title_area(style, row);
        draw_value_row(canvas, env, style, title, focused, view.title, view.shown);
        let band = band(style, row);
        let band_view = SliderView {
            area: band,
            ..*view
        };
        self.draw_band(canvas, env, style, &band_view);
        if !view.note.is_empty() {
            let below = band.y + band.height;
            draw_note_below(canvas, env, style, row, below, focused, view.note);
        }
    }
}

/// Whether `spec` is shown as a slider here: a number marked so
/// ([`SettingExtra::slider`](crate::SettingExtra::slider)), on a touch
/// screen. With buttons alone it keeps its compact row with `−` and `+`.
pub fn shows(env: &Env<'_>, spec: &SettingSpec) -> bool {
    env.device.input.touch && marked(spec)
}

/// Whether `spec` is a number marked to be set with a slider, wherever
/// one is shown ([`shows`]).
pub fn marked(spec: &SettingSpec) -> bool {
    matches!(spec.kind, SettingKind::Int { .. }) && spec.extra().slider
}

/// The size of the knob on `track`.
fn knob_size(track: Rect) -> u16 {
    (track.height / 2).max(4).min(track.width.max(1))
}

/// The height of a slider's band: as the first line of a row.
pub fn band_height(style: &UiStyle) -> u16 {
    style.title_band() as u16
}

/// The height of a row with a slider (see [`Slider::draw_row`]), with
/// room for a note if `noted`.
pub fn row_height(style: &UiStyle, noted: bool) -> u16 {
    let band = u32::from(band_height(style));
    let height = if noted {
        style.noted_row() + band
    } else {
        style.title_band() + band + style.gap * 3 / 2
    };
    height as u16
}

/// The slider's band in a row of [`row_height`]: below its first line.
pub fn band(style: &UiStyle, row: Rect) -> Rect {
    let title = title_area(style, row);
    let top = title.y + title.height;
    let height = band_height(style).min(row.bottom().saturating_sub(u32::from(top)) as u16);
    Rect::new(row.x, top, row.width, height)
}

/// Where `−`, the track and `+` are in `band`: the buttons as large as
/// other rows' and flush with the margins (where titles start and values
/// end), the track between them.
pub fn parts(style: &UiStyle, band: Rect) -> SliderParts {
    let size = button_height(band);
    let y = band.y + (band.height - size) / 2;
    let margin = style.margin as u16;
    let gap = style.gap as u16;
    let minus = Rect::new(band.x + margin, y, size, size);
    let plus = Rect::new(
        (band.x + band.width).saturating_sub(margin + size),
        y,
        size,
        size,
    );
    let start = minus.x + size + gap;
    let track = Rect::new(start, y, plus.x.saturating_sub(start + gap), size);
    SliderParts { minus, track, plus }
}

/// How many steps a button moves a slider (or a number) being changed
/// with the buttons: one down for left, down and the buttons back, one
/// up for right, up and the buttons forward; `None` for other actions.
pub fn key_step(action: &Id) -> Option<i64> {
    let down = [
        actions::LEFT,
        actions::DOWN,
        actions::PREV_PAGE,
        actions::PREVIOUS,
    ];
    let up = [
        actions::RIGHT,
        actions::UP,
        actions::NEXT_PAGE,
        actions::NEXT,
    ];
    if down.contains(action) {
        Some(-1)
    } else if up.contains(action) {
        Some(1)
    } else {
        None
    }
}

/// A knob being dragged (see the [module documentation](self)).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SliderDrag {
    drag: Option<Drag>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Drag {
    pointer: u8,
    value: i64,
}

/// What a pointer sample did to a slider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SliderStep {
    /// The knob was taken or moved to this value: show it, not kept yet.
    Moved(i64),
    /// The finger moved within the same step: nothing to draw.
    Same,
    /// The finger was lifted here: keep this value.
    Chosen(i64),
    /// The finger was taken away (e.g. by the system): show the value
    /// kept again.
    Cancelled,
}

impl SliderDrag {
    /// The value the knob is dragged to, while it is.
    pub fn dragging(&self) -> Option<i64> {
        self.drag.map(|d| d.value)
    }

    /// Follows a finger on `slider`, drawn on `track`: one that comes
    /// down on `track` takes the knob along. `None` for samples it does
    /// not take: the screen then handles them as it would.
    pub fn on_pointer(
        &mut self,
        event: &PointerEvent,
        slider: &Slider,
        track: Rect,
    ) -> Option<SliderStep> {
        let at = |event: &PointerEvent| slider.value_at(track, event.point.x);
        match (event.phase, self.drag) {
            (PointerPhase::Down, None) => {
                if !track.contains(event.point) {
                    return None;
                }
                let value = at(event);
                self.drag = Some(Drag {
                    pointer: event.id,
                    value,
                });
                Some(SliderStep::Moved(value))
            }
            (PointerPhase::Move, Some(drag)) if drag.pointer == event.id => {
                let value = at(event);
                if value == drag.value {
                    return Some(SliderStep::Same);
                }
                self.drag = Some(Drag { value, ..drag });
                Some(SliderStep::Moved(value))
            }
            (PointerPhase::Up, Some(drag)) if drag.pointer == event.id => {
                self.drag = None;
                Some(SliderStep::Chosen(at(event)))
            }
            (PointerPhase::Cancel, Some(drag)) if drag.pointer == event.id => {
                self.drag = None;
                Some(SliderStep::Cancelled)
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Point, PointerTool};
    use alloc::borrow::Cow;
    use paperpaw_graphics::Framebuffer;
    use paperpaw_platform::PixelFormat;

    const STYLE: UiStyle = UiStyle {
        text: 20.0,
        small: 15.0,
        title: 23.0,
        row: 60,
        header: 64,
        margin: 12,
        gap: 8,
        line: 2,
        dark_header: false,
        corner: 0,
        strong: false,
    };

    fn pointer(phase: PointerPhase, x: i32, y: i32) -> PointerEvent {
        PointerEvent {
            id: 0,
            phase,
            point: Point::new(x, y),
            tool: PointerTool::Finger,
            pressure: 1000,
        }
    }

    #[test]
    fn a_tap_sets_the_value_where_it_lands() {
        // Knob 15 wide: the track runs from 107 to 192.
        let slider = Slider::new(0, 255);
        let track = Rect::new(100, 0, 100, 30);
        assert_eq!(slider.value_at(track, 50), 0);
        assert_eq!(slider.value_at(track, 107), 0);
        assert_eq!(slider.value_at(track, 150), 129);
        assert_eq!(slider.value_at(track, 192), 255);
        assert_eq!(slider.value_at(track, 400), 255);
    }

    #[test]
    fn values_keep_to_the_steps_and_the_range() {
        // 100 to 250 in fives: 31 places.
        let slider = Slider::new(100, 250).step(5);
        let track = Rect::new(0, 0, 330, 30);
        for x in (0..400).step_by(7) {
            let value = slider.value_at(track, x);
            assert!((100..=250).contains(&value), "{value}");
            assert_eq!((value - 100) % 5, 0, "{value}");
        }
        // The end is reached even where the range is not whole steps.
        let odd = Slider::new(0, 12).step(5);
        assert_eq!(odd.value_at(track, 400), 12);
        assert_eq!(odd.stepped(10, 1), 12);
        assert_eq!(odd.stepped(0, -1), 0);
        assert_eq!(slider.stepped(135, 1), 140);
        assert_eq!(slider.stepped(135, -100), 100);
        // Made right from wrong numbers.
        assert_eq!(Slider::new(5, 1), Slider::new(5, 5));
        assert_eq!(Slider::new(0, 9).step(0).step, 1);
        assert_eq!(Slider::new(3, 3).share(3), 0.0);
    }

    #[test]
    fn settings_give_their_range_and_step() {
        let spec = crate::SettingSpec::number(
            &crate::setting::Setting::new("a.spacing"),
            "a.spacing",
            100,
            250,
            135,
        )
        .step(5);
        assert_eq!(Slider::of(&spec), Some(Slider::new(100, 250).step(5)));
        assert!(!marked(&spec));
        assert!(marked(&spec.clone().slider()));
        let switch = crate::SettingSpec::switch(
            &crate::setting::Setting::new("a.on"),
            Cow::Borrowed("a.on"),
            true,
        );
        assert_eq!(Slider::of(&switch), None);
        assert!(!marked(&switch.slider()));
    }

    #[test]
    fn its_parts_keep_to_the_margins() {
        let row = Rect::new(0, 100, 480, row_height(&STYLE, true));
        let band = band(&STYLE, row);
        assert_eq!(band.y, title_area(&STYLE, row).bottom() as u16);
        assert!(band.bottom() <= row.bottom());
        let SliderParts { minus, track, plus } = parts(&STYLE, band);
        // Flush with the margins, where titles start and values end.
        assert_eq!(minus.x, 12);
        assert_eq!(plus.right(), 480 - 12);
        assert_eq!(minus.height, button_height(band));
        assert_eq!(track.x, minus.right() as u16 + 8);
        assert_eq!(track.right() + 8, u32::from(plus.x));
        // A row with a note is higher by the note's lines.
        assert!(row_height(&STYLE, true) > row_height(&STYLE, false));
    }

    #[test]
    fn a_finger_takes_the_knob_along() {
        let slider = Slider::new(0, 10);
        let track = Rect::new(100, 50, 220, 20);
        let mut drag = SliderDrag::default();
        // Not on the track: left to the screen.
        assert_eq!(
            drag.on_pointer(&pointer(PointerPhase::Down, 50, 60), &slider, track),
            None
        );
        let down = drag.on_pointer(&pointer(PointerPhase::Down, 110, 60), &slider, track);
        assert_eq!(down, Some(SliderStep::Moved(0)));
        assert_eq!(drag.dragging(), Some(0));
        // A little further, the same step; then the middle.
        let near = drag.on_pointer(&pointer(PointerPhase::Move, 112, 90), &slider, track);
        assert_eq!(near, Some(SliderStep::Same));
        let middle = drag.on_pointer(&pointer(PointerPhase::Move, 210, 90), &slider, track);
        assert_eq!(middle, Some(SliderStep::Moved(5)));
        let up = drag.on_pointer(&pointer(PointerPhase::Up, 400, 90), &slider, track);
        assert_eq!(up, Some(SliderStep::Chosen(10)));
        assert_eq!(drag.dragging(), None);
        // Taken away: the value kept shows again.
        drag.on_pointer(&pointer(PointerPhase::Down, 210, 60), &slider, track);
        let gone = drag.on_pointer(&pointer(PointerPhase::Cancel, 0, 0), &slider, track);
        assert_eq!(gone, Some(SliderStep::Cancelled));
        assert_eq!(
            drag.on_pointer(&pointer(PointerPhase::Up, 0, 0), &slider, track),
            None
        );
    }

    #[test]
    fn buttons_move_it_a_step() {
        assert_eq!(key_step(&actions::LEFT), Some(-1));
        assert_eq!(key_step(&actions::PREV_PAGE), Some(-1));
        assert_eq!(key_step(&actions::RIGHT), Some(1));
        assert_eq!(key_step(&actions::NEXT), Some(1));
        assert_eq!(key_step(&actions::CONFIRM), None);
    }

    #[test]
    fn the_track_is_filled_up_to_the_value() {
        let slider = Slider::new(0, 100);
        let track = Rect::new(10, 0, 220, 40);
        let draw = |value: i64, ink: Color, paper: Color| {
            let mut frame = Framebuffer::new(240, 40, PixelFormat::Gray8).unwrap();
            let mut canvas = frame.canvas();
            canvas.clear(paper);
            slider.draw_track(&mut canvas, &STYLE, track, value, ink);
            frame
        };
        // Knob 20 wide: the track runs from 20 to 220, the middle at 120.
        let half = draw(50, Color::BLACK, Color::WHITE);
        let view = half.view();
        assert_eq!(view.pixel(40, 20), Color::BLACK);
        assert_eq!(view.pixel(120, 11), Color::BLACK, "the knob");
        assert_eq!(view.pixel(120, 29), Color::BLACK, "the knob");
        assert_ne!(view.pixel(180, 20), Color::BLACK, "light after it");
        assert_ne!(view.pixel(180, 20), Color::WHITE, "but there");
        assert_eq!(view.pixel(40, 5), Color::WHITE, "a thin line");
        // At the end, filled all the way; white on the focus bar.
        let full = draw(100, Color::WHITE, Color::BLACK);
        assert_eq!(full.view().pixel(200, 20), Color::WHITE);
        assert_eq!(full.view().pixel(219, 12), Color::WHITE, "the knob");
        assert_eq!(full.view().pixel(5, 20), Color::BLACK, "nothing before it");
    }
}
