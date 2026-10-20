//! Updates over the air: the flash has two app partitions (see
//! `partitions.csv`). An update is written to the one not running and
//! chosen for the next start; the bootloader's choice is the OTA data
//! partition's.
//!
//! The bootloader espflash brings does not undo updates itself, so the
//! firmware does: a new firmware's first start marks it as on trial
//! (`PendingVerify`), before anything else could fail, and the main loop
//! marks it as good (`Valid`) once it ran half a minute or went to sleep
//! or restarted by itself. Found on trial at a start (the last start
//! ended before, e.g. in a panic or a hang the watchdog ended), it is
//! marked bad and the device restarts into the firmware before it, which
//! says so ([`Ota::undone`]) and takes its place back.
//!
//! Only update files signed with a key built in here (PaperPaw's) or one
//! the reader trusts (`paperpaw_sdk::firmware`) are installed; the
//! signature is checked while the image is written, and the new firmware
//! chosen only if it holds. Its first word is written only then: until
//! it is, the partition holds no image the bootloader would start, even
//! if it tried that partition. An update installed and not started yet
//! is given up before another is written, which would otherwise go into
//! the partition chosen to start.

use alloc::boxed::Box;
use alloc::string::String;

use esp_bootloader_esp_idf::ota::OtaImageState;
use esp_bootloader_esp_idf::ota_updater::OtaUpdater;
use esp_bootloader_esp_idf::partitions::{
    self, AppPartitionSubType, DataPartitionSubType, FlashStorage, PARTITION_TABLE_MAX_LEN,
    PartitionType,
};
use log::{info, warn};
use paperpaw_platform::Storage;
use paperpaw_sdk::firmware::{
    Check, Firmware, FirmwareState, InstallFailure, Plugins, Signer, Update, trusted_keys,
};
use paperpaw_x4::ota_data;

// The plugin crates built in (`PLUGINS`), written by `build.rs`.
include!(concat!(env!("OUT_DIR"), "/plugins.rs"));

/// PaperPaw's keys: updates signed with them install without asking.
const KEYS: &[[u8; 32]] = &[
    // The release key, fingerprint 55A9 F626 DB17 EB90.
    [
        0x55, 0xa9, 0xf6, 0x26, 0xdb, 0x17, 0xeb, 0x90, 0x58, 0x70, 0x73, 0x23, 0xd2, 0x9f, 0x52,
        0x47, 0x0b, 0xfc, 0xd0, 0x96, 0x38, 0x89, 0x8e, 0x17, 0xbd, 0x01, 0x78, 0x4e, 0xf9, 0xdf,
        0x0c, 0x51,
    ],
];
/// The chip id in this device's images: the X4's ESP32-C3, the ESP32-S3
/// of the X4 Pro and the X4 Classic.
#[cfg(feature = "x4")]
const CHIP: u16 = paperpaw_sdk::firmware::CHIP_ESP32_C3;
#[cfg(feature = "esp32s3")]
const CHIP: u16 = paperpaw_sdk::firmware::CHIP_ESP32_S3;
/// Which firmware this is (the image says it, `main.rs`): updates must be
/// the same. The X4 Pro and the X4 Classic share their chip, not their
/// boards.
#[cfg(feature = "x4")]
pub const PROJECT: &str = "paperpaw-xteink";
#[cfg(feature = "x4-pro")]
pub const PROJECT: &str = "paperpaw-xteink-pro";
#[cfg(feature = "x4-classic")]
pub const PROJECT: &str = "paperpaw-xteink-classic";
/// The flash's erase block: erased a block at a time, before it is
/// written.
const BLOCK: u32 = 64 * 1024;
/// Read from the card and written a piece at a time.
const PIECE: usize = 4096;
/// Written per [`Ota::step`]: the main loop goes on in between.
const STEP: u32 = BLOCK;
/// The image's first bytes, a flash word: its magic byte, which makes it
/// an image to the bootloader, is in them. Written last, once the
/// signature held.
const HEAD: usize = 4;

