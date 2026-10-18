//! Remote control over USB, for testing on the device (built with the
//! `remote` feature): the commands of [`quireink_x4::remote`] read from
//! the USB unit, and every `QI:` answer. What only asks (the screen, the
//! heap, a file) is answered here; what changes the X4's state (a press,
//! an action, the network) is the main loop's (`firmware`).

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write;

use esp_hal::peripherals::USB_DEVICE;
use esp_println::println;
use quireink_core::{HttpResponse, JoinFailure, Kernel, WifiNetwork, WifiState};
use quireink_platform::FrameView;
use quireink_x4::remote::{Command, hex_bytes, part_state, runs};

/// A line is never longer: `data` and 512 bytes in hexadecimal.
const MAX_LINE: usize = 1100;
/// A file is written in pieces this large.
const CHUNK: usize = 4096;
/// Without data for this long, a file is given up.
const QUIET_MS: u64 = 5000;

/// Reads commands from USB, a line at a time.
///
/// It reads the USB unit's receive buffer itself: esp-hal's driver resets
/// the unit when it starts, which cuts the connection the log goes out on.
pub struct Remote {
    _usb: USB_DEVICE<'static>,
    line: Vec<u8>,
}

impl Remote {
    pub fn new(usb: USB_DEVICE<'static>) -> Self {
        Self {
            _usb: usb,
            line: Vec::new(),
        }
    }

    /// The next byte the computer sent, if any.
    fn read_byte(&mut self) -> Option<u8> {
        let regs = USB_DEVICE::regs();
        regs.ep1_conf()
            .read()
            .serial_out_ep_data_avail()
            .bit_is_set()
            .then(|| regs.ep1().read().rdwr_byte().bits())
    }

    /// The next whole line that came, if one did.
    fn next_line(&mut self) -> Option<Vec<u8>> {
        while let Some(byte) = self.read_byte() {
            match byte {
                b'\n' | b'\r' if self.line.is_empty() => {}
                b'\n' | b'\r' => return Some(core::mem::take(&mut self.line)),
                _ if self.line.len() < MAX_LINE => self.line.push(byte),
                _ => {}
            }
        }
        None
    }

    /// The next command, once a whole line has come.
    pub fn poll(&mut self) -> Option<Command> {
        let line = self.next_line()?;
        let line = String::from_utf8_lossy(&line);
        let command = quireink_x4::remote::parse(&line);
        if command.is_none() {
            error(&line);
        }
        command
    }

