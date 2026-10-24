//! Bluetooth Low Energy (see [ADR 0027](../../../docs/adr/0027-bluetooth.md)):
//! [`Env::bluetooth`](crate::Env::bluetooth) is `Some` where the kernel
//! may use a radio ([`Capabilities::bluetooth`](crate::Capabilities)).
//!
//! A plugin that declares [`Uses::BLUETOOTH`](crate::Uses::BLUETOOTH)
//! sends [`Command::Bluetooth`], written with
//! the functions here, and hears what happens as its own events, each a
//! [`BtEvent`]: one event for a scan ([`scan`]), one for a link
//! ([`connect`]), one for being seen ([`advertise`]). Every command is
//! answered, by its result or a [`BtEvent::Failed`].
//!
//! - **Scanning**: [`scan`] with filters and a duration finds devices in
//!   reach ([`BtEvent::Found`]). Several plugins' scans are one scan of
//!   the radio, each plugin hearing what passes its own filters.
//! - **A GATT client**: [`connect`], [`discover`] the services and their
//!   characteristics, [`read`], [`write()`], [`subscribe`] to notifications
//!   ([`BtEvent::Value`]).
//! - **Pairing** ([`pair`]) is always confirmed by the reader, in the
//!   kernel's own prompt: a plugin only asks.
//! - **Being seen**: [`advertise`] for a while, and a GATT server of
//!   services declared at registration
//!   ([`Registrar::add_gatt_service`](crate::Registrar::add_gatt_service)):
//!   centrals' reads and writes come as events ([`BtEvent::ReadRequest`],
//!   [`BtEvent::WriteRequest`]), answered with [`respond`], and [`notify`]
//!   sends to those subscribed.
//! - **Keys of a remote or a keyboard**: a bonded device that types is
//!   the kernel's: [`keys`] connects it and its keys arrive as inputs of
//!   the key maps (`hid.right`, `hid.page-down`, `hid.volume-up` …), with
//!   no plugin parsing its reports. A plugin that wants the reports
//!   themselves asks with [`hid_reports`].
//!
//! A plugin sees only devices its own scans found and bonded devices the
//! reader gave it; commands about others fail with [`BtError::NotFound`].
//! Devices are known by a [`DeviceId`], never by their address. The keys
//! of a bond are kept with the passwords and keys. Scans and advertising
//! always end; scans, advertising and links end with the screen that was
//! showing when they were asked for, and all before the device sleeps.
//! The kernel switches the radio on for the first request and off once
//! none is left.

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::event::{Event, EventData, ParamValue};
use crate::{Command, Id, Params};

/// The longest a scan runs; a longer one is cut to it.
pub const SCAN_MOST_MS: u32 = 60_000;
/// The longest a plugin is advertised at a time; a longer one is cut to it.
pub const ADVERTISE_MOST_MS: u32 = 10 * 60_000;

/// A Bluetooth UUID: of a service, a characteristic or a descriptor.
/// Those of the Bluetooth SIG are 16-bit numbers in a base UUID
/// ([`Uuid::sig`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Uuid(u128);

/// The Bluetooth base UUID, `00000000-0000-1000-8000-00805f9b34fb`.
const BASE: u128 = 0x0000_0000_0000_1000_8000_0080_5f9b_34fb;

impl Uuid {
    /// A number of the Bluetooth SIG's, e.g. `0x180D` (heart rate) or
    /// `0x1812` (HID).
    pub const fn sig(number: u16) -> Uuid {
        Uuid(BASE | ((number as u128) << 96))
    }

    pub const fn from_u128(value: u128) -> Uuid {
        Uuid(value)
    }

    pub const fn as_u128(self) -> u128 {
        self.0
    }

    /// Its 16-bit number, where it is one of the SIG's.
    pub const fn short(self) -> Option<u16> {
        if self.0 & !(0xffff_u128 << 96) == BASE {
            Some((self.0 >> 96) as u16)
        } else {
            None
        }
    }

    /// From `180d` or `0x180D` (a SIG number), or the 36 characters of a
    /// full UUID.
    pub fn parse(text: &str) -> Option<Uuid> {
        let text = text.trim();
        let short = text.strip_prefix("0x").unwrap_or(text);
        if short.len() == 4 {
            return u16::from_str_radix(short, 16).ok().map(Uuid::sig);
        }
        let hex: String = text.chars().filter(|c| *c != '-').collect();
        if text.len() != 36 || hex.len() != 32 {
            return None;
        }
        u128::from_str_radix(&hex, 16).ok().map(Uuid)
    }
}

impl core::fmt::Display for Uuid {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let v = self.0;
        write!(
            f,
            "{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
            (v >> 96) as u32,
            (v >> 80) as u16,
            (v >> 64) as u16,
            (v >> 48) as u16,
            v & 0xffff_ffff_ffff
        )
    }
}

