//! The buttons and the touch screen (the X4 Pro's), read in a timer's
//! interrupt every [`SAMPLE_MS`]: a press is seen while PaperPaw is busy
//! for a second or more (laying out a page, counting a book's pages), and
//! waits in a queue until the main loop takes it ([`next`]). Each keeps
//! the time it came, so a long press stays long.
//!
//! The battery and the clock chip change slowly: the main loop reads and
//! sets them when it needs to ([`percent`], [`keep_time`]), with the parts
//! taken out of the interrupt's hands meanwhile. On the I²C bus of the
//! X4 Pro and the X4 Classic a reading takes a millisecond or two: in the
//! interrupt it would hold off every other one (the radio's too).
//!
//! The same interrupt can note where the processor is each time, for
//! finding out where the time goes ([`profile`]).

#[cfg(feature = "remote")]
use alloc::vec::Vec;
use core::cell::{Cell, RefCell};
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use critical_section::Mutex;
use esp_hal::Blocking;
use esp_hal::gpio::Input;
use esp_hal::handler;
use esp_hal::time::Duration;
use esp_hal::timer::PeriodicTimer;
use paperpaw_x4::buttons::Debouncer;
use paperpaw_x4::changes::{Finger, Queue, Reading};

use crate::board::Keys;

/// How often the buttons are read.
const SAMPLE_MS: u64 = 5;
/// How often the battery is read at most.
const BATTERY_MS: u64 = 1000;
/// Presses and releases waiting; more are dropped. A finger's moves
/// while the last one waits take its place (see [`Queue::push`]).
const QUEUE: usize = 32;

/// A finger on the touch screen, on the upright screen: down (with where
/// the controller had it, to look into), moved, lifted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    not(any(feature = "x4-pro", feature = "remote")),
    expect(dead_code, reason = "only the X4 Pro has a touch screen")
)]
pub enum Touch {
    Down(u16, u16, (u16, u16)),
    Move(u16, u16),
    Up(u16, u16),
    /// The home key (a touch key below the X4 Pro's screen) went down or
    /// up: a button.
    #[cfg_attr(
        all(feature = "remote", not(feature = "x4-pro")),
        expect(dead_code, reason = "only the X4 Pro has a home key")
    )]
    Home(bool),
}

/// What happened, and when (ms).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    /// A button went down (`true`) or up.
    Button(&'static str, bool, u64),
    Touch(Touch, u64),
}

/// What the interrupt reads with, and what it found.
pub struct Sampler {
    timer: PeriodicTimer<'static, Blocking>,
    /// Taken out while the main loop reads the battery or sets the clock
    /// ([`with_keys`]): the interrupt passes over them meanwhile.
    keys: Option<Keys>,
    power: Input<'static>,
    debouncer: Debouncer,
    /// Woken by the power button, it is still down: it counts once let go.
    power_armed: bool,
    queue: Queue<Change, QUEUE>,
}

static SAMPLER: Mutex<RefCell<Option<Sampler>>> = Mutex::new(RefCell::new(None));
/// Where the processor was at each reading, while profiling.
#[cfg(feature = "remote")]
static PROFILE: Mutex<RefCell<Option<Vec<u32>>>> = Mutex::new(RefCell::new(None));
/// Places noted at most (4 bytes each): 15 s.
#[cfg(feature = "remote")]
const PROFILE_SAMPLES: usize = 3000;
/// Memory left free for PaperPaw while profiling.
#[cfg(feature = "remote")]
const PROFILE_SPARE: usize = 64 * 1024;
/// The battery's last reading: when (ms since the start), its charge in
/// percent and its voltage in millivolts.
static BATTERY: Mutex<Cell<Option<(u64, u8, u16)>>> = Mutex::new(Cell::new(None));
/// When the readings began (ms since the start).
static STARTED: AtomicU32 = AtomicU32::new(0);
/// The power button is down now (not debounced).
static POWER_DOWN: AtomicBool = AtomicBool::new(false);
/// Readings so far, and the last ones of the two ladders, to look into.
static SAMPLES: AtomicU32 = AtomicU32::new(0);
static LEVELS: AtomicU32 = AtomicU32::new(0);

fn millis() -> u64 {
    esp_hal::time::Instant::now()
        .duration_since_epoch()
        .as_millis()
}

impl Sampler {
    pub fn new(
        mut timer: PeriodicTimer<'static, Blocking>,
        keys: Keys,
        power: Input<'static>,
    ) -> Self {
        timer.set_interrupt_handler(sample);
        Self {
            timer,
            keys: Some(keys),
            power,
            debouncer: Debouncer::new(),
            power_armed: false,
            queue: Queue::new(),
        }
    }

