//! Plugins' sockets (see [ADR 0014](../../../docs/adr/0014-sockets-for-plugins.md)):
//! TCP connections and UDP broadcasts the shell makes, numbered for it,
//! and what it reports turned into the plugins' events. A file read goes
//! to storage in parts as they come; text keeps its characters whole
//! where a read ends inside one.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_platform::Storage;
use paperpaw_sdk::event::EventData;
use paperpaw_sdk::{Command, Id, SocketNotice, SocketRead};

/// The most a shell is asked to read at once; a file read goes in such
/// parts.
pub const SOCKET_PART: u32 = 64 * 1024;

/// What a shell does for plugins' sockets ([`Io::Socket`](super::Io::Socket)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SocketCall {
    /// Open a TCP connection; report `Connected` or `Failed`.
    Connect {
        socket: u32,
        host: String,
        port: u16,
    },
    Send {
        socket: u32,
        data: Vec<u8>,
    },
    /// Report what comes next as one `Data` of at most `max` bytes (at
    /// least one, or none when the other side closed).
    Read {
        socket: u32,
        max: u32,
    },
    Close {
        socket: u32,
    },
    /// Send `data` to the local network's broadcast address on each of
    /// `ports`; report each answer within `wait_ms` as `Answer`, then
    /// `Done`.
    Broadcast {
        broadcast: u32,
        ports: Vec<u16>,
        data: Vec<u8>,
        wait_ms: u32,
    },
}

/// What happened with a socket or a broadcast (see `Kernel::socket_event`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SocketEvent {
    Connected {
        socket: u32,
    },
    /// What a `Read` brought; empty when the other side closed.
    Data {
        socket: u32,
        data: Vec<u8>,
    },
    /// The connection ended or could not be made.
    Failed {
        socket: u32,
        reason: String,
    },
    Answer {
        broadcast: u32,
        from: String,
        data: Vec<u8>,
    },
    Done {
        broadcast: u32,
    },
}

/// What a socket is reading.
#[derive(Debug)]
enum Reading {
    /// Text, up to `max` bytes a read.
    Text { max: u32 },
    /// Into `path`, `left` bytes still to come.
    File { path: String, left: u64 },
}

#[derive(Debug)]
struct Socket {
    number: u32,
    id: Id,
    events: Id,
    /// Screens open when it was opened; it closes once fewer are.
    screens: usize,
    reading: Option<Reading>,
    /// The start of a character a text read ended in (at most 3 bytes),
    /// kept for the next one.
    partial: Vec<u8>,
}

/// The sockets and broadcasts of all plugins.
#[derive(Debug, Default)]
pub(crate) struct Sockets {
    sockets: Vec<Socket>,
    /// Broadcasts waiting for answers: number, id, events.
    broadcasts: Vec<(u32, Id, Id)>,
    next: u32,
    calls: Vec<SocketCall>,
}

/// The command that tells a plugin `notice` on `events`.
fn emit(events: &Id, notice: SocketNotice) -> Command {
    Command::Emit(events.clone(), notice.to_params())
}

impl Sockets {
    /// What the shell is to do.
    pub fn take(&mut self) -> Vec<SocketCall> {
        core::mem::take(&mut self.calls)
    }

    /// Whether calls wait for the shell (for a network).
    pub fn waiting(&self) -> bool {
        !self.calls.is_empty()
    }

    /// The calls waited too long for a network: each fails, for `reason`
    /// (a broadcast ends).
    pub fn fail_waiting(&mut self, reason: &str, storage: &dyn Storage, out: &mut Vec<Command>) {
        for call in self.take() {
            let event = match call {
                SocketCall::Connect { socket, .. }
                | SocketCall::Send { socket, .. }
                | SocketCall::Read { socket, .. }
                | SocketCall::Close { socket } => SocketEvent::Failed {
                    socket,
                    reason: String::from(reason),
                },
                SocketCall::Broadcast { broadcast, .. } => SocketEvent::Done { broadcast },
            };
            self.event(event, storage, out);
        }
    }

    fn number(&mut self) -> u32 {
        self.next = self.next.wrapping_add(1);
        self.next
    }

    fn at(&self, id: &Id) -> Option<usize> {
        self.sockets.iter().position(|s| s.id == *id)
    }