/// A device as the kernel knows it: a number of its own, stable across
/// starts for a bonded device. Its address is never shown to plugins: it
/// may be random and change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DeviceId(u32);

impl DeviceId {
    /// The device numbered `number` by the kernel.
    #[cfg(any(feature = "kernel", test))]
    pub const fn new(number: u32) -> DeviceId {
        DeviceId(number)
    }

    pub const fn number(self) -> u32 {
        self.0
    }
}

/// A characteristic's place in a device's attribute table, from
/// [`BtEvent::Discovered`], or in the device's own GATT server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Handle(u16);

impl Handle {
    /// The attribute at `handle`.
    #[cfg(any(feature = "kernel", test))]
    pub const fn new(handle: u16) -> Handle {
        Handle(handle)
    }

    pub const fn get(self) -> u16 {
        self.0
    }
}

/// A central's read or write of the device's GATT server, to answer with
/// [`respond`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Request(u32);

impl Request {
    #[cfg(any(feature = "kernel", test))]
    pub const fn new(number: u32) -> Request {
        Request(number)
    }

    pub const fn number(self) -> u32 {
        self.0
    }
}

/// What a device is, from its appearance and its services.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum DeviceKind {
    /// It types: a keyboard.
    Keyboard,
    /// A remote, a page-turner or a media remote (an HID device that is no
    /// keyboard).
    Remote,
    /// A sensor (heart rate, temperature …).
    Sensor,
    /// A phone or a computer.
    Phone,
    #[default]
    Other,
}

impl DeviceKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            DeviceKind::Keyboard => "keyboard",
            DeviceKind::Remote => "remote",
            DeviceKind::Sensor => "sensor",
            DeviceKind::Phone => "phone",
            DeviceKind::Other => "other",
        }
    }

    pub fn parse(text: &str) -> DeviceKind {
        match text {
            "keyboard" => DeviceKind::Keyboard,
            "remote" => DeviceKind::Remote,
            "sensor" => DeviceKind::Sensor,
            "phone" => DeviceKind::Phone,
            _ => DeviceKind::Other,
        }
    }

    /// Whether its keys can go to the key maps ([`keys`]).
    pub const fn types(self) -> bool {
        matches!(self, DeviceKind::Keyboard | DeviceKind::Remote)
    }
}

/// What can be done with a characteristic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct Properties {
    pub read: bool,
    pub write: bool,
    pub write_without_response: bool,
    pub notify: bool,
    pub indicate: bool,
}

impl Properties {
    /// Nothing; add with the methods.
    pub const NONE: Properties = Properties {
        read: false,
        write: false,
        write_without_response: false,
        notify: false,
        indicate: false,
    };

    pub const fn read(mut self) -> Self {
        self.read = true;
        self
    }

    pub const fn write(mut self) -> Self {
        self.write = true;
        self
    }

    pub const fn write_without_response(mut self) -> Self {
        self.write_without_response = true;
        self
    }

    pub const fn notify(mut self) -> Self {
        self.notify = true;
        self
    }

    pub const fn indicate(mut self) -> Self {
        self.indicate = true;
        self
    }

    /// The bits of the characteristic declaration (Core Specification
    /// Vol 3, Part G, 3.3.1.1).
    pub const fn bits(self) -> u8 {
        (self.read as u8) << 1
            | (self.write_without_response as u8) << 2
            | (self.write as u8) << 3
            | (self.notify as u8) << 4
            | (self.indicate as u8) << 5
    }

    pub const fn from_bits(bits: u8) -> Properties {
        Properties {
            read: bits & 0x02 != 0,
            write_without_response: bits & 0x04 != 0,
            write: bits & 0x08 != 0,
            notify: bits & 0x10 != 0,
            indicate: bits & 0x20 != 0,
        }
    }
}

/// A characteristic of a device's service.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Characteristic {
    pub uuid: Uuid,
    /// Where it is read, written and subscribed to.
    pub handle: Handle,
    pub properties: Properties,
}

impl Characteristic {
    pub fn new(uuid: Uuid, handle: Handle, properties: Properties) -> Self {
        Self {
            uuid,
            handle,
            properties,
        }
    }
}

/// A service of a device, with its characteristics.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Service {
    pub uuid: Uuid,
    pub characteristics: Vec<Characteristic>,
}

impl Service {
    pub fn new(uuid: Uuid, characteristics: Vec<Characteristic>) -> Self {
        Self {
            uuid,
            characteristics,
        }
    }
}

/// Characteristic `characteristic` of service `service` among `services`
/// (those [`BtEvent::Discovered`] lists).
pub fn characteristic(
    services: &[Service],
    service: Uuid,
    characteristic: Uuid,
) -> Option<&Characteristic> {
    services
        .iter()
        .filter(|s| s.uuid == service)
        .flat_map(|s| s.characteristics.iter())
        .find(|c| c.uuid == characteristic)
}