    /// One reading of the buttons and the touch screen.
    fn read(&mut self) {
        let Some(keys) = self.keys.as_mut() else {
            return;
        };
        let now = millis();
        let power_down = self.power.is_low();
        POWER_DOWN.store(power_down, Ordering::Relaxed);
        self.power_armed |= !power_down;
        let (reading, levels) = keys.read(self.power_armed && power_down);
        // Only the interrupt writes it (the X4's chip has no atomic adds).
        SAMPLES.store(
            SAMPLES.load(Ordering::Relaxed).wrapping_add(1),
            Ordering::Relaxed,
        );
        LEVELS.store(levels, Ordering::Relaxed);
        let queue = &mut self.queue;
        let mut push = |change: Change| {
            queue.push(change);
        };
        self.debouncer.update(reading, now, &mut |name, down| {
            push(Change::Button(name, down, now));
        });
        keys.touch(&mut |touch| push(Change::Touch(touch, now)));
        #[cfg(feature = "remote")]
        if let Some(touch) = dragged(now) {
            push(Change::Touch(touch, now));
        }
    }
}

impl Reading for Change {
    fn finger(&self) -> Option<Finger> {
        match self {
            Change::Touch(Touch::Down(x, y, _), _) => Some(Finger::Down(*x, *y)),
            Change::Touch(Touch::Move(x, y), _) => Some(Finger::Move(*x, *y)),
            Change::Touch(Touch::Up(..), _) => Some(Finger::Up),
            Change::Touch(Touch::Home(_), _) | Change::Button(..) => None,
        }
    }
}

/// A finger moved by the remote control (`drag`), read as the touch
/// controller's readings are: where it goes, in how many steps, how far
/// apart, the step it is at and when the next is due.
#[cfg(feature = "remote")]
#[derive(Debug, Clone, Copy)]
struct Drag {
    from: (u16, u16),
    to: (u16, u16),
    steps: u32,
    ms: u16,
    step: u32,
    due: u64,
}

#[cfg(feature = "remote")]
static DRAG: Mutex<Cell<Option<Drag>>> = Mutex::new(Cell::new(None));

/// Moves a finger from `from` to `to` in `steps` steps `ms` apart, read in
/// the interrupt as the touch controller's readings are.
#[cfg(feature = "remote")]
pub fn drag(from: (u16, u16), to: (u16, u16), steps: u16, ms: u16) {
    let drag = Drag {
        from,
        to,
        steps: u32::from(steps),
        ms,
        step: 0,
        due: millis(),
    };
    critical_section::with(|cs| DRAG.borrow(cs).set(Some(drag)));
}

/// The finger of the remote control's drag, where a reading is due.
#[cfg(feature = "remote")]
fn dragged(now: u64) -> Option<Touch> {
    critical_section::with(|cs| {
        let cell = DRAG.borrow(cs);
        let mut drag = cell.get().filter(|d| d.due <= now)?;
        let along = |a: u16, b: u16| {
            let (a, b) = (i64::from(a), i64::from(b));
            let (n, i) = (i64::from(drag.steps), i64::from(drag.step));
            (a + (b - a) * i / n) as u16
        };
        let touch = match drag.step {
            0 => Touch::Down(drag.from.0, drag.from.1, drag.from),
            step if step <= drag.steps => {
                Touch::Move(along(drag.from.0, drag.to.0), along(drag.from.1, drag.to.1))
            }
            _ => Touch::Up(drag.to.0, drag.to.1),
        };
        drag.step += 1;
        drag.due = now + u64::from(drag.ms);
        cell.set((drag.step <= drag.steps + 1).then_some(drag));
        Some(touch)
    })
}

#[handler]
fn sample() {
    critical_section::with(|cs| {
        #[cfg(feature = "remote")]
        if let Some(places) = PROFILE.borrow_ref_mut(cs).as_mut()
            && places.len() < places.capacity()
        {
            // The place the interrupt came at.
            #[cfg(target_arch = "riscv32")]
            {
                let pc: u32;
                // SAFETY: reads a register, changes nothing.
                unsafe { core::arch::asm!("csrr {0}, mepc", out(reg) pc) };
                places.push(pc);
            }
        }
        if let Some(sampler) = SAMPLER.borrow_ref_mut(cs).as_mut() {
            sampler.timer.clear_interrupt();
            sampler.read();
        }
    });
}

/// Starts reading in the timer's interrupt.
pub fn start(mut sampler: Sampler) {
    STARTED.store(millis() as u32, Ordering::Relaxed);
    sampler.read();
    let _ = sampler.timer.start(Duration::from_millis(SAMPLE_MS));
    sampler.timer.listen();
    critical_section::with(|cs| SAMPLER.replace(cs, Some(sampler)));
}

