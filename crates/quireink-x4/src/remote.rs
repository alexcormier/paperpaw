//! The X4's remote control over USB, for testing on the device: a computer
//! sends lines with commands (`tools/x4-remote.py` does), the X4 answers
//! with lines that start with `QI:`, amid its log. Here what is the same
//! on any X4 firmware: the commands read from lines ([`parse`]) and how
//! data is written in answers; the firmware carries them out.
//!
//! - `press <button>`: the button pressed and let go;
//! - `open <path>`: opens the book at `path` (hexadecimal) in the reader
//!   (`QI:OK`);
//! - `run <action>`: runs an action as a button bound to it would, e.g.
//!   `run calibre.open` to open a tool at once (`QI:OK`);
//! - `set <setting> <value>`: sets a setting as the settings screen would,
//!   the value as configuration files write it, in hexadecimal (`QI:OK`,
//!   or `QI:ERR` and why);
//! - `setting <setting>`: `QI:SETTING <value>`, as configuration files
//!   write it;
//! - `hold <button> <ms>`: held that long, e.g. for a long press;
//! - `shot`: the screen, as `QI:FRAME <width> <height> <bits per pixel>`,
//!   then `QI:ROW <y> <hex>` for every row (its bytes as the frame holds
//!   them, in [`runs`]) and `QI:END`;
//! - `screen`: `QI:SCREEN <id>`, the screen shown;
//! - `describe`: what it shows, a `QI:PART <role> <x> <y> <width> <height>
//!   <state> <label>` for each row, button or heading (state: see
//!   [`part_state`]), then `QI:END`;
//! - `heap`: `QI:HEAP <used> <free> <stack left> <stack size> <largest>`,
//!   in bytes (the stack: what it never used since start; the largest
//!   block that could be had now, of the free memory in pieces);
//! - `input`: `QI:INPUT <readings> <front> <side> <power> <millivolts>`, how
//!   the buttons and the battery are read;
//! - `profile start`, `profile stop`: notes where the processor is every
//!   5 ms in between, sent as `QI:PC <hex> …` lines and `QI:END`;
//! - `trace start`, `trace stop`: what is done on the storage in between,
//!   as `QI:IO <what> <path> …` lines and `QI:END`;
//! - `peek <address> <words>`: memory, as `QI:MEM <address> <hex words>`
//!   lines of 16 words and `QI:END` (hexadecimal address, at most
//!   [`PEEK_WORDS`] words; only the [`ROM`], e.g. to find where its
//!   functions lie);
//! - `refresh fast`, `refresh full`: the panel refreshed from what the
//!   controller holds, nothing sent (`QI:OK`): whether it holds the frame;
//! - `gray [<path>]`: four grays on the panel to judge them, with the
//!   decoded gray picture at `path` (hexadecimal) below; then the screen
//!   is drawn again (`QI:OK`);
//! - `put <size> <path>`: a file for the storage (the SD card), answered
//!   with `QI:READY`; then lines `data <hex>` until all its bytes came, and
//!   `QI:OK <size>` once it is written;
//! - `idle sleep <seconds>`: light sleep as on battery for that long
//!   (`QI:OK`; the connection goes meanwhile), then start again;
//! - `hang <seconds>`: hangs that long (at most a minute) without the
//!   watchdog fed, to try that it starts the X4 again (`QI:OK` if not);
//! - `boot`: `QI:BOOT <reason> <place> <naps>`, why the X4 last started,
//!   where the run before was then and its light sleeps, e.g. after a
//!   watchdog's reset or an `idle sleep`, and `QI:PANIC <message>` if the
//!   run before panicked;
//! - `nap <seconds>`: one light sleep that long (at most a minute), then a
//!   new start, to try that the RTC's watchdog starts it again first;
//! - `wifi scan`: switches the radio on and lists the networks in reach,
//!   as `QI:NET <signal> <secured> <name>` lines, then `QI:END`;
//! - `wifi connect <name> <password>`: joins that network (both in
//!   hexadecimal, `-` for no password), answered once joined with
//!   `QI:CONNECTED <name>` and `QI:IP <address>`, or `QI:ERR`; the
//!   password is not shown anywhere and is kept with the passwords and
//!   keys, where the `wifi` plugin finds it to join the network again;
//! - `wifi status`: `QI:WIFI <off|on|joining|online> [<name> <address>]`;
//! - `wifi off`: switches the radio off (`QI:OK`);
//! - `http get <url> [<path>]`: fetches a plain `http://` address as
//!   plugins' requests are made, into memory or the file `path`, answered
//!   with `QI:HTTP <status> <bytes> saved <true|false>`, the heap and, for
//!   a failure, why (the radio must be online);
//! - `get <path>`: a file of the storage, as `QI:SIZE <size>`, then
//!   `QI:DATA <hex>` lines and `QI:END` (e.g. the battery's log).
//!
//! Any other line is answered with `QI:ERR <line>`.

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write;
use core::ops::Range;

