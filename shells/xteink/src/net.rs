//! The network over Wi-Fi: smoltcp (a TCP/IP stack made for devices
//! without an operating system) on the radio's station interface. It is
//! polled: from the main loop, and in a loop while something waits for an
//! answer (QuireInk's firmware runs no async code). Its address, router and
//! name servers come by DHCP.

use alloc::vec;
use alloc::vec::Vec;
use core::task::{Context, Waker};

use embassy_net_driver::Driver;
use esp_radio::wifi::Interface;
use smoltcp::iface::{Config, Interface as Stack, SocketHandle, SocketSet};
use smoltcp::phy::{Device, DeviceCapabilities, Medium};
use smoltcp::socket::{dhcpv4, dns, tcp};
use smoltcp::time::Instant;
use smoltcp::wire::{DnsQueryType, EthernetAddress, HardwareAddress, IpAddress, IpCidr};

/// The longest anything of the network is waited for here.
const WAIT_MS: u64 = 15_000;
/// Bytes buffered per connection, received and to send.
const RECEIVE_BUF: usize = 16 * 1024;
const SEND_BUF: usize = 1024;

#[derive(Debug)]
pub enum Error {
    /// No address yet (DHCP has not answered).
    Offline,
    /// The name could not be resolved.
    Name,
    /// The address is not of the form `http://host[:port]/path`.
    Url,
    /// The connection failed or was reset.
    Connection,
    /// It took longer than [`WAIT_MS`].
    Timeout,
}

fn now() -> Instant {
    Instant::from_millis(
        esp_hal::time::Instant::now()
            .duration_since_epoch()
            .as_millis() as i64,
    )
}

/// The radio's interface as smoltcp wants a device.
struct Phy(Interface);

/// The radio's tokens for smoltcp.
struct Rx<T>(T);
struct Tx<T>(T);

impl<T: embassy_net_driver::RxToken> smoltcp::phy::RxToken for Rx<T> {
    fn consume<R, F: FnOnce(&[u8]) -> R>(self, f: F) -> R {
        self.0.consume(|buf| f(buf))
    }
}

impl<T: embassy_net_driver::TxToken> smoltcp::phy::TxToken for Tx<T> {
    fn consume<R, F: FnOnce(&mut [u8]) -> R>(self, len: usize, f: F) -> R {
        self.0.consume(len, f)
    }
}

impl Device for Phy {
    type RxToken<'a> = Rx<<Interface as Driver>::RxToken<'a>>;
    type TxToken<'a> = Tx<<Interface as Driver>::TxToken<'a>>;

    fn receive(&mut self, _: Instant) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        // Polled: nothing to wake.
        let mut cx = Context::from_waker(Waker::noop());
        Driver::receive(&mut self.0, &mut cx).map(|(rx, tx)| (Rx(rx), Tx(tx)))
    }

    fn transmit(&mut self, _: Instant) -> Option<Self::TxToken<'_>> {
        let mut cx = Context::from_waker(Waker::noop());
        Driver::transmit(&mut self.0, &mut cx).map(Tx)
    }

    fn capabilities(&self) -> DeviceCapabilities {
        let theirs = Driver::capabilities(&self.0);
        let mut caps = DeviceCapabilities::default();
        caps.medium = Medium::Ethernet;
        caps.max_transmission_unit = theirs.max_transmission_unit;
        caps.max_burst_size = theirs.max_burst_size;
        caps
    }
}

pub struct Net {
    phy: Phy,
    stack: Stack,
    sockets: SocketSet<'static>,
    dhcp: SocketHandle,
    /// Created once DHCP named the name servers.
    dns: Option<SocketHandle>,
    /// The address DHCP gave, with its network's size.
    address: Option<smoltcp::wire::Ipv4Cidr>,
    /// The next local port for a connection.
    port: u16,
}

impl Net {
    pub fn new(interface: Interface) -> Self {
        let mut phy = Phy(interface);
        let mac = EthernetAddress(phy.0.mac_address());
        let rng = esp_hal::rng::Rng::new();
        let mut config = Config::new(HardwareAddress::Ethernet(mac));
        config.random_seed = u64::from(rng.random()) << 32 | u64::from(rng.random());
        let stack = Stack::new(config, &mut phy, now());
        let mut sockets = SocketSet::new(Vec::new());
        let dhcp = sockets.add(dhcpv4::Socket::new());
        Self {
            phy,
            stack,
            sockets,
            dhcp,
            dns: None,
            address: None,
            // In the range for ports a client picks.
            port: 49152 + (rng.random() % 16000) as u16,
        }
    }

