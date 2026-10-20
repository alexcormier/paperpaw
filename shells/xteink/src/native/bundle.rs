//! The web flasher's bundle of plugins (`paperpaw_native::bundle`), found
//! behind the firmware's image at its first start. Its plugins are copied
//! onto the card into folders PaperPaw does not load (`plugins/.<id>.part`),
//! each file checked by its SHA-256 as it is copied and a plugin's code by
//! its author's signature, for the kernel to install before it loads
//! plugins (`KernelBuilder::install_plugin`), so they run at this start.
//! Once they are all on the card, the bundle's first sector is erased
//! (on the X4 its code plugins go there).
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

use super::partition::behind_image;
use crate::ota::Flash;

/// Read and copied a piece at a time.
const PIECE: usize = 4096;

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

/// Copies the plugins of the bundle behind the image onto the card, if
/// there is one, and erases it (see the [module documentation](self)):
/// each plugin's id and the folder it is in, for the kernel to install.
pub fn stage(flash: &Flash, storage: &dyn Storage) -> Option<Vec<(Id, String)>> {
    let mut flash = flash.borrow_mut();
    let behind = behind_image(&mut flash).ok()?;
    let mut head = [0u8; 4];
    flash.read(behind.start, &mut head).ok()?;
    if head != BUNDLE_MAGIC {
        return None;
    }
    let mut source = InFlash {
        flash: &mut flash,
        at: behind.start,
    };
    let bundle = match Bundle::read(&mut source, &behind.firmware, behind.end - behind.start) {
        Ok(bundle) => bundle,
        Err(why) => {
            warn!("bundle: {why}: erased, nothing installed");
            erase(&mut flash, behind.start);
            return None;
        }
    };
    let started = crate::micros();
    let mut buf = Vec::new();
    buf.try_reserve_exact(PIECE).ok()?;
    buf.resize(PIECE, 0);
    let mut source = InFlash {
        flash: &mut flash,
        at: behind.start,
    };
    let mut plugins: Vec<(Id, String)> = Vec::new();
    for plugin in &bundle.plugins {
        let folder = format!("{PLUGIN_DIR}/.{}.part", plugin.id);
        if let Err(why) = copy(&mut source, storage, plugin, &folder, &mut buf) {
            // The card, most likely: tried again at the next start.
            warn!("bundle: {}: {why}; kept for the next start", plugin.id);
            for (_, folder) in &plugins {
                let _ = storage.remove_dir_all(folder);
            }
            let _ = storage.remove_dir_all(&folder);
            return None;
        }
        if let Some(why) = refused(storage, &folder) {
            warn!("bundle: {}: {why}; not installed", plugin.id);
            let _ = storage.remove_dir_all(&folder);
            continue;
        }
        if let Ok(id) = Id::parse(&plugin.id) {
            plugins.push((id, folder));
        }
    }
    erase(&mut flash, behind.start);
    info!(
        "bundle: {} plugins copied onto the card in {} ms",
        plugins.len(),
        (crate::micros() - started) / 1000
    );
    Some(plugins)
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

/// Why the plugin copied into `folder` is not installed: its code is not a
/// plugin file, or its author's signature does not hold.
fn refused(storage: &dyn Storage, folder: &str) -> Option<String> {
    let code = format!("{folder}/code.ppn");
    if !storage.exists(&code) {
        return None;
    }
    paperpaw_sdk::native::check_file(storage, &code)
        .err()
        .map(|problem| format!("{problem}"))
}

/// Erases the bundle's first sector: what is behind it is no bundle.
fn erase(flash: &mut FlashStorage<'static>, at: u32) {
    if let Err(e) = flash.erase(at, at + SECTOR) {
        warn!("bundle: not erased ({e:?})");
    }
}