/// Reads the buttons now, e.g. after a light sleep, in which the timer
/// that reads them stops.
pub fn sample_now() {
    critical_section::with(|cs| {
        if let Some(sampler) = SAMPLER.borrow_ref_mut(cs).as_mut() {
            sampler.read();
        }
    });
}

/// The next press or release, oldest first.
pub fn next() -> Option<Change> {
    critical_section::with(|cs| {
        let mut sampler = SAMPLER.borrow_ref_mut(cs);
        sampler.as_mut()?.queue.pop()
    })
}

/// Starts noting where the processor is (`true`), or stops and gives what
/// was noted.
#[cfg(feature = "remote")]
pub fn profile(start: bool) -> Option<Vec<u32>> {
    critical_section::with(|cs| {
        let mut profile = PROFILE.borrow_ref_mut(cs);
        if start {
            let mut places = Vec::new();
            if esp_alloc::HEAP.free() > PROFILE_SAMPLES * 4 + PROFILE_SPARE {
                let _ = places.try_reserve_exact(PROFILE_SAMPLES);
            }
            *profile = Some(places);
            None
        } else {
            profile.take()
        }
    })
}

/// Runs `work` on the parts the interrupt reads with, taken out of its
/// hands for the time (it passes over them), so slow work on them holds
/// no interrupt off. From the main loop only.
fn with_keys<R>(work: impl FnOnce(&mut Keys) -> R) -> Option<R> {
    let mut keys = critical_section::with(|cs| SAMPLER.borrow_ref_mut(cs).as_mut()?.keys.take())?;
    let done = work(&mut keys);
    critical_section::with(|cs| {
        if let Some(s) = SAMPLER.borrow_ref_mut(cs).as_mut() {
            s.keys = Some(keys);
        }
    });
    Some(done)
}

/// Gives the clock chip the time (on the X4 Pro and X4 Classic; the X4
/// has none).
pub fn keep_time(unix_ms: i64) {
    with_keys(|keys| keys.set_clock(unix_ms));
}

/// The battery's charge in percent and its voltage in millivolts (0 until
/// read), read again when the last reading is [`BATTERY_MS`] old.
fn battery() -> (u8, u16) {
    let now = millis();
    let last = critical_section::with(|cs| BATTERY.borrow(cs).get());
    if let Some((at, percent, millivolts)) = last
        && now - at < BATTERY_MS
    {
        return (percent, millivolts);
    }
    match with_keys(Keys::battery).flatten() {
        Some((percent, millivolts)) => {
            critical_section::with(|cs| BATTERY.borrow(cs).set(Some((now, percent, millivolts))));
            (percent, millivolts)
        }
        None => last.map_or((0, 0), |(_, percent, millivolts)| (percent, millivolts)),
    }
}

/// The battery's voltage, in millivolts.
pub fn millivolts() -> u16 {
    battery().1
}

/// The battery's charge, in percent.
pub fn percent() -> u8 {
    battery().0
}

/// Readings so far, and the last readings of the front and side ladders.
#[cfg(feature = "remote")]
pub fn stats() -> (u32, u16, u16) {
    let levels = LEVELS.load(Ordering::Relaxed);
    (
        SAMPLES.load(Ordering::Relaxed),
        (levels >> 16) as u16,
        levels as u16,
    )
}

/// Whether `button` is held now (the X4 Pro's home key as one), once the
/// readings since the start had time to settle: for keys held at
/// power-on.
pub fn held(button: &str) -> bool {
    // A few readings past the debouncing.
    let settled = paperpaw_x4::buttons::DEBOUNCE_MS + 4 * SAMPLE_MS + 2 * SAMPLE_MS;
    while millis() < STARTED.load(Ordering::Relaxed) as u64 + settled {
        core::hint::spin_loop();
    }
    critical_section::with(|cs| {
        SAMPLER.borrow_ref(cs).as_ref().is_some_and(|s| {
            s.debouncer.is_down(button)
                || (button == paperpaw_platform::buttons::HOME
                    && s.keys.as_ref().is_some_and(Keys::home_down))
        })
    })
}

/// Whether the power button is down now.
pub fn power_down() -> bool {
    POWER_DOWN.load(Ordering::Relaxed)
}

/// After the power button woke the X4 from its light sleep: that press
/// does not count, and what waits from before is dropped.
pub fn rearm() {
    critical_section::with(|cs| {
        if let Some(s) = SAMPLER.borrow_ref_mut(cs).as_mut() {
            s.power_armed = false;
            s.queue.clear();
        }
    });
}

/// Stops reading and gives back the power button, e.g. to wake the X4
/// from its deep sleep.
pub fn stop() -> Option<Input<'static>> {
    critical_section::with(|cs| {
        let mut sampler = SAMPLER.borrow_ref_mut(cs).take()?;
        sampler.timer.unlisten();
        let _ = sampler.timer.cancel();
        Some(sampler.power)
    })
}
