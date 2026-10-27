//! The waveform lab: a screen to try, on the device, how the panel is
//! driven (Tools, *Waveform lab*; for panels with an SSD1677 or a UC8179
//! controller, and only in the developer firmware, the feature
//! `waveform-lab`, which `remote` builds of the X4 and the X4 Pro have).
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
//! On a UC8179 (the X4 Pro) the direct drive has more: the frame rate
//! (the controller's `PLL` register, 20 to 100 Hz, its own 50), the group's
//! repeat (0 or 1: the datasheet says "0: 0 time", so if nothing changes
//! at 0, try 1), and the levels of white and black, assumed from the stock
//! gray waveform and turned round with *Polarity* if the picture comes
//! out the wrong way. The voltages are the controller's own; none is set.
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
use paperpaw_x4::display::{self, TEMPERATURE_RANGE};
use paperpaw_x4::panel::Experiment;
use paperpaw_x4::uc8179::{self, FRAME_RATES, OWN_RATE};
use paperpaw_x4::waveform::{Direct, MAX_FRAMES, MAX_KICK};

pub const SCREEN: Id = Id::new_static("x4.waveform-lab");
pub const OPEN: Id = Id::new_static("x4.open-waveform-lab");

/// After this many flips without a clean-up the lab says to clean up, and
/// a picture that flips on its own stops after the second number.
const CLEAN_AFTER: u32 = 20;
const AUTO_STOPS_AFTER: u32 = 60;

/// What the panel can do: only the waveforms of the SSD1677 (the X4, some
/// X4 Pros) and of the UC8179 (the X4 Pro) can be tried.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Support {
    None,
    Ssd1677,
    Uc8179,
}

static SUPPORT: AtomicU8 = AtomicU8::new(0);
/// What is chosen: the kind of waveform (0 the panel's own, 1 the panel's
/// at a temperature, SSD1677 only, 2 direct) and its numbers.
static KIND: AtomicU8 = AtomicU8::new(0);
static TEMPERATURE: AtomicU8 = AtomicU8::new(25);
static FRAMES: AtomicU8 = AtomicU8::new(8);
static KICK: AtomicU8 = AtomicU8::new(0);
/// The UC8179's: times to repeat, frame rate (an index of `FRAME_RATES`),
/// and whether the levels of white and black are turned round.
static REPEAT: AtomicU8 = AtomicU8::new(0);
static RATE: AtomicU8 = AtomicU8::new(OWN_RATE as u8);
static SWAPPED: AtomicBool = AtomicBool::new(false);

/// The firmware says what this panel takes (at start).
pub fn set_support(support: Support) {
    SUPPORT.store(
        match support {
            Support::None => 0,
            Support::Ssd1677 => 1,
            Support::Uc8179 => 2,
        },
        Ordering::Relaxed,
    );
}

fn support() -> Support {
    match SUPPORT.load(Ordering::Relaxed) {
        1 => Support::Ssd1677,
        2 => Support::Uc8179,
        _ => Support::None,
    }
}

/// The kinds of waveform the panel offers, the panel's own first.
fn kinds() -> &'static [u8] {
    match support() {
        Support::Ssd1677 => &[0, 1, 2],
        Support::Uc8179 => &[0, 2],
        Support::None => &[0],
    }
}

/// Whether the lab is the screen shown (`active` is the screen on top).
pub fn is_open(active: Option<&Id>) -> bool {
    active == Some(&SCREEN)
}

/// How a fast refresh is driven while the lab is open: as the lab says,
/// or (`None`) the panel's own.
pub fn experiment(active: Option<&Id>) -> Option<Experiment> {
    if !is_open(active) {
        return None;
    }
    let drive = || Direct::new(FRAMES.load(Ordering::Relaxed), KICK.load(Ordering::Relaxed));
    match (support(), KIND.load(Ordering::Relaxed)) {
        (Support::Ssd1677, 1) => Some(Experiment::Ssd1677(display::Experiment::Temperature(
            TEMPERATURE.load(Ordering::Relaxed) as i8,
        ))),
        (Support::Ssd1677, 2) => Some(Experiment::Ssd1677(display::Experiment::Direct(drive()))),
        (Support::Uc8179, 2) => Some(Experiment::Uc8179(uc8179::Experiment {
            drive: drive(),
            swapped: SWAPPED.load(Ordering::Relaxed),
            repeat: REPEAT.load(Ordering::Relaxed),
            rate: usize::from(RATE.load(Ordering::Relaxed)),
        })),
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
    Repeat,
    Rate,
    Polarity,
    Picture,
    Auto,
    Clean,
}

/// The rows, by the kind of waveform chosen and what the panel takes.
fn controls(kind: u8) -> &'static [Control] {
    use Control::*;
    match (support(), kind) {
        (Support::Ssd1677, 1) => &[Waveform, Temperature, Picture, Auto, Clean],
        (Support::Ssd1677, 2) => &[Waveform, Frames, Kick, Picture, Auto, Clean],
        (Support::Uc8179, 2) => &[
            Waveform, Frames, Kick, Rate, Repeat, Polarity, Picture, Auto, Clean,
        ],
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
            Control::Repeat => "x4.lab.repeat",
            Control::Rate => "x4.lab.rate",
            Control::Polarity => "x4.lab.polarity",
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
            Control::Repeat => format!("{}", REPEAT.load(Ordering::Relaxed)),
            Control::Rate => {
                let rate = usize::from(RATE.load(Ordering::Relaxed)).min(FRAME_RATES.len() - 1);
                format!("{} Hz", FRAME_RATES[rate].1)
            }
            Control::Polarity => tr(if SWAPPED.load(Ordering::Relaxed) {
                "x4.lab.polarity.swapped"
            } else {
                "x4.lab.polarity.normal"
            }),
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
                let kinds = kinds();
                let at = kinds.iter().position(|&k| k == Self::kind()).unwrap_or(0);
                let next = (at as i16 + kinds.len() as i16 + step) % kinds.len() as i16;
                KIND.store(kinds[next as usize], Ordering::Relaxed);
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
            Control::Repeat => bump(&REPEAT, step, 0, 1),
            Control::Rate => bump(&RATE, step, 0, FRAME_RATES.len() as i16 - 1),
            Control::Polarity => SWAPPED.store(!SWAPPED.load(Ordering::Relaxed), Ordering::Relaxed),
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
        let len = Self::controls().len();
        self.header.draw(
            &mut ui,
            env.i18n().tr("x4.lab.title"),
            None,
            self.list.paging(len),
        );
        if support() == Support::None {
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
        if support() == Support::None {
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
