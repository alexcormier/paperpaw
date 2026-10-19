//! Native code plugins from the card (ADR 0024): the kernel asks for the
//! code of each code plugin switched on (`plugins/<id>/code.ppn`) at the
//! start; [`Loader`] checks the file (`paperpaw_native`), puts its image
//! where it runs, relocated, and makes its plugin.
//!
//! - **X4 Pro**: the image is read into PSRAM and runs from there, through
//!   the instruction bus's view of the same memory (`psram`).
//! - **X4**: the image is written once into the free end of the app
//!   partition the firmware runs from and mapped through free MMU entries,
//!   so its code runs from flash and only its data takes RAM (`flash`).
//!
//! Plugins are never unloaded: switching one takes a restart. Where each
//! lies is kept ([`attribute`]), so a panic's calls inside a plugin are
//! written with its name and the offset into its file.

mod api;
#[cfg(feature = "x4")]
mod flash;
#[cfg(feature = "x4-pro")]
mod psram;

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::RefCell;

use critical_section::Mutex;
use log::{info, warn};
use paperpaw_core::CodeLoader;
use paperpaw_native::{CODE_OFFSET, Chip, Entry, HEADER_LEN, Header, Problem};
use paperpaw_platform::Storage;
use paperpaw_sdk::native::{FINGERPRINT, TOOLCHAIN};
use paperpaw_sdk::{Plugin, PluginProblem};

/// The chip plugin files must be made for.
#[cfg(feature = "x4")]
const CHIP: Chip = Chip::Esp32C3;
#[cfg(feature = "x4-pro")]
const CHIP: Chip = Chip::Esp32S3;

/// A plugin's image, where it runs.
struct Image {
    /// The plugin's id (its folder's name).
    id: String,
    /// The instruction address of the image's first byte.
    code_base: u32,
    len: u32,
}

/// Every image loaded, for naming the plugin a panic's call was in.
static IMAGES: Mutex<RefCell<Vec<Image>>> = Mutex::new(RefCell::new(Vec::new()));

/// The plugin whose code the instruction address `pc` lies in, and the
/// offset into its file (the image begins after the header): with the
/// plugin's ELF, its function is at `0x3C000000 + 0x06000000 + offset -
/// 80`. Called from the panic handler: allocates nothing.
pub fn attribute(pc: u32, found: &mut dyn FnMut(&str, u32)) {
    critical_section::with(|cs| {
        if let Ok(images) = IMAGES.borrow(cs).try_borrow() {
            for image in images.iter() {
                let offset = pc.wrapping_sub(image.code_base);
                if offset < image.len {
                    found(&image.id, offset + HEADER_LEN as u32);
                    return;
                }
            }
        }
    });
}

/// Where images go: PSRAM on the X4 Pro, flash on the X4.
#[cfg(feature = "x4-pro")]
pub type Place = psram::Psram;
#[cfg(feature = "x4")]
pub type Place = flash::Flash;

/// Loads code plugins for the kernel (see the [module documentation](self)).
pub struct Loader {
    place: Place,
}

impl Loader {
    pub fn new(place: Place) -> Self {
        Self { place }
    }

