//! Bluetooth Low Energy on the X4 Pro (feature `bluetooth`, ADR 0027):
//! what the kernel asks of the radio (`Io::Bluetooth`), done with
//! trouble-host, Embassy's host stack, on esp-radio's controller.
//!
//! The controller is up only while the kernel wants the radio: its
//! 35 KB of internal RAM come back when it goes down. While up, one
//! future holds everything (the controller, the host and its resources,
//! scanning, each link), polled from the main loop as the Wi-Fi tasks
//! are; it talks to the main loop through [`Shared`]. A link is a slot
//! of its own ([`LINKS`] of them, the kernel's `max_links`): connecting,
//! its GATT client, pairing and the HID reports of a remote.
//!
//! Switched off it costs no heap: the host, its resources, its packets
//! and what the main loop shares with it are made when the radio comes
//! up and dropped when it goes down. What stays is code and some 0.5 KB
//! of the controller's statics.
//!
//! Advertising and the GATT server come with step 4 of the ADR: until
//! then advertising ends at once and the server's calls are dropped.

use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::format;
use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::{Cell, RefCell};
use core::future::{Future, poll_fn};
use core::pin::Pin;
use core::sync::atomic::{AtomicUsize, Ordering};
use core::task::{Context, Poll, Waker};

use bt_hci::param::FilterDuplicates;
use embassy_futures::join::{join, join3};
use embassy_futures::select::{Either, select};
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_time::{Duration, Instant, Timer, with_timeout};
use esp_radio::ble::controller::BleConnector;
use paperpaw_core::sdk::bluetooth::{
    AttError, BtError, BtOp, GattCharacteristic, GattDescriptor, GattService, Handle, LinkPower,
    ManufacturerData, Properties, ScanPower, Security, ServiceData, Uuid as SdkUuid, ValueKind,
};
use paperpaw_core::{Bond, BtCall, BtReport, Found, PairQuestion};
use trouble_host::advertise::AdStructure;
use trouble_host::prelude::*;

/// Watching for bonded devices to come back: a short look every 1.28 s
/// (2.3 % of the time), enough for a remote advertising after a key.
const WATCH_INTERVAL: Duration = Duration::from_millis(1_280);
const WATCH_WINDOW: Duration = Duration::from_millis(30);
/// How long the scan waits after a connection came up.
const SCAN_RESUME: Duration = Duration::from_millis(500);
/// How soon a link's power is tried again while the controller is busy.
const POWER_RETRY: Duration = Duration::from_secs(2);
/// The links kept at once: a remote and one more (the kernel is built
/// with as many).
pub const LINKS: usize = 2;
/// How long a connection is tried before it fails.
const CONNECT: Duration = Duration::from_secs(15);
/// Services and characteristics of a device, at most, as discovered.
const SERVICES: usize = 12;
const CHARACTERISTICS: usize = 16;
const DESCRIPTORS: usize = 4;
/// The HID service and its characteristics (Bluetooth SIG numbers).
const HID: u16 = 0x1812;
const REPORT_MAP: u16 = 0x2A4B;
const REPORT: u16 = 0x2A4D;
const BOOT_KEYBOARD_INPUT: u16 = 0x2A22;
const REPORT_REFERENCE: u16 = 0x2908;
/// The longest report map read.
const REPORT_MAP_MOST: usize = 512;
/// AD types trouble-host leaves undecoded.
const AD_APPEARANCE: u8 = 0x19;
const AD_SERVICE_DATA_128: u8 = 0x21;

/// Packets the host has at once, at most, and the bytes of each.
const PACKETS: usize = 24;
const PACKET_BYTES: usize = 251;

type Hci = ExternalController<BleConnector<'static>, 10>;
type Pool = HeapPool;
type Host<'a> = Stack<'a, Hci, Pool>;
type Resources = HostResources<Hci, Pool, LINKS, 1, 1, 8>;
type Client<'a> = GattClient<'a, Hci, Pool, SERVICES>;
/// The device's own attributes, as a central has them: GAP (its name
/// and appearance) and GATT. A peripheral reads them (keyboards do), and
/// gives up on the link where nothing answers.
type Server<'a> = AttributeServer<'a, NoopRawMutex, Pool, ATTRIBUTES, LINKS>;
const ATTRIBUTES: usize = 10;
/// The name the device's GAP service gives.
const NAME: &str = "PaperPaw";

/// embassy-time's clock, for the host's timeouts: the main loop polls
/// the host while the radio is up, so a timer needs no wake-up of its
/// own (and no executor, whose code would stay in IRAM).
struct Clock;

impl embassy_time_driver::Driver for Clock {
    fn now(&self) -> u64 {
        crate::micros() * embassy_time_driver::TICK_HZ / 1_000_000
    }

    fn schedule_wake(&self, _at: u64, _waker: &Waker) {}
}

embassy_time_driver::time_driver_impl!(static CLOCK: Clock = Clock);

/// The host's packets, from the heap and only while the radio is up
/// (trouble-host's own pool is a static of 4 KB of internal RAM, kept
/// while Bluetooth is off).
struct HeapPool;

/// Packets alive now.
static LIVE_PACKETS: AtomicUsize = AtomicUsize::new(0);

impl PacketPool for HeapPool {
    type Packet = HeapPacket;
    const MTU: usize = PACKET_BYTES;

    fn allocate() -> Option<HeapPacket> {
        if LIVE_PACKETS.fetch_add(1, Ordering::Relaxed) >= PACKETS {
            LIVE_PACKETS.fetch_sub(1, Ordering::Relaxed);
            return None;
        }
        let mut bytes = Vec::new();
        if bytes.try_reserve_exact(PACKET_BYTES).is_err() {
            LIVE_PACKETS.fetch_sub(1, Ordering::Relaxed);
            return None;
        }
        bytes.resize(PACKET_BYTES, 0);
        Some(HeapPacket(bytes))
    }

    fn capacity() -> usize {
        PACKETS
    }
}

/// A packet of [`HeapPool`].
struct HeapPacket(Vec<u8>);

impl Packet for HeapPacket {}

