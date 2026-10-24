//! Updates over the air: the flash has two app partitions (see
//! `partitions.csv`). An update is written to the one not running and
//! chosen for the next start; the bootloader's choice is the OTA data
//! partition's (`paperpaw_x4::ota`, where what can be is tested).
//!
//! The bootloader espflash brings does not undo updates itself, so the
//! firmware does: a new firmware's first start marks it as on trial
//! (`PendingVerify`), before anything else could fail, and it is marked as
//! good (`Valid`) once it ran [`TRIAL_MS`] in all. Going to sleep or
//! restarting before marks it new again with the time it ran, so the next
//! start (on the X4 also a wake from sleep) goes on with its trial. Found
//! on trial at a start (the last run ended otherwise, e.g. in a panic or a
//! hang the watchdog ended), it is marked bad and the device restarts into
//! the firmware before it, which says so ([`Ota::undone`]) and takes its
//! place back. These marks go to the running partition's entry, also when
//! an update was chosen meanwhile.
//!
//! Only update files signed with a key built in here (PaperPaw's) or one
//! the reader accepted for the install, and newer ones unless the reader
//! chose an older one, are installed (`paperpaw_sdk::firmware`); the
//! signature is checked while the image is written, and the new firmware
//! chosen only if it holds. Its first word is written only then: until
//! it is, the partition holds no image the bootloader would start, even
//! if it tried that partition. An update installed and not started yet
//! is given up before another is written, which would otherwise go into
//! the partition chosen to start.

use alloc::boxed::Box;
use alloc::string::String;

use esp_bootloader_esp_idf::partitions::{
    self, AppPartitionSubType, DataPartitionSubType, FlashStorage, PARTITION_TABLE_MAX_LEN,
    PartitionType,
};
use log::{info, warn};
use paperpaw_platform::Storage;
use paperpaw_sdk::firmware::{Check, Firmware, FirmwareState, InstallFailure, Plugins, Update};
use paperpaw_x4::ota::{self, FlashError, Image, OtaData, State};

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
/// Read from the card and written a piece at a time.
const PIECE: usize = 4096;
/// Written per [`Ota::step`]: the main loop goes on in between.
const STEP: u32 = ota::BLOCK;
/// An update on trial is taken as good once it ran this long in all,
/// across starts (its start and the book it opens at once behind it).
const TRIAL_MS: u64 = 30_000;

/// An install going on.
struct Install {
    path: String,
    image: Image,
    /// Where in the flash the image goes.
    offset: u32,
    /// The partition it goes to.
    slot: u32,
    check: Check,
    buf: Box<[u8; PIECE]>,
    /// The last twentieth of the image said to be written.
    said: u32,
}

/// The running firmware on trial.
struct Trial {
    /// How long it ran in starts before this one.
    before: u64,
    /// Since when it runs now (ms since the start; anew after a sleep on
    /// USB).
    since: u64,
}

/// The flash, one handle for updates and native plugins (`native`): a
/// second one would find its size anew, and not always.
pub type Flash = alloc::rc::Rc<core::cell::RefCell<FlashStorage<'static>>>;

/// A partition of the flash, as `paperpaw_x4::ota` reads and writes it.
struct Partition<'a> {
    flash: &'a mut FlashStorage<'static>,
    offset: u32,
}

impl ota::Flash for Partition<'_> {
    fn read(&mut self, at: u32, buf: &mut [u8]) -> Result<(), FlashError> {
        self.flash
            .read(self.offset + at, buf)
            .map_err(|_| FlashError)
    }

    fn program(&mut self, at: u32, data: &[u8]) -> Result<(), FlashError> {
        self.flash
            .write_nor(self.offset + at, data)
            .map_err(|_| FlashError)
    }

    fn erase(&mut self, from: u32, to: u32) -> Result<(), FlashError> {
        self.flash
            .erase(self.offset + from, self.offset + to)
            .map_err(|_| FlashError)
    }
}

pub struct Ota {
    flash: Flash,
    /// Where the OTA data partition is.
    data: u32,
    /// Where the two app partitions are, and their sizes.
    apps: [(u32, u32); 2],
    /// The app partition running.
    running: u32,
    trial: Option<Trial>,
    /// An update did not start cleanly, and this firmware came back.
    undone: bool,
    install: Option<Install>,
}

