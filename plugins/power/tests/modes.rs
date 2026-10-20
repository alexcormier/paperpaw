//! The power mode with the real kernel: chosen by hand, or by the battery
//! in Automatic (with a margin, so it does not go to and fro, and Normal
//! while charging); a mode's values worn over the reader's own and taken
//! off again, nothing written.

// Tests panic on purpose when something is wrong.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use paperpaw_core::Kernel;
use paperpaw_platform::{Battery, Rotation, SystemStatus};
use paperpaw_plugin_power::{MODE, Power};
use paperpaw_sdk::builtin::settings;
use paperpaw_sdk::power::{self, PowerLevel};
use paperpaw_sdk::{
    DeviceInfo, DisplayInfo, Id, InputCaps, Instant, MemoryClass, PixelFormat, SettingValue,
};

const DEVICE: DeviceInfo = DeviceInfo {
    id: "test",
    name: "Test device",
    display: DisplayInfo {
        width: 480,
        height: 800,
        dpi: 220,
        format: PixelFormat::Gray4,
        rotation: Rotation::Deg0,
        color: None,
    },
    input: InputCaps {
        buttons: true,
        touch: true,
        pen: false,
        keys: &[],
    },
    memory: MemoryClass::Large,
};

struct Device {
    kernel: Kernel,
    now: u64,
}

impl Device {
    fn new() -> Self {
        let mut kernel = Kernel::builder(DEVICE).plugin(Power).build().unwrap();
        kernel.start(Instant(0));
        Self { kernel, now: 0 }
    }

    fn battery(&mut self, percent: u8, charging: bool) {
        self.now += 60_000;
        self.kernel.set_status(
            Instant(self.now),
            SystemStatus {
                clock: None,
                battery: Some(Battery {
                    percent,
                    charging,
                    millivolts: None,
                }),
            },
        );
    }

    fn mode(&self) -> (Id, PowerLevel, bool) {
        let power = self.kernel.power();
        (power.mode.clone(), power.level, power.automatic)
    }

    fn sleep_after(&self) -> i64 {
        self.kernel.settings().int(settings::SLEEP_AFTER.as_str())
    }
}

#[test]
fn automatic_goes_by_the_battery_and_back_with_a_margin() {
    let mut device = Device::new();
    device.battery(80, false);
    assert_eq!(device.mode(), (power::NORMAL, PowerLevel::Normal, true));
    device.battery(19, false);
    assert_eq!(device.mode(), (power::SAVING, PowerLevel::Saving, true));
    // Just above the level: still saving, not to and fro.
    device.battery(22, false);
    assert_eq!(device.mode().1, PowerLevel::Saving);
    device.battery(9, false);
    assert_eq!(device.mode(), (power::OFFLINE, PowerLevel::Offline, true));
    device.battery(12, false);
    assert_eq!(device.mode().1, PowerLevel::Offline);
    // Charging: Normal.
    device.battery(12, true);
    assert_eq!(device.mode().1, PowerLevel::Normal);
    device.battery(30, false);
    assert_eq!(device.mode().1, PowerLevel::Normal);
}

#[test]
fn a_mode_wears_its_values_and_takes_them_off_again() {
    let mut device = Device::new();
    device.battery(80, false);
    let own = device.sleep_after();
    assert_ne!(own, 5);
    device.battery(15, false);
    assert_eq!(device.sleep_after(), 5, "Saving's value");
    device.battery(60, false);
    assert_eq!(device.sleep_after(), own, "the reader's own again");
    assert!(
        !device
            .kernel
            .settings()
            .is_set(settings::SLEEP_AFTER.as_str())
    );
}

#[test]
fn a_mode_chosen_by_hand_stays_whatever_the_battery() {
    let mut device = Device::new();
    device.battery(80, false);
    device.now += 10;
    device
        .kernel
        .set_setting(Instant(device.now), MODE.as_str(), "power.mode.offline")
        .unwrap();
    assert_eq!(device.mode(), (power::OFFLINE, PowerLevel::Offline, false));
    device.battery(90, true);
    assert_eq!(device.mode().1, PowerLevel::Offline);
    assert_eq!(
        device.kernel.settings().get(MODE.as_str()),
        Some(&SettingValue::Text("power.mode.offline".into()))
    );
}