impl AsRef<[u8]> for HeapPacket {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl AsMut<[u8]> for HeapPacket {
    fn as_mut(&mut self) -> &mut [u8] {
        &mut self.0
    }
}

impl Drop for HeapPacket {
    fn drop(&mut self) {
        LIVE_PACKETS.fetch_sub(1, Ordering::Relaxed);
    }
}

/// What the main loop and the future share: calls in, reports out.
#[derive(Default)]
struct Shared {
    calls: RefCell<VecDeque<BtCall>>,
    reports: RefCell<Vec<BtReport>>,
    /// The bonds as the kernel keeps them, and whether the host has
    /// them yet.
    bonds: RefCell<Vec<Bond>>,
    bonds_new: Cell<bool>,
    /// What each remote's HID service is, as found once.
    hid: RefCell<Vec<HidCache>>,
    /// Wi-Fi joins or moves data: the links run slowly meanwhile, so that
    /// the shared radio leaves it time.
    wifi_busy: Cell<bool>,
}

/// A remote's HID service as discovered, kept while the device runs so
/// that it is not discovered again on each connection (slow at an MTU of
/// 23, and keys typed meanwhile were lost): its address, its input
/// reports (value handle, report id), their configuration descriptors,
/// its report map. A few hundred bytes per remote.
#[derive(Debug, Clone)]
struct HidCache {
    address: String,
    inputs: Vec<(u16, u8)>,
    configured: Vec<(u16, u16, Properties)>,
    map: Vec<u8>,
}

impl Shared {
    fn report(&self, report: BtReport) {
        self.reports.borrow_mut().push(report);
    }

    fn fail(&self, link: u32, op: BtOp, handle: Option<u16>, error: BtError) {
        self.report(BtReport::Failed {
            link,
            op,
            handle,
            error,
        });
    }
}

/// The radio as the main loop sees it.
pub struct Bluetooth {
    /// The bonds as the kernel keeps them, for the next start.
    bonds: Vec<Bond>,
    /// Everything while the controller is up.
    on: Option<On>,
    /// What was reported as the radio went down, for the next poll.
    reports: Vec<BtReport>,
    /// The remotes' HID services, kept while the radio is off.
    hid: Vec<HidCache>,
}

/// The radio up: what the main loop shares with the host, and the host.
struct On {
    shared: Rc<Shared>,
    session: Pin<Box<dyn Future<Output = ()>>>,
}

impl Bluetooth {
    pub fn new() -> Self {
        Self {
            bonds: Vec::new(),
            on: None,
            reports: Vec::new(),
            hid: Vec::new(),
        }
    }

    /// The controller is up: the main loop keeps coming by (no light
    /// sleep).
    pub fn is_on(&self) -> bool {
        self.on.is_some()
    }

    /// Does what the kernel asked. `start_scheduler` starts the tasks
    /// the controller runs on, where they are not yet.
    pub fn apply(&mut self, call: BtCall, start_scheduler: impl FnOnce()) {
        match (call, &self.on) {
            (BtCall::Radio { on: true }, None) => {
                start_scheduler();
                self.start();
            }
            (BtCall::Radio { on: false }, Some(on)) => {
                // Dropped: the host, the links and the controller go down,
                // and everything they had on the heap.
                self.hid = on.shared.hid.take();
                self.on = None;
                log::info!("bluetooth: off, heap {}", crate::http::heap());
                self.reports.push(BtReport::Radio { on: false });
            }
            (BtCall::Bonds(bonds), on) => {
                if let Some(on) = on {
                    on.shared.bonds.borrow_mut().clone_from(&bonds);
                    on.shared.bonds_new.set(true);
                }
                self.bonds = bonds;
            }
            (call, Some(on)) => on.shared.calls.borrow_mut().push_back(call),
            // A bond forgotten comes with the bonds left; the GATT server
            // comes with step 4 of the ADR.
            (BtCall::Forget { address }, None) => self.hid.retain(|h| h.address != address),
            (BtCall::Radio { .. } | BtCall::Server(_) | BtCall::Watch { .. }, None) => {}
            (call, None) => log::warn!("bluetooth: off, dropped {call:?}"),
        }
    }

    fn start(&mut self) {
        // SAFETY: only this module takes the Bluetooth peripheral, and
        // only while no controller holds it (the last one was dropped).
        let bt = unsafe { esp_hal::peripherals::BT::steal() };
        log::info!("bluetooth: starting, heap {}", crate::http::heap());
        match BleConnector::new(bt, esp_radio::ble::Config::default()) {
            Ok(connector) => {
                log::info!("bluetooth: on, heap {}", crate::http::heap());
                let shared = Rc::new(Shared::default());
                shared.bonds.borrow_mut().clone_from(&self.bonds);
                shared.bonds_new.set(true);
                *shared.hid.borrow_mut() = core::mem::take(&mut self.hid);
                shared.report(BtReport::Radio { on: true });
                let session = Box::pin(session(shared.clone(), connector));
                log::info!(
                    "bluetooth: the host takes {} bytes",
                    core::mem::size_of_val(&*session)
                );
                self.on = Some(On { shared, session });
            }
            Err(e) => {
                log::warn!("bluetooth: controller: {e:?}");
                self.reports.push(BtReport::Radio { on: false });
            }
        }
    }

    /// Whether Wi-Fi joins or moves data now (see [`Shared::wifi_busy`]).
    pub fn set_wifi_busy(&mut self, busy: bool) {
        if let Some(on) = &self.on {
            on.shared.wifi_busy.set(busy);
        }
    }

