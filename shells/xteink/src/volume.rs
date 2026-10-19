//! The SD card's FAT file system as PaperPaw's storage, on any card's
//! blocks (the X4's on SPI, the X4 Pro's on SDMMC).

use alloc::boxed::Box;

use log::warn;
use paperpaw_fat::fatfs::{FileSystem, FsOptions};
use paperpaw_fat::{BlockDisk, Blocks, FatStorage, SharedDisk, VolumeError};
use paperpaw_platform::{NoStorage, Storage};

/// The FAT file system on `blocks`; without one PaperPaw can read, no
/// storage.
pub fn mount<B: Blocks + 'static>(blocks: B) -> Box<dyn Storage> {
    // The disk shared with the storage, which writes back its changed
    // blocks (folders, the FAT) on sync.
    let fs = BlockDisk::volume(blocks)
        .map(SharedDisk::new)
        .and_then(|disk| {
            FileSystem::new(disk.clone(), FsOptions::new())
                .map(|fs| (fs, disk))
                .map_err(|_| VolumeError::NoFat)
        });
    match fs {
        Ok((fs, disk)) => Box::new(FatStorage::new(Box::leak(Box::new(fs))).with_disk(disk)),
        Err(e) => {
            match e {
                VolumeError::ExFat => warn!("SD card: exFAT; PaperPaw reads FAT32"),
                VolumeError::NoFat => warn!("SD card: no FAT file system"),
                VolumeError::Disk => warn!("SD card: not readable"),
            }
            Box::new(NoStorage)
        }
    }
}
