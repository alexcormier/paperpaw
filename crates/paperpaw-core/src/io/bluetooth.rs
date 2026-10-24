//! Bluetooth (see [ADR 0027](../../../docs/adr/0027-bluetooth.md)): the
//! radio the shell drives, shared by plugins. The kernel keeps each
//! plugin's scans, links and advertising, numbers links for the shell,
//! lets a plugin see only devices it found or was given, turns a bonded
//! remote's reports into keys, and switches the radio on for the first
//! request and off a little after the last.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_hid::{Decoder, ReportMap};
use paperpaw_platform::Instant;
use paperpaw_sdk::bluetooth::{
    Advertising, Bluetooth, Bonded, BtCommand, BtError, BtEvent, Device, DeviceId, DeviceKind,
    GattServiceSpec, Handle, Properties, RadioState, Request, Scan, Service, Uuid,
};
use paperpaw_sdk::event::EventData;
use paperpaw_sdk::{Command, Id};

/// How long the radio stays on after the last request, for the next one.
pub const RADIO_GRACE_MS: u64 = 3_000;
/// The secrets that keep the bonds: `core.bluetooth.<device number>`.
pub const BOND_PREFIX: &str = "core.bluetooth.";

/// What the shell is to do with the radio ([`Io::Bluetooth`](super::Io)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BtCall {
    /// The radio on or off; report `Radio`.
    Radio {
        on: bool,
    },
    /// The bonds kept (at start and whenever they change), for the
    /// shell's host to know the devices again.
    Bonds(Vec<Bond>),
    /// Scan for devices, or no longer; report each one as `Found`.
    Scan {
        on: bool,
    },
    Connect {
        link: u32,
        address: String,
    },
    Disconnect {
        link: u32,
    },
    /// Find the services and characteristics; report `Discovered`.
    Discover {
        link: u32,
    },
    Read {
        link: u32,
        handle: u16,
    },
    Write {
        link: u32,
        handle: u16,
        bytes: Vec<u8>,
        response: bool,
    },
    Subscribe {
        link: u32,
        handle: u16,
        on: bool,
    },
    /// Pair and bond; report `Ask` where the device asks something of the
    /// reader, then `Paired` or `Failed`.
    Pair {
        link: u32,
    },
    /// The reader's answer to the pairing's question.
    Answer {
        link: u32,
        accept: bool,
    },
    /// The device's HID reports: report `HidMap` once, then each
    /// `HidReport`.
    Hid {
        link: u32,
        on: bool,
    },
    /// The device's own GATT server: its services and their handles.
    Server(Vec<ServedService>),
    Advertise {
        name: Option<String>,
        services: Vec<Uuid>,
        on: bool,
    },
    Notify {
        handle: u16,
        bytes: Vec<u8>,
    },
    Respond {
        request: u32,
        bytes: Vec<u8>,
    },
    /// Forget the bond with the device at `address`.
    Forget {
        address: String,
    },
}

/// A bond: the device's address and the keys the shell's host keeps for
/// it, as the host wrote them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bond {
    pub address: String,
    pub keys: String,
}

/// A service of the device's own GATT server, its characteristics with
/// the handles the kernel gave them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServedService {
    pub uuid: Uuid,
    pub characteristics: Vec<(Uuid, Properties, u16)>,
}

/// What a pairing asks of the reader.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PairQuestion {
    /// Only whether to pair (Just Works).
    Confirm,
    /// Whether both show this number (numeric comparison).
    Compare(u32),
    /// The number to type on the other device (passkey display).
    Show(u32),
}

/// What the shell reports ([`Kernel::bluetooth_event`](crate::Kernel)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BtReport {
    Radio {
        on: bool,
    },
    Found {
        address: String,
        name: Option<String>,
        rssi: Option<i8>,
        services: Vec<Uuid>,
        /// Its GAP appearance, where it gives one.
        appearance: Option<u16>,
    },
    Connected {
        link: u32,
    },
    Disconnected {
        link: u32,
        reason: Option<BtError>,
    },
    Discovered {
        link: u32,
        services: Vec<Service>,
    },
    Value {
        link: u32,
        handle: u16,
        bytes: Vec<u8>,
    },
    Written {
        link: u32,
        handle: u16,
    },
    Failed {
        link: u32,
        error: BtError,
    },
    Ask {
        link: u32,
        question: PairQuestion,
    },
    Paired {
        link: u32,
        keys: String,
    },
    HidMap {
        link: u32,
        map: Vec<u8>,
    },
    HidReport {
        link: u32,
        report: Option<u8>,
        bytes: Vec<u8>,
    },
    ReadRequest {
        request: u32,
        handle: u16,
    },
    WriteRequest {
        handle: u16,
        bytes: Vec<u8>,
    },
    Subscribed {
        handle: u16,
        on: bool,
    },
    Advertising {
        on: bool,
    },
}

/// A device the kernel knows: found this start, or bonded.
#[derive(Debug, Clone)]
struct Known {
    id: DeviceId,
    address: String,
    name: Option<String>,
    kind: DeviceKind,
    services: Vec<Uuid>,
    rssi: Option<i8>,
    /// The plugins whose scans found it.
    found_by: Vec<String>,
    /// The bond's keys, where it is bonded.
    bond: Option<String>,
    given: Vec<Id>,
    /// Its keys go to the key maps.
    keys: bool,
}

