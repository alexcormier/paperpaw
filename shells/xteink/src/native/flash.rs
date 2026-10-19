//! The X4: its chip has no PSRAM, and its RAM is too small to run
//! plugins from, so their code runs from flash. Images are written once
//! into the free end of the app partition the firmware runs from, behind
//! its image, one after the other; the index there
//! (`paperpaw_native::index`) says what lies where, so later starts only
//! map them. The region is mapped through MMU entries the firmware leaves
//! free, at the end of the table, so code and data of every plugin lie in
//! one window of addresses.
//!
//! - Updates are written into the other app partition: plugins in use are
//!   never erased under the code, and going back to the firmware before
//!   finds its own plugins behind it (the index holds the firmware it was
//!   made by).
//! - A bundle the web flasher wrote behind the image (`PPNB`) is left
//!   alone: the region is used once the bundle is installed.

use alloc::vec::Vec;

use esp_bootloader_esp_idf::partitions::{self, FlashStorage, PARTITION_TABLE_MAX_LEN};
use esp_hal::peripherals::MMU_TABLE;
use log::{info, warn};
use paperpaw_native::index::{Index, Layout, Region, SECTOR};
use paperpaw_native::{CODE_OFFSET, HEADER_LEN, Header, LINK_BASE, Relocator};
use paperpaw_platform::Storage;
use paperpaw_sdk::PluginProblem;
use paperpaw_update::ImageExtent;

use super::api::API;

/// An MMU page: 64 KiB of flash per entry.
const PAGE: u32 = 0x1_0000;
/// The MMU's entries (data and instruction bus share them).
const ENTRIES: u32 = 128;
/// The first bytes of the web flasher's bundle of plugins.
const BUNDLE: [u8; 4] = *b"PPNB";

unsafe extern "C" {
    fn Cache_Dbus_MMU_Set(
        ext_ram: u32,
        vaddr: u32,
        paddr: u32,
        psize: u32,
        num: u32,
        fixed: u32,
    ) -> i32;
    fn Cache_Invalidate_Addr(addr: u32, size: u32);
}

/// Places images in flash.
pub struct Flash {
    flash: FlashStorage<'static>,
    /// Where they go, once the region is found and mapped; `None` if there
    /// is none (no room, no free MMU entries, a bundle to install first).
    layout: Option<Layout>,
    /// The index sector was erased at this start.
    erased: bool,
}

impl Flash {
    /// The plugins' region behind the running firmware, mapped.
    pub fn new(flash: esp_hal::peripherals::FLASH<'static>) -> Self {
        let mut flash = FlashStorage::new(flash);
        let layout = region(&mut flash).map(|(region, firmware)| {
            let found = read_index(&mut flash, region.start);
            Layout::new(region, firmware, &API as *const _ as u32, found)
        });
        Self {
            flash,
            layout,
            erased: false,
        }
    }

    /// Writes the image of the plugin file at `path` into the region,
    /// relocated, unless it lies there already; its data address.
    pub fn put(
        &mut self,
        storage: &dyn Storage,
        path: &str,
        header: &Header,
        words: &[u32],
    ) -> Result<u32, PluginProblem> {
        let layout = self.layout.as_mut().ok_or(PluginProblem::NoRoom)?;
        let placement = layout.place(header).map_err(|_| PluginProblem::NoRoom)?;
        if placement.present {
            info!("native: {path} lies in flash at {:#x}", placement.at);
            return Ok(placement.base);
        }
        // Written only once checked whole: a damaged file never gets into
        // the index.
        check(storage, path, header, words)?;
        if placement.erase_index && !self.erased {
            let start = layout.region().start;
            self.flash
                .erase(start, start + SECTOR)
                .map_err(|_| PluginProblem::NoRoom)?;
            self.erased = true;
        }
        let relocator = Relocator::new(header, words, placement.base, &API as *const _ as u32)
            .map_err(super::problem)?;
        let mut piece = Vec::new();
        piece
            .try_reserve_exact(SECTOR as usize)
            .map_err(|_| PluginProblem::NoRoom)?;
        piece.resize(SECTOR as usize, 0);
        let mut done = 0;
        while done < header.image_len {
            crate::power::feed();
            let len = (header.image_len - done).min(SECTOR) as usize;
            let bytes = &mut piece[..len];
            super::read_exact(storage, path, HEADER_LEN as u64 + u64::from(done), bytes)
                .map_err(|_| PluginProblem::Damaged)?;
            relocator.apply(done, bytes).map_err(super::problem)?;
            // Whole words: the last piece filled up as erased flash.
            let words_len = len.next_multiple_of(4);
            piece[len..words_len].fill(0xFF);
            let at = placement.at + done;
            self.flash
                .erase(at, at + SECTOR)
                .and_then(|()| self.flash.write_nor(at, &piece[..words_len]))
                .map_err(|_| PluginProblem::NoRoom)?;
            done += len as u32;
        }
        // Nothing of what the region held before stays in the caches.
        let len = header.image_len.next_multiple_of(4);
        // SAFETY: ROM functions on addresses mapped to this region.
        unsafe {
            Cache_Invalidate_Addr(placement.base, len);
            Cache_Invalidate_Addr(placement.base + CODE_OFFSET, len);
        }
        info!(
            "native: {path} written into flash at {:#x} ({} sectors)",
            placement.at,
            header.image_len.div_ceil(SECTOR)
        );
        Ok(placement.base)
    }

