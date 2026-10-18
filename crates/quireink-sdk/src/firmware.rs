//! Firmware updates, on devices whose firmware QuireInk is (the X4):
//! [`Env::firmware`](crate::Env) is `Some` there. Elsewhere QuireInk is
//! an app, updated by the system's store.
//!
//! The shell does what it is told ([`Command::Firmware`](crate::Command)
//! with a [`FirmwareCommand`]: install an update file, restart) and
//! reports how it goes in [`Firmware::state`], with
//! [`events::FIRMWARE_CHANGED`](crate::builtin::events::FIRMWARE_CHANGED).
//! Finding updates and asking the reader is a plugin's (the `update`
//! plugin, which another can replace).
//!
//! Update files are signed (the format is `quireink-update`'s, its parts
//! are here too). The shell installs only those signed with one of
//! [`Firmware::keys`] (QuireInk's) or a key listed in [`TRUSTED_KEYS`],
//! which a plugin adds once the reader chose to trust it (their own
//! builds). It writes the new firmware beside the running one and starts
//! it at the next start; if the new one does not start cleanly, the old
//! one comes back ([`Firmware::undone`]). Books, settings and plugins on
//! the card stay as they are.

use alloc::string::String;
use alloc::vec::Vec;
use core::cmp::Ordering;

use quireink_platform::{Storage, StorageError};
pub use quireink_update::{
    APP_HEAD_LEN, AppInfo, CHIP_ESP32_C3, Check, Problem, TRAILER_LEN, Trailer, fingerprint,
    key_hex, parse_key,
};

/// The folder on the card where update files are looked for.
pub const UPDATE_DIR: &str = "updates";
/// The keys the reader chose to trust besides QuireInk's: one per line,
/// as 64 hexadecimal digits.
pub const TRUSTED_KEYS: &str = crate::data_path!("firmware-keys.txt");

/// What the shell is told to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FirmwareCommand {
    /// Installs the update file at this storage path: checks it, writes
    /// it beside the running firmware and makes it the one to start next.
    Install { path: String },
    /// Restarts the device, e.g. into the new firmware once
    /// [`FirmwareState::Installed`]. What plugins keep is saved first.
    Restart,
}

/// Where an install is.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum FirmwareState {
    #[default]
    Idle,
    /// Writing: `done` of `total` bytes.
    Installing { done: u64, total: u64 },
    /// Written and checked: the new firmware starts at the next start.
    Installed,
    /// Nothing was changed: the running firmware stays.
    Failed(InstallFailure),
}

/// Why an update cannot be installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallFailure {
    /// The file could not be read.
    Unreadable,
    /// It is no update file (e.g. a whole flash image for the web
    /// flasher).
    NotAnUpdate,
    /// It is made for a newer QuireInk than this one can install.
    NewerFormat,
    /// It is firmware for another device.
    OtherDevice,
    /// It is larger than the room for firmware.
    TooLarge,
    /// It is signed with a key that is not trusted here.
    Untrusted,
    /// It was changed or damaged: its signature does not hold.
    Damaged,
    /// Writing it failed.
    Write,
}

impl From<Problem> for InstallFailure {
    fn from(problem: Problem) -> Self {
        match problem {
            Problem::NotAnUpdate | Problem::NotAnImage => InstallFailure::NotAnUpdate,
            Problem::NewerFormat => InstallFailure::NewerFormat,
            Problem::Length | Problem::Signature => InstallFailure::Damaged,
        }
    }
}

/// Firmware as the kernel knows it ([`Env::firmware`](crate::Env)).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Firmware {
    /// The running firmware's version, e.g. `0.4.0`.
    pub version: String,
    /// Which firmware it is, e.g. `quireink-xteink`: updates must be the
    /// same.
    pub project: String,
    /// The chip it runs on, as update images name it.
    pub chip: u16,
    /// The keys built in, QuireInk's (Ed25519 public keys).
    pub keys: Vec<[u8; 32]>,
    /// The room for firmware, in bytes.
    pub room: u64,
    /// An update installed before did not start cleanly, and the firmware
    /// before it came back at this start.
    pub undone: bool,
    pub state: FirmwareState,
}