/// An install going on.
struct Install {
    path: String,
    /// The image's length.
    total: u32,
    /// Bytes written.
    done: u32,
    /// Where in the flash the image goes.
    offset: u32,
    check: Check,
    buf: Box<[u8; PIECE]>,
    /// The image's first word, kept back until the signature held.
    head: [u8; HEAD],
    /// The last twentieth of the image said to be written.
    said: u32,
}

/// The flash, one handle for updates and native plugins (`native`): a
/// second one would find its size anew, and not always.
pub type Flash = alloc::rc::Rc<core::cell::RefCell<FlashStorage<'static>>>;

pub struct Ota {
    flash: Flash,
    table: Box<[u8; PARTITION_TABLE_MAX_LEN]>,
    /// The running firmware is on trial: started for the first time.
    trial: bool,
    /// An update did not start cleanly, and this firmware came back.
    undone: bool,
    /// The app partition running.
    running: Option<AppPartitionSubType>,
    /// The partition an update goes to, and its size.
    room: u32,
    install: Option<Install>,
}

impl Ota {
    /// Looks at what started and why (see the [module documentation](self));
    /// `None` without two app partitions (flashed before they came: the
    /// web flasher sets them up once).
    pub fn start(shared: Flash) -> Option<Self> {
        let mut flash = shared.borrow_mut();
        let mut table = Box::new([0u8; PARTITION_TABLE_MAX_LEN]);
        let booted = partitions::read_partition_table(&mut flash, &mut table[..])
            .and_then(|pt| pt.booted_partition())
            .ok()
            .flatten()
            .and_then(|p| match p.partition_type() {
                PartitionType::App(running) => Some(running),
                _ => None,
            });
        let mut updater = match OtaUpdater::new(&mut flash, &mut table) {
            Ok(updater) => updater,
            Err(e) => {
                warn!("OTA: no partitions for updates ({e:?})");
                return None;
            }
        };
        let selected = updater.selected_partition().ok();
        let state = updater.current_ota_state().ok();
        info!("OTA: started {booted:?}, chosen {selected:?} ({state:?})");
        let mut trial = false;
        let mut undone = false;
        match (booted, selected) {
            // The one chosen did not start (marked bad here at the last
            // start, or not startable): this one takes its place back.
            (Some(running), Some(chosen)) if chosen_other(chosen, running) => {
                warn!("OTA: the update did not start, {running:?} is back");
                undone = true;
                if let Err(e) = choose(&mut updater, running) {
                    warn!("OTA: {e:?}");
                }
            }
            _ => match state {
                // Undefined: chosen, and the power cut before its state
                // was written (see `paperpaw_x4::ota_data`).
                Some(OtaImageState::New | OtaImageState::Undefined) => {
                    info!("OTA: first start of an update, on trial");
                    trial = updater
                        .set_current_ota_state(OtaImageState::PendingVerify)
                        .is_ok();
                }
                Some(OtaImageState::PendingVerify) => {
                    // Started before and never ran long enough.
                    warn!("OTA: the update failed on trial: back to the one before");
                    let _ = updater.set_current_ota_state(OtaImageState::Invalid);
                    esp_hal::system::software_reset();
                }
                _ => {}
            },
        }
        let room = match updater.next_partition() {
            Ok((region, _)) => region.partition_size() as u32,
            Err(_) => 0,
        };
        drop(flash);
        Some(Self {
            flash: shared,
            table,
            trial,
            undone,
            running: booted,
            room,
            install: None,
        })
    }

    /// Whether the running firmware is on trial (an update's first start).
    pub fn on_trial(&self) -> bool {
        self.trial
    }

    /// The running firmware works: no longer on trial.
    pub fn confirm(&mut self) {
        if !self.trial {
            return;
        }
        self.trial = false;
        let confirmed = OtaUpdater::new(&mut self.flash.borrow_mut(), &mut self.table)
            .and_then(|mut updater| updater.set_current_ota_state(OtaImageState::Valid));
        match confirmed {
            Ok(()) => info!("OTA: the update started cleanly"),
            Err(e) => warn!("OTA: {e:?}"),
        }
    }