/// A device found by a scan.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Device {
    pub id: DeviceId,
    /// The name it gives, where it gives one.
    pub name: Option<String>,
    /// Its signal, in dBm (e.g. -50 strong, -90 weak), where known.
    pub rssi: Option<i8>,
    /// The services it says it offers.
    pub services: Vec<Uuid>,
    /// It is bonded with this device.
    pub bonded: bool,
    pub kind: DeviceKind,
}

impl Device {
    pub fn new(id: DeviceId) -> Self {
        Self {
            id,
            name: None,
            rssi: None,
            services: Vec::new(),
            bonded: false,
            kind: DeviceKind::Other,
        }
    }

    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    pub fn rssi(mut self, rssi: i8) -> Self {
        self.rssi = Some(rssi);
        self
    }

    pub fn service(mut self, service: Uuid) -> Self {
        self.services.push(service);
        self
    }

    pub fn bonded(mut self) -> Self {
        self.bonded = true;
        self
    }

    pub fn kind(mut self, kind: DeviceKind) -> Self {
        self.kind = kind;
        self
    }
}

/// Why something with Bluetooth did not happen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum BtError {
    /// There is no radio here, or it could not be switched on.
    Off,
    /// The plugin does not declare [`Uses::BLUETOOTH`](crate::Uses), or
    /// asked what only the reader may (forgetting, giving).
    NotAllowed,
    /// The device is not one of the plugin's, or not in reach; the handle
    /// is not the device's.
    NotFound,
    /// No link is left ([`Bluetooth::max_links`]), or the device's memory
    /// cannot take the radio now.
    Busy,
    /// The device did not answer in time.
    Timeout,
    /// The reader said no (to pairing).
    Refused,
    /// The link was lost.
    Lost,
    /// The device answered with this ATT error (Core Specification Vol 3,
    /// Part F, 3.4.1.1), e.g. `0x05`, insufficient authentication.
    Att(u8),
    /// Not done here (e.g. advertising on a shell that cannot).
    Unsupported,
}

impl BtError {
    pub fn to_text(self) -> String {
        match self {
            BtError::Off => "off".to_string(),
            BtError::NotAllowed => "not-allowed".to_string(),
            BtError::NotFound => "not-found".to_string(),
            BtError::Busy => "busy".to_string(),
            BtError::Timeout => "timeout".to_string(),
            BtError::Refused => "refused".to_string(),
            BtError::Lost => "lost".to_string(),
            BtError::Att(code) => alloc::format!("att-{code:02x}"),
            BtError::Unsupported => "unsupported".to_string(),
        }
    }

    pub fn parse(text: &str) -> Option<BtError> {
        Some(match text {
            "off" => BtError::Off,
            "not-allowed" => BtError::NotAllowed,
            "not-found" => BtError::NotFound,
            "busy" => BtError::Busy,
            "timeout" => BtError::Timeout,
            "refused" => BtError::Refused,
            "lost" => BtError::Lost,
            "unsupported" => BtError::Unsupported,
            other => BtError::Att(u8::from_str_radix(other.strip_prefix("att-")?, 16).ok()?),
        })
    }
}

/// What happened, as a plugin's event (see the [module documentation](self)).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum BtEvent {
    /// A scan found `device` (each device once a scan, again where its
    /// name or services come later).
    Found(Device),
    /// The scan ended (its time was up, it was stopped, or its screen
    /// closed).
    ScanEnded,
    Connected {
        device: DeviceId,
    },
    /// The link ended; `reason` where it was not asked for.
    Disconnected {
        device: DeviceId,
        reason: Option<BtError>,
    },
    /// The device's services, after [`discover`].
    Discovered {
        device: DeviceId,
        services: Vec<Service>,
    },
    /// A value read ([`read`]) or notified ([`subscribe`]).
    Value {
        device: DeviceId,
        handle: Handle,
        bytes: Vec<u8>,
    },
    /// A [`write()`] with a response was done.
    Written {
        device: DeviceId,
        handle: Handle,
    },
    /// Pairing ([`pair`]) is done: the device is bonded and given to the
    /// plugin that asked.
    Paired {
        device: DeviceId,
    },
    /// A command could not be done; `device` where it was about one.
    Failed {
        device: Option<DeviceId>,
        error: BtError,
    },
    /// The device is advertised ([`advertise`]).
    Advertising,
    /// Advertising ended (its time was up, it was stopped, its screen
    /// closed).
    AdvertisingEnded,
    /// A central reads characteristic `handle` of the plugin's service:
    /// answer with [`respond`].
    ReadRequest {
        request: Request,
        handle: Handle,
    },
    /// A central wrote `bytes` to characteristic `handle` of the plugin's
    /// service.
    WriteRequest {
        handle: Handle,
        bytes: Vec<u8>,
    },
    /// A central subscribed to characteristic `handle` (or no longer).
    Subscribed {
        handle: Handle,
        on: bool,
    },
    /// An input report of an HID device ([`hid_reports`]), as it came,
    /// with its report id where the device numbers its reports.
    HidReport {
        device: DeviceId,
        report: Option<u8>,
        bytes: Vec<u8>,
    },
}