    /// The address DHCP gave, if it did yet.
    pub fn address(&self) -> Option<smoltcp::wire::Ipv4Cidr> {
        self.address
    }

    /// The link came up (a network was joined): DHCP asks at once, not
    /// after the waits it grew while nothing answered.
    pub fn joined(&mut self) {
        self.sockets.get_mut::<dhcpv4::Socket>(self.dhcp).reset();
    }

    /// Sends and receives what is due, and takes what DHCP says.
    pub fn poll(&mut self) {
        self.stack.poll(now(), &mut self.phy, &mut self.sockets);
        let event = self.sockets.get_mut::<dhcpv4::Socket>(self.dhcp).poll();
        match event {
            None => {}
            Some(dhcpv4::Event::Configured(config)) => {
                log::info!("net: address {}", config.address);
                self.address = Some(config.address);
                self.stack.update_ip_addrs(|addrs| {
                    addrs.clear();
                    let _ = addrs.push(IpCidr::Ipv4(config.address));
                });
                match config.router {
                    Some(router) => {
                        let _ = self.stack.routes_mut().add_default_ipv4_route(router);
                    }
                    None => {
                        self.stack.routes_mut().remove_default_ipv4_route();
                    }
                }
                let servers: Vec<IpAddress> = config
                    .dns_servers
                    .iter()
                    .map(|&s| IpAddress::Ipv4(s))
                    .collect();
                match self.dns {
                    Some(handle) => self
                        .sockets
                        .get_mut::<dns::Socket>(handle)
                        .update_servers(&servers),
                    None => {
                        let socket = dns::Socket::new(&servers, vec![None]);
                        self.dns = Some(self.sockets.add(socket));
                    }
                }
            }
            Some(dhcpv4::Event::Deconfigured) => {
                log::info!("net: address gone");
                self.address = None;
                self.stack.update_ip_addrs(|addrs| addrs.clear());
                self.stack.routes_mut().remove_default_ipv4_route();
            }
        }
    }

    /// Polls until `done` says so, at most [`WAIT_MS`].
    fn wait<T>(&mut self, mut done: impl FnMut(&mut Self) -> Option<T>) -> Result<T, Error> {
        let until = esp_hal::time::Instant::now() + esp_hal::time::Duration::from_millis(WAIT_MS);
        loop {
            self.poll();
            if let Some(value) = done(self) {
                return Ok(value);
            }
            if esp_hal::time::Instant::now() > until {
                return Err(Error::Timeout);
            }
            crate::power::feed();
        }
    }

    /// Waits for an address from DHCP.
    pub fn wait_online(&mut self) -> Result<smoltcp::wire::Ipv4Cidr, Error> {
        self.wait(|net| net.address)
    }

    /// The address of the host `name`.
    fn resolve(&mut self, name: &str) -> Result<IpAddress, Error> {
        if let Ok(address) = name.parse::<core::net::Ipv4Addr>() {
            return Ok(IpAddress::Ipv4(address));
        }
        let handle = self.dns.ok_or(Error::Offline)?;
        let query = self
            .sockets
            .get_mut::<dns::Socket>(handle)
            .start_query(self.stack.context(), name, DnsQueryType::A)
            .map_err(|_| Error::Name)?;
        self.wait(|net| {
            match net
                .sockets
                .get_mut::<dns::Socket>(handle)
                .get_query_result(query)
            {
                Ok(addresses) => Some(addresses.first().copied().ok_or(Error::Name)),
                Err(dns::GetQueryResultError::Pending) => None,
                Err(dns::GetQueryResultError::Failed) => Some(Err(Error::Name)),
            }
        })?
    }