impl Known {
    fn device(&self) -> Device {
        let mut device = Device::new(self.id).kind(self.kind);
        if let Some(name) = &self.name {
            device = device.name(name.clone());
        }
        if let Some(rssi) = self.rssi {
            device = device.rssi(rssi);
        }
        for service in &self.services {
            device = device.service(*service);
        }
        if self.bond.is_some() {
            device = device.bonded();
        }
        device
    }

    fn mine(&self, plugin: &str) -> bool {
        self.found_by.iter().any(|p| p == plugin) || self.given.iter().any(|g| g.as_str() == plugin)
    }

    /// The text kept as its bond's secret.
    fn written(&self) -> String {
        let given: Vec<&str> = self.given.iter().map(Id::as_str).collect();
        alloc::format!(
            "{}\t{}\t{}\t{}\t{}\t{}",
            self.address,
            self.name.as_deref().unwrap_or_default().replace('\t', " "),
            self.kind.as_str(),
            u8::from(self.keys),
            given.join(","),
            self.bond.as_deref().unwrap_or_default()
        )
    }

    fn read(number: u32, text: &str) -> Option<Known> {
        let mut parts = text.split('\t');
        let address = parts.next()?.to_string();
        let name = Some(parts.next()?.to_string()).filter(|n| !n.is_empty());
        let kind = DeviceKind::parse(parts.next()?);
        let keys = parts.next()? == "1";
        let given = parts
            .next()?
            .split(',')
            .filter_map(|g| Id::parse(g).ok())
            .collect();
        let bond = parts.next().map(String::from);
        Some(Known {
            id: DeviceId::new(number),
            address,
            name,
            kind,
            services: Vec::new(),
            rssi: None,
            found_by: Vec::new(),
            bond,
            given,
            keys,
        })
    }
}

/// Who asked for something, where it goes, and while how many screens.
#[derive(Debug, Clone)]
struct Asker {
    plugin: String,
    events: Id,
    screens: usize,
}

#[derive(Debug)]
struct ScanRequest {
    asker: Asker,
    scan: Scan,
    until: Instant,
}

#[derive(Debug)]
struct AdvertRequest {
    asker: Asker,
    advertising: Advertising,
    until: Instant,
}

#[derive(Debug)]
struct Link {
    number: u32,
    device: DeviceId,
    up: bool,
    users: Vec<Asker>,
    /// The kernel's own, for the device's keys.
    keys: bool,
    decoder: Option<Decoder>,
    /// Plugins hearing its HID reports as they are.
    raw: Vec<Asker>,
    /// Who waits for what: a read of a handle (`Some`), a discovery
    /// (`None`), a write's answer.
    reads: Vec<(Option<u16>, Id)>,
    writes: Vec<(u16, Id)>,
    /// Who subscribed to which handle.
    subscribed: Vec<(u16, Id)>,
    /// Pairing asked by this plugin's events.
    pairing: Option<Id>,
}

/// Bluetooth, as the kernel keeps it.
#[derive(Debug, Default)]
pub(crate) struct Radio {
    max_links: u8,
    known: Vec<Known>,
    scans: Vec<ScanRequest>,
    adverts: Vec<AdvertRequest>,
    links: Vec<Link>,
    /// The device's own GATT server: each characteristic's handle, the
    /// plugin's events.
    served: Vec<(u16, Id)>,
    services: Vec<ServedService>,
    /// Reads of the server waiting for their answer: request, events.
    requests: Vec<(u32, Id)>,
    radio_on: bool,
    scanning: bool,
    advertised: Option<(Option<String>, Vec<Uuid>)>,
    /// Since when nothing uses the radio.
    idle_since: Option<Instant>,
    next_device: u32,
    next_link: u32,
    calls: Vec<BtCall>,
    /// Keys of remotes to give the key maps: name, pressed.
    keys_out: Vec<(&'static str, bool)>,
    /// The bonds changed and are to be kept.
    bonds_changed: bool,
    /// A pairing asks the reader: device, question.
    asking: Option<(DeviceId, PairQuestion)>,
}

/// The command that tells `events` of `event`.
fn tell(events: &Id, event: BtEvent) -> Command {
    Command::Emit(events.clone(), event.into_params())
}

/// The device kind from a GAP appearance and the services.
fn kind_of(appearance: Option<u16>, services: &[Uuid]) -> DeviceKind {
    match appearance.map(|a| a >> 6) {
        Some(0x0F) => match appearance.map(|a| a & 0x3f) {
            Some(1) => DeviceKind::Keyboard,
            _ => DeviceKind::Remote,
        },
        Some(1 | 2) => DeviceKind::Phone,
        Some(0x0D | 0x0E | 0x31 | 0x03) => DeviceKind::Sensor,
        _ if services.contains(&Uuid::sig(0x1812)) => DeviceKind::Remote,
        _ if services.contains(&Uuid::sig(0x180D)) => DeviceKind::Sensor,
        _ => DeviceKind::Other,
    }
}

impl Radio {
    /// A radio keeping up to `max_links` links.
    pub fn new(max_links: u8) -> Self {
        Radio {
            max_links,
            next_device: 1,
            next_link: 1,
            ..Radio::default()
        }
    }

    /// The bonds kept in `secrets` (id, value), as read at start.
    pub fn load<'s>(&mut self, secrets: impl Iterator<Item = (&'s str, &'s str)>) {
        self.known.retain(|k| k.bond.is_none());
        for (id, value) in secrets {
            let Some(number) = id.strip_prefix(BOND_PREFIX).and_then(|n| n.parse().ok()) else {
                continue;
            };
            if let Some(known) = Known::read(number, value) {
                self.next_device = self.next_device.max(number + 1);
                self.known.push(known);
            }
        }
    }