    /// Carries out a plugin's socket command, with `network` whether the
    /// device has one and `screens` the screens open now; events to emit
    /// go to `out`.
    pub fn command(
        &mut self,
        command: Command,
        network: bool,
        screens: usize,
        storage: &dyn Storage,
        out: &mut Vec<Command>,
    ) {
        match command {
            Command::Connect {
                socket,
                host,
                port,
                events,
            } => {
                if let Some(at) = self.at(&socket) {
                    let old = self.sockets.remove(at);
                    self.calls.push(SocketCall::Close { socket: old.number });
                }
                if !network {
                    let reason = String::from("no network");
                    out.push(emit(&events, SocketNotice::Failed { socket, reason }));
                    return;
                }
                let number = self.number();
                self.sockets.push(Socket {
                    number,
                    id: socket,
                    events,
                    screens,
                    reading: None,
                    partial: Vec::new(),
                });
                self.calls.push(SocketCall::Connect {
                    socket: number,
                    host,
                    port,
                });
            }
            Command::SocketSend { socket, data } => {
                if let Some(at) = self.at(&socket) {
                    let number = self.sockets[at].number;
                    self.calls.push(SocketCall::Send {
                        socket: number,
                        data,
                    });
                }
            }
            Command::SocketRead { socket, read } => {
                let Some(at) = self.at(&socket) else {
                    return;
                };
                let s = &mut self.sockets[at];
                if s.reading.is_some() {
                    let reason = String::from("already reading");
                    out.push(s.notice(|socket| SocketNotice::Failed { socket, reason }));
                    return;
                }
                let max = match read {
                    SocketRead::Text { max } => {
                        let max = max.clamp(1, SOCKET_PART);
                        s.reading = Some(Reading::Text { max });
                        max
                    }
                    SocketRead::File { path, len } => {
                        if let Err(e) = storage.write(&path, &[]) {
                            let reason = format!("{e}");
                            out.push(s.notice(|socket| SocketNotice::Failed { socket, reason }));
                            return;
                        }
                        if len == 0 {
                            out.push(s.notice(|socket| SocketNotice::Saved { socket, path }));
                            return;
                        }
                        let max = len.min(u64::from(SOCKET_PART)) as u32;
                        s.reading = Some(Reading::File { path, left: len });
                        max
                    }
                };
                self.calls.push(SocketCall::Read {
                    socket: s.number,
                    max,
                });
            }
            Command::Disconnect(socket) => {
                if let Some(at) = self.at(&socket) {
                    self.close(at, None, out);
                }
            }
            Command::Broadcast {
                id,
                ports,
                data,
                wait_ms,
                events,
            } => {
                if !network {
                    let reason = String::from("no network");
                    out.push(emit(&events, SocketNotice::Done { socket: id, reason }));
                    return;
                }
                let number = self.number();
                self.broadcasts.retain(|(_, b, _)| *b != id);
                self.broadcasts.push((number, id, events));
                self.calls.push(SocketCall::Broadcast {
                    broadcast: number,
                    ports,
                    data,
                    wait_ms,
                });
            }
            _ => {}
        }
    }

    /// Closes socket `at` for the shell and tells its plugin, with why if
    /// it was not asked to.
    fn close(&mut self, at: usize, reason: Option<String>, out: &mut Vec<Command>) {
        let socket = self.sockets.remove(at);
        self.calls.push(SocketCall::Close {
            socket: socket.number,
        });
        let reason = reason.unwrap_or_default();
        out.push(socket.notice(|socket| SocketNotice::Closed { socket, reason }));
    }

    /// Closes the sockets opened while more than `screens` screens were
    /// open.
    pub fn close_for_closed_screens(&mut self, screens: usize, out: &mut Vec<Command>) {
        while let Some(at) = self.sockets.iter().position(|s| s.screens > screens) {
            self.close(at, Some(String::from("screen closed")), out);
        }
    }

    /// Whether a socket or broadcast is open, e.g. to keep polling.
    pub fn any(&self) -> bool {
        !self.sockets.is_empty() || !self.broadcasts.is_empty()
    }

