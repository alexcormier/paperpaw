//! Handing out addresses in the device's own network (the setup with a
//! phone, `WifiCommand::Host`): the few messages of a DHCP server (RFC
//! 2131) a phone needs to join, an address from a small range for each
//! device that asks, kept by its hardware address. An address offered and
//! not taken is free again after a minute, one taken after its lease.
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
/// How long an address offered is kept for its device to take it.
const OFFER_SECONDS: u64 = 60;
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

/// An address given (or offered) to a device, until a time (seconds);
/// `taken` once the device took it (a phone that joined).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Lease {
    hardware: [u8; 6],
    until: u64,
    taken: bool,
}

/// The server: its own address (the network is its /24) and the
/// addresses given.
#[derive(Debug, Clone)]
pub struct Server {
    address: [u8; 4],
    leases: [Option<Lease>; LEASES],
}

impl Server {
    pub fn new(address: [u8; 4]) -> Self {
        Self {
            address,
            leases: [None; LEASES],
        }
    }

    /// The answer to `request`, a message a client sent to the server's
    /// port, at `now` (seconds, any start): an offer, an acknowledgement
    /// or a refusal, to broadcast to [`CLIENT_PORT`]; `None` for what
    /// needs none (a release, another server's business, not DHCP).
    pub fn answer(&mut self, request: &[u8], now: u64) -> Option<Vec<u8>> {
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
                let given = self.lease(hardware, now, OFFER_SECONDS)?;
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
                match self.lease(hardware, now, u64::from(LEASE_SECONDS)) {
                    Some(given) if given == wanted => {
                        self.take(hardware);
                        Some(self.reply(request, ACK, given))
                    }
                    _ => Some(self.reply(request, NAK, [0; 4])),
                }
            }
            RELEASE => {
                if let Some(slot) = self
                    .leases
                    .iter_mut()
                    .find(|l| l.is_some_and(|l| l.hardware == hardware))
                {
                    *slot = None;
                }
                None
            }
            _ => None,
        }
    }

    /// How many devices took an address and still have it at `now`
    /// (seconds, as [`Server::answer`]'s): the phones in the network.
    pub fn guests(&self, now: u64) -> u8 {
        let taken = self
            .leases
            .iter()
            .flatten()
            .filter(|l| l.taken && l.until > now)
            .count();
        u8::try_from(taken).unwrap_or(u8::MAX)
    }

    /// Device `hardware` took the address it was given.
    fn take(&mut self, hardware: [u8; 6]) {
        if let Some(lease) = self
            .leases
            .iter_mut()
            .flatten()
            .find(|l| l.hardware == hardware)
        {
            lease.taken = true;
        }
    }

    /// The address of device `hardware`, kept for it `seconds` from
    /// `now`: the one it has, or the next one free (never given, or no
    /// longer); `None` when all are given. An offer does not shorten a
    /// lease taken.
    fn lease(&mut self, hardware: [u8; 6], now: u64, seconds: u64) -> Option<[u8; 4]> {
        let until = now + seconds;
        let slot = match self
            .leases
            .iter()
            .position(|l| l.is_some_and(|l| l.hardware == hardware))
        {
            Some(slot) => slot,
            None => self
                .leases
                .iter()
                .position(|l| l.is_none_or(|l| l.until <= now))?,
        };
        let (kept, taken) = self.leases[slot]
            .filter(|l| l.hardware == hardware && l.until > now)
            .map_or((0, false), |l| (l.until, l.taken));
        self.leases[slot] = Some(Lease {
            hardware,
            until: until.max(kept),
            taken,
        });
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

    impl Server {
        /// An answer at time 0.
        fn answer_at(&mut self, request: &[u8]) -> Option<Vec<u8>> {
            self.answer(request, 0)
        }
    }

    fn kind(answer: &[u8]) -> u8 {
        option(answer, 53).unwrap()[0]
    }

    #[test]
    fn a_phone_is_offered_an_address_and_gets_it() {
        let mut server = Server::new(SERVER);
        let phone = [1, 1, 1, 1, 1, 1];
        let offer = server.answer_at(&message(DISCOVER, phone, &[])).unwrap();
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
            .answer_at(&message(
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
        let a = server.answer_at(&message(DISCOVER, [1; 6], &[])).unwrap();
        let b = server.answer_at(&message(DISCOVER, [2; 6], &[])).unwrap();
        assert_eq!(a[16..20], [192, 168, 4, 2]);
        assert_eq!(b[16..20], [192, 168, 4, 3]);
        let again = server.answer_at(&message(DISCOVER, [1; 6], &[])).unwrap();
        assert_eq!(again[16..20], [192, 168, 4, 2]);
        // Given back, its address goes to the next.
        assert!(server.answer_at(&message(RELEASE, [1; 6], &[])).is_none());
        let c = server.answer_at(&message(DISCOVER, [3; 6], &[])).unwrap();
        assert_eq!(c[16..20], [192, 168, 4, 2]);
    }

    #[test]
    fn a_wrong_address_is_refused_and_others_are_passed_over() {
        let mut server = Server::new(SERVER);
        let nak = server
            .answer_at(&message(REQUEST, [1; 6], &[50, 4, 10, 0, 0, 9]))
            .unwrap();
        assert_eq!(kind(&nak), NAK);
        // Another server chosen, or not DHCP.
        assert!(
            server
                .answer_at(&message(REQUEST, [1; 6], &[54, 4, 10, 0, 0, 1]))
                .is_none()
        );
        assert!(server.answer_at(&[1, 2, 3]).is_none());
        let mut reply = message(DISCOVER, [1; 6], &[]);
        reply[0] = 2;
        assert!(server.answer_at(&reply).is_none());
        // All addresses given (one already to the device refused).
        for n in 1..LEASES as u8 {
            assert!(
                server
                    .answer_at(&message(DISCOVER, [10 + n; 6], &[]))
                    .is_some()
            );
        }
        assert!(server.answer_at(&message(DISCOVER, [99; 6], &[])).is_none());
        // Offers not taken are free again after a minute.
        let later = server.answer(&message(DISCOVER, [99; 6], &[]), OFFER_SECONDS + 1);
        assert!(later.is_some());
    }

    #[test]
    fn a_lease_taken_is_kept_until_it_ends() {
        let mut server = Server::new(SERVER);
        let phone = [1; 6];
        server.answer(&message(DISCOVER, phone, &[]), 0).unwrap();
        let take = &[50, 4, 192, 168, 4, 2];
        server.answer(&message(REQUEST, phone, take), 0).unwrap();
        for n in 2..=LEASES as u8 {
            assert!(server.answer(&message(DISCOVER, [n; 6], &[]), 0).is_some());
        }
        // Long after the offers, before the lease ends: its address stays.
        let late = u64::from(LEASE_SECONDS) - 1;
        let other = server
            .answer(&message(DISCOVER, [50; 6], &[]), late)
            .unwrap();
        assert_ne!(other[16..20], [192, 168, 4, 2]);
        let after = u64::from(LEASE_SECONDS) + 1;
        let next = server.answer(&message(DISCOVER, [51; 6], &[]), after);
        assert!(next.is_some());
    }

    /// A device counts as a guest once it took its address, until its
    /// lease ends or it gives it back; an offer alone is no guest.
    #[test]
    fn guests_are_those_that_took_an_address() {
        let mut server = Server::new(SERVER);
        let phone = [1; 6];
        server.answer(&message(DISCOVER, phone, &[]), 0).unwrap();
        server.answer(&message(DISCOVER, [2; 6], &[]), 0).unwrap();
        assert_eq!(server.guests(0), 0);
        let take = &[50, 4, 192, 168, 4, 2];
        server.answer(&message(REQUEST, phone, take), 0).unwrap();
        assert_eq!(server.guests(1), 1);
        // Offered again (renewing): still a guest.
        server.answer(&message(DISCOVER, phone, &[]), 10).unwrap();
        assert_eq!(server.guests(10), 1);
        assert_eq!(server.guests(u64::from(LEASE_SECONDS) + 1), 0);
        server.answer(&message(RELEASE, phone, &[]), 20);
        assert_eq!(server.guests(20), 0);
    }
}