    /// A service of the device's own GATT server, from a plugin's
    /// registration.
    pub fn add_service(&mut self, spec: &GattServiceSpec) {
        let first = self.served.last().map_or(1, |(h, _)| h + 1);
        let mut characteristics = Vec::new();
        for (handle, (uuid, properties)) in (first..).zip(&spec.characteristics) {
            characteristics.push((*uuid, *properties, handle));
            self.served.push((handle, spec.events.clone()));
        }
        self.services.push(ServedService {
            uuid: spec.uuid,
            characteristics,
        });
    }

    /// The bonds and the GATT server for the shell, at start.
    pub fn start(&mut self) {
        self.calls.push(BtCall::Bonds(self.bonds()));
        if !self.services.is_empty() {
            self.calls.push(BtCall::Server(self.services.clone()));
        }
    }

    fn bonds(&self) -> Vec<Bond> {
        self.known
            .iter()
            .filter_map(|k| {
                Some(Bond {
                    address: k.address.clone(),
                    keys: k.bond.clone()?,
                })
            })
            .collect()
    }

    pub fn take(&mut self) -> Vec<BtCall> {
        core::mem::take(&mut self.calls)
    }

    /// The keys of remotes that came, for the key maps.
    pub fn take_keys(&mut self) -> Vec<(&'static str, bool)> {
        core::mem::take(&mut self.keys_out)
    }

    /// The bonds as secrets to keep (id, label, value; an empty value to
    /// remove), where they changed.
    pub fn take_bonds(&mut self, all: &[(String, String)]) -> Vec<(String, String, String)> {
        if !core::mem::take(&mut self.bonds_changed) {
            return Vec::new();
        }
        let mut out: Vec<(String, String, String)> = self
            .known
            .iter()
            .filter(|k| k.bond.is_some())
            .map(|k| {
                let id = alloc::format!("{BOND_PREFIX}{}", k.id.number());
                let label = k.name.clone().unwrap_or_else(|| k.address.clone());
                (id, label, k.written())
            })
            .collect();
        for (id, _) in all {
            if id.starts_with(BOND_PREFIX) && !out.iter().any(|(o, _, _)| o == id) {
                out.push((id.clone(), String::new(), String::new()));
            }
        }
        out
    }

    /// The pairing question waiting for the reader, once.
    pub fn take_question(&mut self) -> Option<(DeviceId, PairQuestion, String)> {
        let (device, question) = self.asking.take()?;
        let name = self
            .known
            .iter()
            .find(|k| k.id == device)
            .and_then(|k| k.name.clone())
            .unwrap_or_default();
        Some((device, question, name))
    }

    /// What every plugin may know.
    pub fn state(&self) -> Bluetooth {
        let mut state = Bluetooth::new(self.max_links);
        state.state = if self.radio_on {
            RadioState::On
        } else {
            RadioState::Off
        };
        state.busy = self.wanted();
        state.bonded = self
            .known
            .iter()
            .filter(|k| k.bond.is_some())
            .map(|k| {
                let mut bonded = Bonded::new(
                    k.id,
                    k.name.clone().unwrap_or_else(|| k.address.clone()),
                    k.kind,
                );
                bonded.connected = self.links.iter().any(|l| l.device == k.id && l.up);
                bonded.keys = k.keys;
                bonded.given.clone_from(&k.given);
                bonded
            })
            .collect();
        state
    }

    /// Something needs the radio.
    fn wanted(&self) -> bool {
        !self.scans.is_empty() || !self.adverts.is_empty() || !self.links.is_empty()
    }

    /// When something it waits for is due: a scan's or advertising's end,
    /// the radio's going off.
    pub fn deadline(&self) -> Option<Instant> {
        let ends = self.scans.iter().map(|s| s.until);
        let adverts = self.adverts.iter().map(|a| a.until);
        let off = self
            .idle_since
            .filter(|_| self.radio_on)
            .map(|since| since.add_millis(RADIO_GRACE_MS));
        ends.chain(adverts).chain(off).min()
    }

    /// Time passed: scans and advertising ended, the radio off.
    pub fn tick(&mut self, now: Instant, out: &mut Vec<Command>) {
        let (ended, kept): (Vec<_>, Vec<_>) = self.scans.drain(..).partition(|s| s.until <= now);
        self.scans = kept;
        for scan in ended {
            out.push(tell(&scan.asker.events, BtEvent::ScanEnded));
        }
        let (ended, kept): (Vec<_>, Vec<_>) = self.adverts.drain(..).partition(|a| a.until <= now);
        self.adverts = kept;
        for advert in ended {
            out.push(tell(&advert.asker.events, BtEvent::AdvertisingEnded));
        }
        self.settle(now);
    }

