//! What shells tell the kernel and ask of it besides input: the battery
//! and the wall clock (also readings that make no sense), sleeping and
//! waking, passwords and keys kept on the storage, slots opened and
//! events sent from outside.

#![allow(clippy::unwrap_used, clippy::expect_used)]
// Tests panic on purpose when something is wrong.

mod common;

use std::cell::Cell;
use std::rc::Rc;

use common::{BASE, DEVICE, Harness, LATER};
use paperpaw_core::Kernel;
use paperpaw_core::sdk::builtin::{events, slots};
use paperpaw_core::sdk::secrets::{FILE as SECRETS, SecretChange};
use paperpaw_core::sdk::{
    Battery, DirEntry, Manifest, MemoryStorage, Params, Plugin, RegisterError, Registrar, Storage,
    StorageError, SystemStatus, WallClock,
};
use paperpaw_platform::Instant;
use proptest::prelude::*;
use proptest::test_runner::Config;

/// Logs the kernel's events about the device.
struct Listener {
    harness: Harness,
}

impl Plugin for Listener {
    fn manifest(&self) -> Manifest {
        common::manifest("listener")
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        for event in [
            events::STATUS_CHANGED.id().clone(),
            events::BATTERY_MEASURED.id().clone(),
            events::SUSPEND.id().clone(),
            events::RESUME.id().clone(),
            events::STOPPING.id().clone(),
        ] {
            let log = self.harness.log.clone();
            r.subscribe(
                event.clone(),
                Box::new(move |_, _, _| log.borrow_mut().push(format!("heard {event}"))),
            )?;
        }
        Ok(())
    }
}

/// A storage that counts how often it is told to write back what it kept
/// back.
#[derive(Clone, Default)]
struct Synced {
    files: Rc<MemoryStorage>,
    syncs: Rc<Cell<u32>>,
}

impl Storage for Synced {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.files.read_at(path, offset, buf)
    }

    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.files.size(path)
    }

    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.files.write(path, data)
    }

    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.files.append(path, data)
    }

    fn sync(&self) -> Result<(), StorageError> {
        self.syncs.set(self.syncs.get() + 1);
        Ok(())
    }

    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.files.list(dir, visit)
    }

    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.files.remove(path)
    }

    fn create_dir(&self, path: &str) -> Result<(), StorageError> {
        self.files.create_dir(path)
    }
}

fn kernel(harness: &Harness, storage: impl Storage + 'static) -> Kernel {
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(harness.plugin())
        .plugin(Listener {
            harness: harness.clone(),
        })
        .storage(storage)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    common::render(&mut kernel);
    harness.take();
    kernel
}

fn heard(harness: &Harness) -> Vec<String> {
    let heard = harness.lines("heard");
    harness.take();
    heard
}

fn battery(percent: u8, millivolts: u16) -> Option<Battery> {
    Some(Battery {
        percent,
        charging: false,
        millivolts: Some(millivolts),
    })
}

fn clock(at: u64, unix_millis: i64, offset: Option<i32>) -> Option<WallClock> {
    Some(WallClock {
        at: Instant(at),
        unix_millis,
        utc_offset_minutes: offset,
    })
}

