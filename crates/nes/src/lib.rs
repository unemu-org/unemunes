//! `nes`: the Nintendo Entertainment System over the `x65` processor.
//!
//! Cartridges and their boards, the PPU, the APU, the controllers and the
//! console's bus and clock (design note `backlog/docs/design/nes-core.md`
//! §5 to §8). Today: iNES and NES 2.0 images, the NROM board, the memory map
//! with open bus, the standard controllers, OAM DMA, and the PPU's timing and
//! object memory; the picture and the sound follow (beads `nes-a3h.4` and
//! `nes-a3h.5`).
//!
//! The crate touches nothing of the host (decision-41 (unemupc)): images
//! arrive as bytes, and time is CPU and master-clock cycles.

#![forbid(unsafe_code)]

pub mod cartridge;
pub mod controller;
pub mod machine;
pub mod mapper;
pub mod ppu;

pub use crate::cartridge::{Cartridge, CartridgeError, Format, Mirroring, Region};
pub use crate::controller::{Buttons, Controller};
pub use crate::machine::{Config, Nes};
pub use crate::mapper::{Mapper, Nrom, UnsupportedBoard};
pub use crate::ppu::Ppu;