/// The parameters of a [`BtEvent`] written out (for observers and rules).
pub mod bt_params {
    pub const KIND: &str = "kind";
    pub const DEVICE: &str = "device";
    pub const NAME: &str = "name";
    pub const RSSI: &str = "rssi";
    pub const SERVICES: &str = "services";
    pub const BONDED: &str = "bonded";
    pub const DEVICE_KIND: &str = "device-kind";
    pub const HANDLE: &str = "handle";
    pub const BYTES: &str = "bytes";
    pub const ERROR: &str = "error";
    pub const REQUEST: &str = "request";
    pub const ON: &str = "on";
    pub const REPORT: &str = "report";
    /// A discovered service's characteristics:
    /// `<service uuid>` → `<uuid>/<handle>/<property bits>,…`.
    pub const SERVICE: &str = "service.";
}

fn hex(bytes: &[u8]) -> String {
    crate::digest::hex::encode(bytes)
}

fn write_device(params: &mut Params, device: &Device) {
    use bt_params as p;
    params.set(p::DEVICE, device.id.0.to_param());
    if let Some(name) = &device.name {
        params.set(p::NAME, name.clone());
    }
    if let Some(rssi) = device.rssi {
        params.set(p::RSSI, rssi.to_string());
    }
    let services: Vec<String> = device.services.iter().map(Uuid::to_string).collect();
    params.set(p::SERVICES, services.join(","));
    params.set(p::BONDED, device.bonded.to_param());
    params.set(p::DEVICE_KIND, device.kind.as_str());
}

fn write_services(params: &mut Params, services: &[Service]) {
    for service in services {
        let list: Vec<String> = service
            .characteristics
            .iter()
            .map(|c| alloc::format!("{}/{}/{}", c.uuid, c.handle.0, c.properties.bits()))
            .collect();
        params.set(
            alloc::format!("{}{}", bt_params::SERVICE, service.uuid),
            list.join(","),
        );
    }
}

impl EventData for BtEvent {
    /// Written out as text; a value's bytes in hex.
    fn to_params(&self) -> Params {
        use bt_params as p;
        let mut params = Params::new();
        let kind = match self {
            BtEvent::Found(device) => {
                write_device(&mut params, device);
                "found"
            }
            BtEvent::ScanEnded => "scan-ended",
            BtEvent::Connected { device } => {
                params.set(p::DEVICE, device.0.to_param());
                "connected"
            }
            BtEvent::Disconnected { device, reason } => {
                params.set(p::DEVICE, device.0.to_param());
                if let Some(reason) = reason {
                    params.set(p::ERROR, reason.to_text());
                }
                "disconnected"
            }
            BtEvent::Discovered { device, services } => {
                params.set(p::DEVICE, device.0.to_param());
                write_services(&mut params, services);
                "discovered"
            }
            BtEvent::Value {
                device,
                handle,
                bytes,
            } => {
                params.set(p::DEVICE, device.0.to_param());
                params.set(p::HANDLE, handle.0.to_param());
                params.set(p::BYTES, hex(bytes));
                "value"
            }
            BtEvent::Written { device, handle } => {
                params.set(p::DEVICE, device.0.to_param());
                params.set(p::HANDLE, handle.0.to_param());
                "written"
            }
            BtEvent::Paired { device } => {
                params.set(p::DEVICE, device.0.to_param());
                "paired"
            }
            BtEvent::Failed { device, error } => {
                if let Some(device) = device {
                    params.set(p::DEVICE, device.0.to_param());
                }
                params.set(p::ERROR, error.to_text());
                "failed"
            }
            BtEvent::Advertising => "advertising",
            BtEvent::AdvertisingEnded => "advertising-ended",
            BtEvent::ReadRequest { request, handle } => {
                params.set(p::REQUEST, request.0.to_param());
                params.set(p::HANDLE, handle.0.to_param());
                "read-request"
            }
            BtEvent::WriteRequest { handle, bytes } => {
                params.set(p::HANDLE, handle.0.to_param());
                params.set(p::BYTES, hex(bytes));
                "write-request"
            }
            BtEvent::Subscribed { handle, on } => {
                params.set(p::HANDLE, handle.0.to_param());
                params.set(p::ON, on.to_param());
                "subscribed"
            }
            BtEvent::HidReport {
                device,
                report,
                bytes,
            } => {
                params.set(p::DEVICE, device.0.to_param());
                if let Some(report) = report {
                    params.set(p::REPORT, report.to_param());
                }
                params.set(p::BYTES, hex(bytes));
                "hid-report"
            }
        };
        params.set(p::KIND, kind);
        params
    }