    /// Answers a command that only asks (the screen, a screenshot, what it
    /// shows, the heap) or brings a file; the others it gives back for the
    /// main loop.
    pub fn answer(&mut self, kernel: &mut Kernel, command: Command) -> Option<Command> {
        match command {
            Command::Shot => shot(kernel),
            Command::Describe => describe(kernel),
            Command::Screen => {
                let id = kernel.active_screen().map_or("", |id| id.as_str());
                println!("QI:SCREEN {id}");
            }
            Command::Heap => {
                let (left, size) = crate::stack::left();
                println!(
                    "QI:HEAP {} {} {left} {size} {}",
                    esp_alloc::HEAP.used(),
                    esp_alloc::HEAP.free(),
                    largest_block()
                );
            }
            Command::Put { size, path } => self.receive(kernel, size, &path),
            Command::Get(path) => send(kernel, &path),
            Command::Hang(seconds) => {
                println!("QI:HANGING");
                let until = esp_hal::time::Instant::now()
                    + esp_hal::time::Duration::from_secs(seconds.min(60));
                while esp_hal::time::Instant::now() < until {}
                ok();
            }
            Command::Nap(seconds) => {
                println!("QI:NAPPING");
                crate::power::try_long_nap(seconds.min(60));
            }
            Command::Boot => {
                let (place, naps, reason) = crate::power::last_run();
                println!("QI:BOOT {reason:?} {place} {naps}");
                if let Some(message) = critical_section::with(|cs| PANIC.borrow_ref(cs).clone()) {
                    println!("QI:PANIC {message}");
                }
            }
            Command::Profile(true) => {
                crate::input::profile(true);
                ok();
            }
            Command::Profile(false) => {
                let places = crate::input::profile(false).unwrap_or_default();
                let mut line = String::new();
                for chunk in places.chunks(24) {
                    line.clear();
                    for pc in chunk {
                        let _ = write!(line, " {pc:x}");
                    }
                    println!("QI:PC{line}");
                }
                println!("QI:END");
            }
            Command::Trace(true) => {
                crate::traced::trace(true);
                ok();
            }
            Command::Trace(false) => {
                for line in crate::traced::trace(false).unwrap_or_default() {
                    println!("QI:IO {line}");
                }
                println!("QI:END");
            }
            Command::Peek { at, words } => {
                let mut line = String::new();
                for start in (0..words).step_by(16) {
                    line.clear();
                    for i in start..(start + 16).min(words) {
                        let address = (at + i * 4) as *const u32;
                        // SAFETY: an aligned word of the ROM (`parse` lets
                        // no other through), always mapped.
                        let word = unsafe { core::ptr::read_volatile(address) };
                        let _ = write!(line, " {word:x}");
                    }
                    println!("QI:MEM {:x}{line}", at + start * 4);
                }
                println!("QI:END");
            }
            Command::Input => {
                let (samples, front, side) = crate::input::stats();
                let power = crate::input::power_down();
                let millivolts = crate::input::millivolts();
                println!("QI:INPUT {samples} {front} {side} {power} {millivolts}");
            }
            command @ (Command::Press { .. }
            | Command::Run(_)
            | Command::Open(_)
            | Command::Refresh { .. }
            | Command::IdleSleep(_)
            | Command::Network(_)) => return Some(command),
        }
        None
    }

    /// Writes a file of `size` bytes to `path` from the lines `data <hex>`
    /// that follow, a piece at a time.
    fn receive(&mut self, kernel: &Kernel, size: usize, path: &str) {
        let storage = kernel.storage();
        if let Err(e) = storage.write(path, &[]) {
            return println!("QI:ERR {path}: {e:?}");
        }
        println!("QI:READY");
        let millis = || {
            esp_hal::time::Instant::now()
                .duration_since_epoch()
                .as_millis()
        };
        let mut piece = Vec::with_capacity(CHUNK);
        let (mut got, mut last) = (0, millis());
        while got < size {
            crate::power::feed();
            let Some(line) = self.next_line() else {
                if millis() - last > QUIET_MS {
                    return println!("QI:ERR {path}: {got} of {size} bytes came");
                }
                continue;
            };
            last = millis();
            let Some(bytes) = line.strip_prefix(b"data ").and_then(hex_bytes) else {
                return println!("QI:ERR {path}: not data");
            };
            got += bytes.len();
            piece.extend_from_slice(&bytes);
            if piece.len() >= CHUNK || got >= size {
                if let Err(e) = storage.append(path, &piece) {
                    return println!("QI:ERR {path}: {e:?}");
                }
                piece.clear();
            }
        }
        println!("QI:OK {got}");
    }
}

/// Sends the file `path` of the storage.
fn send(kernel: &Kernel, path: &str) {
    let storage = kernel.storage();
    let Ok(size) = storage.size(path) else {
        return println!("QI:ERR {path}: not found");
    };
    println!("QI:SIZE {size}");
    let mut buf = [0u8; 256];
    let mut line = String::with_capacity(buf.len() * 2);
    let mut at = 0u64;
    while at < size {
        crate::power::feed();
        let Ok(n) = storage.read_at(path, at, &mut buf) else {
            return println!("QI:ERR {path}: not readable");
        };
        if n == 0 {
            break;
        }
        line.clear();
        for byte in &buf[..n] {
            let _ = write!(line, "{byte:02x}");
        }
        println!("QI:DATA {line}");
        at += n as u64;
    }
    println!("QI:END");
}

