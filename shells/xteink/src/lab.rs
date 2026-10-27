//! The waveform lab: a screen to try, on the device, how the X4's panel is
//! driven (Tools, *Waveform lab*; the X4 only, and only in the developer
//! firmware, the feature `remote`).
//!
//! A test picture (stripes, squares, halves or lines) is flipped between
//! its two states, so that pixels change both ways, and each flip is shown
//! with the waveform chosen here instead of the panel's own fast one:
//!
//! - *Panel's own*: the stock fast refresh, to compare with.
//! - *Panel's, at a temperature*: the panel's own tables, made by the
//!   vendor, as the controller takes them for a panel at that temperature
//!   (0 to 90 degrees; the stock half refresh asks for 90). The warmer the
//!   faster, and the less complete at a cool panel.
//! - *Direct drive*: a waveform made here (`paperpaw_x4::waveform`): a
//!   pixel that changes is driven to its new colour for some frames, after
//!   a kick of the other colour for up to three. The stock voltages; the
//!   frames and the kick are what is tried.
//!
//! Up and down choose a row, left and right change its value, *Select*
//! flips the picture (or does the row: *Flip on its own*, *Clean up*).
//! *Clean up* shows everything with the stock refresh that drives every
//! pixel, which clears what the experiments left behind. The time the
//! panel took for the last frame is told above the rows.
//!
//! Nothing is kept: leaving the screen, or a restart, is the stock
//! firmware again.

use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use paperpaw_sdk::builtin::{actions, menus};
use paperpaw_sdk::realtime::ScreenMode;
use paperpaw_sdk::ui::{self, ListView, UiStyle};
use paperpaw_sdk::{
    Canvas, Color, Command, Cx, Env, FixedAction, FixedScreen, Handled, Id, Instances, MenuEntry,
    Params, Rect, RefreshMode, Screen,
};
use paperpaw_x4::display::{Experiment, TEMPERATURE_RANGE};
use paperpaw_x4::waveform::{Direct, MAX_FRAMES, MAX_KICK};

pub const SCREEN: Id = Id::new_static("x4.waveform-lab");
pub const OPEN: Id = Id::new_static("x4.open-waveform-lab");

/// After this many flips without a clean-up the lab says to clean up, and
/// a picture that flips on its own stops after the second number.
const CLEAN_AFTER: u32 = 20;
const AUTO_STOPS_AFTER: u32 = 60;

/// What the panel can do: only the SSD1677's waveforms can be tried.
static SUPPORTED: AtomicBool = AtomicBool::new(false);
/// What is chosen: the kind of waveform (0 the panel's own, 1 the panel's
/// at a temperature, 2 direct) and its numbers.
static KIND: AtomicU8 = AtomicU8::new(0);
static TEMPERATURE: AtomicU8 = AtomicU8::new(25);
static FRAMES: AtomicU8 = AtomicU8::new(8);
static KICK: AtomicU8 = AtomicU8::new(0);

/// The firmware says whether this panel takes experiments.
pub fn set_supported(yes: bool) {
    SUPPORTED.store(yes, Ordering::Relaxed);
}

/// How a fast refresh is driven now: as the lab says while it is the
/// screen shown (`active` is the screen on top), else as the panel does.
pub fn experiment(active: Option<&Id>) -> Option<Experiment> {
    if active != Some(&SCREEN) || !SUPPORTED.load(Ordering::Relaxed) {
        return None;
    }
    match KIND.load(Ordering::Relaxed) {
        1 => Some(Experiment::Temperature(
            TEMPERATURE.load(Ordering::Relaxed) as i8
        )),
        2 => Some(Experiment::Direct(Direct::new(
            FRAMES.load(Ordering::Relaxed),
            KICK.load(Ordering::Relaxed),
        ))),
        _ => None,
    }
}

pub static ACTIONS: [FixedAction; 1] = [FixedAction::new(OPEN, "x4.lab.title")
    .handler(|_, _, out| out.push(Command::OpenScreen(SCREEN, Params::new())))];

pub static ENTRIES: [MenuEntry; 1] = [MenuEntry::new(menus::TOOLS, OPEN).order(97)];

pub static SCREENS: [FixedScreen; 1] =
    [
        FixedScreen::new(SCREEN, "x4.lab.title", |env, _| Box::new(Lab::new(env)))
            .instances(Instances::SINGLE),
    ];

/// What a row of the lab does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Control {
    Waveform,
    Temperature,
    Frames,
    Kick,
    Picture,
    Auto,
    Clean,
}

/// The rows, by the kind of waveform chosen.
fn controls(kind: u8) -> &'static [Control] {
    use Control::*;
    match kind {
        1 => &[Waveform, Temperature, Picture, Auto, Clean],
        2 => &[Waveform, Frames, Kick, Picture, Auto, Clean],
        _ => &[Waveform, Picture, Auto, Clean],
    }
}

const PICTURES: u8 = 4;
/// The most rows there are.
const MOST_ROWS: u16 = 6;