use crate::buttons;

/// How long `press` holds a button, in milliseconds.
pub const PRESS_MS: u64 = 60;
/// The memory `peek` may read: the ROM (elsewhere a read may fault).
pub const ROM: Range<u32> = 0x4000_0000..0x4006_0000;
/// The most words `peek` reads at once.
pub const PEEK_WORDS: u32 = 4096;

/// What the computer asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// A button pressed, let go after so many milliseconds.
    Press {
        button: &'static str,
        ms: u64,
    },
    /// An action run, by its id (checked by the firmware).
    Run(String),
    /// A book opened, by its path.
    Open(String),
    /// A setting set, by its id, to a value as configuration files write
    /// it.
    Set {
        id: String,
        value: String,
    },
    /// A setting's value asked for, by its id.
    Setting(String),
    Shot,
    Screen,
    Describe,
    Heap,
    Input,
    /// Profiling starts (`true`) or stops.
    Profile(bool),
    /// Tracing the storage starts (`true`) or stops.
    Trace(bool),
    /// Words of the ROM from an address.
    Peek {
        at: u32,
        words: u32,
    },
    /// The panel refreshed from the controller's memory, full or fast.
    Refresh {
        full: bool,
    },
    /// Four grays on the panel, with a decoded gray picture if named.
    Gray(Option<String>),
    /// Light sleep while idle even on USB, for so many seconds.
    IdleSleep(u64),
    /// A file of so many bytes for that path.
    Put {
        size: usize,
        path: String,
    },
    Get(String),
    Boot,
    /// About the network.
    Network(Ask),
    Hang(u64),
    Nap(u64),
}

/// What is asked of the network.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ask {
    /// Networks in reach (the radio switched on).
    Scan,
    /// Join a network, kept with its password once joined.
    Connect {
        ssid: String,
        password: String,
    },
    Status,
    Off,
    /// Fetch an address, into memory or the file `save_to`.
    Get {
        url: String,
        save_to: Option<String>,
    },
}