    /// Its bytes go along as they are, not copied.
    fn into_params(self) -> Params {
        self.to_params().carrying(self)
    }

    fn from_params(params: &Params) -> Option<Self> {
        use bt_params as p;
        let device = || {
            params
                .get(p::DEVICE)
                .and_then(u32::from_param)
                .map(DeviceId)
        };
        let handle = || params.get(p::HANDLE).and_then(u16::from_param).map(Handle);
        let bytes = || crate::digest::hex::decode(params.get(p::BYTES).unwrap_or_default());
        let error = || params.get(p::ERROR).and_then(BtError::parse);
        Some(match params.get(p::KIND)? {
            "found" => BtEvent::Found(Device {
                id: device()?,
                name: params.get(p::NAME).map(String::from),
                rssi: params.get(p::RSSI).and_then(|r| r.parse().ok()),
                services: params
                    .get(p::SERVICES)
                    .unwrap_or_default()
                    .split(',')
                    .filter_map(Uuid::parse)
                    .collect(),
                bonded: params.get(p::BONDED) == Some("true"),
                kind: DeviceKind::parse(params.get(p::DEVICE_KIND).unwrap_or_default()),
            }),
            "scan-ended" => BtEvent::ScanEnded,
            "connected" => BtEvent::Connected { device: device()? },
            "disconnected" => BtEvent::Disconnected {
                device: device()?,
                reason: error(),
            },
            "discovered" => BtEvent::Discovered {
                device: device()?,
                services: params
                    .iter()
                    .filter_map(|(key, list)| {
                        let uuid = Uuid::parse(key.strip_prefix(p::SERVICE)?)?;
                        let characteristics = list
                            .split(',')
                            .filter_map(|c| {
                                let mut parts = c.split('/');
                                let uuid = Uuid::parse(parts.next()?)?;
                                let handle = Handle(parts.next()?.parse().ok()?);
                                let bits = parts.next()?.parse().ok()?;
                                Some(Characteristic::new(
                                    uuid,
                                    handle,
                                    Properties::from_bits(bits),
                                ))
                            })
                            .collect();
                        Some(Service::new(uuid, characteristics))
                    })
                    .collect(),
            },
            "value" => BtEvent::Value {
                device: device()?,
                handle: handle()?,
                bytes: bytes()?,
            },
            "written" => BtEvent::Written {
                device: device()?,
                handle: handle()?,
            },
            "paired" => BtEvent::Paired { device: device()? },
            "failed" => BtEvent::Failed {
                device: device(),
                error: error()?,
            },
            "advertising" => BtEvent::Advertising,
            "advertising-ended" => BtEvent::AdvertisingEnded,
            "read-request" => BtEvent::ReadRequest {
                request: Request(params.get(p::REQUEST).and_then(u32::from_param)?),
                handle: handle()?,
            },
            "write-request" => BtEvent::WriteRequest {
                handle: handle()?,
                bytes: bytes()?,
            },
            "subscribed" => BtEvent::Subscribed {
                handle: handle()?,
                on: params.get(p::ON) == Some("true"),
            },
            "hid-report" => BtEvent::HidReport {
                device: device()?,
                report: params.get(p::REPORT).and_then(u8::from_param),
                bytes: bytes()?,
            },
            _ => return None,
        })
    }
}

/// How a [`write()`] is made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum WriteKind {
    /// Answered with [`BtEvent::Written`].
    #[default]
    WithResponse,
    /// Not answered (faster; nothing says it arrived).
    WithoutResponse,
}

/// A scan: what it looks for, for how long, and where its results go.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Scan {
    pub events: Id,
    /// Only devices that say they offer one of these (all, where empty).
    pub services: Vec<Uuid>,
    /// Only devices whose name begins so.
    pub name_prefix: Option<String>,
    /// How long it runs, at most [`SCAN_MOST_MS`].
    pub ms: u32,
}

impl Scan {
    /// Whether `device` passes its filters.
    pub fn passes(&self, device: &Device) -> bool {
        let service =
            self.services.is_empty() || self.services.iter().any(|s| device.services.contains(s));
        let name = self.name_prefix.as_deref().is_none_or(|prefix| {
            device
                .name
                .as_deref()
                .is_some_and(|name| name.starts_with(prefix))
        });
        service && name
    }

    pub fn service(mut self, service: Uuid) -> Self {
        self.services.push(service);
        self
    }

