//! The microSD card's blocks (read and written by [`paperpaw_x4::card`])
//! for PaperPaw's FAT storage.

use alloc::boxed::Box;

use embedded_hal::delay::DelayNs;
use embedded_hal::digital::OutputPin;
use embedded_hal::spi::SpiBus;
use esp_hal::time::Rate;
use log::{info, warn};
use paperpaw_fat::{BLOCK, Blocks, DiskError};
use paperpaw_platform::{NoStorage, Storage};

use crate::board::{self, CardParts};

/// The card's FAT file system as PaperPaw's storage, then the bus at full
/// speed; without a card or one PaperPaw cannot read, no storage.
pub fn mount(parts: CardParts) -> Box<dyn Storage> {
    let CardParts {
        bus,
        spi,
        select,
        delay,
    } = parts;
    // A card wants clocks with nothing selected before it starts.
    let _ = bus.borrow_mut().write(&[0xFF; 10]);
    let sd = match paperpaw_x4::card::Card::new(bus, select, delay) {
        Ok(sd) => sd,
        Err(e) => {
            warn!("SD card: {e:?}");
            return Box::new(NoStorage);
        }
    };
    let _ = bus
        .borrow_mut()
        .apply_config(&spi.with_frequency(Rate::from_mhz(board::SPI_MHZ)));
    info!("SD card: {} MB", sd.blocks() >> 11);
    crate::volume::mount(Card(sd))
}

/// The card, as blocks.
pub struct Card<'a, BUS, CS, D>(pub paperpaw_x4::card::Card<'a, BUS, CS, D>);

impl<BUS: SpiBus, CS: OutputPin, D: DelayNs> Blocks for Card<'_, BUS, CS, D> {
    fn read_block(&mut self, index: u32, block: &mut [u8; BLOCK]) -> Result<(), DiskError> {
        self.read_blocks(index, block)
    }

    fn write_block(&mut self, index: u32, block: &[u8; BLOCK]) -> Result<(), DiskError> {
        self.write_blocks(index, block)
    }

    fn read_blocks(&mut self, index: u32, data: &mut [u8]) -> Result<(), DiskError> {
        // Long work on the card must not look hung to the watchdogs.
        crate::power::working();
        let blocks = data.as_chunks_mut::<BLOCK>().0;
        #[cfg(feature = "remote")]
        crate::traced::blocks(index, blocks.len() as u32, 0);
        self.0.read(index, blocks).map_err(|_| DiskError)
    }

    fn write_blocks(&mut self, index: u32, data: &[u8]) -> Result<(), DiskError> {
        crate::power::working();
        let blocks = data.as_chunks::<BLOCK>().0;
        #[cfg(feature = "remote")]
        crate::traced::blocks(index, 0, blocks.len() as u32);
        self.0.write(index, blocks).map_err(|_| DiskError)
    }

    fn blocks(&mut self) -> Result<u32, DiskError> {
        Ok(self.0.blocks())
    }
}