/// Who signed an update.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signer {
    /// QuireInk ([`Firmware::keys`]).
    QuireInk,
    /// A key the reader chose to trust ([`TRUSTED_KEYS`]).
    Trusted,
    /// A key not known here.
    Unknown,
}

/// What an update file holds, from its start and its end (its signature
/// is checked as it is installed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Update {
    pub path: String,
    pub info: AppInfo,
    pub trailer: Trailer,
}

impl Update {
    /// Reads the update file at `path`.
    pub fn read(storage: &dyn Storage, path: &str) -> Result<Self, InstallFailure> {
        let size = storage.size(path).map_err(|_| InstallFailure::Unreadable)?;
        let mut tail = [0; TRAILER_LEN];
        let start = size
            .checked_sub(TRAILER_LEN as u64)
            .ok_or(InstallFailure::NotAnUpdate)?;
        read_exact(storage, path, start, &mut tail)?;
        let trailer = Trailer::read(&tail, size)?;
        let mut head = [0; APP_HEAD_LEN];
        read_exact(storage, path, 0, &mut head)?;
        let info = AppInfo::read(&head)?;
        Ok(Self {
            path: String::from(path),
            info,
            trailer,
        })
    }

    /// Whether it is for this device and has room.
    pub fn fits(&self, firmware: &Firmware) -> Result<(), InstallFailure> {
        if self.info.chip != firmware.chip || self.info.project != firmware.project {
            return Err(InstallFailure::OtherDevice);
        }
        if u64::from(self.trailer.image_len) > firmware.room {
            return Err(InstallFailure::TooLarge);
        }
        Ok(())
    }

    /// Who signed it, as far as this device knows.
    pub fn signer(&self, firmware: &Firmware, trusted: &[[u8; 32]]) -> Signer {
        let key = &self.trailer.key;
        if firmware.keys.contains(key) {
            Signer::QuireInk
        } else if trusted.contains(key) {
            Signer::Trusted
        } else {
            Signer::Unknown
        }
    }
}

fn read_exact(
    storage: &dyn Storage,
    path: &str,
    offset: u64,
    buf: &mut [u8],
) -> Result<(), InstallFailure> {
    match storage.read_at(path, offset, buf) {
        Ok(n) if n == buf.len() => Ok(()),
        _ => Err(InstallFailure::Unreadable),
    }
}

/// The keys the reader chose to trust ([`TRUSTED_KEYS`]).
pub fn trusted_keys(storage: &dyn Storage) -> Vec<[u8; 32]> {
    let Ok(size) = storage.size(TRUSTED_KEYS) else {
        return Vec::new();
    };
    let mut text = alloc::vec![0; size.min(64 * 1024) as usize];
    let n = storage.read_at(TRUSTED_KEYS, 0, &mut text).unwrap_or(0);
    core::str::from_utf8(&text[..n])
        .unwrap_or_default()
        .lines()
        .filter_map(parse_key)
        .collect()
}

/// Trusts `key` from now on (see [`TRUSTED_KEYS`]).
pub fn trust(storage: &dyn Storage, key: &[u8; 32]) -> Result<(), StorageError> {
    if trusted_keys(storage).contains(key) {
        return Ok(());
    }
    let _ = storage.create_dir(crate::data::DIR);
    let mut line = key_hex(key);
    line.push('\n');
    storage.append(TRUSTED_KEYS, line.as_bytes())
}