    pub fn name_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.name_prefix = Some(prefix.into());
        self
    }

    pub fn for_ms(mut self, ms: u32) -> Self {
        self.ms = ms.min(SCAN_MOST_MS);
        self
    }

    pub fn command(self) -> Command {
        BtCommand::Scan(self).command()
    }
}

/// Being seen: the name and services advertised, for how long, and where
/// what happens goes.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Advertising {
    pub events: Id,
    pub name: Option<Cow<'static, str>>,
    pub services: Vec<Uuid>,
    /// How long, at most [`ADVERTISE_MOST_MS`].
    pub ms: u32,
}

impl Advertising {
    pub fn name(mut self, name: impl Into<Cow<'static, str>>) -> Self {
        self.name = Some(name.into());
        self
    }

    pub fn service(mut self, service: Uuid) -> Self {
        self.services.push(service);
        self
    }

    pub fn for_ms(mut self, ms: u32) -> Self {
        self.ms = ms.min(ADVERTISE_MOST_MS);
        self
    }

    pub fn command(self) -> Command {
        BtCommand::Advertise(self).command()
    }
}

/// What a plugin asks of the radio ([`Command::Bluetooth`]); written with
/// the functions of this module.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum BtCommand {
    Scan(Scan),
    /// Ends the plugin's scan whose results go to `events`.
    StopScan {
        events: Id,
    },
    Connect {
        device: DeviceId,
        events: Id,
    },
    Disconnect {
        device: DeviceId,
    },
    Discover {
        device: DeviceId,
    },
    Read {
        device: DeviceId,
        handle: Handle,
    },
    Write {
        device: DeviceId,
        handle: Handle,
        bytes: Vec<u8>,
        kind: WriteKind,
    },
    Subscribe {
        device: DeviceId,
        handle: Handle,
    },
    Unsubscribe {
        device: DeviceId,
        handle: Handle,
    },
    Pair {
        device: DeviceId,
    },
    /// Removes the bond and what the device was given; the reader's own
    /// doing only.
    Forget {
        device: DeviceId,
    },
    /// Gives bonded `device` to plugin `plugin` (or takes it back); the
    /// reader's own doing only.
    Give {
        device: DeviceId,
        plugin: Id,
        given: bool,
    },
    /// The bonded device's keys go to the key maps (connecting it), or no
    /// longer (letting it go).
    Keys {
        device: DeviceId,
        on: bool,
    },
    HidReports {
        device: DeviceId,
        events: Id,
    },
    Advertise(Advertising),
    StopAdvertising,
    Notify {
        handle: Handle,
        bytes: Vec<u8>,
    },
    Respond {
        request: Request,
        bytes: Vec<u8>,
    },
}

impl BtCommand {
    pub fn command(self) -> Command {
        Command::Bluetooth(Box::new(BtRequest {
            command: self,
            from: None,
        }))
    }
}

/// A [`BtCommand`] on its way to the kernel, with the plugin that sent it
/// (filled in by the kernel).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BtRequest {
    command: BtCommand,
    from: Option<Id>,
}

impl BtRequest {
    pub fn command(&self) -> &BtCommand {
        &self.command
    }

    /// The plugin that sent it, where the kernel said.
    pub fn from(&self) -> Option<&Id> {
        self.from.as_ref()
    }

    /// Says that plugin `plugin` sent it.
    #[cfg(feature = "kernel")]
    pub fn set_from(&mut self, plugin: Id) {
        self.from = Some(plugin);
    }

    #[cfg(feature = "kernel")]
    pub fn into_parts(self) -> (BtCommand, Option<Id>) {
        (self.command, self.from)
    }
}

/// Looks for devices for a while; each found comes as `events`
/// ([`BtEvent::Found`]), then [`BtEvent::ScanEnded`].
pub fn scan(events: &Event<BtEvent>) -> Scan {
    Scan {
        events: events.id().clone(),
        services: Vec::new(),
        name_prefix: None,
        ms: 10_000,
    }
}

/// Ends the scan whose results go to `events`.
pub fn stop_scan(events: &Event<BtEvent>) -> Command {
    BtCommand::StopScan {
        events: events.id().clone(),
    }
    .command()
}

/// Connects to `device`; all about the link comes as `events`.
pub fn connect(device: DeviceId, events: &Event<BtEvent>) -> Command {
    BtCommand::Connect {
        device,
        events: events.id().clone(),
    }
    .command()
}

pub fn disconnect(device: DeviceId) -> Command {
    BtCommand::Disconnect { device }.command()
}

/// Finds the services of a connected device and their characteristics
/// ([`BtEvent::Discovered`]).
pub fn discover(device: DeviceId) -> Command {
    BtCommand::Discover { device }.command()
}

/// Reads characteristic `handle` ([`BtEvent::Value`]).
pub fn read(device: DeviceId, handle: Handle) -> Command {
    BtCommand::Read { device, handle }.command()
}