    /// Turns what the shell reports into the plugins' events.
    pub fn event(&mut self, event: SocketEvent, storage: &dyn Storage, out: &mut Vec<Command>) {
        match event {
            SocketEvent::Connected { socket } => {
                if let Some(s) = self.sockets.iter().find(|s| s.number == socket) {
                    out.push(s.notice(|socket| SocketNotice::Connected { socket }));
                }
            }
            SocketEvent::Failed { socket, reason } => {
                if let Some(at) = self.sockets.iter().position(|s| s.number == socket) {
                    let s = self.sockets.remove(at);
                    out.push(s.notice(|socket| SocketNotice::Failed { socket, reason }));
                }
            }
            SocketEvent::Data { socket, data } => {
                let Some(at) = self.sockets.iter().position(|s| s.number == socket) else {
                    return;
                };
                self.received(at, data, storage, out);
            }
            SocketEvent::Answer {
                broadcast,
                from,
                data,
            } => {
                if let Some((_, id, events)) =
                    self.broadcasts.iter().find(|(n, ..)| *n == broadcast)
                {
                    let text = String::from_utf8_lossy(&data).into_owned();
                    let socket = id.clone();
                    out.push(emit(events, SocketNotice::Answer { socket, text, from }));
                }
            }
            SocketEvent::Done { broadcast } => {
                if let Some(at) = self.broadcasts.iter().position(|(n, ..)| *n == broadcast) {
                    let (_, socket, events) = self.broadcasts.remove(at);
                    let reason = String::new();
                    out.push(emit(&events, SocketNotice::Done { socket, reason }));
                }
            }
        }
    }

    /// What a read of socket `at` brought.
    fn received(
        &mut self,
        at: usize,
        data: Vec<u8>,
        storage: &dyn Storage,
        out: &mut Vec<Command>,
    ) {
        let s = &mut self.sockets[at];
        match s.reading.take() {
            // The other side closed; half a character is no text.
            Some(Reading::Text { .. }) if data.is_empty() => {
                s.partial.clear();
                let text = String::new();
                out.push(s.notice(|socket| SocketNotice::Received { socket, text }));
            }
            Some(Reading::Text { max }) => {
                let mut bytes = core::mem::take(&mut s.partial);
                bytes.extend_from_slice(&data);
                let whole = bytes.len() - incomplete_end(&bytes);
                s.partial = bytes.split_off(whole);
                if bytes.is_empty() {
                    // Only the start of a character came: read on, as
                    // empty text would say the other side closed.
                    s.reading = Some(Reading::Text { max });
                    let socket = s.number;
                    self.calls.push(SocketCall::Read { socket, max });
                    return;
                }
                let text = String::from_utf8(bytes)
                    .unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned());
                out.push(s.notice(|socket| SocketNotice::Received { socket, text }));
            }
            Some(Reading::File { path, left }) => {
                if data.is_empty() {
                    let reason = String::from("closed before the file came");
                    return self.close(at, Some(reason), out);
                }
                if let Err(e) = storage.append(&path, &data) {
                    let reason = format!("{e}");
                    return self.close(at, Some(reason), out);
                }
                let left = left.saturating_sub(data.len() as u64);
                if left == 0 {
                    out.push(s.notice(|socket| SocketNotice::Saved { socket, path }));
                } else {
                    let max = left.min(u64::from(SOCKET_PART)) as u32;
                    s.reading = Some(Reading::File { path, left });
                    self.calls.push(SocketCall::Read {
                        socket: s.number,
                        max,
                    });
                }
            }
            // Data not asked for: shells send none.
            None => {}
        }
    }
}

/// How many bytes at the end of `bytes` begin a UTF-8 character that is
/// not complete yet (0 to 3).
fn incomplete_end(bytes: &[u8]) -> usize {
    let start = bytes.len().saturating_sub(3);
    for at in (start..bytes.len()).rev() {
        let byte = bytes[at];
        if byte & 0xC0 == 0x80 {
            // A continuation byte: its character starts before.
            continue;
        }
        let length = match byte {
            0xC0..=0xDF => 2,
            0xE0..=0xEF => 3,
            0xF0..=0xF7 => 4,
            _ => 1,
        };
        let present = bytes.len() - at;
        return if present < length { present } else { 0 };
    }
    0
}