    /// Writes the index, if what lies in the region changed.
    pub fn finish(&mut self) {
        let Some(layout) = &self.layout else {
            return;
        };
        let Some(index) = layout.index() else {
            return;
        };
        let start = layout.region().start;
        let mut bytes = index.to_bytes();
        bytes.resize(bytes.len().next_multiple_of(4), 0xFF);
        let written = self
            .flash
            .erase(start, start + SECTOR)
            .and_then(|()| self.flash.write_nor(start, &bytes));
        match written {
            Ok(()) => info!(
                "native: index of {} plugins written, {} KB of the region used",
                index.placed.len(),
                (layout.end() - start) / 1024
            ),
            Err(e) => warn!("native: the index: {e:?}"),
        }
    }
}

/// Checks the file's digest, reading its image once.
fn check(
    storage: &dyn Storage,
    path: &str,
    header: &Header,
    words: &[u32],
) -> Result<(), PluginProblem> {
    let mut sha = header.digest();
    let mut buf = [0u8; 512];
    let mut done = 0u32;
    while done < header.image_len {
        let len = (header.image_len - done).min(buf.len() as u32) as usize;
        super::read_exact(
            storage,
            path,
            HEADER_LEN as u64 + u64::from(done),
            &mut buf[..len],
        )
        .map_err(|_| PluginProblem::Damaged)?;
        sha.update(&buf[..len]);
        done += len as u32;
    }
    super::digest_words(&mut sha, words);
    if header.holds(sha) {
        Ok(())
    } else {
        Err(PluginProblem::Damaged)
    }
}

/// The region behind the running firmware's image, mapped, and the
/// firmware (the SHA-256 its image ends with).
fn region(flash: &mut FlashStorage<'static>) -> Option<(Region, [u8; 32])> {
    let mut table = [0u8; PARTITION_TABLE_MAX_LEN];
    let running = partitions::read_partition_table(flash, &mut table)
        .and_then(|pt| pt.booted_partition())
        .ok()
        .flatten()?;
    let (offset, size) = (running.offset(), running.len());
    let extent = ImageExtent::read(&mut |at, buf| flash.read(offset + at, buf).is_ok());
    let Some(extent) = extent.filter(|e| e.sha256 && e.len < size) else {
        warn!("native: the running image cannot be measured: no code plugins");
        return None;
    };
    let mut firmware = [0u8; 32];
    flash.read(offset + extent.len - 32, &mut firmware).ok()?;
    let start = offset + extent.len.next_multiple_of(SECTOR);
    let mut head = [0u8; 4];
    flash.read(start, &mut head).ok()?;
    if head == BUNDLE {
        warn!(
            "native: a bundle of plugins waits behind the firmware: no code plugins until it is installed"
        );
        return None;
    }
    let end = offset + size;
    // As many pages as the region spans, from the end of the MMU's table.
    let first_page = start / PAGE;
    let pages = (end.div_ceil(PAGE) - first_page).min(free_entries_at_end());
    if pages == 0 {
        warn!("native: no MMU entries free: no code plugins");
        return None;
    }
    let entry = ENTRIES - pages;
    let vaddr = LINK_BASE + entry * PAGE;
    // SAFETY: entries found free, mapped to the region only this module
    // writes.
    let set = unsafe { Cache_Dbus_MMU_Set(0, vaddr, first_page * PAGE, 64, pages, 0) };
    if set != 0 {
        warn!("native: the MMU did not map the region ({set})");
        return None;
    }
    // SAFETY: ROM functions on the addresses just mapped.
    unsafe {
        Cache_Invalidate_Addr(vaddr, pages * PAGE);
        Cache_Invalidate_Addr(vaddr + CODE_OFFSET, pages * PAGE);
    }
    let end = end.min((first_page + pages) * PAGE);
    let base = vaddr + (start - first_page * PAGE);
    info!(
        "native: the region {start:#x}..{end:#x} ({} KB) mapped at {base:#x}, MMU entries {entry}..{ENTRIES}",
        (end - start) / 1024
    );
    Region::new(start, end, base).map(|region| (region, firmware))
}

/// How many MMU entries at the end of the table map nothing.
fn free_entries_at_end() -> u32 {
    let table = MMU_TABLE::regs();
    (0..ENTRIES)
        .rev()
        .take_while(|&i| table.entry(i as usize).read().invalid().bit())
        .count() as u32
}

/// The index at the region's start, if one is there.
fn read_index(flash: &mut FlashStorage<'static>, start: u32) -> Option<Index> {
    let mut head = [0u8; 8];
    flash.read(start, &mut head).ok()?;
    let count = usize::from(u16::from_le_bytes([head[6], head[7]]));
    let len = (72 + count * 48).min(SECTOR as usize).next_multiple_of(4);
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(len).ok()?;
    bytes.resize(len, 0);
    flash.read(start, &mut bytes).ok()?;
    Index::read(&bytes)
}