/// Writes `bytes` to characteristic `handle`, with a response
/// ([`BtEvent::Written`]) unless [`Write::without_response`].
pub fn write(device: DeviceId, handle: Handle, bytes: impl Into<Vec<u8>>) -> Write {
    Write {
        device,
        handle,
        bytes: bytes.into(),
        kind: WriteKind::WithResponse,
    }
}

/// A [`write()`] being written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Write {
    device: DeviceId,
    handle: Handle,
    bytes: Vec<u8>,
    kind: WriteKind,
}

impl Write {
    pub fn without_response(mut self) -> Self {
        self.kind = WriteKind::WithoutResponse;
        self
    }

    pub fn command(self) -> Command {
        BtCommand::Write {
            device: self.device,
            handle: self.handle,
            bytes: self.bytes,
            kind: self.kind,
        }
        .command()
    }
}

/// Notifications (or indications) of characteristic `handle` come as
/// [`BtEvent::Value`].
pub fn subscribe(device: DeviceId, handle: Handle) -> Command {
    BtCommand::Subscribe { device, handle }.command()
}

pub fn unsubscribe(device: DeviceId, handle: Handle) -> Command {
    BtCommand::Unsubscribe { device, handle }.command()
}

/// Asks the reader to pair with `device` ([`BtEvent::Paired`], or
/// [`BtError::Refused`]).
pub fn pair(device: DeviceId) -> Command {
    BtCommand::Pair { device }.command()
}

/// Forgets bonded `device`: its keys and what it was given. Only as the
/// reader's own doing (a page's row they chose).
pub fn forget(device: DeviceId) -> Command {
    BtCommand::Forget { device }.command()
}

/// Gives bonded `device` to `plugin`: it may connect to it. Only as the
/// reader's own doing.
pub fn give(device: DeviceId, plugin: impl Into<Id>) -> Command {
    BtCommand::Give {
        device,
        plugin: plugin.into(),
        given: true,
    }
    .command()
}

/// Takes bonded `device` back from `plugin`. Only as the reader's own
/// doing.
pub fn take_back(device: DeviceId, plugin: impl Into<Id>) -> Command {
    BtCommand::Give {
        device,
        plugin: plugin.into(),
        given: false,
    }
    .command()
}

/// The keys of bonded `device` go to the key maps from now on: the kernel
/// connects it.
pub fn keys(device: DeviceId) -> Command {
    BtCommand::Keys { device, on: true }.command()
}

/// The keys of `device` no longer go to the key maps: the kernel lets it
/// go.
pub fn keys_off(device: DeviceId) -> Command {
    BtCommand::Keys { device, on: false }.command()
}

/// The input reports of HID `device` (given to the plugin) come as
/// `events` ([`BtEvent::HidReport`]).
pub fn hid_reports(device: DeviceId, events: &Event<BtEvent>) -> Command {
    BtCommand::HidReports {
        device,
        events: events.id().clone(),
    }
    .command()
}

/// Advertises the device for a while; what happens comes as `events`.
pub fn advertise(events: &Event<BtEvent>) -> Advertising {
    Advertising {
        events: events.id().clone(),
        name: None,
        services: Vec::new(),
        ms: 60_000,
    }
}

pub fn stop_advertising() -> Command {
    BtCommand::StopAdvertising.command()
}

/// Sends `bytes` of the plugin's characteristic `handle` to the centrals
/// subscribed to it.
pub fn notify(handle: Handle, bytes: impl Into<Vec<u8>>) -> Command {
    BtCommand::Notify {
        handle,
        bytes: bytes.into(),
    }
    .command()
}

/// Answers a central's read ([`BtEvent::ReadRequest`]).
pub fn respond(request: Request, bytes: impl Into<Vec<u8>>) -> Command {
    BtCommand::Respond {
        request,
        bytes: bytes.into(),
    }
    .command()
}

/// A service of the device's own GATT server, declared at registration
/// ([`Registrar::add_gatt_service`](crate::Registrar::add_gatt_service)):
/// centrals' reads, writes and subscriptions of its characteristics come
/// as `events`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct GattServiceSpec {
    pub uuid: Uuid,
    pub characteristics: Vec<(Uuid, Properties)>,
    pub events: Id,
}

impl GattServiceSpec {
    pub fn new(uuid: Uuid, events: &Event<BtEvent>) -> Self {
        Self {
            uuid,
            characteristics: Vec::new(),
            events: events.id().clone(),
        }
    }

    pub fn characteristic(mut self, uuid: Uuid, properties: Properties) -> Self {
        self.characteristics.push((uuid, properties));
        self
    }
}

/// Where the radio is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum RadioState {
    #[default]
    Off,
    On,
}