    /// Moves the host on; what it reports.
    // A frame of its own: the host's futures are deep.
    #[inline(never)]
    pub fn poll(&mut self) -> Vec<BtReport> {
        let mut reports = core::mem::take(&mut self.reports);
        if let Some(on) = &mut self.on {
            let mut cx = Context::from_waker(Waker::noop());
            let stopped = on.session.as_mut().poll(&mut cx).is_ready();
            reports.append(&mut on.shared.reports.borrow_mut());
            if stopped {
                log::warn!("bluetooth: the host stopped");
                self.hid = on.shared.hid.take();
                self.on = None;
                reports.push(BtReport::Radio { on: false });
            }
        }
        reports
    }
}

/// A link's slot: what the kernel asked of it.
#[derive(Default)]
struct Slot {
    /// The kernel's number for the link, while it is asked for or up.
    link: Option<u32>,
    /// A connection asked for and not yet begun: the address, the power.
    connect: Option<(String, LinkPower)>,
    /// Let go before it came up.
    cancelled: bool,
    calls: VecDeque<BtCall>,
}

/// What the scan should be: off, or at a power and whether active; and
/// whether that changed since the scan began.
#[derive(Default)]
struct ScanWanted {
    wanted: Cell<Option<(ScanPower, bool)>>,
    /// The bonded devices watched for while no scan is wanted (their
    /// addresses as the kernel keeps them).
    watch: RefCell<Vec<String>>,
    /// While only watching: the addresses whose advertisements are told,
    /// none else (the kernel would keep every device passing by).
    only: RefCell<Option<Vec<String>>>,
    /// Connections being made: the scan stops meanwhile (connecting sets
    /// the controller's accept list, which a scan using it forbids).
    connecting: Cell<u8>,
    changed: Cell<bool>,
}

/// Everything while the controller is up.
async fn session(shared: Rc<Shared>, connector: BleConnector<'static>) {
    let mut resources: Box<Resources> = Box::new(HostResources::new());
    let stack = trouble_host::new(ExternalController::new(connector), &mut resources)
        .set_io_capabilities(IoCapabilities::DisplayYesNo)
        .build();
    let mut runner = stack.runner();

    let slots: [RefCell<Slot>; LINKS] = Default::default();
    let scan = ScanWanted::default();
    let handler = Advertised {
        shared: &shared,
        scan: &scan,
    };
    let mut table: AttributeTable<'_, NoopRawMutex, ATTRIBUTES> = AttributeTable::new();
    let mut gap = table.add_service(Service::new(service::GAP));
    gap.add_characteristic_ro(characteristic::DEVICE_NAME, NAME);
    gap.add_characteristic_ro(characteristic::APPEARANCE, &appearance::UNKNOWN);
    gap.build();
    table.add_service(Service::new(service::GATT));
    let server: Server<'_> = AttributeServer::new(table);
    let work = join3(
        dispatch(&stack, &shared, &slots, &scan),
        scanning(&stack, &scan),
        join(
            link(&stack, &server, &scan, &slots[0], &shared),
            link(&stack, &server, &scan, &slots[1], &shared),
        ),
    );
    if let Either::First(Err(e)) = select(runner.run_with_handler(&handler), work).await {
        log::warn!("bluetooth: host: {e:?}");
    }
}

/// Hands the kernel's calls to the scan, the links and the bonds.
async fn dispatch(
    stack: &Host<'_>,
    shared: &Shared,
    slots: &[RefCell<Slot>; LINKS],
    scan: &ScanWanted,
) {
    loop {
        if shared.bonds_new.take() {
            keep_bonds(stack, &shared.bonds.borrow());
            // The host brings its address resolution up to date with the
            // bonds once nothing scans or connects: a moment for it first
            // (a scan begun meanwhile made the controller refuse, and the
            // host stopped).
            Timer::after(SCAN_RESUME).await;
        }
        let call = poll_fn(|_| {
            if shared.bonds_new.get() {
                return Poll::Ready(None);
            }
            match shared.calls.borrow_mut().pop_front() {
                Some(call) => Poll::Ready(Some(call)),
                None => Poll::Pending,
            }
        })
        .await;
        let Some(call) = call else { continue };
        let slot_of = |link: u32| slots.iter().find(|s| s.borrow().link == Some(link));
        match call {
            BtCall::Scan { on, power, active } => {
                scan.wanted.set(on.then_some((power, active)));
                scan.changed.set(true);
            }
            BtCall::Watch { addresses } => {
                log::info!("bluetooth: watching for {addresses:?}");
                *scan.watch.borrow_mut() = addresses;
                scan.changed.set(true);
            }
            BtCall::Connect {
                link,
                address,
                power,
            } => match slots.iter().find(|s| s.borrow().link.is_none()) {
                Some(slot) => {
                    let mut slot = slot.borrow_mut();
                    *slot = Slot::default();
                    slot.link = Some(link);
                    slot.connect = Some((address, power));
                }
                None => shared.fail(link, BtOp::Connect, None, BtError::Busy),
            },
            BtCall::Disconnect { link } => {
                if let Some(slot) = slot_of(link) {
                    let mut slot = slot.borrow_mut();
                    slot.cancelled = true;
                    slot.calls.push_back(BtCall::Disconnect { link });
                }
            }
            BtCall::Forget { address } => {
                shared.hid.borrow_mut().retain(|h| h.address != address);
                if let Some(address) = address_of(&address) {
                    let _ = stack.remove_bond_information(Identity::from(address));
                }
            }
            BtCall::Advertise(advert) => {
                // Step 4 of the ADR; until then it ends at once.
                if advert.on {
                    log::info!("bluetooth: advertising is not built yet");
                    shared.report(BtReport::Advertising { on: false });
                }
            }
            BtCall::Server(_)
            | BtCall::Notify { .. }
            | BtCall::Respond { .. }
            | BtCall::Refuse { .. } => {}
            call @ (BtCall::LinkPower { link, .. }
            | BtCall::Discover { link, .. }
            | BtCall::Read { link, .. }
            | BtCall::Write { link, .. }
            | BtCall::Subscribe { link, .. }
            | BtCall::Pair { link }
            | BtCall::Answer { link, .. }
            | BtCall::Hid { link, .. }) => {
                if let Some(slot) = slot_of(link) {
                    slot.borrow_mut().calls.push_back(call);
                }
            }
            BtCall::Radio { .. } | BtCall::Bonds(_) => {}
        }
    }
}

/// The bonds the kernel keeps, given to the host (which keeps none of
/// its own across starts).
fn keep_bonds(stack: &Host<'_>, bonds: &[Bond]) {
    let kept: Vec<Identity> =
        stack.with_bond_information(|b| b.iter().map(|b| b.identity).collect());
    for identity in kept {
        let _ = stack.remove_bond_information(identity);
    }
    for bond in bonds {
        match bond_of(bond) {
            Some(information) => {
                if let Err(e) = stack.add_bond_information(information) {
                    log::warn!("bluetooth: bond: {e:?}");
                }
            }
            None => log::warn!("bluetooth: a bond unreadable"),
        }
    }
}

/// Scans as wanted: one scan for everyone, ended and begun anew where
/// what is wanted changes.
async fn scanning(stack: &Host<'_>, scan: &ScanWanted) {
    let mut scanner = Scanner::new(stack.central());
    loop {
        poll_fn(|_| {
            if scan.changed.take() {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        })
        .await;
        let watch: Vec<String> = scan.watch.borrow().clone();
        *scan.only.borrow_mut() = None;
        if scan.connecting.get() > 0 {
            continue;
        }
        let config = match scan.wanted.get() {
            Some((power, active)) => {
                let (interval, window) = scan_timing(power);
                ScanConfig {
                    active,
                    interval,
                    window,
                    filter_duplicates: FilterDuplicates::Enabled,
                    ..ScanConfig::default()
                }
            }
            // Only the bonded devices, passively and seldom; each
            // advertisement told (one that slept and woke is new).
            // Passively and seldom, all advertisements heard but only the
            // bonded devices' told; each one (one that slept and woke is
            // new). Not with the controller's accept list: connecting sets
            // it too, and clearing it while a scan ends crashed the
            // controller.
            None if !watch.is_empty() => {
                *scan.only.borrow_mut() = Some(watch.clone());
                ScanConfig {
                    active: false,
                    interval: WATCH_INTERVAL,
                    window: WATCH_WINDOW,
                    filter_duplicates: FilterDuplicates::Disabled,
                    ..ScanConfig::default()
                }
            }
            None => continue,
        };
        match scanner.scan(&config).await {
            // On until what is wanted changes; dropped, the scan ends.
            Ok(_session) => {
                poll_fn(|_| {
                    if scan.changed.get() {
                        Poll::Ready(())
                    } else {
                        Poll::Pending
                    }
                })
                .await;
            }
            Err(e) => log::warn!("bluetooth: scan: {e:?}"),
        }
    }
}

/// How often and how long the radio listens: the less, the cheaper.
fn scan_timing(power: ScanPower) -> (Duration, Duration) {
    let (interval, window) = match power {
        ScanPower::Low => (1_280, 40),
        ScanPower::Balanced => (400, 40),
        _ => (60, 30),
    };
    (
        Duration::from_millis(interval),
        Duration::from_millis(window),
    )
}

/// The connection's intervals for `power`: long ones with peripheral
/// latency by default; the central (this device) wakes at each interval.
fn link_timing(power: LinkPower) -> RequestedConnParams {
    let (min, max, latency, timeout) = match power {
        // Long supervision timeouts: beside Wi-Fi the radio is shared, and
        // a link must not drop for a few connection events missed.
        LinkPower::Low => (100, 150, 2, 8_000),
        LinkPower::Responsive => (30, 50, 4, 8_000),
        _ => (8, 15, 0, 4_000),
    };
    RequestedConnParams {
        min_connection_interval: Duration::from_millis(min),
        max_connection_interval: Duration::from_millis(max),
        max_latency: latency,
        min_event_length: Duration::from_ticks(0),
        max_event_length: Duration::from_ticks(0),
        supervision_timeout: Duration::from_millis(timeout),
    }
}

/// Turns advertising reports into devices found.
struct Advertised<'a> {
    shared: &'a Shared,
    scan: &'a ScanWanted,
}

impl EventHandler for Advertised<'_> {
    fn on_adv_reports(&self, reports: LeAdvReportsIter<'_>) {
        for report in reports.flatten() {
            let mut found = Found {
                address: address_text(report.addr_kind, report.addr),
                signal: (report.rssi != 127).then_some(report.rssi),
                ..Found::default()
            };
            if let Some(only) = &*self.scan.only.borrow() {
                if !only.contains(&found.address) {
                    continue;
                }
                log::info!("bluetooth: {} is back", found.address);
            }
            read_advertised(report.data, &mut found);
            self.shared.report(BtReport::Found(found));
        }
    }
}

