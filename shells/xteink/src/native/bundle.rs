//! The web flasher's bundle of plugins (`paperpaw_native::bundle`), found
//! at the firmware's first start at the start of the other app partition
//! (the flasher puts it there for firmware that says it looks there,
//! [`OTHER_SLOT`]: room for the whole store), or behind the firmware's
//! image (where it put it before). Its plugins are copied
//! onto the card into folders PaperPaw does not load (`plugins/.<id>.part`),
//! each file checked by its SHA-256 as it is copied and a plugin's code by
//! its author's signature, for the kernel to install before it loads
//! plugins (`KernelBuilder::install_plugin`), so they run at this start.
//! The author's key is remembered at a plugin's first install
//! (`native::remember_author`), and the code installed by its digest
//! (`native::remember_code`: only that file runs); code signed by another
//! author than the version installed before is not installed, and the plugin manager says
//! so (`Native::not_installed`): the reader decides in the store, warned.
//! Once they are all on the card, the bundle's first sector is erased
//! (on the X4 its code plugins go there; in the other app partition an
//! update goes there, which writes it whole anyway). A power cut before
//! that leaves the bundle as it was: copied again at the next start.
//!
//! A bundle for another image, or one that is damaged, is erased without
//! installing anything; one that cannot be copied (no card, a card full)
//! waits for the next start. On the X4 the bundle lies where its code
//! plugins go: they get room once it is installed.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use esp_bootloader_esp_idf::partitions::FlashStorage;
use log::{info, warn};
use paperpaw_digest::sha256::Sha256;
use paperpaw_native::bundle::{BUNDLE_MAGIC, Bundle, BundleFile, BundlePlugin, Source};
use paperpaw_native::index::SECTOR;
use paperpaw_platform::Storage;
use paperpaw_sdk::Id;
use paperpaw_sdk::builtin::PLUGIN_DIR;
use paperpaw_sdk::native::{self, Author, NotInstalled, NotInstalledReason};

use super::partition::{behind_image, other_app};
use crate::ota::Flash;

/// Read and copied a piece at a time.
const PIECE: usize = 4096;

/// Says in the image that this firmware takes a bundle from the start of
/// the other app partition: the web flasher looks for these bytes before
/// putting it there, and puts it behind the image for a firmware without
/// them.
#[used]
pub static OTHER_SLOT: [u8; 32] = *b"PaperPaw bundle: other slot, 1\0\0";

/// Where a bundle lies, if one does: from `start` to at most `end`.
fn found(flash: &mut FlashStorage<'static>, behind: (u32, u32)) -> Option<(u32, u32)> {
    let mut head = [0u8; 4];
    let other = other_app(flash)
        .ok()
        .map(|(offset, size)| (offset, offset + size));
    [Some(behind), other]
        .into_iter()
        .flatten()
        .find(|&(start, _)| flash.read(start, &mut head).is_ok() && head == BUNDLE_MAGIC)
}

/// The bundle's bytes, from flash.
struct InFlash<'a> {
    flash: &'a mut FlashStorage<'static>,
    at: u32,
}

impl Source for InFlash<'_> {
    fn read(&mut self, at: u32, buf: &mut [u8]) -> bool {
        self.flash.read(self.at + at, buf).is_ok()
    }
}

/// What a bundle brought: each plugin to install, by its id and the
/// folder it is in, and those not installed.
#[derive(Default)]
pub struct Staged {
    pub installs: Vec<(Id, String)>,
    pub not_installed: Vec<NotInstalled>,
}