struct Lab {
    style: UiStyle,
    header: ui::Header,
    list: ListView,
    picture: Rect,
    info: Rect,
    help: Rect,
    pattern: u8,
    flipped: bool,
    auto: bool,
    /// Flips since the screen was cleaned up.
    flips: u32,
}

impl Lab {
    fn new(env: &Env<'_>) -> Self {
        let style = UiStyle::new(env);
        let header = ui::Header::new(env, &style, true);
        let display = env.device().display;
        let (gap, row) = (style.gap(), style.row());
        let line = row * 3 / 4;
        let top = header.area().height + gap;
        let rows_height = MOST_ROWS * row;
        let help = Rect::new(0, display.height.saturating_sub(line), display.width, line);
        let rows_top = help.y.saturating_sub(rows_height);
        let info = Rect::new(0, rows_top.saturating_sub(line), display.width, line);
        let picture = Rect::new(
            0,
            top,
            display.width,
            info.y.saturating_sub(top + gap).max(row),
        );
        let rows = Rect::new(0, rows_top, display.width, rows_height);
        Self {
            list: ListView::new(rows, u32::from(row)).focus_on_keys(env),
            style,
            header,
            picture,
            info,
            help,
            pattern: 0,
            flipped: false,
            auto: false,
            flips: 0,
        }
    }

    fn kind() -> u8 {
        KIND.load(Ordering::Relaxed)
    }

    fn controls() -> &'static [Control] {
        controls(Self::kind())
    }

    fn focused(&self) -> Control {
        let controls = Self::controls();
        controls[self.list.focus().min(controls.len() - 1)]
    }

    fn title(env: &Env<'_>, control: Control) -> String {
        String::from(env.i18n().tr(match control {
            Control::Waveform => "x4.lab.waveform",
            Control::Temperature => "x4.lab.temperature",
            Control::Frames => "x4.lab.frames",
            Control::Kick => "x4.lab.kick",
            Control::Picture => "x4.lab.picture",
            Control::Auto => "x4.lab.auto",
            Control::Clean => "x4.lab.clean",
        }))
    }

    /// The value shown on a row.
    fn value(&self, env: &Env<'_>, control: Control) -> String {
        let tr = |key: &str| String::from(env.i18n().tr(key));
        match control {
            Control::Waveform => tr(match Self::kind() {
                1 => "x4.lab.waveform.temperature",
                2 => "x4.lab.waveform.direct",
                _ => "x4.lab.waveform.panel",
            }),
            Control::Temperature => format!("{} °C", TEMPERATURE.load(Ordering::Relaxed)),
            Control::Frames => format!("{}", FRAMES.load(Ordering::Relaxed)),
            Control::Kick => format!("{}", KICK.load(Ordering::Relaxed)),
            Control::Picture => tr(match self.pattern {
                0 => "x4.lab.picture.stripes",
                1 => "x4.lab.picture.squares",
                2 => "x4.lab.picture.halves",
                _ => "x4.lab.picture.lines",
            }),
            Control::Auto => tr(if self.auto { "core.on" } else { "core.off" }),
            Control::Clean => String::new(),
        }
    }

    /// The part below the header drawn anew and shown fast.
    fn redraw(&self, cx: &mut Cx<'_, '_>) {
        let top = self.picture.y;
        let below = Rect::new(
            0,
            top,
            self.picture.width,
            (self.help.bottom() as u16).saturating_sub(top),
        );
        cx.request_redraw_area(below);
        cx.request_refresh(RefreshMode::Fast);
    }

    /// Flips the picture: its black pixels white and the other way, with
    /// the line telling what the last frame took.
    fn flip(&mut self, cx: &mut Cx<'_, '_>) {
        self.flipped = !self.flipped;
        self.flips += 1;
        if self.auto && self.flips >= AUTO_STOPS_AFTER {
            self.auto = false;
        }
        let area = Rect::new(
            0,
            self.picture.y,
            self.picture.width,
            (self.info.bottom() as u16).saturating_sub(self.picture.y),
        );
        cx.request_redraw_area(area);
        cx.request_refresh(RefreshMode::Fast);
        if self.auto {
            cx.wake_at(cx.env().now().add_millis(1));
        }
    }

    /// Every pixel driven, the stock way: what the experiments left of
    /// the picture is cleared.
    fn clean(&mut self, cx: &mut Cx<'_, '_>) {
        self.flips = 0;
        self.flipped = false;
        self.auto = false;
        cx.request_redraw();
        cx.request_refresh(RefreshMode::Full);
    }

    fn change(&mut self, control: Control, step: i16, cx: &mut Cx<'_, '_>) {
        let bump = |value: &AtomicU8, step: i16, low: i16, high: i16| {
            let next = (i16::from(value.load(Ordering::Relaxed)) + step).clamp(low, high);
            value.store(next as u8, Ordering::Relaxed);
        };
        match control {
            Control::Waveform => {
                let kind = (i16::from(Self::kind()) + 3 + step) % 3;
                KIND.store(kind as u8, Ordering::Relaxed);
                self.list.set_len(Self::controls().len());
            }
            Control::Temperature => bump(
                &TEMPERATURE,
                5 * step,
                i16::from(TEMPERATURE_RANGE.0),
                i16::from(TEMPERATURE_RANGE.1),
            ),
            Control::Frames => bump(&FRAMES, step, 1, i16::from(MAX_FRAMES)),
            Control::Kick => bump(&KICK, step, 0, i16::from(MAX_KICK)),
            Control::Picture => {
                self.pattern = ((i16::from(self.pattern) + i16::from(PICTURES) + step)
                    % i16::from(PICTURES)) as u8;
            }
            Control::Auto => {
                self.auto = !self.auto;
                if self.auto {
                    cx.wake_at(cx.env().now().add_millis(1));
                }
            }
            Control::Clean => {}
        }
        self.redraw(cx);
    }
}