/// What an advertisement says: name, services, data, power, appearance.
fn read_advertised(data: &[u8], found: &mut Found) {
    for item in AdStructure::decode(data).flatten() {
        match item {
            AdStructure::CompleteLocalName(name) | AdStructure::ShortenedLocalName(name) => {
                found.name = Some(String::from_utf8_lossy(name).into_owned());
            }
            AdStructure::IncompleteServiceUuids16(uuids)
            | AdStructure::CompleteServiceUuids16(uuids) => {
                for uuid in uuids {
                    found.services.push(SdkUuid::sig(u16::from_le_bytes(*uuid)));
                }
            }
            AdStructure::IncompleteServiceUuids32(uuids)
            | AdStructure::CompleteServiceUuids32(uuids) => {
                for uuid in uuids {
                    found.services.push(sdk_uuid(&Uuid::Uuid32(*uuid)));
                }
            }
            AdStructure::IncompleteServiceUuids128(uuids)
            | AdStructure::CompleteServiceUuids128(uuids) => {
                for uuid in uuids {
                    found.services.push(sdk_uuid(&Uuid::Uuid128(*uuid)));
                }
            }
            AdStructure::TxPowerLevel(power) => found.tx_power = Some(power),
            AdStructure::ServiceData16 { uuid, data } => {
                let uuid = SdkUuid::sig(u16::from_le_bytes(uuid));
                found.service_data.push(ServiceData::new(uuid, data));
            }
            AdStructure::ManufacturerSpecificData {
                company_identifier,
                payload,
            } => {
                let data = ManufacturerData::new(company_identifier, payload);
                found.manufacturer_data.push(data);
            }
            AdStructure::Unknown {
                ty: AD_APPEARANCE,
                data: [low, high, ..],
            } => found.appearance = Some(u16::from_le_bytes([*low, *high])),
            AdStructure::Unknown {
                ty: AD_SERVICE_DATA_128,
                data,
            } if data.len() >= 16 => {
                let mut uuid = [0; 16];
                uuid.copy_from_slice(&data[..16]);
                let uuid = sdk_uuid(&Uuid::Uuid128(uuid));
                found.service_data.push(ServiceData::new(uuid, &data[16..]));
            }
            _ => {}
        }
    }
}

/// A slot's link, again and again: connected when asked, served while
/// up, reported when gone.
async fn link(
    stack: &Host<'_>,
    server: &Server<'_>,
    scan: &ScanWanted,
    slot: &RefCell<Slot>,
    shared: &Shared,
) {
    loop {
        let (number, address, power) = poll_fn(|_| {
            let mut slot = slot.borrow_mut();
            match (slot.link, slot.connect.take()) {
                (Some(number), Some((address, power))) => Poll::Ready((number, address, power)),
                _ => Poll::Pending,
            }
        })
        .await;
        // The scan stops while connecting, and goes on after.
        scan.connecting.set(scan.connecting.get() + 1);
        scan.changed.set(true);
        let connected = connect(stack, slot, &address, power).await;
        if matches!(connected, Ok(Some(_))) {
            // The host updates its address resolution once nothing scans
            // or connects: a moment for it before the scan goes on (a scan
            // begun meanwhile made the controller refuse, and the host
            // stopped).
            Timer::after(SCAN_RESUME).await;
        }
        scan.connecting.set(scan.connecting.get() - 1);
        scan.changed.set(true);
        let reason = match connected {
            Ok(Some(connection)) => {
                let (mtu, security) = (connection.att_mtu(), security_of(&connection));
                log::info!("bluetooth: link {number} up, heap {}", crate::http::heap());
                shared.report(BtReport::Connected {
                    link: number,
                    mtu,
                    security,
                });
                let reason = serve(
                    stack,
                    server,
                    &connection,
                    (number, &address),
                    power,
                    slot,
                    shared,
                )
                .await;
                log::info!("bluetooth: link {number} down: {reason:?}");
                shared.report(BtReport::Disconnected {
                    link: number,
                    reason,
                });
                Ok(())
            }
            // Let go before it came up: the kernel has forgotten it.
            Ok(None) => Ok(()),
            Err(error) => Err(error),
        };
        if let Err(error) = reason {
            shared.fail(number, BtOp::Connect, None, error);
        }
        *slot.borrow_mut() = Slot::default();
    }
}