/// Copies the plugins of the bundle behind the image onto the card, if
/// there is one, and erases it (see the [module documentation](self)).
// A frame of its own, off the stack before the kernel is built.
#[inline(never)]
pub fn stage(flash: &Flash, storage: &dyn Storage) -> Option<Staged> {
    let mut flash = flash.borrow_mut();
    let behind = behind_image(&mut flash).ok()?;
    let (start, end) = found(&mut flash, (behind.start, behind.end))?;
    info!("bundle: at {start:#x}");
    let mut source = InFlash {
        flash: &mut flash,
        at: start,
    };
    let bundle = match Bundle::read(&mut source, &behind.firmware, end - start) {
        Ok(bundle) => bundle,
        Err(why) => {
            warn!("bundle: {why}: erased, nothing installed");
            erase(&mut flash, start);
            return None;
        }
    };
    let started = crate::micros();
    let mut buf = Vec::new();
    buf.try_reserve_exact(PIECE).ok()?;
    buf.resize(PIECE, 0);
    let mut source = InFlash {
        flash: &mut flash,
        at: start,
    };
    let mut staged = Staged::default();
    // The authors' keys and the code of the plugins installed, kept once
    // all are.
    let mut authors: Vec<(&str, [u8; 32])> = Vec::new();
    let mut codes: Vec<(&str, [u8; 32])> = Vec::new();
    for plugin in &bundle.plugins {
        // The bundle's names are those of plugins (`Bundle::read`).
        let Ok(id) = Id::parse(&plugin.id) else {
            continue;
        };
        let folder = format!("{PLUGIN_DIR}/.{}.part", plugin.id);
        if let Err(why) = copy(&mut source, storage, plugin, &folder, &mut buf) {
            // The card, most likely: tried again at the next start.
            warn!("bundle: {}: {why}; kept for the next start", plugin.id);
            for (_, folder) in &staged.installs {
                let _ = storage.remove_dir_all(folder);
            }
            let _ = storage.remove_dir_all(&folder);
            return None;
        }
        match check(storage, &plugin.id, &folder) {
            Ok(checked) => {
                let name = plugin.id.as_str();
                authors.extend(checked.author.map(|key| (name, key)));
                codes.extend(checked.sha256.map(|sha256| (name, sha256)));
                staged.installs.push((id, folder));
            }
            Err(reason) => {
                warn!("bundle: {}: {reason:?}; not installed", plugin.id);
                let _ = storage.remove_dir_all(&folder);
                staged.not_installed.push(NotInstalled::new(id, reason));
            }
        }
    }
    for (plugin, key) in authors {
        if !native::remember_author(storage, plugin, &key) {
            warn!("bundle: {plugin}: its author's key not kept");
        }
    }
    for (plugin, sha256) in codes {
        if !native::remember_code(storage, plugin, &sha256) {
            warn!("bundle: {plugin}: its code not named as installed");
        }
    }
    erase(&mut flash, start);
    info!(
        "bundle: {} plugins copied onto the card in {} ms, {} not installed",
        staged.installs.len(),
        (crate::micros() - started) / 1000,
        staged.not_installed.len()
    );
    Some(staged)
}

/// Copies `plugin`'s files into `folder`, each checked by its SHA-256.
fn copy(
    source: &mut InFlash<'_>,
    storage: &dyn Storage,
    plugin: &BundlePlugin,
    folder: &str,
    buf: &mut [u8],
) -> Result<(), &'static str> {
    let _ = storage.remove_dir_all(folder);
    let _ = storage.create_dir(PLUGIN_DIR);
    storage.create_dir(folder).map_err(|_| "no folder for it")?;
    for file in &plugin.files {
        crate::power::feed();
        let path = format!("{folder}/{}", file.path);
        // Its folders first (`i18n/de.ini`).
        let mut at = folder.len();
        while let Some(slash) = path[at + 1..].find('/') {
            at += 1 + slash;
            let _ = storage.create_dir(&path[..at]);
        }
        copy_file(source, storage, file, &path, buf)?;
    }
    Ok(())
}

fn copy_file(
    source: &mut InFlash<'_>,
    storage: &dyn Storage,
    file: &BundleFile,
    path: &str,
    buf: &mut [u8],
) -> Result<(), &'static str> {
    storage.write(path, &[]).map_err(|_| "not written")?;
    let mut sha = Sha256::new();
    let mut done = 0u32;
    while done < file.size {
        let len = (file.size - done).min(buf.len() as u32) as usize;
        if !source.read(file.at + done, &mut buf[..len]) {
            return Err("the flash cannot be read");
        }
        sha.update(&buf[..len]);
        storage
            .append(path, &buf[..len])
            .map_err(|_| "not written")?;
        done += len as u32;
    }
    if sha.finish() != file.sha256 {
        return Err("a file does not hold");
    }
    Ok(())
}

/// What a plugin checked to install brings to remember: its author's key
/// at its first install, and its code's digest.
#[derive(Default)]
struct Checked {
    author: Option<[u8; 32]>,
    sha256: Option<[u8; 32]>,
}

/// Whether `plugin`, copied into `folder`, is installed: a data plugin
/// is; a code plugin when its code is a plugin file whose author's
/// signature holds, by the author of the version installed before if
/// there was one.
fn check(storage: &dyn Storage, plugin: &str, folder: &str) -> Result<Checked, NotInstalledReason> {
    let code = format!("{folder}/code.ppn");
    if !storage.exists(&code) {
        return Ok(Checked::default());
    }
    let (header, signature) = native::check_file(storage, &code).map_err(|problem| {
        warn!("bundle: {plugin}: {problem}");
        NotInstalledReason::Damaged
    })?;
    let sha256 = Some(header.sha256);
    let Some(signature) = signature else {
        // Not signed: one's own build, unless a signed one was installed.
        return match native::known_author(storage, plugin) {
            Some(_) => Err(NotInstalledReason::OtherAuthor),
            None => Ok(Checked {
                author: None,
                sha256,
            }),
        };
    };
    match native::author(storage, plugin, &signature.key) {
        Author::Known => Ok(Checked {
            author: None,
            sha256,
        }),
        Author::New => Ok(Checked {
            author: Some(signature.key),
            sha256,
        }),
        // Another author, and verdicts to come: not trusted.
        _ => Err(NotInstalledReason::OtherAuthor),
    }
}

/// Erases the bundle's first sector: what is behind it is no bundle.
fn erase(flash: &mut FlashStorage<'static>, at: u32) {
    if let Err(e) = flash.erase(at, at + SECTOR) {
        warn!("bundle: not erased ({e:?})");
    }
}