    /// The firmware as plugins see it.
    pub fn firmware(&self) -> Firmware {
        Firmware {
            version: String::from(env!("PAPERPAW_VERSION")),
            project: String::from(PROJECT),
            chip: CHIP,
            keys: KEYS.to_vec(),
            room: u64::from(self.room),
            undone: self.undone,
            state: FirmwareState::Idle,
            plugins: Plugins::read_static(&PLUGINS).unwrap_or_default(),
        }
    }

    /// Whether an install goes on (the device neither sleeps nor waits
    /// meanwhile).
    pub fn installing(&self) -> bool {
        self.install.is_some()
    }

    /// Starts installing the update file at `path`: what it holds is
    /// checked first, the signature as it is written.
    pub fn begin(&mut self, storage: &dyn Storage, path: &str) -> FirmwareState {
        match self.prepare(storage, path) {
            Ok(install) => {
                info!(
                    "OTA: installing {path} ({} bytes) at {:#x}",
                    install.total, install.offset
                );
                let total = u64::from(install.total);
                self.install = Some(install);
                FirmwareState::Installing { done: 0, total }
            }
            Err(why) => {
                warn!("OTA: {path}: {why:?}");
                FirmwareState::Failed(why)
            }
        }
    }

    fn prepare(&mut self, storage: &dyn Storage, path: &str) -> Result<Install, InstallFailure> {
        let update = Update::read(storage, path)?;
        let firmware = self.firmware();
        update.fits(&firmware)?;
        if update.signer(&firmware, &trusted_keys(storage)) == Signer::Unknown {
            return Err(InstallFailure::Untrusted);
        }
        let check = Check::new(&update.trailer)?;
        let mut flash = self.flash.borrow_mut();
        let mut updater =
            OtaUpdater::new(&mut flash, &mut self.table).map_err(|_| InstallFailure::Write)?;
        // An update installed and not started yet: given up, or the next
        // partition would be the one it chose.
        if let Some(running) = self.running
            && let Ok(chosen) = updater.selected_partition()
            && chosen_other(chosen, running)
        {
            info!("OTA: the update installed before is given up");
            choose(&mut updater, running).map_err(|_| InstallFailure::Write)?;
        }
        let slot = updater
            .next_partition()
            .map(|(_, slot)| slot)
            .map_err(|_| InstallFailure::Write)?;
        if Some(slot) == self.running {
            return Err(InstallFailure::Write);
        }
        let offset = partitions::read_partition_table(&mut flash, &mut self.table[..])
            .and_then(|pt| pt.find_partition(PartitionType::App(slot)))
            .ok()
            .flatten()
            .map(|p| p.offset())
            .ok_or(InstallFailure::Write)?;
        Ok(Install {
            path: String::from(path),
            total: update.trailer.image_len,
            done: 0,
            offset,
            check,
            buf: Box::new([0; PIECE]),
            head: [0xFF; HEAD],
            said: 0,
        })
    }

    /// Writes the next part; the new state when the install went on (every
    /// 5 %: the page shows each with a refresh), ended or failed.
    pub fn step(&mut self, storage: &dyn Storage) -> Option<FirmwareState> {
        let mut install = self.install.take()?;
        match self.write(storage, &mut install) {
            Err(why) => {
                warn!("OTA: {}: {why:?} at {}", install.path, install.done);
                Some(FirmwareState::Failed(why))
            }
            Ok(()) if install.done < install.total => {
                let twentieth = (u64::from(install.done) * 20 / u64::from(install.total)) as u32;
                let state = (twentieth != install.said).then_some(FirmwareState::Installing {
                    done: u64::from(install.done),
                    total: u64::from(install.total),
                });
                install.said = twentieth;
                self.install = Some(install);
                state
            }
            Ok(()) => Some(self.finish(install)),
        }
    }

