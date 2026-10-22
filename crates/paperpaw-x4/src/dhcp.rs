//! Handing out addresses in the device's own network (the setup with a
//! phone, `WifiCommand::Host`): the few messages of a DHCP server (RFC
//! 2131) a phone needs to join, an address from a small range for each
//! device that asks, kept by its hardware address.
//!
//! The device is the network's router too, so phones treat it as a
//! network of its own (without a way out, which they say); no name
//! server is named: the page is opened by its address.

use alloc::vec::Vec;

/// The port the server listens on, and the one answers go to.
pub const SERVER_PORT: u16 = 67;
pub const CLIENT_PORT: u16 = 68;
/// Devices given an address at most (the first from `.2` on).
const LEASES: usize = 4;
/// How long an address is given for, in seconds.
const LEASE_SECONDS: u32 = 3600;
/// Where the options begin, after the fixed part and the magic cookie.
const OPTIONS: usize = 240;
const COOKIE: [u8; 4] = [99, 130, 83, 99];

/// The kinds of messages (option 53).
const DISCOVER: u8 = 1;
const OFFER: u8 = 2;
const REQUEST: u8 = 3;
const ACK: u8 = 5;
const NAK: u8 = 6;
const RELEASE: u8 = 7;

/// The server: its own address (the network is its /24) and the
/// addresses given.
#[derive(Debug, Clone)]
pub struct Server {
    address: [u8; 4],
    leases: [Option<[u8; 6]>; LEASES],
}

impl Server {
    pub fn new(address: [u8; 4]) -> Self {
        Self {
            address,
            leases: [None; LEASES],
        }
    }

    /// The answer to `request`, a message a client sent to the server's
    /// port: an offer, an acknowledgement or a refusal, to broadcast to
    /// [`CLIENT_PORT`]; `None` for what needs none (a release, another
    /// server's business, not DHCP).
    pub fn answer(&mut self, request: &[u8]) -> Option<Vec<u8>> {
        // A request from a client with an Ethernet address and the cookie.
        if request.len() < OPTIONS || request[0] != 1 || request[1] != 1 || request[2] != 6 {
            return None;
        }
        if request[236..OPTIONS] != COOKIE {
            return None;
        }
        let mut hardware = [0u8; 6];
        hardware.copy_from_slice(&request[28..34]);
        let kind = option(request, 53).and_then(|v| v.first().copied())?;
        match kind {
            DISCOVER => {
                let given = self.lease(hardware)?;
                Some(self.reply(request, OFFER, given))
            }
            REQUEST => {
                // Another server chosen: nothing to say.
                if let Some(server) = option(request, 54)
                    && server != self.address
                {
                    return None;
                }
                let wanted = option(request, 50)
                    .and_then(|v| <[u8; 4]>::try_from(v).ok())
                    .unwrap_or_else(|| {
                        let mut renewing = [0u8; 4];
                        renewing.copy_from_slice(&request[12..16]);
                        renewing
                    });
                match self.lease(hardware) {
                    Some(given) if given == wanted => Some(self.reply(request, ACK, given)),
                    _ => Some(self.reply(request, NAK, [0; 4])),
                }
            }
            RELEASE => {
                if let Some(slot) = self.leases.iter_mut().find(|l| **l == Some(hardware)) {
                    *slot = None;
                }
                None
            }
            _ => None,
        }
    }

    /// The address of device `hardware`: the one it has, or the next one
    /// free; `None` when all are given.
    fn lease(&mut self, hardware: [u8; 6]) -> Option<[u8; 4]> {
        let slot = match self.leases.iter().position(|l| *l == Some(hardware)) {
            Some(slot) => slot,
            None => {
                let free = self.leases.iter().position(Option::is_none)?;
                self.leases[free] = Some(hardware);
                free
            }
        };
        let [a, b, c, _] = self.address;
        Some([a, b, c, 2 + slot as u8])
    }

    /// An answer of `kind` to `request`, giving `address`.
    fn reply(&self, request: &[u8], kind: u8, address: [u8; 4]) -> Vec<u8> {
        let mut out = alloc::vec![0u8; OPTIONS];
        // A reply, Ethernet, its transaction and flags as asked.
        out[0] = 2;
        out[1] = 1;
        out[2] = 6;
        out[4..8].copy_from_slice(&request[4..8]);
        out[10..12].copy_from_slice(&request[10..12]);
        out[16..20].copy_from_slice(&address);
        out[20..24].copy_from_slice(&self.address);
        out[28..44].copy_from_slice(&request[28..44]);
        out[236..OPTIONS].copy_from_slice(&COOKIE);
        out.extend_from_slice(&[53, 1, kind]);
        out.extend_from_slice(&[54, 4]);
        out.extend_from_slice(&self.address);
        if kind != NAK {
            out.extend_from_slice(&[51, 4]);
            out.extend_from_slice(&LEASE_SECONDS.to_be_bytes());
            out.extend_from_slice(&[1, 4, 255, 255, 255, 0]);
            out.extend_from_slice(&[3, 4]);
            out.extend_from_slice(&self.address);
        }
        out.push(255);
        // The least a BOOTP message is.
        if out.len() < 300 {
            out.resize(300, 0);
        }
        out
    }
}