/// The command a line says, if it is one.
pub fn parse(line: &str) -> Option<Command> {
    let line = line.trim();
    let mut words = line.split_ascii_whitespace();
    let command = match words.next()? {
        "press" => Command::Press {
            button: button(words.next()?)?,
            ms: PRESS_MS,
        },
        "hold" => Command::Press {
            button: button(words.next()?)?,
            ms: words.next()?.parse().ok()?,
        },
        "shot" => Command::Shot,
        "screen" => Command::Screen,
        "describe" => Command::Describe,
        "heap" => Command::Heap,
        "boot" => Command::Boot,
        "input" => Command::Input,
        "run" => Command::Run(String::from(words.next()?)),
        "open" => Command::Open(unhex(words.next()?)?),
        "set" => Command::Set {
            id: String::from(words.next()?),
            value: unhex(words.next().unwrap_or(""))?,
        },
        "setting" => Command::Setting(String::from(words.next()?)),
        "wifi" => Command::Network(match words.next()? {
            "scan" => Ask::Scan,
            "connect" => Ask::Connect {
                ssid: unhex(words.next()?)?,
                password: unhex(words.next()?)?,
            },
            "status" => Ask::Status,
            "off" => Ask::Off,
            _ => return None,
        }),
        "http" => match words.next()? {
            "get" => Command::Network(Ask::Get {
                url: String::from(words.next()?),
                save_to: words.next().map(String::from),
            }),
            _ => return None,
        },
        "hang" => Command::Hang(words.next()?.parse().ok()?),
        "nap" => Command::Nap(words.next()?.parse().ok()?),
        "gray" => Command::Gray(match words.next() {
            Some(path) => Some(unhex(path)?),
            None => None,
        }),
        "refresh" => Command::Refresh {
            full: on_off(words.next()?, "full", "fast")?,
        },
        "idle" => match words.next()? {
            "sleep" => Command::IdleSleep(words.next()?.parse().ok()?),
            _ => return None,
        },
        "trace" => Command::Trace(on_off(words.next()?, "start", "stop")?),
        "profile" => Command::Profile(on_off(words.next()?, "start", "stop")?),
        "peek" => {
            let at = u32::from_str_radix(words.next()?, 16).ok()?;
            let words: u32 = words.next()?.parse().ok()?;
            let end = at.checked_add(words.checked_mul(4)?)?;
            let fits = (1..=PEEK_WORDS).contains(&words)
                && at % 4 == 0
                && ROM.contains(&at)
                && end <= ROM.end;
            return fits.then_some(Command::Peek { at, words });
        }
        // The path is the rest of the line, spaces and all.
        "get" => {
            let path = line.split_once(' ')?.1.trim();
            return (!path.is_empty()).then(|| Command::Get(String::from(path)));
        }
        "put" => {
            let size = words.next()?.parse().ok()?;
            let path = line
                .split_once(' ')?
                .1
                .trim_start()
                .split_once(' ')?
                .1
                .trim();
            return (!path.is_empty()).then(|| Command::Put {
                size,
                path: String::from(path),
            });
        }
        _ => return None,
    };
    words.next().is_none().then_some(command)
}

/// `true` for `on`, `false` for `off`, nothing for any other word.
fn on_off(word: &str, on: &str, off: &str) -> Option<bool> {
    match word {
        w if w == on => Some(true),
        w if w == off => Some(false),
        _ => None,
    }
}

/// The X4's button of that name.
fn button(name: &str) -> Option<&'static str> {
    buttons::ALL.iter().find(|b| **b == name).copied()
}

/// Bytes written as hexadecimal, two digits each.
pub fn hex_bytes(hex: &[u8]) -> Option<Vec<u8>> {
    if !hex.len().is_multiple_of(2) {
        return None;
    }
    hex.as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            let digit = |c: u8| char::from(c).to_digit(16);
            Some((digit(pair[0])? << 4 | digit(pair[1])?) as u8)
        })
        .collect()
}

/// Text sent as hexadecimal (`-` for none), so that it may hold spaces.
pub fn unhex(word: &str) -> Option<String> {
    if word == "-" {
        return Some(String::new());
    }
    String::from_utf8(hex_bytes(word.as_bytes())?).ok()
}

/// `bytes` as runs of one byte, written in hexadecimal: a count from 1 to
/// 255, then the byte.
pub fn runs(bytes: &[u8], out: &mut String) {
    let mut rest = bytes;
    while let Some(&byte) = rest.first() {
        let count = rest.iter().take(255).take_while(|b| **b == byte).count();
        let _ = write!(out, "{count:02x}{byte:02x}");
        rest = &rest[count..];
    }
}