    fn write(
        &mut self,
        storage: &dyn Storage,
        install: &mut Install,
    ) -> Result<(), InstallFailure> {
        let end = (install.done + STEP).min(install.total);
        while install.done < end {
            let at = install.offset + install.done;
            if install.done.is_multiple_of(BLOCK) {
                self.flash
                    .borrow_mut()
                    .erase(at, at + BLOCK)
                    .map_err(|_| InstallFailure::Write)?;
            }
            let len = (end - install.done).min(PIECE as u32) as usize;
            let piece = &mut install.buf[..len];
            match storage.read_at(&install.path, u64::from(install.done), piece) {
                Ok(n) if n == len => {}
                _ => return Err(InstallFailure::Unreadable),
            }
            install.check.absorb(piece);
            // The first word kept back, the flash left erased there.
            if install.done == 0 {
                let head = HEAD.min(len);
                install.head[..head].copy_from_slice(&install.buf[..head]);
                install.buf[..head].fill(0xFF);
            }
            // Written in words: the last piece filled up as erased flash.
            let words = len.div_ceil(4) * 4;
            install.buf[len..words].fill(0xFF);
            self.flash
                .borrow_mut()
                .write_nor(at, &install.buf[..words])
                .map_err(|_| InstallFailure::Write)?;
            install.done += len as u32;
        }
        Ok(())
    }

    /// All written: the new firmware is made whole and chosen if its
    /// signature holds.
    fn finish(&mut self, mut install: Install) -> FirmwareState {
        if let Err(e) = install.check.finish() {
            warn!("OTA: {}: {e}", install.path);
            return FirmwareState::Failed(e.into());
        }
        // From the buffer: the flash writes words from memory aligned so.
        install.buf[..HEAD].copy_from_slice(&install.head);
        let mut flash = self.flash.borrow_mut();
        if let Err(e) = flash.write_nor(install.offset, &install.buf[..HEAD]) {
            warn!("OTA: {e:?}");
            return FirmwareState::Failed(InstallFailure::Write);
        }
        let chosen = mark_next_new(&mut flash, &mut self.table).and_then(|()| {
            let mut updater = OtaUpdater::new(&mut flash, &mut self.table)?;
            updater.activate_next_partition()?;
            updater.set_current_ota_state(OtaImageState::New)
        });
        match chosen {
            Ok(()) => {
                info!("OTA: {} installed, started at the next start", install.path);
                FirmwareState::Installed
            }
            Err(e) => {
                warn!("OTA: {e:?}");
                FirmwareState::Failed(InstallFailure::Write)
            }
        }
    }
}

/// Whether the OTA data chose another app partition than the one running
/// (not the factory one, which it names when it never chose).
fn chosen_other(chosen: AppPartitionSubType, running: AppPartitionSubType) -> bool {
    chosen != running && chosen != AppPartitionSubType::Factory
}

/// Chooses `slot`, the one running, to start, as good.
fn choose(
    updater: &mut OtaUpdater<'_, '_>,
    slot: AppPartitionSubType,
) -> Result<(), partitions::Error> {
    updater.ota_data()?.set_current_app_partition(slot)?;
    updater.set_current_ota_state(OtaImageState::Valid)
}

/// Marks the entry of the OTA data that choosing the next partition
/// writes as holding a new image, before it is chosen (see
/// `paperpaw_x4::ota_data`).
fn mark_next_new(
    flash: &mut FlashStorage<'static>,
    table: &mut [u8; PARTITION_TABLE_MAX_LEN],
) -> Result<(), partitions::Error> {
    let entry = partitions::read_partition_table(flash, &mut table[..])?
        .find_partition(PartitionType::Data(DataPartitionSubType::Ota))?
        .ok_or(partitions::Error::Invalid)?;
    let mut region = entry.as_flash_region(flash);
    let mut entries = [[0; ota_data::ENTRY_LEN]; 2];
    for (entry, at) in entries.iter_mut().zip(ota_data::ENTRIES) {
        region.read(at, entry)?;
    }
    if let Some(next) = ota_data::next_entry(&entries) {
        ota_data::mark_new(&mut entries[next]);
        region.write(ota_data::ENTRIES[next], &entries[next])?;
    }
    Ok(())
}