/// Connects to `address`; `None` where the kernel let go meanwhile.
async fn connect<'s>(
    stack: &'s Host<'_>,
    slot: &RefCell<Slot>,
    address: &str,
    power: LinkPower,
) -> Result<Option<Connection<'s, Pool>>, BtError> {
    let address = address_of(address).ok_or(BtError::NotFound)?;
    let accept = [address];
    let config = ConnectConfig {
        scan_config: ScanConfig {
            filter_accept_list: &accept,
            interval: Duration::from_millis(60),
            window: Duration::from_millis(30),
            ..ScanConfig::default()
        },
        connect_params: link_timing(power),
    };
    let mut central = stack.central();
    let cancelled = poll_fn(|_| {
        if slot.borrow().cancelled {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    });
    match select(with_timeout(CONNECT, central.connect(&config)), cancelled).await {
        Either::First(Ok(Ok(connection))) => Ok(Some(connection)),
        Either::First(Ok(Err(e))) => Err(error_of(&e)),
        Either::First(Err(_)) => Err(BtError::Timeout),
        Either::Second(()) => Ok(None),
    }
}

/// How the link is secured now.
fn security_of(connection: &Connection<'_, Pool>) -> Security {
    match connection.security_level() {
        Ok(SecurityLevel::EncryptedAuthenticated) => Security::Authenticated,
        Ok(SecurityLevel::Encrypted) => Security::Encrypted,
        _ => Security::None,
    }
}

/// Where pairing stands on a link.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Pairing {
    #[default]
    No,
    /// The reader was asked whether to pair at all.
    Asked,
    Running,
    /// The reader was asked to compare a number, or shown one to type.
    Number,
}

/// What a link knows of its device.
#[derive(Default)]
struct Known {
    /// Each characteristic's value handle, configuration descriptor and
    /// properties, as discovered.
    configured: Vec<(u16, u16, Properties)>,
    /// The HID input reports subscribed: value handle and report id.
    hid: Vec<(u16, u8)>,
    pairing: Pairing,
    /// The power the kernel asked for the link (pairing runs faster).
    power: LinkPower,
    /// The power last tried, when it is tried again, and how often; the
    /// power the link has now.
    target: LinkPower,
    retry: Option<Instant>,
    tries: u8,
    applied: Option<LinkPower>,
    /// The link runs slowly while Wi-Fi is busy.
    slowed: bool,
    /// The device's address, its HID service kept under it.
    address: String,
}

/// Serves an established link until it goes; why it went.
async fn serve(
    stack: &Host<'_>,
    server: &Server<'_>,
    connection: &Connection<'_, Pool>,
    (number, address): (u32, &str),
    power: LinkPower,
    slot: &RefCell<Slot>,
    shared: &Shared,
) -> Option<BtError> {
    // The MTU agreed, and a bonded device's link encrypted.
    let client: Client<'_> = match Client::new(stack, connection).await {
        Ok(client) => client,
        Err(e) => {
            log::warn!("bluetooth: link {number}: {e:?}");
            connection.disconnect();
            return gone(connection).await;
        }
    };
    let Ok(mut listener) = client.listen_all() else {
        connection.disconnect();
        return gone(connection).await;
    };
    // The link's events, and the peer's requests to the attributes.
    let Ok(gatt) = connection.clone().with_attribute_server(server) else {
        connection.disconnect();
        return gone(connection).await;
    };
    let known = RefCell::new(Known {
        power,
        applied: Some(power),
        address: String::from(address),
        ..Known::default()
    });
    // A remote met before: its reports are told as such from the first,
    // with no discovery (keys typed at once are not lost).
    if let Some(cache) = shared.hid.borrow().iter().find(|h| h.address == address) {
        let mut known = known.borrow_mut();
        known.hid.clone_from(&cache.inputs);
        known.configured.clone_from(&cache.configured);
        shared.report(BtReport::HidMap {
            link: number,
            map: cache.map.clone(),
        });
    }
    // What the device sends (events, its requests, notifications) is
    // taken at once, also while a call waits for the device's answers:
    // two loops side by side.
    let events = async {
        loop {
            match select(gatt.next(), listener.next()).await {
                Either::First(event) => {
                    if let Some(reason) =
                        on_event(stack, connection, number, event, &known, shared).await
                    {
                        return reason;
                    }
                }
                Either::Second(notification) => {
                    on_notification(&client, number, &notification, &known, shared).await;
                }
            }
        }
    };
    let calls = async {
        loop {
            let next_call = poll_fn(|_| match slot.borrow_mut().calls.pop_front() {
                Some(call) => Poll::Ready(call),
                None => Poll::Pending,
            });
            // The link's power is due again: tried again, or Wi-Fi came or
            // went.
            let retry = poll_fn(|_| {
                let known = known.borrow();
                let due = known.retry.is_some_and(|at| Instant::now() >= at);
                if due || known.slowed != shared.wifi_busy.get() {
                    Poll::Ready(())
                } else {
                    Poll::Pending
                }
            });
            match select(next_call, retry).await {
                Either::First(call) => {
                    on_call(stack, connection, &client, number, call, &known, shared).await;
                }
                Either::Second(()) => {
                    known.borrow_mut().slowed = shared.wifi_busy.get();
                    let target = wanted_power(&known.borrow());
                    try_power(stack, connection, &known, target).await;
                }
            }
        }
    };
    let work = async {
        match select(events, calls).await {
            Either::First(reason) => reason,
            Either::Second(()) => None,
        }
    };
    match select(client.task(), work).await {
        Either::First(_) => gone(connection).await,
        Either::Second(reason) => reason,
    }
}

/// The link's intervals for `power`; where the controller is busy with
/// another procedure (encryption, an update), tried again a little later
/// without holding up the link meanwhile.
async fn try_power(
    stack: &Host<'_>,
    connection: &Connection<'_, Pool>,
    known: &RefCell<Known>,
    power: LinkPower,
) {
    {
        let mut known = known.borrow_mut();
        if known.applied == Some(power) {
            known.retry = None;
            return;
        }
        known.target = power;
        known.retry = None;
        known.tries += 1;
    }
    match connection
        .update_connection_params(stack, &link_timing(power))
        .await
    {
        Ok(()) => {
            let mut known = known.borrow_mut();
            known.tries = 0;
            known.applied = Some(power);
        }
        Err(e) => {
            log::info!("bluetooth: power {power:?}: {e:?}");
            let mut known = known.borrow_mut();
            if known.tries < 6 {
                known.retry = Some(Instant::now() + POWER_RETRY);
            }
        }
    }
}

