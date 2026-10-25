//! What a [`BtEvent`] says: its parts, how it is written out for
//! observers and read back, and the events the kernel makes.

use super::*;

impl BtEvent {
    /// Its kind, as written out.
    pub const fn kind(&self) -> &'static str {
        match self {
            BtEvent::Found(_) => "found",
            BtEvent::ScanEnded => "scan-ended",
            BtEvent::Connected { .. } => "connected",
            BtEvent::Disconnected { .. } => "disconnected",
            BtEvent::Discovered { .. } => "discovered",
            BtEvent::ServicesChanged { .. } => "services-changed",
            BtEvent::Value { .. } => "value",
            BtEvent::Written { .. } => "written",
            BtEvent::Sent { .. } => "sent",
            BtEvent::Paired { .. } => "paired",
            BtEvent::Done { .. } => "done",
            BtEvent::Failed(_) => "failed",
            BtEvent::Advertising => "advertising",
            BtEvent::AdvertisingEnded => "advertising-ended",
            BtEvent::ReadRequest { .. } => "read-request",
            BtEvent::WriteRequest { .. } => "write-request",
            BtEvent::Subscribed { .. } => "subscribed",
            BtEvent::Unsubscribed { .. } => "unsubscribed",
            BtEvent::HidReportMap { .. } => "hid-report-map",
            BtEvent::HidReport { .. } => "hid-report",
            BtEvent::Extended { .. } => "extended",
        }
    }

    /// The device it is about, where it is about one.
    pub const fn device(&self) -> Option<DeviceId> {
        match self {
            BtEvent::Found(device) => Some(device.id),
            BtEvent::Connected { device, .. }
            | BtEvent::Disconnected { device, .. }
            | BtEvent::Discovered { device, .. }
            | BtEvent::ServicesChanged { device }
            | BtEvent::Value { device, .. }
            | BtEvent::Written { device, .. }
            | BtEvent::Sent { device, .. }
            | BtEvent::Paired { device }
            | BtEvent::Done { device, .. }
            | BtEvent::HidReportMap { device, .. }
            | BtEvent::HidReport { device, .. } => Some(*device),
            BtEvent::ReadRequest { central, .. }
            | BtEvent::WriteRequest { central, .. }
            | BtEvent::Subscribed { central, .. }
            | BtEvent::Unsubscribed { central, .. } => Some(*central),
            BtEvent::Failed(failure) => failure.device,
            _ => None,
        }
    }

    /// The service and characteristic of the plugin's GATT server it is
    /// about, where it is about one.
    const fn served(&self) -> Option<(Uuid, Uuid)> {
        match self {
            BtEvent::ReadRequest {
                service,
                characteristic,
                ..
            }
            | BtEvent::WriteRequest {
                service,
                characteristic,
                ..
            }
            | BtEvent::Subscribed {
                service,
                characteristic,
                ..
            }
            | BtEvent::Unsubscribed {
                service,
                characteristic,
                ..
            } => Some((*service, *characteristic)),
            _ => None,
        }
    }
}

/// Makers of the events, for the kernel and a plugin's own tests.
#[cfg(any(feature = "kernel", feature = "testing", test))]
impl BtEvent {
    pub fn connected(device: DeviceId, mtu: u16, security: Security) -> Self {
        BtEvent::Connected {
            device,
            mtu,
            security,
        }
    }

    pub fn disconnected(device: DeviceId, reason: Option<BtError>) -> Self {
        BtEvent::Disconnected { device, reason }
    }

    pub fn discovered(device: DeviceId, services: Vec<GattService>) -> Self {
        BtEvent::Discovered { device, services }
    }

    pub fn services_changed(device: DeviceId) -> Self {
        BtEvent::ServicesChanged { device }
    }

    pub fn value(device: DeviceId, handle: Handle, kind: ValueKind, bytes: Vec<u8>) -> Self {
        BtEvent::Value {
            device,
            handle,
            kind,
            bytes,
        }
    }

    pub fn written(device: DeviceId, handle: Handle) -> Self {
        BtEvent::Written { device, handle }
    }

    pub fn sent(device: DeviceId, handle: Handle) -> Self {
        BtEvent::Sent { device, handle }
    }

    pub fn paired(device: DeviceId) -> Self {
        BtEvent::Paired { device }
    }

    pub fn done(op: BtOp, device: DeviceId) -> Self {
        BtEvent::Done { op, device }
    }

    pub fn read_request(
        request: GattRequest,
        service: Uuid,
        characteristic: Uuid,
        central: DeviceId,
    ) -> Self {
        BtEvent::ReadRequest {
            request,
            service,
            characteristic,
            central,
        }
    }