#[test]
fn status_lines_are_told_only_of_what_they_show() {
    let harness = Harness::new();
    let mut kernel = kernel(&harness, MemoryStorage::new());
    let status = |battery, clock| SystemStatus { clock, battery };
    let day = 1_760_000_000_000;
    kernel.set_status(
        Instant(10),
        status(battery(80, 4000), clock(10, day, Some(60))),
    );
    assert_eq!(
        heard(&harness),
        ["heard core.status-changed", "heard core.battery-measured"]
    );
    // The voltage moved, the clock was corrected by a few seconds.
    kernel.set_status(
        Instant(20),
        status(battery(80, 3990), clock(20, day + 5_010, Some(60))),
    );
    assert_eq!(heard(&harness), ["heard core.battery-measured"]);
    // The percent, the time zone, a jump of the clock, the clock gone.
    for (at, next) in [
        (
            30,
            status(battery(79, 3990), clock(20, day + 5_010, Some(60))),
        ),
        (
            40,
            status(battery(79, 3990), clock(20, day + 5_010, Some(120))),
        ),
        (
            50,
            status(battery(79, 3990), clock(20, day + 3_600_000, Some(120))),
        ),
        (60, status(battery(79, 3990), None)),
    ] {
        kernel.set_status(Instant(at), next);
        assert_eq!(
            heard(&harness),
            ["heard core.status-changed", "heard core.battery-measured"],
            "at {at}"
        );
    }
    // Nothing measured, nothing changed: nothing heard.
    kernel.set_status(Instant(70), status(None, None));
    kernel.set_status(Instant(80), status(None, None));
    assert_eq!(heard(&harness), ["heard core.status-changed"]);
}

#[test]
fn wall_clock_readings_that_make_no_sense_do_not_panic() {
    let harness = Harness::new();
    let mut kernel = kernel(&harness, MemoryStorage::new());
    for (now, reading) in [
        (10, clock(0, i64::MAX, None)),
        (20, clock(0, i64::MIN, None)),
        (u64::MAX, clock(0, 0, Some(i32::MAX))),
        (30, clock(u64::MAX, i64::MAX, Some(i32::MIN))),
    ] {
        let status = SystemStatus {
            clock: reading,
            battery: None,
        };
        kernel.set_status(Instant(now), status);
    }
    assert_eq!(kernel.active_screen(), Some(&BASE));
}

#[test]
fn battery_readings_out_of_range_do_not_panic() {
    let harness = Harness::new();
    let mut kernel = kernel(&harness, MemoryStorage::new());
    for (at, percent, millivolts) in [(10, 255, u16::MAX), (20, 0, 0), (30, 101, 1)] {
        kernel.set_status(
            Instant(at),
            SystemStatus {
                clock: None,
                battery: battery(percent, millivolts),
            },
        );
        common::render(&mut kernel);
    }
    assert_eq!(kernel.active_screen(), Some(&BASE));
}

#[test]
fn sleeping_and_waking_are_heard_and_counted() {
    let harness = Harness::new();
    let mut kernel = kernel(&harness, MemoryStorage::new());
    for i in 0..3 {
        kernel.suspend(Instant(1000 + i * 100));
        kernel.resume(Instant(1050 + i * 100));
    }
    // Woken twice without sleeping, slept twice without waking.
    kernel.resume(Instant(2000));
    kernel.suspend(Instant(2100));
    kernel.suspend(Instant(2200));
    let heard = heard(&harness);
    assert_eq!(heard.iter().filter(|l| l.ends_with("suspend")).count(), 5);
    assert_eq!(heard.iter().filter(|l| l.ends_with("resume")).count(), 4);
    assert_eq!(kernel.usage().sleeps, 5);
    assert_eq!(kernel.active_screen(), Some(&BASE));
}

#[test]
fn stopping_is_heard_and_written_back_but_no_sleep() {
    let harness = Harness::new();
    let storage = Synced::default();
    let mut kernel = kernel(&harness, storage.clone());
    let synced = storage.syncs.get();
    kernel.stop(Instant(1000));
    assert_eq!(heard(&harness), ["heard core.stopping"]);
    assert_eq!(kernel.usage().sleeps, 0);
    assert_eq!(storage.syncs.get(), synced + 1);
}