    fn load_file(
        &mut self,
        storage: &dyn Storage,
        path: &str,
    ) -> Result<Box<dyn Plugin>, PluginProblem> {
        let heap = esp_alloc::HEAP.used();
        let started = crate::micros();
        let mut head = [0; HEADER_LEN];
        read_exact(storage, path, 0, &mut head).map_err(|_| PluginProblem::Damaged)?;
        let header = Header::read(&head).map_err(problem)?;
        if header.chip != CHIP || header.fingerprint != FINGERPRINT.0 {
            warn!(
                "native: {path} is for {} {:016x}, this firmware {CHIP} {FINGERPRINT} ({TOOLCHAIN})",
                header.chip, header.fingerprint
            );
            return Err(PluginProblem::OtherBuild);
        }
        if storage.size(path).ok() != Some(header.file_len()) {
            return Err(PluginProblem::Damaged);
        }
        let words = read_words(storage, path, &header)?;
        let base = self.place.put(storage, path, &header, &words)?;
        drop(words);
        let placed = crate::micros();
        // SAFETY: the entry lies in the image just placed and relocated,
        // its address checked to be in the image's read-only data.
        let entry = unsafe { &*((base + header.entry) as *const Entry) };
        if entry.fingerprint != FINGERPRINT.0 {
            return Err(PluginProblem::Damaged);
        }
        let id = path
            .rsplit('/')
            .nth(1)
            .map_or_else(String::new, String::from);
        critical_section::with(|cs| {
            IMAGES.borrow_ref_mut(cs).push(Image {
                id,
                code_base: base + CODE_OFFSET,
                len: header.image_len,
            })
        });
        // SAFETY: a plugin of this firmware's compiler, target and SDK
        // (its fingerprint), placed as its file says.
        let plugin =
            unsafe { paperpaw_sdk::native::take((entry.make)()) }.ok_or(PluginProblem::Damaged)?;
        let made = crate::micros();
        info!(
            "native: {path}: {} B at {base:#x}, {} words moved; placed in {} us, made in {} us; heap {:+} B",
            header.image_len,
            header.relocs,
            placed - started,
            made - placed,
            esp_alloc::HEAP.used() as isize - heap as isize
        );
        Ok(plugin)
    }
}

impl CodeLoader for Loader {
    fn load(
        &mut self,
        storage: &dyn Storage,
        path: &str,
    ) -> Result<Box<dyn Plugin>, PluginProblem> {
        let id = path.rsplit('/').nth(1).unwrap_or(path);
        crate::safe_mode::loading(id);
        self.load_file(storage, path)
    }

    fn finish(&mut self, _storage: &dyn Storage) {
        self.place.finish();
    }
}

/// What the plugin manager says for a file that is not right.
fn problem(problem: Problem) -> PluginProblem {
    match problem {
        Problem::NewerFormat | Problem::UnknownChip => PluginProblem::OtherBuild,
        _ => PluginProblem::Damaged,
    }
}

/// The words to relocate, read from the end of the file.
fn read_words(
    storage: &dyn Storage,
    path: &str,
    header: &Header,
) -> Result<Vec<u32>, PluginProblem> {
    let count = header.relocs as usize;
    let mut words = Vec::new();
    words
        .try_reserve_exact(count)
        .map_err(|_| PluginProblem::NoRoom)?;
    let mut buf = [0u8; 256];
    let mut at = header.relocs_at();
    while words.len() < count {
        let want = ((count - words.len()) * 4).min(buf.len());
        read_exact(storage, path, at, &mut buf[..want]).map_err(|_| PluginProblem::Damaged)?;
        words.extend(
            buf[..want]
                .as_chunks::<4>()
                .0
                .iter()
                .map(|w| u32::from_le_bytes(*w)),
        );
        at += want as u64;
    }
    paperpaw_native::check_relocations(header, words.iter().copied()).map_err(problem)?;
    Ok(words)
}

/// Fills `buf` from `path` at `at`.
fn read_exact(
    storage: &dyn Storage,
    path: &str,
    mut at: u64,
    mut buf: &mut [u8],
) -> Result<(), ()> {
    while !buf.is_empty() {
        match storage.read_at(path, at, buf) {
            Ok(0) | Err(_) => return Err(()),
            Ok(n) => {
                at += n as u64;
                buf = &mut buf[n..];
            }
        }
    }
    Ok(())
}

/// The bytes after the header the digest covers once the image is read:
/// the words to relocate as the file has them.
fn digest_words(sha: &mut paperpaw_digest::sha256::Sha256, words: &[u32]) {
    for word in words {
        sha.update(&word.to_le_bytes());
    }
}