/// The power a link is to run at: the fastest while pairing, slow while
/// Wi-Fi is busy, else what the kernel asked.
fn wanted_power(known: &Known) -> LinkPower {
    if known.pairing != Pairing::No {
        LinkPower::Burst
    } else if known.slowed {
        LinkPower::Low
    } else {
        known.power
    }
}

/// Waits (a little) for the link's end and says why it ended.
async fn gone(connection: &Connection<'_, Pool>) -> Option<BtError> {
    let ended = async {
        loop {
            if let ConnectionEvent::Disconnected { reason } = connection.next().await {
                return reason_of(reason);
            }
        }
    };
    with_timeout(Duration::from_secs(2), ended)
        .await
        .unwrap_or(Some(BtError::Lost))
}

/// Why a link ended: `None` where either side ended it on purpose.
fn reason_of(status: bt_hci::param::Status) -> Option<BtError> {
    match status.into_inner() {
        0x13 | 0x16 => None,
        _ => Some(BtError::Lost),
    }
}

/// What happened on the link; why it ended, where it did.
async fn on_event(
    stack: &Host<'_>,
    connection: &Connection<'_, Pool>,
    number: u32,
    event: GattConnectionEvent<'_, '_, Pool>,
    known: &RefCell<Known>,
    shared: &Shared,
) -> Option<Option<BtError>> {
    let ask = |question| {
        shared.report(BtReport::Ask {
            link: number,
            question,
        });
    };
    match event {
        GattConnectionEvent::Disconnected { reason } => return Some(reason_of(reason)),
        GattConnectionEvent::Gatt { event } => match event.accept() {
            Ok(reply) => reply.send().await,
            Err(e) => log::info!("bluetooth: link {number}: attribute request: {e:?}"),
        },
        GattConnectionEvent::ConnectionParamsUpdated {
            conn_interval,
            peripheral_latency,
            ..
        } => log::info!(
            "bluetooth: link {number}: interval {} ms, latency {peripheral_latency}",
            conn_interval.as_millis()
        ),
        GattConnectionEvent::RequestConnectionParams(request) => {
            let _ = request.accept(None, stack).await;
        }
        GattConnectionEvent::PassKeyConfirm(key) => {
            known.borrow_mut().pairing = Pairing::Number;
            ask(PairQuestion::Compare(key.value()));
        }
        GattConnectionEvent::PassKeyDisplay(key) => {
            known.borrow_mut().pairing = Pairing::Number;
            ask(PairQuestion::Show(key.value()));
        }
        GattConnectionEvent::PassKeyInput => {
            // The reader would type a number the device shows: the X4 Pro
            // has no keyboard for that yet.
            let _ = connection.pass_key_cancel();
            known.borrow_mut().pairing = Pairing::No;
            shared.fail(number, BtOp::Pair, None, BtError::Unsupported);
        }
        GattConnectionEvent::PairingComplete { bond, .. } => {
            let asked = known.borrow().pairing != Pairing::No;
            log::info!(
                "bluetooth: link {number}: paired, bonded {}, asked {asked}",
                bond.is_some()
            );
            if !asked {
                // A bonded device's link encrypted again: nothing new.
                return None;
            }
            known.borrow_mut().pairing = Pairing::No;
            let power = wanted_power(&known.borrow());
            try_power(stack, connection, known, power).await;
            match bond {
                Some(bond) => shared.report(BtReport::Paired {
                    link: number,
                    keys: keys_text(&bond),
                }),
                None => shared.fail(number, BtOp::Pair, None, BtError::Unsupported),
            }
        }
        GattConnectionEvent::PairingFailed(e) => {
            log::info!("bluetooth: link {number}: pairing failed: {e:?}");
            let asked = known.borrow().pairing != Pairing::No;
            known.borrow_mut().pairing = Pairing::No;
            let power = wanted_power(&known.borrow());
            try_power(stack, connection, known, power).await;
            if asked {
                shared.fail(number, BtOp::Pair, None, BtError::Declined);
            }
        }
        _ => {}
    }
    None
}

/// A notification or indication: a remote's report where it is one of
/// its HID input reports, else a value.
async fn on_notification(
    client: &Client<'_>,
    number: u32,
    notification: &Notification<512>,
    known: &RefCell<Known>,
    shared: &Shared,
) {
    let handle = notification.handle();
    let bytes = notification.as_ref().to_vec();
    let report = known
        .borrow()
        .hid
        .iter()
        .find(|(h, _)| *h == handle)
        .map(|(_, id)| *id);
    log::debug!("bluetooth: link {number}: {handle} {bytes:02x?}");
    match report {
        Some(id) => shared.report(BtReport::HidReport {
            link: number,
            report: (id != 0).then_some(id),
            bytes,
        }),
        None => {
            let kind = if notification.is_indication() {
                ValueKind::Indicated
            } else {
                ValueKind::Notified
            };
            shared.report(BtReport::Value {
                link: number,
                handle,
                kind,
                bytes,
            });
        }
    }
    if notification.is_indication() {
        let _ = client.confirm_indication().await;
    }
}

