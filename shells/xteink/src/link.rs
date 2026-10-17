//! The connection an HTTP request goes over: a TCP socket of `net`, plain
//! or with TLS over it (`tls`). Either way it is used alike: moved on
//! with [`Link::pump`], ready for the request once its handshake is done,
//! then plain bytes sent and received.

use alloc::boxed::Box;

use smoltcp::iface::SocketHandle;
use smoltcp::socket::tcp;

use crate::net::Net;
use crate::tls;

/// Plain bytes read from TLS at a time.
const READ_PART: usize = 1024;

pub struct Link {
    pub socket: SocketHandle,
    tls: Option<Box<tls::Session>>,
}

/// What receiving brought.
#[derive(Debug, PartialEq, Eq)]
pub enum Received {
    /// Bytes were passed on (or not, for now).
    Some(bool),
    /// The other side is done: nothing more comes.
    Closed,
}

fn tcp_ended(socket: &tcp::Socket<'_>) -> bool {
    matches!(
        socket.state(),
        tcp::State::CloseWait | tcp::State::Closed | tcp::State::TimeWait
    ) && !socket.can_recv()
}

impl Link {
    /// Over the open socket `socket`, with TLS if given a session (made
    /// first: it takes the most memory).
    pub fn new(socket: SocketHandle, tls: Option<Box<tls::Session>>) -> Self {
        Self { socket, tls }
    }

    /// Moves the encrypted bytes between the socket and TLS, and TLS's
    /// handshake on; whether anything moved.
    pub fn pump(&mut self, net: &mut Net) -> Result<bool, &'static str> {
        let Some(session) = self.tls.as_mut() else {
            return Ok(false);
        };
        let socket = net.socket(self.socket);
        let mut moved = false;
        while session.wants_input() && socket.can_recv() {
            let got = socket
                .recv(|data| {
                    session.feed(data);
                    (data.len(), data.len())
                })
                .map_err(|_| "connection lost")?;
            moved |= got > 0;
            if got == 0 {
                break;
            }
        }
        if tcp_ended(socket) {
            session.ended();
        }
        session.handshake().map_err(tls::Error::reason)?;
        moved |= Self::flush(session, socket)?;
        Ok(moved)
    }

    /// Sends what TLS gave to send, as far as the socket takes it.
    fn flush(
        session: &mut tls::Session,
        socket: &mut tcp::Socket<'_>,
    ) -> Result<bool, &'static str> {
        let output = session.output();
        if output.is_empty() || !socket.can_send() {
            return Ok(false);
        }
        let n = socket.send_slice(output).map_err(|_| "connection lost")?;
        session.sent(n);
        Ok(n > 0)
    }

    /// Where it is, for the log when it stalls.
    pub fn describe(&self, net: &mut Net) -> alloc::string::String {
        let socket = net.socket(self.socket);
        let state = socket.state();
        let (rx, tx) = (socket.recv_queue(), socket.send_queue());
        match &self.tls {
            None => alloc::format!("tcp {state} rx {rx} tx {tx}"),
            Some(session) => {
                alloc::format!("tcp {state} rx {rx} tx {tx}, tls {}", session.describe())
            }
        }
    }

    /// Ready for the request: connected, and TLS's handshake done.
    pub fn ready(&mut self, net: &mut Net) -> bool {
        let socket = net.socket(self.socket);
        if !socket.may_send() {
            return false;
        }
        match self.tls.as_mut() {
            None => true,
            Some(session) => session.handshake() == Ok(true),
        }
    }

    /// Sends plain bytes; how many were taken (0 when there is no room).
    pub fn send(&mut self, net: &mut Net, data: &[u8]) -> Result<usize, &'static str> {
        let socket = net.socket(self.socket);
        match self.tls.as_mut() {
            None if socket.can_send() => socket.send_slice(data).map_err(|_| "connection lost"),
            None => Ok(0),
            Some(session) => {
                let n = session.write(data).map_err(tls::Error::reason)?;
                Self::flush(session, socket)?;
                Ok(n)
            }
        }
    }

    /// Passes the plain bytes that came on to `take`, a piece at a time,
    /// while `more` says so.
    pub fn receive(
        &mut self,
        net: &mut Net,
        more: &mut dyn FnMut() -> bool,
        take: &mut dyn FnMut(&[u8]),
    ) -> Result<Received, &'static str> {
        let socket = net.socket(self.socket);
        let mut moved = false;
        match self.tls.as_mut() {
            None => {
                while more() && socket.can_recv() {
                    socket
                        .recv(|data| {
                            take(data);
                            (data.len(), ())
                        })
                        .map_err(|_| "connection lost")?;
                    moved = true;
                }
                if more() && tcp_ended(socket) {
                    return Ok(Received::Closed);
                }
            }
            Some(session) => {
                let mut part = [0u8; READ_PART];
                while more() {
                    match session.read(&mut part).map_err(tls::Error::reason)? {
                        tls::Read::Data(n) => {
                            take(&part[..n]);
                            moved = true;
                        }
                        tls::Read::Wait => break,
                        tls::Read::Closed => return Ok(Received::Closed),
                    }
                }
            }
        }
        Ok(Received::Some(moved))
    }
}