#[test]
fn a_slot_opened_by_the_shell_for_something_else_replaces_its_screen() {
    let harness = Harness::new();
    let mut kernel = kernel(&harness, MemoryStorage::new());
    let book = |path: &str| Params::new().with("path", String::from(path));
    kernel.open_slot(Instant(10), slots::HOME, Params::new());
    assert!(harness.take().is_empty(), "the same: nothing to do");
    kernel.open_slot(Instant(20), slots::HOME, book("a.epub"));
    assert_eq!(harness.take(), ["base:close", "base:show"]);
    kernel.open_slot(Instant(30), slots::HOME, book("a.epub"));
    assert!(harness.take().is_empty(), "the same book again");
    assert_eq!(kernel.screens().count(), 1);
    // A slot nobody fills: a warning, nothing else.
    kernel.open_slot(Instant(40), slots::READER, book("b.epub"));
    assert_eq!(
        kernel.active_screen().map(|s| s.as_str()),
        Some("core.placeholder")
    );
    let warnings = kernel.take_warnings();
    assert!(warnings.is_empty(), "{warnings:?}");
}

#[test]
fn events_sent_by_the_shell_reach_plugins_once() {
    let harness = Harness::new();
    let mut kernel = kernel(&harness, MemoryStorage::new());
    kernel.send_event(Instant(10), LATER, Params::new().with("k", "v"));
    assert_eq!(harness.lines("event"), ["event drv.later at 10 k=v"]);
}

#[test]
fn secrets_kept_are_read_at_the_next_start_and_removed_files_go() {
    let harness = Harness::new();
    let mut first = kernel(&harness, MemoryStorage::new());
    first.set_secret(SecretChange::keep("drv.key", "Key", "s3cret\tvalue\n"));
    first.set_secret(SecretChange::keep("drv.other", "Other", ""));
    let saved = paperpaw_core::sdk::read_file(first.storage(), SECRETS, 4096).unwrap();
    assert!(
        !String::from_utf8_lossy(&saved).contains("s3cret"),
        "kept out of sight"
    );
    let second = kernel(&harness, MemoryStorage::new().with(SECRETS, saved));
    assert_eq!(second.secrets().get("drv.key"), Some("s3cret\tvalue\n"));
    assert_eq!(second.secrets().get("drv.other"), Some(""));
    // All removed: the file goes.
    first.set_secret(SecretChange::remove("drv.key"));
    first.set_secret(SecretChange::remove("drv.other"));
    first.set_secret(SecretChange::remove("drv.never-kept"));
    assert!(!first.storage().exists(SECRETS));
}

#[test]
fn a_damaged_secrets_file_keeps_what_can_be_read() {
    let harness = Harness::new();
    let mut good = kernel(&harness, MemoryStorage::new());
    good.set_secret(SecretChange::keep("drv.key", "Key", "value"));
    let line = paperpaw_core::sdk::read_file(good.storage(), SECRETS, 4096).unwrap();
    let mut damaged = b"no tabs at all\n\t\t\t\nid\tlabel\tc\tnot hex\nid\tlabel\tc\tff\n".to_vec();
    damaged.extend_from_slice(&[0xff, 0xfe, b'\n']);
    damaged.extend_from_slice(&line);
    let kernel = kernel(&harness, MemoryStorage::new().with(SECRETS, damaged));
    assert_eq!(kernel.secrets().get("drv.key"), Some("value"));
    assert_eq!(kernel.active_screen(), Some(&BASE));
}

proptest! {
    #![proptest_config(Config { failure_persistence: None, ..Config::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(96)) })]

    /// Any value kept as a secret is read back the same.
    #[test]
    fn any_secret_is_read_back_the_same(value in any::<String>(), label in "[^\t\r\n]{0,20}") {
        let harness = Harness::new();
        let mut first = kernel(&harness, MemoryStorage::new());
        first.set_secret(SecretChange::keep("drv.key", &label, &value));
        let saved = paperpaw_core::sdk::read_file(first.storage(), SECRETS, 1 << 20).unwrap();
        let second = kernel(&harness, MemoryStorage::new().with(SECRETS, saved));
        prop_assert_eq!(second.secrets().get("drv.key"), Some(value.as_str()));
    }
}