impl Ota {
    /// Looks at what started and why (see the [module documentation](self));
    /// `None` without two app partitions (flashed before they came: the
    /// web flasher sets them up once).
    // A frame of its own, off the stack before the kernel is built.
    #[inline(never)]
    pub fn start(shared: Flash) -> Option<Self> {
        let mut flash = shared.borrow_mut();
        let Some(Partitions {
            data,
            apps,
            running,
        }) = partitions_of(&mut flash)
        else {
            warn!("OTA: no partitions for updates");
            return None;
        };
        let partition = &mut Partition {
            flash: &mut flash,
            offset: data,
        };
        let mut ota_data = OtaData::read(partition).ok()?;
        let chosen = ota_data.chosen();
        let state = ota_data.state_of(running);
        info!("OTA: started {running}, chosen {chosen:?} ({state:?})");
        let mut trial = None;
        let mut undone = false;
        match (chosen, state) {
            // The one chosen did not start (marked bad here at the last
            // start, or not startable): this one takes its place back.
            (Some(chosen), _) if chosen != running => {
                warn!("OTA: the update did not start, {running} is back");
                undone = true;
                if ota_data
                    .choose(partition, running, State::Valid, 0)
                    .is_err()
                {
                    warn!("OTA: could not choose {running}");
                }
            }
            // Undefined: chosen by a tool.
            (Some(_), Some((State::New | State::Undefined, ran))) => {
                info!("OTA: an update on trial, {ran} ms run before");
                if ota_data
                    .set_state(partition, running, State::PendingVerify, ran)
                    .is_ok()
                {
                    trial = Some(Trial {
                        before: u64::from(ran),
                        since: 0,
                    });
                }
            }
            (Some(_), Some((State::PendingVerify, _))) => {
                // The last run on trial ended other than cleanly.
                warn!("OTA: the update failed on trial: back to the one before");
                let _ = ota_data.set_state(partition, running, State::Invalid, 0);
                esp_hal::system::software_reset();
            }
            _ => {}
        }
        drop(flash);
        Some(Self {
            flash: shared,
            data,
            apps,
            running,
            trial,
            undone,
            install: None,
        })
    }

    /// Whether the running firmware is on trial (an update not yet run
    /// [`TRIAL_MS`]).
    pub fn on_trial(&self) -> bool {
        self.trial.is_some()
    }

    /// The running firmware on trial ran long enough, at `now` (ms since
    /// the start): it is marked as good. Whether it was, just now.
    pub fn check_trial(&mut self, now: u64) -> bool {
        let Some(trial) = &self.trial else {
            return false;
        };
        if trial.before + now.saturating_sub(trial.since) < TRIAL_MS {
            return false;
        }
        self.trial = None;
        match self.mark(State::Valid, 0) {
            Ok(()) => info!("OTA: the update works"),
            Err(_) => warn!("OTA: could not mark the update as good"),
        }
        true
    }

    /// The running firmware goes to sleep or restarts at `now`, ending
    /// cleanly: on trial, it is new again with the time it ran, so the
    /// next start goes on with the trial.
    pub fn pause_trial(&mut self, now: u64) {
        let Some(trial) = &mut self.trial else {
            return;
        };
        trial.before += now.saturating_sub(trial.since);
        trial.since = now;
        let ran = u32::try_from(trial.before).unwrap_or(u32::MAX - 1);
        if self.mark(State::New, ran).is_err() {
            warn!("OTA: could not note the time on trial");
        }
    }

    /// Awake again at `now` without a new start (asleep on USB power): on
    /// trial again.
    pub fn resume_trial(&mut self, now: u64) {
        let Some(trial) = &mut self.trial else {
            return;
        };
        trial.since = now;
        let ran = u32::try_from(trial.before).unwrap_or(u32::MAX - 1);
        if self.mark(State::PendingVerify, ran).is_err() {
            warn!("OTA: could not put the update on trial again");
        }
    }

    /// Marks the running partition's image (its entry, also with another
    /// chosen meanwhile).
    fn mark(&mut self, state: State, ran: u32) -> Result<(), FlashError> {
        let mut flash = self.flash.borrow_mut();
        let partition = &mut Partition {
            flash: &mut flash,
            offset: self.data,
        };
        OtaData::read(partition)?.set_state(partition, self.running, state, ran)
    }

    /// The firmware as plugins see it.
    pub fn firmware(&self) -> Firmware {
        let target = (self.running + 1) % ota::SLOTS;
        Firmware::new(env!("PAPERPAW_VERSION"), PROJECT, CHIP)
            .keys(KEYS.to_vec())
            .room(u64::from(self.apps[target as usize].1))
            .undone(self.undone)
            .state(FirmwareState::Idle)
            .plugins(Plugins::read_static(&PLUGINS).unwrap_or_default())
    }

    /// Whether an install goes on (the device neither sleeps nor waits
    /// meanwhile).
    pub fn installing(&self) -> bool {
        self.install.is_some()
    }

    /// Starts installing the update file at `path`, with the `key` the
    /// reader accepted and whether they chose an `older` one (see
    /// `FirmwareCommand::Install`): what it holds is checked first, the
    /// signature as it is written.
    pub fn begin(
        &mut self,
        storage: &dyn Storage,
        path: &str,
        key: Option<&[u8; 32]>,
        older: bool,
    ) -> FirmwareState {
        match self.prepare(storage, path, key, older) {
            Ok(install) => {
                info!(
                    "OTA: installing {path} ({} bytes) at {:#x}",
                    install.image.total(),
                    install.offset
                );
                let total = u64::from(install.image.total());
                self.install = Some(install);
                FirmwareState::Installing { done: 0, total }
            }
            Err(why) => {
                warn!("OTA: {path}: {why:?}");
                FirmwareState::Failed(why)
            }
        }
    }