/// The value of option `code` in `message`, if it has it.
fn option(message: &[u8], code: u8) -> Option<&[u8]> {
    let mut at = OPTIONS;
    while at < message.len() {
        match message[at] {
            0 => at += 1,
            255 => return None,
            found => {
                let len = usize::from(*message.get(at + 1)?);
                let value = message.get(at + 2..at + 2 + len)?;
                if found == code {
                    return Some(value);
                }
                at += 2 + len;
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const SERVER: [u8; 4] = [192, 168, 4, 1];

    /// A client's message of `kind` from `hardware`, with `options` more.
    fn message(kind: u8, hardware: [u8; 6], options: &[u8]) -> Vec<u8> {
        let mut m = alloc::vec![0u8; OPTIONS];
        m[0] = 1;
        m[1] = 1;
        m[2] = 6;
        m[4..8].copy_from_slice(&[1, 2, 3, 4]);
        m[10] = 0x80;
        m[28..34].copy_from_slice(&hardware);
        m[236..240].copy_from_slice(&COOKIE);
        m.extend_from_slice(&[53, 1, kind]);
        m.extend_from_slice(options);
        m.push(255);
        m
    }

    fn kind(answer: &[u8]) -> u8 {
        option(answer, 53).unwrap()[0]
    }

    #[test]
    fn a_phone_is_offered_an_address_and_gets_it() {
        let mut server = Server::new(SERVER);
        let phone = [1, 1, 1, 1, 1, 1];
        let offer = server.answer(&message(DISCOVER, phone, &[])).unwrap();
        assert_eq!(kind(&offer), OFFER);
        assert_eq!(offer[16..20], [192, 168, 4, 2]);
        // Its transaction, flags and hardware address as it sent them.
        assert_eq!(offer[4..8], [1, 2, 3, 4]);
        assert_eq!(offer[10], 0x80);
        assert_eq!(offer[28..34], phone);
        assert_eq!(option(&offer, 54), Some(&SERVER[..]));
        assert_eq!(option(&offer, 3), Some(&SERVER[..]));
        assert_eq!(option(&offer, 1), Some(&[255, 255, 255, 0][..]));
        assert!(offer.len() >= 300);
        let ack = server
            .answer(&message(
                REQUEST,
                phone,
                &[50, 4, 192, 168, 4, 2, 54, 4, 192, 168, 4, 1],
            ))
            .unwrap();
        assert_eq!(kind(&ack), ACK);
        assert_eq!(ack[16..20], [192, 168, 4, 2]);
    }

    #[test]
    fn each_device_has_its_own_address_and_keeps_it() {
        let mut server = Server::new(SERVER);
        let a = server.answer(&message(DISCOVER, [1; 6], &[])).unwrap();
        let b = server.answer(&message(DISCOVER, [2; 6], &[])).unwrap();
        assert_eq!(a[16..20], [192, 168, 4, 2]);
        assert_eq!(b[16..20], [192, 168, 4, 3]);
        let again = server.answer(&message(DISCOVER, [1; 6], &[])).unwrap();
        assert_eq!(again[16..20], [192, 168, 4, 2]);
        // Given back, its address goes to the next.
        assert!(server.answer(&message(RELEASE, [1; 6], &[])).is_none());
        let c = server.answer(&message(DISCOVER, [3; 6], &[])).unwrap();
        assert_eq!(c[16..20], [192, 168, 4, 2]);
    }

    #[test]
    fn a_wrong_address_is_refused_and_others_are_passed_over() {
        let mut server = Server::new(SERVER);
        let nak = server
            .answer(&message(REQUEST, [1; 6], &[50, 4, 10, 0, 0, 9]))
            .unwrap();
        assert_eq!(kind(&nak), NAK);
        // Another server chosen, or not DHCP.
        assert!(
            server
                .answer(&message(REQUEST, [1; 6], &[54, 4, 10, 0, 0, 1]))
                .is_none()
        );
        assert!(server.answer(&[1, 2, 3]).is_none());
        let mut reply = message(DISCOVER, [1; 6], &[]);
        reply[0] = 2;
        assert!(server.answer(&reply).is_none());
        // All addresses given (one already to the device refused).
        for n in 1..LEASES as u8 {
            assert!(
                server
                    .answer(&message(DISCOVER, [10 + n; 6], &[]))
                    .is_some()
            );
        }
        assert!(server.answer(&message(DISCOVER, [99; 6], &[])).is_none());
    }
}