    pub fn write_request(
        request: Option<GattRequest>,
        service: Uuid,
        characteristic: Uuid,
        central: DeviceId,
        bytes: Vec<u8>,
    ) -> Self {
        BtEvent::WriteRequest {
            request,
            service,
            characteristic,
            central,
            bytes,
        }
    }

    pub fn subscribed(service: Uuid, characteristic: Uuid, central: DeviceId) -> Self {
        BtEvent::Subscribed {
            service,
            characteristic,
            central,
        }
    }

    pub fn unsubscribed(service: Uuid, characteristic: Uuid, central: DeviceId) -> Self {
        BtEvent::Unsubscribed {
            service,
            characteristic,
            central,
        }
    }

    pub fn hid_report_map(device: DeviceId, bytes: Vec<u8>) -> Self {
        BtEvent::HidReportMap { device, bytes }
    }

    pub fn hid_report(device: DeviceId, report: Option<u8>, bytes: Vec<u8>) -> Self {
        BtEvent::HidReport {
            device,
            report,
            bytes,
        }
    }

    pub fn extended(key: Id, params: Params) -> Self {
        BtEvent::Extended { key, params }
    }
}

impl EventData for BtEvent {
    /// What happened, for observers and rules: its kind, the device, the
    /// command and the error, the length of a payload but never the
    /// payload (keys typed, readings, what a phone sent).
    fn to_params(&self) -> Params {
        use bt_params as p;
        let mut params = Params::new().with(p::KIND, self.kind());
        if let Some(device) = self.device() {
            params.set(p::DEVICE, device.to_param());
        }
        match self {
            BtEvent::Value { handle, bytes, .. } => {
                params.set(p::HANDLE, handle.0.to_param());
                params.set(p::LENGTH, bytes.len().to_param());
            }
            BtEvent::HidReport { bytes, .. }
            | BtEvent::HidReportMap { bytes, .. }
            | BtEvent::WriteRequest { bytes, .. } => {
                params.set(p::LENGTH, bytes.len().to_param());
            }
            BtEvent::Written { handle, .. } | BtEvent::Sent { handle, .. } => {
                params.set(p::HANDLE, handle.0.to_param());
            }
            BtEvent::Done { op, .. } => params.set(p::OP, op.as_str()),
            BtEvent::Failed(failure) => {
                params.set(p::OP, failure.op.as_str());
                params.set(p::ERROR, failure.error.to_param());
                if let Some(handle) = failure.handle {
                    params.set(p::HANDLE, handle.0.to_param());
                }
            }
            BtEvent::Disconnected {
                reason: Some(reason),
                ..
            } => params.set(p::ERROR, reason.to_param()),
            _ => {}
        }
        if let Some((service, characteristic)) = self.served() {
            params.set(p::SERVICE, service.to_string());
            params.set(p::CHARACTERISTIC, characteristic.to_string());
        }
        params
    }

    /// The event itself goes along, payloads and all, for the plugin it
    /// is for.
    fn into_params(self) -> Params {
        self.to_params().carrying(self)
    }

    /// What can be read back from the written-out parameters: events
    /// without a payload.
    fn from_params(params: &Params) -> Option<Self> {
        use bt_params as p;
        let device = || params.get(p::DEVICE).and_then(DeviceId::from_param);
        let handle = || params.get(p::HANDLE).and_then(u16::from_param).map(Handle);
        Some(match params.get(p::KIND)? {
            "scan-ended" => BtEvent::ScanEnded,
            "disconnected" => BtEvent::Disconnected {
                device: device()?,
                reason: params.get(p::ERROR).and_then(BtError::from_param),
            },
            "services-changed" => BtEvent::ServicesChanged { device: device()? },
            "written" => BtEvent::Written {
                device: device()?,
                handle: handle()?,
            },
            "sent" => BtEvent::Sent {
                device: device()?,
                handle: handle()?,
            },
            "paired" => BtEvent::Paired { device: device()? },
            "done" => BtEvent::Done {
                op: BtOp::parse(params.get(p::OP)?)?,
                device: device()?,
            },
            "failed" => BtEvent::Failed(BtFailure {
                op: BtOp::parse(params.get(p::OP)?)?,
                device: device(),
                handle: handle(),
                error: params.get(p::ERROR).and_then(BtError::from_param)?,
            }),
            "advertising" => BtEvent::Advertising,
            "advertising-ended" => BtEvent::AdvertisingEnded,
            _ => return None,
        })
    }
}