/// A call of the kernel's for an established link.
async fn on_call(
    stack: &Host<'_>,
    connection: &Connection<'_, Pool>,
    client: &Client<'_>,
    number: u32,
    call: BtCall,
    known: &RefCell<Known>,
    shared: &Shared,
) {
    match call {
        BtCall::Disconnect { .. } => connection.disconnect(),
        BtCall::LinkPower { power, .. } => {
            known.borrow_mut().power = power;
            let target = wanted_power(&known.borrow());
            try_power(stack, connection, known, target).await;
        }
        BtCall::Discover { services, .. } => match discover(client, &services, known).await {
            Ok(services) => shared.report(BtReport::Discovered {
                link: number,
                services,
            }),
            Err(error) => shared.fail(number, BtOp::Discover, None, error),
        },
        BtCall::Read { handle, .. } => {
            let mut buffer = [0; 512];
            match client.read_handle(handle, &mut buffer).await {
                Ok(n) => shared.report(BtReport::Value {
                    link: number,
                    handle,
                    kind: ValueKind::Read,
                    bytes: buffer[..n].to_vec(),
                }),
                Err(e) => shared.fail(number, BtOp::Read, Some(handle), error_of(&e)),
            }
        }
        BtCall::Write {
            handle,
            bytes,
            response,
            ..
        } => {
            let written = if response {
                client.write_handle(handle, &bytes).await
            } else {
                client.write_handle_without_response(handle, &bytes).await
            };
            match (written, response) {
                (Ok(()), true) => shared.report(BtReport::Written {
                    link: number,
                    handle,
                }),
                (Ok(()), false) => shared.report(BtReport::Sent {
                    link: number,
                    handle,
                }),
                (Err(e), _) => shared.fail(number, BtOp::Write, Some(handle), error_of(&e)),
            }
        }
        BtCall::Subscribe { handle, on, .. } => {
            if let Err(error) = subscribe(client, handle, on, known).await {
                shared.fail(number, BtOp::Subscribe, Some(handle), error);
            }
        }
        BtCall::Pair { .. } => {
            // The reader confirms first, in the kernel's question: Just
            // Works pairing would ask nothing.
            known.borrow_mut().pairing = Pairing::Asked;
            shared.report(BtReport::Ask {
                link: number,
                question: PairQuestion::Confirm,
            });
        }
        BtCall::Answer { accept, .. } => {
            let pairing = known.borrow().pairing;
            match (pairing, accept) {
                (Pairing::Asked, true) => {
                    known.borrow_mut().pairing = Pairing::Running;
                    // Pairing takes some 80 messages each way and the
                    // device's patience: the shortest intervals meanwhile.
                    try_power(stack, connection, known, LinkPower::Burst).await;
                    let started = connection
                        .set_bondable(true)
                        .and_then(|()| connection.request_security());
                    if let Err(e) = started {
                        log::info!("bluetooth: link {number}: pairing: {e:?}");
                        known.borrow_mut().pairing = Pairing::No;
                        shared.fail(number, BtOp::Pair, None, BtError::Unsupported);
                    }
                }
                (Pairing::Number, true) => {
                    known.borrow_mut().pairing = Pairing::Running;
                    let _ = connection.pass_key_confirm();
                }
                (Pairing::Asked, false) => {
                    known.borrow_mut().pairing = Pairing::No;
                    shared.fail(number, BtOp::Pair, None, BtError::Declined);
                }
                (Pairing::Number, false) => {
                    // Told when the pairing fails.
                    let _ = connection.pass_key_cancel();
                }
                _ => {}
            }
        }
        BtCall::Hid { on, .. } => {
            if on {
                if let Err(error) = hid(client, number, known, shared).await {
                    log::info!("bluetooth: link {number}: HID: {error:?}");
                    shared.fail(number, BtOp::HidReports, None, error);
                }
            } else {
                let handles: Vec<u16> = known.borrow_mut().hid.drain(..).map(|(h, _)| h).collect();
                for handle in handles {
                    let _ = subscribe(client, handle, false, known).await;
                }
            }
        }
        _ => {}
    }
}

/// The device's services (only `wanted`, where some are named), their
/// characteristics and descriptors.
async fn discover(
    client: &Client<'_>,
    wanted: &[SdkUuid],
    known: &RefCell<Known>,
) -> Result<Vec<GattService>, BtError> {
    let mut handles: Vec<ServiceHandle> = Vec::new();
    if wanted.is_empty() {
        handles.extend(client.services().await.map_err(|e| error_of(&e))?);
    } else {
        for uuid in wanted {
            let found = client.services_by_uuid(&host_uuid(*uuid)).await;
            handles.extend(found.map_err(|e| error_of(&e))?);
        }
    }
    let mut services = Vec::new();
    for service in &handles {
        let found = client
            .characteristics::<CHARACTERISTICS>(service)
            .await
            .map_err(|e| error_of(&e))?;
        let mut characteristics = Vec::new();
        for c in &found {
            let properties = Properties::from_bits(c.props.to_raw());
            if let Some(cccd) = c.cccd_handle {
                let mut known = known.borrow_mut();
                known.configured.retain(|(h, _, _)| *h != c.handle);
                known.configured.push((c.handle, cccd, properties));
            }
            let mut characteristic =
                GattCharacteristic::new(sdk_uuid(&c.uuid), Handle::new(c.handle), properties);
            let descriptors = client
                .descriptors::<_, DESCRIPTORS>(c)
                .await
                .map_err(|e| error_of(&e))?;
            characteristic.descriptors = descriptors
                .iter()
                .map(|d| GattDescriptor::new(sdk_uuid(d.uuid()), Handle::new(d.handle())))
                .collect();
            characteristics.push(characteristic);
        }
        services.push(GattService::new(sdk_uuid(&service.uuid()), characteristics));
    }
    Ok(services)
}

/// Notifications (or indications, where the characteristic only
/// indicates) of `handle` on, or off.
async fn subscribe(
    client: &Client<'_>,
    handle: u16,
    on: bool,
    known: &RefCell<Known>,
) -> Result<(), BtError> {
    let (cccd, properties) = known
        .borrow()
        .configured
        .iter()
        .find(|(h, _, _)| *h == handle)
        .map(|(_, cccd, properties)| (*cccd, *properties))
        .ok_or(BtError::NotFound)?;
    let value: u16 = match (on, properties.contains(Properties::NOTIFY)) {
        (false, _) => 0,
        (true, true) => 1,
        (true, false) => 2,
    };
    client
        .write_handle(cccd, &value.to_le_bytes())
        .await
        .map_err(|e| error_of(&e))
}