    /// Fetches `url` (plain HTTP, `http://host[:port]/path`): its status,
    /// with the body passed to `body` piece by piece as it comes (kept
    /// whole, a large one would not fit in memory).
    pub fn get(&mut self, url: &str, body: &mut dyn FnMut(&[u8])) -> Result<u16, Error> {
        if self.address.is_none() {
            return Err(Error::Offline);
        }
        let rest = url.strip_prefix("http://").ok_or(Error::Url)?;
        let (authority, path) = rest.split_at(rest.find('/').unwrap_or(rest.len()));
        let path = if path.is_empty() { "/" } else { path };
        let (host, port) = match authority.rsplit_once(':') {
            Some((host, port)) => (host, port.parse().map_err(|_| Error::Url)?),
            None => (authority, 80u16),
        };
        let address = self.resolve(host)?;

        let socket = tcp::Socket::new(
            tcp::SocketBuffer::new(vec![0; RECEIVE_BUF]),
            tcp::SocketBuffer::new(vec![0; SEND_BUF]),
        );
        let handle = self.sockets.add(socket);
        let request = alloc::format!(
            "GET {path} HTTP/1.1\r\nHost: {host}\r\nUser-Agent: QuireInk\r\nConnection: close\r\n\r\n"
        );
        let result = self.exchange(handle, (address, port), request.as_bytes(), body);
        self.sockets.remove(handle);
        result
    }

    /// Connects the socket `handle`, sends `request` and reads the response
    /// until the server closes the connection after it. Gives up after
    /// [`WAIT_MS`] without progress.
    fn exchange(
        &mut self,
        handle: SocketHandle,
        to: (IpAddress, u16),
        request: &[u8],
        body: &mut dyn FnMut(&[u8]),
    ) -> Result<u16, Error> {
        let local = self.port;
        self.port = if self.port >= 65000 {
            49152
        } else {
            self.port + 1
        };
        self.sockets
            .get_mut::<tcp::Socket>(handle)
            .connect(self.stack.context(), to, local)
            .map_err(|_| Error::Connection)?;
        let mut sent = 0;
        let mut head = Head::default();
        let wait = esp_hal::time::Duration::from_millis(WAIT_MS);
        let mut until = esp_hal::time::Instant::now() + wait;
        loop {
            self.poll();
            let socket = self.sockets.get_mut::<tcp::Socket>(handle);
            let mut progress = false;
            if sent < request.len() && socket.can_send() {
                let n = socket
                    .send_slice(&request[sent..])
                    .map_err(|_| Error::Connection)?;
                sent += n;
                progress |= n > 0;
            }
            while socket.can_recv() {
                socket
                    .recv(|data| {
                        head.take(data, body);
                        (data.len(), ())
                    })
                    .map_err(|_| Error::Connection)?;
                progress = true;
            }
            if head.failed {
                socket.abort();
                return Err(Error::Connection);
            }
            match socket.state() {
                // Closed by the server once all was sent: done.
                tcp::State::CloseWait | tcp::State::Closed | tcp::State::TimeWait
                    if sent == request.len() =>
                {
                    socket.close();
                    return head.status.ok_or(Error::Connection);
                }
                tcp::State::Closed => return Err(Error::Connection),
                _ => {}
            }
            let at = esp_hal::time::Instant::now();
            if progress {
                until = at + wait;
            } else if at > until {
                socket.abort();
                return Err(Error::Timeout);
            }
            crate::power::feed();
        }
    }
}

/// The head of an HTTP response, gathered until it is whole; what follows
/// is the body.
#[derive(Default)]
struct Head {
    bytes: Vec<u8>,
    /// Its status, once the head is whole.
    status: Option<u16>,
    failed: bool,
}

impl Head {
    /// The longest head taken.
    const MAX: usize = 8192;

    /// Takes received bytes: into the head until it is whole, the rest to
    /// `body`.
    fn take(&mut self, data: &[u8], body: &mut dyn FnMut(&[u8])) {
        if self.status.is_some() {
            return body(data);
        }
        let from = self.bytes.len().saturating_sub(3);
        self.bytes.extend_from_slice(data);
        let Some(end) = self.bytes[from..]
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .map(|at| from + at)
        else {
            self.failed = self.bytes.len() > Self::MAX;
            return;
        };
        self.status = core::str::from_utf8(&self.bytes[..end])
            .ok()
            .and_then(|head| head.split_ascii_whitespace().nth(1))
            .and_then(|status| status.parse().ok());
        self.failed = self.status.is_none();
        if !self.failed {
            body(&self.bytes[end + 4..]);
        }
        self.bytes = Vec::new();
    }
}