impl Screen for Lab {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let mut ui = ui::Ui::new(canvas, env, &style);
        self.header
            .draw(&mut ui, env.i18n().tr("x4.lab.title"), None, None);
        if !SUPPORTED.load(Ordering::Relaxed) {
            let area = Rect::new(0, self.picture.y, self.picture.width, self.picture.height);
            ui.message(area, env.i18n().tr("x4.lab.unsupported"));
            return;
        }
        draw_picture(ui.canvas(), self.picture, self.pattern, self.flipped);
        let tr = |key: &str| String::from(env.i18n().tr(key));
        let took = match env.panel().last_frame() {
            Some(frame) => format!(
                "{}: {} ms · {}: {}",
                tr("x4.lab.previous"),
                frame.shown().as_millis(),
                tr("x4.lab.flips"),
                self.flips
            ),
            None => tr("x4.lab.none"),
        };
        ui.note_line(self.info, &took);
        let controls = Self::controls();
        let rows: Vec<(String, String)> = controls
            .iter()
            .map(|&control| (Self::title(env, control), self.value(env, control)))
            .collect();
        self.list
            .draw(&mut ui, rows.len(), |ui, index, rect, focused| {
                let (title, value) = &rows[index];
                ui::Row::value(title, value).focused(focused).draw(ui, rect);
            });
        let note = tr(if self.flips >= CLEAN_AFTER {
            "x4.lab.clean-soon"
        } else {
            "x4.lab.help"
        });
        ui.note_line(self.help, &note);
    }

    fn mode(&self) -> ScreenMode {
        // The refreshes are the lab's own: the kernel does not add its
        // cleaning ones.
        ScreenMode::new().cleans_itself()
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        if !SUPPORTED.load(Ordering::Relaxed) {
            return Handled::No;
        }
        let len = Self::controls().len();
        if *action == actions::LEFT {
            let control = self.focused();
            self.change(control, -1, cx);
        } else if *action == actions::RIGHT {
            let control = self.focused();
            self.change(control, 1, cx);
        } else if *action == actions::CONFIRM {
            match self.focused() {
                Control::Auto => self.change(Control::Auto, 1, cx),
                Control::Clean => self.clean(cx),
                _ => self.flip(cx),
            }
        } else if *action == actions::UP || *action == actions::PREV_PAGE {
            self.list.move_focus(-1, len).request(cx);
        } else if *action == actions::DOWN || *action == actions::NEXT_PAGE {
            self.list.move_focus(1, len).request(cx);
        } else {
            return Handled::No;
        }
        Handled::Yes
    }

    fn on_timer(&mut self, cx: &mut Cx<'_, '_>) {
        if self.auto {
            self.flip(cx);
        }
    }
}

/// The test picture in `area`: `pattern` in its two states (`flipped`
/// swaps black and white), with a frame round it.
fn draw_picture(canvas: &mut Canvas<'_>, area: Rect, pattern: u8, flipped: bool) {
    let (ink, paper) = if flipped {
        (Color::WHITE, Color::BLACK)
    } else {
        (Color::BLACK, Color::WHITE)
    };
    canvas.fill(area, paper);
    let (x, y) = (i32::from(area.x), i32::from(area.y));
    let (width, height) = (u32::from(area.width), u32::from(area.height));
    match pattern {
        // Vertical stripes.
        0 => {
            let stripe = 24;
            let mut at = 0;
            while at < width {
                canvas.fill_rect(x + at as i32, y, stripe.min(width - at), height, ink);
                at += 2 * stripe;
            }
        }
        // Squares.
        1 => {
            let square = 24;
            for row in 0..height.div_ceil(square) {
                for column in 0..width.div_ceil(square) {
                    if (row + column) % 2 == 0 {
                        canvas.fill_rect(
                            x + (column * square) as i32,
                            y + (row * square) as i32,
                            square.min(width - column * square),
                            square.min(height - row * square),
                            ink,
                        );
                    }
                }
            }
        }
        // Left half and right half.
        2 => canvas.fill_rect(x, y, width / 2, height, ink),
        // Thin lines, as text has them.
        _ => {
            let mut at = 0;
            while at < height {
                canvas.fill_rect(x, y + at as i32, width, 2.min(height - at), ink);
                at += 8;
            }
        }
    }
    // A frame, so that the edge shows in both states.
    canvas.stroke_rect(x, y, width, height, 2, Color::BLACK);
}