/// A device bonded with this one.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Bonded {
    pub id: DeviceId,
    pub name: String,
    pub kind: DeviceKind,
    /// A link to it is up.
    pub connected: bool,
    /// Its keys go to the key maps ([`keys`]).
    pub keys: bool,
    /// The plugins it is given to.
    pub given: Vec<Id>,
}

impl Bonded {
    pub fn new(id: DeviceId, name: impl Into<String>, kind: DeviceKind) -> Self {
        Self {
            id,
            name: name.into(),
            kind,
            connected: false,
            keys: false,
            given: Vec::new(),
        }
    }
}

/// What every plugin may know about Bluetooth ([`Env::bluetooth`](crate::Env::bluetooth)).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct Bluetooth {
    pub state: RadioState,
    /// Something uses the radio (a scan, a link, advertising).
    pub busy: bool,
    /// The most links the device keeps at once.
    pub max_links: u8,
    pub bonded: Vec<Bonded>,
}

impl Bluetooth {
    pub fn new(max_links: u8) -> Self {
        Self {
            max_links,
            ..Self::default()
        }
    }

    /// Bonded device `id`, where it is one.
    pub fn bond(&self, id: DeviceId) -> Option<&Bonded> {
        self.bonded.iter().find(|b| b.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uuids_of_the_sig_and_in_full() {
        let hr = Uuid::sig(0x180D);
        assert_eq!(hr.to_string(), "0000180d-0000-1000-8000-00805f9b34fb");
        assert_eq!(hr.short(), Some(0x180D));
        assert_eq!(Uuid::parse("0x180D"), Some(hr));
        assert_eq!(Uuid::parse("180d"), Some(hr));
        assert_eq!(Uuid::parse(&hr.to_string()), Some(hr));
        let own = Uuid::parse("6e400001-b5a3-f393-e0a9-e50e24dcca9e").unwrap();
        assert_eq!(own.short(), None);
        assert_eq!(own.to_string(), "6e400001-b5a3-f393-e0a9-e50e24dcca9e");
        assert_eq!(Uuid::parse("6e400001b5a3f393e0a9e50e24dcca9e"), None);
        assert_eq!(Uuid::parse("nope"), None);
    }

    #[test]
    fn events_survive_being_written_out() {
        let device = DeviceId::new(7);
        let handle = Handle::new(0x2a);
        let hr = Uuid::sig(0x180D);
        let all = [
            BtEvent::Found(
                Device::new(device)
                    .name("Pulse")
                    .rssi(-60)
                    .service(hr)
                    .kind(DeviceKind::Sensor),
            ),
            BtEvent::ScanEnded,
            BtEvent::Connected { device },
            BtEvent::Disconnected {
                device,
                reason: Some(BtError::Lost),
            },
            BtEvent::Discovered {
                device,
                services: alloc::vec![Service::new(
                    hr,
                    alloc::vec![Characteristic::new(
                        Uuid::sig(0x2A37),
                        handle,
                        Properties::NONE.notify()
                    )]
                )],
            },
            BtEvent::Value {
                device,
                handle,
                bytes: alloc::vec![0, 72],
            },
            BtEvent::Written { device, handle },
            BtEvent::Paired { device },
            BtEvent::Failed {
                device: None,
                error: BtError::Att(0x05),
            },
            BtEvent::Advertising,
            BtEvent::AdvertisingEnded,
            BtEvent::ReadRequest {
                request: Request::new(3),
                handle,
            },
            BtEvent::WriteRequest {
                handle,
                bytes: alloc::vec![1, 2],
            },
            BtEvent::Subscribed { handle, on: true },
            BtEvent::HidReport {
                device,
                report: Some(1),
                bytes: alloc::vec![0, 0, 0x4f],
            },
        ];
        for event in all {
            let written = event.to_params();
            assert_eq!(
                BtEvent::from_params(&written),
                Some(event.clone()),
                "{written:?}"
            );
        }
    }

    #[test]
    fn scans_filter_by_service_and_name() {
        let found = Device::new(DeviceId::new(1))
            .name("Remote 2")
            .service(Uuid::sig(0x1812));
        let events: Event<BtEvent> = Event::new("a.found");
        assert!(scan(&events).passes(&found));
        assert!(scan(&events).service(Uuid::sig(0x1812)).passes(&found));
        assert!(!scan(&events).service(Uuid::sig(0x180D)).passes(&found));
        assert!(scan(&events).name_prefix("Remote").passes(&found));
        assert!(!scan(&events).name_prefix("Pulse").passes(&found));
        assert_eq!(scan(&events).for_ms(u32::MAX).ms, SCAN_MOST_MS);
    }

    #[test]
    fn properties_as_the_declaration_has_them() {
        let p = Properties::NONE.read().notify();
        assert_eq!(p.bits(), 0x12);
        assert_eq!(Properties::from_bits(0x12), p);
    }
}