    /// The radio, the scan and advertising as what is asked needs them.
    fn settle(&mut self, now: Instant) {
        let wanted = self.wanted();
        if wanted {
            self.idle_since = None;
            if !self.radio_on {
                self.radio_on = true;
                self.calls.push(BtCall::Radio { on: true });
            }
        } else if self.radio_on {
            let since = *self.idle_since.get_or_insert(now);
            if now.since(since) >= RADIO_GRACE_MS {
                self.radio_on = false;
                self.scanning = false;
                self.advertised = None;
                self.idle_since = None;
                self.calls.push(BtCall::Radio { on: false });
                return;
            }
        }
        let scan = !self.scans.is_empty();
        if scan != self.scanning && self.radio_on {
            self.scanning = scan;
            self.calls.push(BtCall::Scan { on: scan });
        }
        let advert = (!self.adverts.is_empty()).then(|| {
            let name = self
                .adverts
                .iter()
                .find_map(|a| a.advertising.name.as_deref().map(String::from));
            let mut services: Vec<Uuid> = Vec::new();
            for a in &self.adverts {
                for s in &a.advertising.services {
                    if !services.contains(s) {
                        services.push(*s);
                    }
                }
            }
            (name, services)
        });
        if advert != self.advertised && self.radio_on {
            self.advertised.clone_from(&advert);
            let (name, services, on) = match advert {
                Some((name, services)) => (name, services, true),
                None => (None, Vec::new(), false),
            };
            self.calls.push(BtCall::Advertise { name, services, on });
        }
    }

    fn known_mut(&mut self, device: DeviceId) -> Option<&mut Known> {
        self.known.iter_mut().find(|k| k.id == device)
    }

    /// The link to `device`, made where there is none (and a link is
    /// left).
    fn link_to(&mut self, device: DeviceId) -> Result<usize, BtError> {
        if let Some(at) = self.links.iter().position(|l| l.device == device) {
            return Ok(at);
        }
        if self.links.len() >= usize::from(self.max_links) {
            return Err(BtError::Busy);
        }
        let address = self
            .known
            .iter()
            .find(|k| k.id == device)
            .map(|k| k.address.clone())
            .ok_or(BtError::NotFound)?;
        let number = self.next_link;
        self.next_link += 1;
        self.links.push(Link {
            number,
            device,
            up: false,
            users: Vec::new(),
            keys: false,
            decoder: None,
            raw: Vec::new(),
            reads: Vec::new(),
            writes: Vec::new(),
            subscribed: Vec::new(),
            pairing: None,
        });
        self.calls.push(BtCall::Connect {
            link: number,
            address,
        });
        Ok(self.links.len() - 1)
    }

    /// The plugin's link to `device`: its number, where it is up and the
    /// plugin one of its users.
    fn plugin_link(&self, plugin: &str, device: DeviceId) -> Result<usize, BtError> {
        let at = self
            .links
            .iter()
            .position(|l| l.device == device && l.users.iter().any(|u| u.plugin == plugin))
            .ok_or(BtError::NotFound)?;
        if self.links[at].up {
            Ok(at)
        } else {
            Err(BtError::NotFound)
        }
    }