    fn prepare(
        &mut self,
        storage: &dyn Storage,
        path: &str,
        key: Option<&[u8; 32]>,
        older: bool,
    ) -> Result<Install, InstallFailure> {
        let update = Update::read(storage, path)?;
        update.may_install(&self.firmware(), key, older)?;
        let check = Check::new(&update.trailer)?;
        let mut flash = self.flash.borrow_mut();
        let partition = &mut Partition {
            flash: &mut flash,
            offset: self.data,
        };
        let slot = OtaData::read(partition)
            .and_then(|mut data| data.prepare_install(partition, self.running))
            .map_err(|_| InstallFailure::Write)?;
        Ok(Install {
            path: String::from(path),
            image: Image::new(update.trailer.image_len),
            offset: self.apps[slot as usize].0,
            slot,
            check,
            buf: Box::new([0; PIECE]),
            said: 0,
        })
    }

    /// Writes the next part; the new state when the install went on (every
    /// 5 %: the page shows each with a refresh), ended or failed.
    pub fn step(&mut self, storage: &dyn Storage) -> Option<FirmwareState> {
        let mut install = self.install.take()?;
        let (done, total) = match self.write(storage, &mut install) {
            Err(why) => {
                warn!("OTA: {}: {why:?} at {}", install.path, install.image.done());
                return Some(FirmwareState::Failed(why));
            }
            Ok(()) => (install.image.done(), install.image.total()),
        };
        if done == total {
            return Some(self.finish(install));
        }
        let twentieth = (u64::from(done) * 20 / u64::from(total)) as u32;
        let state = (twentieth != install.said).then_some(FirmwareState::Installing {
            done: u64::from(done),
            total: u64::from(total),
        });
        install.said = twentieth;
        self.install = Some(install);
        state
    }

    fn write(
        &mut self,
        storage: &dyn Storage,
        install: &mut Install,
    ) -> Result<(), InstallFailure> {
        let image = &mut install.image;
        let end = (image.done() + STEP).min(image.total());
        let mut flash = self.flash.borrow_mut();
        let partition = &mut Partition {
            flash: &mut flash,
            offset: install.offset,
        };
        while image.done() < end {
            let len = (end - image.done()).min(PIECE as u32) as usize;
            let piece = &mut install.buf[..len];
            match storage.read_at(&install.path, u64::from(image.done()), piece) {
                Ok(n) if n == len => {}
                _ => return Err(InstallFailure::Unreadable),
            }
            install.check.absorb(piece);
            image
                .write(partition, &mut install.buf[..], len)
                .map_err(|_| InstallFailure::Write)?;
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
        let mut flash = self.flash.borrow_mut();
        let made_whole = install.image.finish(
            &mut Partition {
                flash: &mut flash,
                offset: install.offset,
            },
            &mut install.buf[..],
        );
        let data = &mut Partition {
            flash: &mut flash,
            offset: self.data,
        };
        // State and choice in one entry, written whole.
        let chosen = made_whole
            .and_then(|()| OtaData::read(data)?.choose(data, install.slot, State::New, 0));
        match chosen {
            Ok(()) => {
                info!("OTA: {} installed, started at the next start", install.path);
                FirmwareState::Installed
            }
            Err(_) => {
                warn!("OTA: {}: could not be chosen", install.path);
                FirmwareState::Failed(InstallFailure::Write)
            }
        }
    }
}

/// The partitions updates use.
struct Partitions {
    /// Where the OTA data partition is.
    data: u32,
    /// Where the two app partitions are, and their sizes.
    apps: [(u32, u32); 2],
    /// The app partition running.
    running: u32,
}

/// The partitions updates use; `None` without two app partitions (and
/// only two) or when which runs cannot be told.
fn partitions_of(flash: &mut FlashStorage<'static>) -> Option<Partitions> {
    let mut table = Box::new([0u8; PARTITION_TABLE_MAX_LEN]);
    let pt = partitions::read_partition_table(flash, &mut table[..]).ok()?;
    let find = |kind| {
        pt.find_partition(kind)
            .ok()
            .flatten()
            .map(|p| (p.offset(), p.len()))
    };
    let (data, _) = find(PartitionType::Data(DataPartitionSubType::Ota))?;
    let apps = [
        find(PartitionType::App(AppPartitionSubType::Ota0))?,
        find(PartitionType::App(AppPartitionSubType::Ota1))?,
    ];
    if find(PartitionType::App(AppPartitionSubType::Ota2)).is_some() {
        return None;
    }
    let running = match pt.booted_partition().ok()??.partition_type() {
        PartitionType::App(AppPartitionSubType::Ota0) => 0,
        PartitionType::App(AppPartitionSubType::Ota1) => 1,
        _ => return None,
    };
    Some(Partitions {
        data,
        apps,
        running,
    })
}