/// The state of a part `describe` sends: `focused`, `on` or `off` joined
/// by `+`, or `-` for none.
pub fn part_state(focused: bool, on: Option<bool>) -> String {
    let words = [
        (focused, "focused"),
        (on == Some(true), "on"),
        (on == Some(false), "off"),
    ];
    let mut state = String::new();
    for (_, word) in words.iter().filter(|(on, _)| *on) {
        if !state.is_empty() {
            state.push('+');
        }
        state.push_str(word);
    }
    if state.is_empty() {
        state.push('-');
    }
    state
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presses_and_holds() {
        assert_eq!(
            parse("press confirm"),
            Some(Command::Press {
                button: "confirm",
                ms: PRESS_MS
            })
        );
        assert_eq!(
            parse(" hold power 1500 "),
            Some(Command::Press {
                button: "power",
                ms: 1500
            })
        );
        assert_eq!(parse("press nothing"), None);
        assert_eq!(parse("press confirm now"), None);
    }

    #[test]
    fn paths_keep_their_spaces() {
        assert_eq!(
            parse("get /books/a book.epub"),
            Some(Command::Get(String::from("/books/a book.epub")))
        );
        assert_eq!(
            parse("put 12 /books/a book.epub"),
            Some(Command::Put {
                size: 12,
                path: String::from("/books/a book.epub")
            })
        );
        assert_eq!(parse("get"), None);
        assert_eq!(parse("put 12"), None);
        assert_eq!(
            parse("open 2f612062"),
            Some(Command::Open(String::from("/a b")))
        );
        assert_eq!(
            parse("set reader.font-size 3132"),
            Some(Command::Set {
                id: String::from("reader.font-size"),
                value: String::from("12")
            })
        );
        assert_eq!(
            parse("set core.time-zone-rule"),
            Some(Command::Set {
                id: String::from("core.time-zone-rule"),
                value: String::new()
            })
        );
        assert_eq!(
            parse("setting core.night-mode"),
            Some(Command::Setting(String::from("core.night-mode")))
        );
    }

    #[test]
    fn networks_in_hexadecimal() {
        assert_eq!(
            parse("wifi connect 4e6574 -"),
            Some(Command::Network(Ask::Connect {
                ssid: String::from("Net"),
                password: String::new()
            }))
        );
        assert_eq!(parse("wifi connect 4e6 -"), None);
        assert_eq!(parse("wifi connect zz -"), None);
        assert_eq!(
            parse("http get http://example.org/ /a"),
            Some(Command::Network(Ask::Get {
                url: String::from("http://example.org/"),
                save_to: Some(String::from("/a"))
            }))
        );
    }

    #[test]
    fn peeks_only_into_the_rom() {
        assert_eq!(
            parse("peek 40000000 16"),
            Some(Command::Peek {
                at: 0x4000_0000,
                words: 16
            })
        );
        assert_eq!(parse("peek 40000002 1"), None);
        assert_eq!(parse("peek 3ffffffc 1"), None);
        assert_eq!(parse("peek 4005fffc 2"), None);
        assert_eq!(parse("peek 40000000 0"), None);
        assert_eq!(parse("peek ffffffff 4096"), None);
    }

    #[test]
    fn switches_and_unknown_lines() {
        assert_eq!(parse("refresh full"), Some(Command::Refresh { full: true }));
        assert_eq!(parse("trace stop"), Some(Command::Trace(false)));
        assert_eq!(parse("idle sleep 30"), Some(Command::IdleSleep(30)));
        assert_eq!(parse("refresh half"), None);
        assert_eq!(parse("dance"), None);
        assert_eq!(parse(""), None);
    }

    #[test]
    fn rows_go_as_runs() {
        let mut out = String::new();
        runs(&[0xFF, 0xFF, 0xFF, 0x00], &mut out);
        assert_eq!(out, "03ff0100");
        out.clear();
        runs(&[7; 300], &mut out);
        assert_eq!(out, "ff072d07");
    }

    #[test]
    fn data_lines() {
        assert_eq!(hex_bytes(b"00ff10"), Some(alloc::vec![0, 0xFF, 0x10]));
        assert_eq!(hex_bytes(b"0g"), None);
        assert_eq!(hex_bytes(b"001"), None);
    }

    #[test]
    fn states_of_parts() {
        assert_eq!(part_state(false, None), "-");
        assert_eq!(part_state(true, Some(true)), "focused+on");
        assert_eq!(part_state(false, Some(false)), "off");
    }
}