    /// Plugin `plugin`'s command (allowed to use Bluetooth: the kernel
    /// checked), with `screens` open and `reader` its own doing.
    #[expect(
        clippy::too_many_lines,
        reason = "one arm a command, each short; split, it reads worse"
    )]
    pub fn command(
        &mut self,
        plugin: &str,
        command: BtCommand,
        screens: usize,
        reader: bool,
        now: Instant,
        out: &mut Vec<Command>,
    ) -> Result<(), (Option<DeviceId>, BtError)> {
        let mine = |radio: &Radio, device: DeviceId| {
            radio.known.iter().any(|k| k.id == device && k.mine(plugin))
        };
        match command {
            BtCommand::Scan(scan) => {
                let asker = Asker {
                    plugin: plugin.to_string(),
                    events: scan.events.clone(),
                    screens,
                };
                self.scans.retain(|s| s.asker.events != scan.events);
                let until = now.add_millis(u64::from(scan.ms));
                // What was found before, for this scan too.
                for known in &mut self.known {
                    if known.rssi.is_some() && scan.passes(&known.device()) {
                        if !known.found_by.iter().any(|p| p == plugin) {
                            known.found_by.push(plugin.to_string());
                        }
                        out.push(tell(&asker.events, BtEvent::Found(known.device())));
                    }
                }
                self.scans.push(ScanRequest { asker, scan, until });
            }
            BtCommand::StopScan { events } => {
                let before = self.scans.len();
                self.scans
                    .retain(|s| !(s.asker.events == events && s.asker.plugin == plugin));
                if self.scans.len() < before {
                    out.push(tell(&events, BtEvent::ScanEnded));
                }
            }
            BtCommand::Connect { device, events } => {
                if !mine(self, device) {
                    return Err((Some(device), BtError::NotFound));
                }
                let at = self.link_to(device).map_err(|e| (Some(device), e))?;
                let link = &mut self.links[at];
                link.users.retain(|u| u.plugin != plugin);
                link.users.push(Asker {
                    plugin: plugin.to_string(),
                    events: events.clone(),
                    screens,
                });
                if link.up {
                    out.push(tell(&events, BtEvent::Connected { device }));
                }
            }
            BtCommand::Disconnect { device } => self.leave(plugin, device, out),
            BtCommand::Discover { device } => {
                let at = self
                    .plugin_link(plugin, device)
                    .map_err(|e| (Some(device), e))?;
                let link = &mut self.links[at];
                let events = link
                    .users
                    .iter()
                    .find(|u| u.plugin == plugin)
                    .map(|u| u.events.clone());
                if let Some(events) = events {
                    link.reads.push((None, events));
                }
                self.calls.push(BtCall::Discover { link: link.number });
            }
            BtCommand::Read { device, handle } => {
                let at = self
                    .plugin_link(plugin, device)
                    .map_err(|e| (Some(device), e))?;
                let link = &mut self.links[at];
                if let Some(u) = link.users.iter().find(|u| u.plugin == plugin) {
                    link.reads.push((Some(handle.get()), u.events.clone()));
                }
                self.calls.push(BtCall::Read {
                    link: link.number,
                    handle: handle.get(),
                });
            }
            BtCommand::Write {
                device,
                handle,
                bytes,
                kind,
            } => {
                let at = self
                    .plugin_link(plugin, device)
                    .map_err(|e| (Some(device), e))?;
                let link = &mut self.links[at];
                let response = kind == paperpaw_sdk::bluetooth::WriteKind::WithResponse;
                if response && let Some(u) = link.users.iter().find(|u| u.plugin == plugin) {
                    link.writes.push((handle.get(), u.events.clone()));
                }
                self.calls.push(BtCall::Write {
                    link: link.number,
                    handle: handle.get(),
                    bytes,
                    response,
                });
            }
            BtCommand::Subscribe { device, handle } => {
                let at = self
                    .plugin_link(plugin, device)
                    .map_err(|e| (Some(device), e))?;
                let link = &mut self.links[at];
                let events = link
                    .users
                    .iter()
                    .find(|u| u.plugin == plugin)
                    .map(|u| u.events.clone());
                if let Some(events) = events {
                    let first = !link.subscribed.iter().any(|(h, _)| *h == handle.get());
                    link.subscribed
                        .retain(|(h, e)| !(*h == handle.get() && *e == events));
                    link.subscribed.push((handle.get(), events));
                    if first {
                        self.calls.push(BtCall::Subscribe {
                            link: link.number,
                            handle: handle.get(),
                            on: true,
                        });
                    }
                }
            }
            BtCommand::Unsubscribe { device, handle } => {
                let at = self
                    .plugin_link(plugin, device)
                    .map_err(|e| (Some(device), e))?;
                let link = &mut self.links[at];
                let users = &link.users;
                link.subscribed.retain(|(h, e)| {
                    !(*h == handle.get()
                        && users.iter().any(|u| u.plugin == plugin && u.events == *e))
                });
                if !link.subscribed.iter().any(|(h, _)| *h == handle.get()) {
                    self.calls.push(BtCall::Subscribe {
                        link: link.number,
                        handle: handle.get(),
                        on: false,
                    });
                }
            }
            other => return self.command_more(plugin, other, screens, reader, now, out),
        }
        self.settle(now);
        Ok(())
    }

    /// [`Radio::command`] for pairing, bonds, keys and being seen.
    fn command_more(
        &mut self,
        plugin: &str,
        command: BtCommand,
        screens: usize,
        reader: bool,
        now: Instant,
        out: &mut Vec<Command>,
    ) -> Result<(), (Option<DeviceId>, BtError)> {
        match command {
            BtCommand::Pair { device } => {
                let ours = self.known.iter().any(|k| k.id == device && k.mine(plugin));
                if !ours {
                    return Err((Some(device), BtError::NotFound));
                }
                let at = self
                    .plugin_link(plugin, device)
                    .map_err(|e| (Some(device), e))?;
                let link = &mut self.links[at];
                link.pairing = link
                    .users
                    .iter()
                    .find(|u| u.plugin == plugin)
                    .map(|u| u.events.clone());
                self.calls.push(BtCall::Pair { link: link.number });
            }
            BtCommand::Forget { device } | BtCommand::Give { device, .. } if !reader => {
                return Err((Some(device), BtError::NotAllowed));
            }
            BtCommand::Forget { device } => {
                let Some(at) = self
                    .known
                    .iter()
                    .position(|k| k.id == device && k.bond.is_some())
                else {
                    return Err((Some(device), BtError::NotFound));
                };
                let address = self.known[at].address.clone();
                self.known.remove(at);
                if let Some(link) = self.links.iter().position(|l| l.device == device) {
                    let link = self.links.remove(link);
                    self.calls.push(BtCall::Disconnect { link: link.number });
                }
                self.calls.push(BtCall::Forget { address });
                self.calls.push(BtCall::Bonds(self.bonds()));
                self.bonds_changed = true;
            }
            BtCommand::Give {
                device,
                plugin: to,
                given,
            } => {
                let known = self
                    .known_mut(device)
                    .filter(|k| k.bond.is_some())
                    .ok_or((Some(device), BtError::NotFound))?;
                known.given.retain(|g| *g != to);
                if given {
                    known.given.push(to);
                }
                self.bonds_changed = true;
            }
            BtCommand::Keys { device, on } => {
                self.keys(device, on).map_err(|e| (Some(device), e))?
            }
            BtCommand::HidReports { device, events } => {
                let ours = self.known.iter().any(|k| k.id == device && k.mine(plugin));
                if !ours {
                    return Err((Some(device), BtError::NotFound));
                }
                let at = self.link_to(device).map_err(|e| (Some(device), e))?;
                let link = &mut self.links[at];
                link.raw.retain(|r| r.plugin != plugin);
                link.raw.push(Asker {
                    plugin: plugin.to_string(),
                    events,
                    screens,
                });
                if link.up {
                    self.calls.push(BtCall::Hid {
                        link: link.number,
                        on: true,
                    });
                }
            }
            other => return self.command_served(plugin, other, screens, now, out),
        }
        self.settle(now);
        Ok(())
    }

    /// [`Radio::command`] for being seen: advertising, the GATT server.
    fn command_served(
        &mut self,
        plugin: &str,
        command: BtCommand,
        screens: usize,
        now: Instant,
        out: &mut Vec<Command>,
    ) -> Result<(), (Option<DeviceId>, BtError)> {
        match command {
            BtCommand::Advertise(advertising) => {
                let asker = Asker {
                    plugin: plugin.to_string(),
                    events: advertising.events.clone(),
                    screens,
                };
                self.adverts.retain(|a| a.asker.plugin != plugin);
                let until = now.add_millis(u64::from(advertising.ms));
                out.push(tell(&asker.events, BtEvent::Advertising));
                self.adverts.push(AdvertRequest {
                    asker,
                    advertising,
                    until,
                });
            }
            BtCommand::StopAdvertising => {
                let (ended, kept): (Vec<_>, Vec<_>) = self
                    .adverts
                    .drain(..)
                    .partition(|a| a.asker.plugin == plugin);
                self.adverts = kept;
                for advert in ended {
                    out.push(tell(&advert.asker.events, BtEvent::AdvertisingEnded));
                }
            }
            BtCommand::Notify { handle, bytes } => {
                let own = self
                    .served
                    .iter()
                    .any(|(h, e)| *h == handle.get() && e.namespace() == plugin);
                if !own {
                    return Err((None, BtError::NotFound));
                }
                self.calls.push(BtCall::Notify {
                    handle: handle.get(),
                    bytes,
                });
            }
            BtCommand::Respond { request, bytes } => {
                let at = self
                    .requests
                    .iter()
                    .position(|(r, e)| *r == request.number() && e.namespace() == plugin)
                    .ok_or((None, BtError::NotFound))?;
                self.requests.remove(at);
                self.calls.push(BtCall::Respond {
                    request: request.number(),
                    bytes,
                });
            }
            _ => return Err((None, BtError::Unsupported)),
        }
        self.settle(now);
        Ok(())
    }

    /// The keys of bonded `device` go to the key maps (`on`), or no
    /// longer.
    fn keys(&mut self, device: DeviceId, on: bool) -> Result<(), BtError> {
        let known = self
            .known_mut(device)
            .filter(|k| k.bond.is_some() && k.kind.types())
            .ok_or(BtError::NotFound)?;
        if known.keys != on {
            known.keys = on;
            self.bonds_changed = true;
        }
        if on {
            let at = self.link_to(device)?;
            let link = &mut self.links[at];
            if !link.keys {
                link.keys = true;
                if link.up {
                    self.calls.push(BtCall::Hid {
                        link: link.number,
                        on: true,
                    });
                }
            }
        } else if let Some(at) = self.links.iter().position(|l| l.device == device) {
            self.links[at].keys = false;
            self.release_keys(at);
            self.drop_unused(at);
        }
        Ok(())
    }

    /// The keys still down on link `at`, released.
    fn release_keys(&mut self, at: usize) {
        if let Some(decoder) = &mut self.links[at].decoder {
            for change in decoder.reset() {
                self.keys_out.push((change.key.name(), false));
            }
        }
    }

    /// Link `at` let go where nothing uses it any more.
    fn drop_unused(&mut self, at: usize) {
        let link = &self.links[at];
        if link.users.is_empty() && link.raw.is_empty() && !link.keys {
            self.calls.push(BtCall::Disconnect { link: link.number });
            self.links.remove(at);
        }
    }

    /// Plugin `plugin` lets go of its link to `device`.
    fn leave(&mut self, plugin: &str, device: DeviceId, out: &mut Vec<Command>) {
        let Some(at) = self.links.iter().position(|l| l.device == device) else {
            return;
        };
        let link = &mut self.links[at];
        let (gone, kept): (Vec<_>, Vec<_>) = link.users.drain(..).partition(|u| u.plugin == plugin);
        link.users = kept;
        link.raw.retain(|r| r.plugin != plugin);
        for user in gone {
            out.push(tell(
                &user.events,
                BtEvent::Disconnected {
                    device,
                    reason: None,
                },
            ));
        }
        self.drop_unused(at);
    }

    /// Fewer screens are open than `screens`: what was asked while more
    /// were ends.
    pub fn close_for_closed_screens(
        &mut self,
        screens: usize,
        now: Instant,
        out: &mut Vec<Command>,
    ) {
        let (ended, kept): (Vec<_>, Vec<_>) = self
            .scans
            .drain(..)
            .partition(|s| s.asker.screens > screens);
        self.scans = kept;
        for scan in ended {
            out.push(tell(&scan.asker.events, BtEvent::ScanEnded));
        }
        let (ended, kept): (Vec<_>, Vec<_>) = self
            .adverts
            .drain(..)
            .partition(|a| a.asker.screens > screens);
        self.adverts = kept;
        for advert in ended {
            out.push(tell(&advert.asker.events, BtEvent::AdvertisingEnded));
        }
        let mut at = 0;
        while at < self.links.len() {
            let device = self.links[at].device;
            let link = &mut self.links[at];
            let (gone, kept): (Vec<_>, Vec<_>) =
                link.users.drain(..).partition(|u| u.screens > screens);
            link.users = kept;
            link.raw.retain(|r| r.screens <= screens);
            for user in gone {
                out.push(tell(
                    &user.events,
                    BtEvent::Disconnected {
                        device,
                        reason: None,
                    },
                ));
            }
            let before = self.links.len();
            self.drop_unused(at);
            if self.links.len() == before {
                at += 1;
            }
        }
        self.settle(now);
    }

    /// The device goes to sleep: everything ends, the radio off.
    pub fn suspend(&mut self, out: &mut Vec<Command>) {
        for scan in self.scans.drain(..) {
            out.push(tell(&scan.asker.events, BtEvent::ScanEnded));
        }
        for advert in self.adverts.drain(..) {
            out.push(tell(&advert.asker.events, BtEvent::AdvertisingEnded));
        }
        for at in (0..self.links.len()).rev() {
            self.release_keys(at);
            let link = self.links.remove(at);
            for user in link.users {
                out.push(tell(
                    &user.events,
                    BtEvent::Disconnected {
                        device: link.device,
                        reason: None,
                    },
                ));
            }
        }
        if self.radio_on {
            self.calls.push(BtCall::Radio { on: false });
        }
        self.radio_on = false;
        self.scanning = false;
        self.advertised = None;
        self.idle_since = None;
    }

    /// The reader answered the pairing question about `device`.
    pub fn answer(&mut self, device: DeviceId, accept: bool) {
        if let Some(link) = self.links.iter().find(|l| l.device == device) {
            self.calls.push(BtCall::Answer {
                link: link.number,
                accept,
            });
        }
    }

    fn link_at(&self, number: u32) -> Option<usize> {
        self.links.iter().position(|l| l.number == number)
    }

    /// What the shell reported.
    pub fn report(&mut self, report: BtReport, now: Instant, out: &mut Vec<Command>) {
        match report {
            BtReport::Radio { on } => {
                if !on && self.radio_on {
                    // Off by itself: what used it fails.
                    self.suspend(out);
                }
            }
            BtReport::Found {
                address,
                name,
                rssi,
                services,
                appearance,
            } => self.found(address, name, rssi, services, appearance, out),
            BtReport::Connected { link } => self.connected(link, out),
            BtReport::Disconnected { link, reason } => self.disconnected(link, reason, out),
            BtReport::Discovered { link, services } => {
                let Some(at) = self.link_at(link) else { return };
                let device = self.links[at].device;
                let waiting: Vec<Id> = take_where(&mut self.links[at].reads, |(h, _)| h.is_none())
                    .into_iter()
                    .map(|(_, e)| e)
                    .collect();
                for events in waiting {
                    let event = BtEvent::Discovered {
                        device,
                        services: services.clone(),
                    };
                    out.push(tell(&events, event));
                }
            }
            BtReport::Value {
                link,
                handle,
                bytes,
            } => self.value(link, handle, bytes, out),
            BtReport::Written { link, handle } => {
                let Some(at) = self.link_at(link) else { return };
                let device = self.links[at].device;
                let writes = &mut self.links[at].writes;
                if let Some(i) = writes.iter().position(|(h, _)| *h == handle) {
                    let (_, events) = writes.remove(i);
                    let handle = Handle::new(handle);
                    out.push(tell(&events, BtEvent::Written { device, handle }));
                }
            }
            BtReport::Failed { link, error } => self.failed(link, error, out),
            other => self.report_more(other, now, out),
        }
        self.settle(now);
    }

    /// [`Radio::report`] for pairing, HID and the GATT server.
    fn report_more(&mut self, report: BtReport, now: Instant, out: &mut Vec<Command>) {
        match report {
            BtReport::Ask { link, question } => {
                if let Some(at) = self.link_at(link) {
                    self.asking = Some((self.links[at].device, question));
                }
            }
            BtReport::Paired { link, keys } => {
                let Some(at) = self.link_at(link) else { return };
                let device = self.links[at].device;
                let events = self.links[at].pairing.take();
                if let Some(known) = self.known_mut(device) {
                    known.bond = Some(keys);
                    if let Some(events) = &events
                        && let Ok(plugin) = Id::parse(events.namespace())
                        && !known.given.contains(&plugin)
                    {
                        known.given.push(plugin);
                    }
                }
                self.bonds_changed = true;
                self.calls.push(BtCall::Bonds(self.bonds()));
                if let Some(events) = events {
                    out.push(tell(&events, BtEvent::Paired { device }));
                }
            }
            BtReport::HidMap { link, map } => {
                let Some(at) = self.link_at(link) else { return };
                let decoder =
                    ReportMap::parse(&map).map_or_else(|_| Decoder::boot_keyboard(), Decoder::new);
                self.links[at].decoder = Some(decoder);
            }
            BtReport::HidReport {
                link,
                report,
                bytes,
            } => {
                let Some(at) = self.link_at(link) else { return };
                let device = self.links[at].device;
                for user in &self.links[at].raw {
                    let event = BtEvent::HidReport {
                        device,
                        report,
                        bytes: bytes.clone(),
                    };
                    out.push(tell(&user.events, event));
                }
                if self.links[at].keys {
                    let decoder = self.links[at]
                        .decoder
                        .get_or_insert_with(Decoder::boot_keyboard);
                    for change in decoder.report(report, &bytes) {
                        self.keys_out.push((change.key.name(), change.pressed));
                    }
                }
            }
            BtReport::ReadRequest { request, handle } => {
                if let Some((_, events)) = self.served.iter().find(|(h, _)| *h == handle) {
                    self.requests.push((request, events.clone()));
                    let event = BtEvent::ReadRequest {
                        request: Request::new(request),
                        handle: Handle::new(handle),
                    };
                    out.push(tell(events, event));
                }
            }
            BtReport::WriteRequest { handle, bytes } => {
                if let Some((_, events)) = self.served.iter().find(|(h, _)| *h == handle) {
                    let handle = Handle::new(handle);
                    out.push(tell(events, BtEvent::WriteRequest { handle, bytes }));
                }
            }
            BtReport::Subscribed { handle, on } => {
                if let Some((_, events)) = self.served.iter().find(|(h, _)| *h == handle) {
                    let handle = Handle::new(handle);
                    out.push(tell(events, BtEvent::Subscribed { handle, on }));
                }
            }
            BtReport::Advertising { on: false } => {
                for advert in self.adverts.drain(..) {
                    out.push(tell(&advert.asker.events, BtEvent::AdvertisingEnded));
                }
                self.advertised = None;
            }
            _ => {}
        }
        let _ = now;
    }

    fn found(
        &mut self,
        address: String,
        name: Option<String>,
        rssi: Option<i8>,
        services: Vec<Uuid>,
        appearance: Option<u16>,
        out: &mut Vec<Command>,
    ) {
        let at = match self.known.iter().position(|k| k.address == address) {
            Some(at) => at,
            None => {
                let id = DeviceId::new(self.next_device);
                self.next_device += 1;
                self.known.push(Known {
                    id,
                    address,
                    name: None,
                    kind: DeviceKind::Other,
                    services: Vec::new(),
                    rssi: None,
                    found_by: Vec::new(),
                    bond: None,
                    given: Vec::new(),
                    keys: false,
                });
                self.known.len() - 1
            }
        };
        let known = &mut self.known[at];
        let new = known.rssi.is_none()
            || name.is_some() && known.name != name
            || services.iter().any(|s| !known.services.contains(s));
        if name.is_some() {
            known.name = name;
        }
        for service in services {
            if !known.services.contains(&service) {
                known.services.push(service);
            }
        }
        known.rssi = rssi.or(known.rssi).or(Some(-100));
        if known.kind == DeviceKind::Other || appearance.is_some() {
            known.kind = kind_of(appearance, &known.services);
        }
        if !new {
            return;
        }
        let device = known.device();
        for scan in &self.scans {
            if scan.scan.passes(&device) {
                if !known.found_by.contains(&scan.asker.plugin) {
                    known.found_by.push(scan.asker.plugin.clone());
                }
                out.push(tell(&scan.asker.events, BtEvent::Found(device.clone())));
            }
        }
    }

    fn connected(&mut self, link: u32, out: &mut Vec<Command>) {
        let Some(at) = self.link_at(link) else { return };
        let link = &mut self.links[at];
        link.up = true;
        let device = link.device;
        for user in &link.users {
            out.push(tell(&user.events, BtEvent::Connected { device }));
        }
        if link.keys || !link.raw.is_empty() {
            let number = link.number;
            self.calls.push(BtCall::Hid {
                link: number,
                on: true,
            });
        }
    }

    fn disconnected(&mut self, link: u32, reason: Option<BtError>, out: &mut Vec<Command>) {
        let Some(at) = self.link_at(link) else { return };
        self.release_keys(at);
        let link = self.links.remove(at);
        let reason = reason.or(Some(BtError::Lost));
        let pairing = link.pairing.iter().cloned();
        for events in link.users.iter().map(|u| u.events.clone()).chain(pairing) {
            out.push(tell(
                &events,
                BtEvent::Disconnected {
                    device: link.device,
                    reason,
                },
            ));
        }
    }

    fn value(&mut self, link: u32, handle: u16, bytes: Vec<u8>, out: &mut Vec<Command>) {
        let Some(at) = self.link_at(link) else { return };
        let device = self.links[at].device;
        let link = &mut self.links[at];
        // A read's answer to who read, else a notification to who
        // subscribed.
        let to: Vec<Id> = match link.reads.iter().position(|(h, _)| *h == Some(handle)) {
            Some(i) => alloc::vec![link.reads.remove(i).1],
            None => link
                .subscribed
                .iter()
                .filter(|(h, _)| *h == handle)
                .map(|(_, e)| e.clone())
                .collect(),
        };
        for events in to {
            let event = BtEvent::Value {
                device,
                handle: Handle::new(handle),
                bytes: bytes.clone(),
            };
            out.push(tell(&events, event));
        }
    }

    fn failed(&mut self, link: u32, error: BtError, out: &mut Vec<Command>) {
        let Some(at) = self.link_at(link) else { return };
        let device = self.links[at].device;
        let l = &mut self.links[at];
        let mut told: Vec<Id> = l.users.iter().map(|u| u.events.clone()).collect();
        told.extend(l.pairing.take());
        l.reads.clear();
        l.writes.clear();
        told.dedup();
        for events in told {
            let event = BtEvent::Failed {
                device: Some(device),
                error,
            };
            out.push(tell(&events, event));
        }
        if !self.links[at].up {
            // Never connected: the link is gone.
            self.links.remove(at);
        }
    }
}

/// The items of `list` where `keep` holds, taken out.
fn take_where<T>(list: &mut Vec<T>, keep: impl Fn(&T) -> bool) -> Vec<T> {
    let (taken, kept): (Vec<T>, Vec<T>) = list.drain(..).partition(|item| keep(item));
    *list = kept;
    taken
}

#[cfg(test)]
mod tests;