/// A remote's HID service: its report map told, its input reports
/// subscribed (or the boot keyboard's, where it has no reports).
async fn hid(
    client: &Client<'_>,
    number: u32,
    known: &RefCell<Known>,
    shared: &Shared,
) -> Result<(), BtError> {
    // Met before: only its reports switched on again.
    let cached: Vec<u16> = known.borrow().hid.iter().map(|(h, _)| *h).collect();
    if !cached.is_empty() {
        let mut all = true;
        for handle in &cached {
            all &= subscribe(client, *handle, true, known).await.is_ok();
        }
        if all {
            log::info!("bluetooth: link {number}: HID as before");
            return Ok(());
        }
        log::info!("bluetooth: link {number}: HID changed, found anew");
        let address = known.borrow().address.clone();
        shared.hid.borrow_mut().retain(|h| h.address != address);
        known.borrow_mut().hid.clear();
    }
    let service = client
        .services_by_uuid(&Uuid::new_short(HID))
        .await
        .map_err(|e| error_of(&e))?
        .into_iter()
        .next()
        .ok_or(BtError::NotFound)?;
    let characteristics = client
        .characteristics::<CHARACTERISTICS>(&service)
        .await
        .map_err(|e| error_of(&e))?;
    let mut map_len = 0;
    let mut kept_map = Vec::new();
    if let Some(map) = characteristics
        .iter()
        .find(|c| c.uuid == Uuid::new_short(REPORT_MAP))
    {
        let mut buffer = alloc::vec![0; REPORT_MAP_MOST];
        let n = client
            .read_handle_blob(map.handle, 0, &mut buffer)
            .await
            .map_err(|e| error_of(&e))?;
        buffer.truncate(n);
        map_len = n;
        kept_map.clone_from(&buffer);
        shared.report(BtReport::HidMap {
            link: number,
            map: buffer,
        });
    }
    let mut inputs: Vec<(u16, u8)> = Vec::new();
    for c in characteristics
        .iter()
        .filter(|c| c.uuid == Uuid::new_short(REPORT) && c.cccd_handle.is_some())
    {
        let reference = client
            .descriptor_by_uuid::<_, [u8]>(c, &Uuid::new_short(REPORT_REFERENCE))
            .await;
        let mut id_and_type = [0, 1];
        if let Ok(reference) = reference {
            let _ = client.read_descriptor(&reference, &mut id_and_type).await;
        }
        // Type 1: an input report.
        if id_and_type[1] == 1 {
            inputs.push((c.handle, id_and_type[0]));
        }
    }
    if inputs.is_empty()
        && let Some(boot) = characteristics
            .iter()
            .find(|c| c.uuid == Uuid::new_short(BOOT_KEYBOARD_INPUT))
    {
        inputs.push((boot.handle, 0));
    }
    if inputs.is_empty() {
        return Err(BtError::NotFound);
    }
    for c in characteristics.iter() {
        if let Some(cccd) = c.cccd_handle {
            let properties = Properties::from_bits(c.props.to_raw());
            let mut known = known.borrow_mut();
            known.configured.retain(|(h, _, _)| *h != c.handle);
            known.configured.push((c.handle, cccd, properties));
        }
    }
    for (handle, _) in &inputs {
        subscribe(client, *handle, true, known).await?;
    }
    log::info!(
        "bluetooth: link {number}: HID reports {inputs:?}, map {} bytes",
        map_len
    );
    let address = known.borrow().address.clone();
    let configured = known.borrow().configured.clone();
    let mut cache = shared.hid.borrow_mut();
    cache.retain(|h| h.address != address);
    cache.push(HidCache {
        address,
        inputs: inputs.clone(),
        configured,
        map: kept_map,
    });
    drop(cache);
    known.borrow_mut().hid = inputs;
    Ok(())
}

/// What a host error means for the plugin.
fn error_of<E: core::fmt::Debug>(error: &BleHostError<E>) -> BtError {
    match error {
        BleHostError::BleHost(Error::Att(code)) => BtError::Att(AttError::new(code.to_u8())),
        BleHostError::BleHost(Error::Timeout) => BtError::Timeout,
        BleHostError::BleHost(Error::Disconnected) => BtError::Lost,
        BleHostError::BleHost(Error::NotFound) => BtError::NotFound,
        BleHostError::BleHost(Error::NotSupported) => BtError::Unsupported,
        other => {
            log::info!("bluetooth: {other:?}");
            BtError::Lost
        }
    }
}

/// The kernel's UUID for the host's.
fn sdk_uuid(uuid: &Uuid) -> SdkUuid {
    /// The Bluetooth base UUID, which 16 and 32 bit numbers fill in.
    const BASE: u128 = 0x0000_0000_0000_1000_8000_0080_5f9b_34fb;
    match uuid {
        Uuid::Uuid16(bytes) => SdkUuid::sig(u16::from_le_bytes(*bytes)),
        Uuid::Uuid32(bytes) => {
            SdkUuid::from_u128(BASE | (u128::from(u32::from_le_bytes(*bytes)) << 96))
        }
        Uuid::Uuid128(bytes) => SdkUuid::from_u128(u128::from_le_bytes(*bytes)),
    }
}

/// The host's UUID for the kernel's.
fn host_uuid(uuid: SdkUuid) -> Uuid {
    match uuid.short() {
        Some(short) => Uuid::new_short(short),
        None => Uuid::new_long(uuid.as_u128().to_le_bytes()),
    }
}

/// An address as the kernel keeps it: most significant byte first, with
/// `/r` for a random one.
fn address_text(kind: AddrKind, address: BdAddr) -> String {
    let b = address.raw();
    let random = if kind.as_raw() & 1 == 1 { "/r" } else { "" };
    format!(
        "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}{random}",
        b[5], b[4], b[3], b[2], b[1], b[0]
    )
}

/// The address `text` names ([`address_text`]).
fn address_of(text: &str) -> Option<Address> {
    let (bytes, kind) = match text.strip_suffix("/r") {
        Some(bytes) => (bytes, AddrKind::RANDOM),
        None => (text, AddrKind::PUBLIC),
    };
    let mut address = [0; 6];
    let mut parts = bytes.split(':');
    for at in (0..6).rev() {
        address[at] = u8::from_str_radix(parts.next()?, 16).ok()?;
    }
    if parts.next().is_some() {
        return None;
    }
    Some(Address::new(kind, BdAddr::new(address)))
}

/// A bond's keys as the kernel keeps them: the long term key, the
/// identity resolving key (`-` for none), the security (0 to 2) and the
/// identity address, separated by dots.
fn keys_text(bond: &BondInformation) -> String {
    let irk = bond
        .identity
        .irk
        .map_or_else(|| String::from("-"), |irk| format!("{:032x}", irk.0.get()));
    let security = match bond.security_level {
        SecurityLevel::NoEncryption => 0,
        SecurityLevel::Encrypted => 1,
        SecurityLevel::EncryptedAuthenticated => 2,
    };
    format!(
        "{:032x}.{irk}.{security}.{}",
        bond.ltk.0,
        address_text(bond.identity.addr.kind, bond.identity.addr.addr)
    )
}

/// The bond the kernel keeps, for the host ([`keys_text`]).
fn bond_of(bond: &Bond) -> Option<BondInformation> {
    let mut parts = bond.keys.split('.');
    let ltk = u128::from_str_radix(parts.next()?, 16).ok()?;
    let irk = match parts.next()? {
        "-" => None,
        irk => IdentityResolvingKey::new(u128::from_str_radix(irk, 16).ok()?),
    };
    let security_level = match parts.next()? {
        "2" => SecurityLevel::EncryptedAuthenticated,
        "1" => SecurityLevel::Encrypted,
        _ => SecurityLevel::NoEncryption,
    };
    let address = parts
        .next()
        .and_then(address_of)
        .or_else(|| address_of(&bond.address))?;
    Some(BondInformation {
        ltk: LongTermKey::new(ltk),
        identity: Identity { addr: address, irk },
        is_bonded: true,
        security_level,
    })
}
