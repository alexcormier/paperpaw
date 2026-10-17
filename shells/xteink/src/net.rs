//! The network over Wi-Fi: smoltcp (a TCP/IP stack made for devices
//! without an operating system) on the radio's station interface. It is
//! polled from the main loop (QuireInk's firmware runs no async code). Its
//! address, router and name servers come by DHCP. What goes over it is in
//! `http`.

use alloc::vec::Vec;
use core::task::{Context, Waker};

use embassy_net_driver::Driver;
use esp_radio::wifi::Interface;
use smoltcp::iface::{Config, Interface as Stack, SocketHandle, SocketSet};
use smoltcp::phy::{Device, DeviceCapabilities, Medium};
use smoltcp::socket::{dhcpv4, dns, tcp};
use smoltcp::time::Instant;
use smoltcp::wire::{DnsQueryType, EthernetAddress, HardwareAddress, IpAddress, IpCidr};

/// Without an address this long after joining a network, DHCP asks anew.
const DHCP_AGAIN_MS: u64 = 10_000;
/// Connections (and name lookups) at a time.
pub const MAX_CALLS: usize = 2;
/// Bytes buffered per connection, received and to send.
const RECEIVE_BUF: usize = 16 * 1024;
const SEND_BUF: usize = 1024;

#[derive(Debug)]
pub enum Error {
    /// No address yet (DHCP has not answered).
    Offline,
    /// The name could not be resolved.
    Name,
    /// The connection could not be opened.
    Connection,
    /// Not enough memory for it.
    Memory,
}

/// `len` zero bytes, or [`Error::Memory`]: a connection's buffers are
/// large, and memory may be short (or in pieces) at any time.
fn zeroed(len: usize) -> Result<Vec<u8>, Error> {
    let mut buf = Vec::new();
    buf.try_reserve_exact(len).map_err(|_| Error::Memory)?;
    buf.resize(len, 0);
    Ok(buf)
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
    /// When DHCP was last (re)started while joined without an address.
    asked: Option<Instant>,
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
            asked: None,
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
        self.asked = Some(now());
    }

    /// Sends and receives what is due, and takes what DHCP says.
    pub fn poll(&mut self) {
        // No answer long after joining (seen once after a start): ask anew.
        if let Some(asked) = self.asked
            && self.address.is_none()
            && now() - asked > smoltcp::time::Duration::from_millis(DHCP_AGAIN_MS)
        {
            log::info!("net: asking DHCP again");
            self.joined();
        }
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
                        let socket = dns::Socket::new(
                            &servers,
                            (0..MAX_CALLS).map(|_| None).collect::<Vec<_>>(),
                        );
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

    /// Starts finding the address of host `name` (at once if it is one);
    /// [`Net::resolved`] tells it.
    pub fn resolve(&mut self, name: &str) -> Result<Resolve, Error> {
        if let Ok(address) = name.parse::<core::net::Ipv4Addr>() {
            return Ok(Resolve::Done(IpAddress::Ipv4(address)));
        }
        let handle = self.dns.ok_or(Error::Offline)?;
        self.sockets
            .get_mut::<dns::Socket>(handle)
            .start_query(self.stack.context(), name, DnsQueryType::A)
            .map(Resolve::Asking)
            .map_err(|_| Error::Name)
    }

    /// The address asked for with [`Net::resolve`], once known.
    pub fn resolved(&mut self, query: dns::QueryHandle) -> Option<Result<IpAddress, Error>> {
        let handle = self.dns?;
        match self
            .sockets
            .get_mut::<dns::Socket>(handle)
            .get_query_result(query)
        {
            Ok(addresses) => Some(addresses.first().copied().ok_or(Error::Name)),
            Err(dns::GetQueryResultError::Pending) => None,
            Err(dns::GetQueryResultError::Failed) => Some(Err(Error::Name)),
        }
    }

    /// Stops finding an address no longer wanted.
    pub fn forget(&mut self, query: dns::QueryHandle) {
        if let Some(handle) = self.dns {
            self.sockets
                .get_mut::<dns::Socket>(handle)
                .cancel_query(query);
        }
    }

    /// Opens a TCP connection to `to`; [`Net::socket`] then reaches it, and
    /// [`Net::release`] lets it go.
    pub fn open(&mut self, to: (IpAddress, u16)) -> Result<SocketHandle, Error> {
        let socket = tcp::Socket::new(
            tcp::SocketBuffer::new(zeroed(RECEIVE_BUF)?),
            tcp::SocketBuffer::new(zeroed(SEND_BUF)?),
        );
        let handle = self.sockets.add(socket);
        let local = self.port;
        self.port = if self.port >= 65000 {
            49152
        } else {
            self.port + 1
        };
        let connected =
            self.sockets
                .get_mut::<tcp::Socket>(handle)
                .connect(self.stack.context(), to, local);
        if connected.is_err() {
            self.sockets.remove(handle);
            return Err(Error::Connection);
        }
        Ok(handle)
    }

    pub fn socket(&mut self, handle: SocketHandle) -> &mut tcp::Socket<'static> {
        self.sockets.get_mut::<tcp::Socket>(handle)
    }

    /// Closes a connection at once and frees it.
    pub fn release(&mut self, handle: SocketHandle) {
        self.sockets.get_mut::<tcp::Socket>(handle).abort();
        // Sends the reset before the socket goes.
        self.stack.poll(now(), &mut self.phy, &mut self.sockets);
        self.sockets.remove(handle);
    }
}

/// Finding a host's address.
pub enum Resolve {
    Done(IpAddress),
    Asking(dns::QueryHandle),
}