/// Compares versions such as `0.4.0` and `0.10.1` number by number; a
/// pre-release such as `0.4.0-dev` comes before its release, as in
/// SemVer.
pub fn compare_versions(a: &str, b: &str) -> Ordering {
    let parts = |v: &str| {
        let v = v.split('+').next().unwrap_or_default();
        let (numbers, pre) = v.split_once('-').map_or((v, None), |(n, p)| (n, Some(p)));
        let numbers: Vec<u64> = numbers
            .split('.')
            .map_while(|part| part.parse().ok())
            .collect();
        // No pre-release is the release itself, after its pre-releases.
        (numbers, pre.is_none(), pre.map(String::from))
    };
    parts(a).cmp(&parts(b))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory_storage::MemoryStorage;

    const SEED: [u8; 32] = [3; 32];

    fn firmware() -> Firmware {
        Firmware {
            version: "0.3.0".into(),
            project: "quireink-xteink".into(),
            chip: quireink_update::CHIP_ESP32_C3,
            keys: alloc::vec![quireink_update::public_key(&[1; 32])],
            room: 100_000,
            ..Firmware::default()
        }
    }

    fn file(project: &[u8], len: usize, seed: &[u8; 32]) -> Vec<u8> {
        let mut image = alloc::vec![0u8; len];
        image[0] = 0xE9;
        image[12..14].copy_from_slice(&quireink_update::CHIP_ESP32_C3.to_le_bytes());
        image[32..36].copy_from_slice(&0xABCD_5432u32.to_le_bytes());
        image[48..53].copy_from_slice(b"0.4.0");
        image[80..80 + project.len()].copy_from_slice(project);
        let trailer = quireink_update::sign(&image, seed).unwrap();
        image.extend_from_slice(&trailer.to_bytes());
        image
    }

    #[test]
    fn an_update_says_what_it_is_and_who_signed_it() {
        let storage = MemoryStorage::new()
            .with("updates/a.bin", file(b"quireink-xteink", 4000, &SEED))
            .with("updates/b.bin", file(b"other-firmware", 4000, &SEED))
            .with("updates/c.bin", file(b"quireink-xteink", 200_000, &SEED))
            .with("updates/d.bin", alloc::vec![0xE9; 5000]);
        let fw = firmware();
        let a = Update::read(&storage, "updates/a.bin").unwrap();
        assert_eq!(a.info.version, "0.4.0");
        assert_eq!(a.fits(&fw), Ok(()));
        assert_eq!(a.signer(&fw, &[]), Signer::Unknown);
        let b = Update::read(&storage, "updates/b.bin").unwrap();
        assert_eq!(b.fits(&fw), Err(InstallFailure::OtherDevice));
        let c = Update::read(&storage, "updates/c.bin").unwrap();
        assert_eq!(c.fits(&fw), Err(InstallFailure::TooLarge));
        assert_eq!(
            Update::read(&storage, "updates/d.bin"),
            Err(InstallFailure::NotAnUpdate)
        );
        assert_eq!(
            Update::read(&storage, "updates/none.bin"),
            Err(InstallFailure::Unreadable)
        );

        // Trusted once, known from then on.
        trust(&storage, &a.trailer.key).unwrap();
        trust(&storage, &a.trailer.key).unwrap();
        let trusted = trusted_keys(&storage);
        assert_eq!(trusted.len(), 1);
        assert_eq!(a.signer(&fw, &trusted), Signer::Trusted);
    }

    #[test]
    fn versions_compare_by_number() {
        assert_eq!(compare_versions("0.10.0", "0.9.1"), Ordering::Greater);
        assert_eq!(compare_versions("0.4.0", "0.4.0"), Ordering::Equal);
        assert_eq!(compare_versions("0.4.0-dev", "0.4.1"), Ordering::Less);
        assert_eq!(compare_versions("0.4.0-dev", "0.4.0"), Ordering::Less);
        assert_eq!(compare_versions("0.4.0-dev", "0.3.0"), Ordering::Greater);
        assert_eq!(compare_versions("0.4.0+x4", "0.4.0"), Ordering::Equal);
    }
}