/// The last run's panic, said by `boot`.
static PANIC: critical_section::Mutex<core::cell::RefCell<Option<String>>> =
    critical_section::Mutex::new(core::cell::RefCell::new(None));

/// Keeps the last run's panic for `boot`.
pub fn note_panic(message: Option<String>) {
    critical_section::with(|cs| PANIC.replace(cs, message));
}

/// The largest block of memory that could be had now (found by asking for
/// blocks, given back at once).
fn largest_block() -> usize {
    let (mut can, mut cannot) = (0, esp_alloc::HEAP.free() + 1);
    while cannot - can > 64 {
        let size = (can + cannot) / 2;
        if Vec::<u8>::new().try_reserve_exact(size).is_ok() {
            can = size;
        } else {
            cannot = size;
        }
    }
    can
}

/// Done.
pub fn ok() {
    println!("QI:OK");
}

/// Not now, e.g. a press while another is held.
pub fn busy() {
    println!("QI:BUSY");
}

/// Not understood, or not done.
pub fn error(what: &str) {
    println!("QI:ERR {what}");
}

/// The panel refreshed from what its controller holds, and how it went.
pub fn refreshed(result: impl core::fmt::Debug) {
    println!("QI:OK {result:?}");
}

/// The networks a scan asked over USB found.
pub fn networks(networks: &[WifiNetwork]) {
    for net in networks {
        println!("QI:NET {} {} {}", net.signal, net.secured, net.ssid);
    }
    println!("QI:END");
}

/// The network asked over USB was joined.
pub fn connected(ssid: &str, address: &str) {
    println!("QI:CONNECTED {ssid}");
    println!("QI:IP {address}");
}

/// The network asked over USB was not joined.
pub fn not_connected(ssid: &str, reason: JoinFailure) {
    error(&alloc::format!("wifi connect {ssid}: {}", reason.as_str()));
}

pub fn wifi_state(state: &WifiState) {
    match state {
        WifiState::Off => println!("QI:WIFI off"),
        WifiState::On => println!("QI:WIFI on"),
        WifiState::Joining { ssid } => println!("QI:WIFI joining {ssid}"),
        WifiState::Online { ssid, address } => println!("QI:WIFI online {ssid} {address}"),
    }
}

/// The answer of a request asked over USB.
pub fn http_answer(answer: &HttpResponse, ms: u64) {
    let why = if answer.status == 0 {
        String::from_utf8_lossy(&answer.body).into_owned()
    } else {
        String::new()
    };
    println!(
        "QI:HTTP {} {} saved {} {ms} ms heap {} {} {why}",
        answer.status,
        answer.body.len(),
        answer.saved,
        esp_alloc::HEAP.used(),
        esp_alloc::HEAP.free()
    );
}

/// Sends the whole screen, as turned (screens may have less of it), or
/// `QI:BUSY` while changes wait to be shown (ask again).
fn shot(kernel: &mut Kernel) {
    let display = kernel.device().display;
    let header = (
        display.width,
        display.height,
        display.format.bits_per_pixel(),
    );
    let mut started = false;
    let mut hex = String::new();
    let done = kernel.capture(&mut |top, rows: FrameView<'_>| {
        if !started {
            println!("QI:FRAME {} {} {}", header.0, header.1, header.2);
            started = true;
        }
        for y in 0..rows.height {
            let at = usize::from(y) * rows.stride;
            let row = rows.data.get(at..at + rows.stride).unwrap_or(&[]);
            hex.clear();
            runs(row, &mut hex);
            println!("QI:ROW {} {hex}", top + y);
        }
    });
    if done {
        println!("QI:END");
    } else {
        busy();
    }
}

/// Sends what the screen shows, part by part.
fn describe(kernel: &mut Kernel) {
    for part in kernel.describe() {
        let state = part_state(part.focused, part.on);
        let a = part.area;
        let label = part.label.replace('\n', " ");
        println!(
            "QI:PART {:?} {} {} {} {} {state} {label}",
            part.role, a.x, a.y, a.width, a.height
        );
    }
    println!("QI:END");
}
