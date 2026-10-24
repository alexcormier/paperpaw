use super::*;
use alloc::vec;

const FOUND: Id = Id::new_static("a.found");

fn radio() -> Radio {
    let mut radio = Radio::new(2);
    radio.start();
    radio.take();
    radio
}

fn found(radio: &mut Radio, address: &str, services: Vec<Uuid>, out: &mut Vec<Command>) {
    radio.report(
        BtReport::Found {
            address: address.into(),
            name: Some(address.to_uppercase()),
            rssi: Some(-50),
            services,
            appearance: None,
        },
        Instant(10),
        out,
    );
}

fn scan(events: &Id) -> Scan {
    paperpaw_sdk::bluetooth::scan(&paperpaw_sdk::Event::new(
        events.as_str().to_string().leak(),
    ))
}

#[test]
fn a_bond_is_kept_and_read_again() {
    let known = Known {
        id: DeviceId::new(4),
        address: "aa:bb".into(),
        name: Some("Remote".into()),
        kind: DeviceKind::Remote,
        services: Vec::new(),
        rssi: None,
        found_by: Vec::new(),
        bond: Some("1234abcd".into()),
        given: vec![Id::new_static("pulse")],
        keys: true,
    };
    let read = Known::read(4, &known.written()).unwrap();
    assert_eq!(read.address, "aa:bb");
    assert_eq!(read.name.as_deref(), Some("Remote"));
    assert_eq!(read.kind, DeviceKind::Remote);
    assert!(read.keys);
    assert_eq!(read.given, vec![Id::new_static("pulse")]);
    assert_eq!(read.bond.as_deref(), Some("1234abcd"));
}

#[test]
fn kinds_come_from_the_appearance_or_the_services() {
    assert_eq!(kind_of(Some(0x03C1), &[]), DeviceKind::Keyboard);
    assert_eq!(kind_of(Some(0x03C0), &[]), DeviceKind::Remote);
    assert_eq!(kind_of(None, &[Uuid::sig(0x1812)]), DeviceKind::Remote);
    assert_eq!(kind_of(None, &[Uuid::sig(0x180D)]), DeviceKind::Sensor);
    assert_eq!(kind_of(None, &[]), DeviceKind::Other);
}

#[test]
fn the_radio_comes_on_for_a_scan_and_goes_off_a_while_after() {
    let mut radio = radio();
    let mut out = Vec::new();
    radio
        .command(
            "a",
            BtCommand::Scan(scan(&FOUND).for_ms(1_000)),
            1,
            false,
            Instant(0),
            &mut out,
        )
        .unwrap();
    assert_eq!(
        radio.take(),
        [BtCall::Radio { on: true }, BtCall::Scan { on: true }]
    );
    // The scan ends at its time; the radio stays on a little.
    radio.tick(Instant(1_000), &mut out);
    assert_eq!(radio.take(), [BtCall::Scan { on: false }]);
    assert_eq!(radio.deadline(), Some(Instant(1_000 + RADIO_GRACE_MS)));
    radio.tick(Instant(1_000 + RADIO_GRACE_MS), &mut out);
    assert_eq!(radio.take(), [BtCall::Radio { on: false }]);
    assert!(
        out.iter()
            .any(|c| matches!(c, Command::Emit(id, _) if *id == FOUND))
    );
}

#[test]
fn a_plugin_hears_only_what_its_filters_pass() {
    let mut radio = radio();
    let mut out = Vec::new();
    let hr = Id::new_static("pulse.found");
    radio
        .command(
            "a",
            BtCommand::Scan(scan(&FOUND)),
            1,
            false,
            Instant(0),
            &mut out,
        )
        .unwrap();
    radio
        .command(
            "pulse",
            BtCommand::Scan(scan(&hr).service(Uuid::sig(0x180D))),
            1,
            false,
            Instant(0),
            &mut out,
        )
        .unwrap();
    found(&mut radio, "remote", vec![Uuid::sig(0x1812)], &mut out);
    found(&mut radio, "belt", vec![Uuid::sig(0x180D)], &mut out);
    let told = |id: &Id| {
        out.iter()
            .filter(|c| matches!(c, Command::Emit(e, _) if e == id))
            .count()
    };
    assert_eq!(told(&FOUND), 2);
    assert_eq!(told(&hr), 1);
    // The belt is pulse's; the remote is not.
    let remote = radio.known[0].id;
    let refused = radio.command(
        "pulse",
        BtCommand::Connect {
            device: remote,
            events: hr.clone(),
        },
        1,
        false,
        Instant(20),
        &mut out,
    );
    assert_eq!(refused, Err((Some(remote), BtError::NotFound)));
}

#[test]
fn forgetting_and_giving_are_the_readers_own_doing() {
    let mut radio = radio();
    radio.load([("core.bluetooth.3", "aa\tRemote\tremote\t0\t\tkeys")].into_iter());
    let mut out = Vec::new();
    let device = DeviceId::new(3);
    let refused = radio.command(
        "a",
        BtCommand::Forget { device },
        1,
        false,
        Instant(0),
        &mut out,
    );
    assert_eq!(refused, Err((Some(device), BtError::NotAllowed)));
    radio
        .command(
            "a",
            BtCommand::Forget { device },
            1,
            true,
            Instant(0),
            &mut out,
        )
        .unwrap();
    assert!(radio.state().bonded.is_empty());
    let kept = radio.take_bonds(&[("core.bluetooth.3".into(), "x".into())]);
    assert_eq!(
        kept,
        [("core.bluetooth.3".into(), String::new(), String::new())]
    );
}

#[test]
fn a_remotes_reports_become_keys_and_are_let_go_when_it_drops() {
    let mut radio = radio();
    radio.load([("core.bluetooth.1", "aa\tRemote\tremote\t0\t\tkeys")].into_iter());
    let mut out = Vec::new();
    let device = DeviceId::new(1);
    radio
        .command(
            "a",
            BtCommand::Keys { device, on: true },
            1,
            false,
            Instant(0),
            &mut out,
        )
        .unwrap();
    let calls = radio.take();
    let link = calls
        .iter()
        .find_map(|c| match c {
            BtCall::Connect { link, .. } => Some(*link),
            _ => None,
        })
        .unwrap();
    radio.report(BtReport::Connected { link }, Instant(10), &mut out);
    assert!(radio.take().contains(&BtCall::Hid { link, on: true }));
    // A boot keyboard's report: right arrow down, then up.
    let report = |keys: [u8; 6]| BtReport::HidReport {
        link,
        report: None,
        bytes: [0, 0].into_iter().chain(keys).collect(),
    };
    radio.report(report([0x4f, 0, 0, 0, 0, 0]), Instant(20), &mut out);
    assert_eq!(radio.take_keys(), [("hid.right", true)]);
    radio.report(report([0x4f, 0, 0, 0, 0, 0]), Instant(30), &mut out);
    assert_eq!(radio.take_keys(), []);
    radio.report(
        BtReport::Disconnected { link, reason: None },
        Instant(40),
        &mut out,
    );
    assert_eq!(radio.take_keys(), [("hid.right", false)]);
}
