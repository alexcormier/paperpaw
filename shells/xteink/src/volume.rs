//! The SD card's FAT file system as PaperPaw's storage, on any card's
//! blocks (the X4's on SPI, the ESP32-S3 devices' on SDMMC).

use alloc::boxed::Box;

use log::warn;
use paperpaw_core::CardTrouble;
use paperpaw_fat::fatfs::{FileSystem, FsOptions};
use paperpaw_fat::{BlockDisk, Blocks, FatStorage, SharedDisk, VolumeError};
use paperpaw_platform::Storage;

/// The FAT file system on `blocks`; without one PaperPaw can read, why.
pub fn mount<B: Blocks + 'static>(blocks: B) -> Result<Box<dyn Storage>, CardTrouble> {
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
        Ok((fs, disk)) => Ok(Box::new(
            FatStorage::new(Box::leak(Box::new(fs))).with_disk(disk),
        )),
        Err(e) => Err(match e {
            VolumeError::ExFat => {
                warn!("SD card: exFAT; PaperPaw reads FAT32");
                CardTrouble::ExFat
            }
            VolumeError::NoFat => {
                warn!("SD card: no FAT file system");
                CardTrouble::NoFat
            }
            VolumeError::Disk => {
                warn!("SD card: not readable");
                CardTrouble::Unreadable
            }
        }),
    }
}