impl Socket {
    /// The command that tells its plugin the notice `make` makes of its id.
    fn notice(&self, make: impl FnOnce(Id) -> SocketNotice) -> Command {
        emit(&self.events, make(self.id.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperpaw_sdk::MemoryStorage;
    use paperpaw_sdk::builtin::params;
    use paperpaw_sdk::socket_params as p;

    const EVENTS: Id = Id::new_static("test.socket-events");
    const SOCKET: Id = Id::new_static("test.socket");

    fn kinds(out: &[Command]) -> Vec<String> {
        out.iter()
            .filter_map(|c| match c {
                Command::Emit(_, params) => params.get(p::KIND).map(String::from),
                _ => None,
            })
            .collect()
    }

    fn connected(storage: &MemoryStorage) -> (Sockets, Vec<Command>) {
        let mut sockets = Sockets::default();
        let mut out = Vec::new();
        let connect = Command::Connect {
            socket: SOCKET,
            host: String::from("calibre.local"),
            port: 9090,
            events: EVENTS,
        };
        sockets.command(connect, true, 2, storage, &mut out);
        assert_eq!(
            sockets.calls.pop(),
            Some(SocketCall::Connect {
                socket: 1,
                host: String::from("calibre.local"),
                port: 9090
            })
        );
        sockets.event(SocketEvent::Connected { socket: 1 }, storage, &mut out);
        (sockets, out)
    }

    #[test]
    fn text_is_read_when_asked() {
        let storage = MemoryStorage::new();
        let (mut sockets, mut out) = connected(&storage);
        let read = Command::SocketRead {
            socket: SOCKET,
            read: SocketRead::Text { max: 100 },
        };
        sockets.command(read.clone(), true, 2, &storage, &mut out);
        // A second read before the first came fails.
        sockets.command(read, true, 2, &storage, &mut out);
        assert_eq!(
            sockets.calls,
            [SocketCall::Read {
                socket: 1,
                max: 100
            }]
        );
        let data = SocketEvent::Data {
            socket: 1,
            data: b"12[0,{}]".to_vec(),
        };
        sockets.event(data, &storage, &mut out);
        assert_eq!(kinds(&out), [p::CONNECTED, p::FAILED, p::RECEIVED]);
        let Some(Command::Emit(_, params)) = out.last() else {
            panic!("no event");
        };
        assert_eq!(params.get(p::TEXT), Some("12[0,{}]"));
        assert_eq!(params.get(params::ID), Some(SOCKET.as_str()));
    }

    #[test]
    fn characters_split_between_reads_stay_whole() {
        let storage = MemoryStorage::new();
        let (mut sockets, mut out) = connected(&storage);
        let mut texts = Vec::new();
        // "Grüße 🐾" with each read ending inside a character, one read
        // bringing only the start of one.
        let reads: [&[u8]; 5] = [b"Gr\xc3", b"\xbc\xc3", b"\x9fe \xf0", b"\x9f\x90", b"\xbe"];
        for data in reads {
            out.clear();
            let read = Command::SocketRead {
                socket: SOCKET,
                read: SocketRead::Text { max: 4 },
            };
            if sockets.calls.is_empty() {
                sockets.command(read, true, 2, &storage, &mut out);
            }
            assert_eq!(
                sockets.calls.pop(),
                Some(SocketCall::Read { socket: 1, max: 4 })
            );
            let data = SocketEvent::Data {
                socket: 1,
                data: data.to_vec(),
            };
            sockets.event(data, &storage, &mut out);
            if let Some(Command::Emit(_, params)) = out.last() {
                texts.push(String::from(params.get(p::TEXT).unwrap()));
            }
        }
        assert_eq!(texts, ["Gr", "ü", "ße ", "🐾"]);
        // An invalid byte is replaced, as before.
        let read = Command::SocketRead {
            socket: SOCKET,
            read: SocketRead::Text { max: 4 },
        };
        sockets.command(read, true, 2, &storage, &mut out);
        let data = SocketEvent::Data {
            socket: 1,
            data: b"a\xffb".to_vec(),
        };
        sockets.event(data, &storage, &mut out);
        let Some(Command::Emit(_, params)) = out.last() else {
            panic!("no event");
        };
        assert_eq!(params.get(p::TEXT), Some("a\u{fffd}b"));
    }

    #[test]
    fn files_come_in_parts_into_storage() {
        let storage = MemoryStorage::new();
        let (mut sockets, mut out) = connected(&storage);
        let len = u64::from(SOCKET_PART) + 10;
        let read = Command::SocketRead {
            socket: SOCKET,
            read: SocketRead::File {
                path: String::from("books/new.epub"),
                len,
            },
        };
        sockets.command(read, true, 2, &storage, &mut out);
        assert_eq!(
            sockets.calls.pop(),
            Some(SocketCall::Read {
                socket: 1,
                max: SOCKET_PART
            })
        );
        let part = alloc::vec![7u8; SOCKET_PART as usize];
        sockets.event(
            SocketEvent::Data {
                socket: 1,
                data: part,
            },
            &storage,
            &mut out,
        );
        assert_eq!(
            sockets.calls.pop(),
            Some(SocketCall::Read { socket: 1, max: 10 })
        );
        let rest = alloc::vec![8u8; 10];
        sockets.event(
            SocketEvent::Data {
                socket: 1,
                data: rest,
            },
            &storage,
            &mut out,
        );
        assert_eq!(kinds(&out), [p::CONNECTED, p::SAVED]);
        assert_eq!(storage.size("books/new.epub").unwrap(), len);
        assert!(sockets.calls.is_empty());
    }

    #[test]
    fn closing_too_early_closes_the_socket() {
        let storage = MemoryStorage::new();
        let (mut sockets, mut out) = connected(&storage);
        let read = Command::SocketRead {
            socket: SOCKET,
            read: SocketRead::File {
                path: String::from("b.epub"),
                len: 5,
            },
        };
        sockets.command(read, true, 2, &storage, &mut out);
        sockets.calls.clear();
        sockets.event(
            SocketEvent::Data {
                socket: 1,
                data: Vec::new(),
            },
            &storage,
            &mut out,
        );
        assert_eq!(kinds(&out), [p::CONNECTED, p::CLOSED]);
        assert_eq!(sockets.calls, [SocketCall::Close { socket: 1 }]);
        assert!(!sockets.any());
    }

    #[test]
    fn sockets_end_with_their_screen() {
        let storage = MemoryStorage::new();
        let (mut sockets, mut out) = connected(&storage);
        sockets.close_for_closed_screens(2, &mut out);
        assert!(sockets.any());
        sockets.close_for_closed_screens(1, &mut out);
        assert!(!sockets.any());
        assert_eq!(kinds(&out), [p::CONNECTED, p::CLOSED]);
    }

    #[test]
    fn without_a_network_everything_fails_at_once() {
        let storage = MemoryStorage::new();
        let mut sockets = Sockets::default();
        let mut out = Vec::new();
        let connect = Command::Connect {
            socket: SOCKET,
            host: String::from("h"),
            port: 1,
            events: EVENTS,
        };
        sockets.command(connect, false, 1, &storage, &mut out);
        let broadcast = Command::Broadcast {
            id: SOCKET,
            ports: alloc::vec![54982],
            data: b"hi".to_vec(),
            wait_ms: 100,
            events: EVENTS,
        };
        sockets.command(broadcast, false, 1, &storage, &mut out);
        assert_eq!(kinds(&out), [p::FAILED, p::DONE]);
        assert!(sockets.calls.is_empty());
    }

    #[test]
    fn broadcast_answers_then_done() {
        let storage = MemoryStorage::new();
        let mut sockets = Sockets::default();
        let mut out = Vec::new();
        let broadcast = Command::Broadcast {
            id: SOCKET,
            ports: alloc::vec![54982, 48123],
            data: b"hi".to_vec(),
            wait_ms: 500,
            events: EVENTS,
        };
        sockets.command(broadcast, true, 1, &storage, &mut out);
        assert!(matches!(
            sockets.calls[0],
            SocketCall::Broadcast { broadcast: 1, .. }
        ));
        let answer = SocketEvent::Answer {
            broadcast: 1,
            from: String::from("192.168.1.5"),
            data: b"calibre wireless device client (on pc);8080,9090".to_vec(),
        };
        sockets.event(answer, &storage, &mut out);
        sockets.event(SocketEvent::Done { broadcast: 1 }, &storage, &mut out);
        assert_eq!(kinds(&out), [p::ANSWER, p::DONE]);
        let Command::Emit(_, params) = &out[0] else {
            panic!("no answer");
        };
        assert_eq!(params.get(p::FROM), Some("192.168.1.5"));
        assert!(!sockets.any());
    }
}
