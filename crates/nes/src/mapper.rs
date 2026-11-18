//! Cartridge boards behind one trait.
//!
//! A board sees the CPU's accesses at `$4020–$FFFF` and the PPU's pattern
//! table accesses at `$0000–$1FFF`, and decides the nametable arrangement
//! (NESdev wiki, "CPU memory map", revision 24447; "Mapper"). Only NROM is
//! modelled; any other mapper is refused by number.

use std::fmt;

use crate::cartridge::{Cartridge, Mirroring};

/// A cartridge board.
pub trait Mapper {
    /// A CPU read at `$4020–$FFFF`; `None` where the board drives nothing, so
    /// the read sees open bus.
    fn cpu_read(&mut self, address: u16) -> Option<u8>;
    /// A CPU write at `$4020–$FFFF`.
    fn cpu_write(&mut self, address: u16, value: u8);
    /// A PPU read at `$0000–$1FFF`.
    fn ppu_read(&mut self, address: u16) -> u8;
    /// A PPU write at `$0000–$1FFF`; only CHR RAM keeps it.
    fn ppu_write(&mut self, address: u16, value: u8);
    /// The nametable arrangement now.
    fn mirroring(&self) -> Mirroring;
}

/// A board this core does not model.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnsupportedBoard {
    pub mapper: u16,
    pub submapper: u8,
}

impl fmt::Display for UnsupportedBoard {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "mapper {} (submapper {}) is not supported yet; this core runs mapper 0 (NROM)",
            self.mapper, self.submapper
        )
    }
}

impl std::error::Error for UnsupportedBoard {}

/// The board a cartridge needs.
pub fn board(cartridge: &Cartridge) -> Result<Box<dyn Mapper>, UnsupportedBoard> {
    match cartridge.mapper {
        0 => Ok(Box::new(Nrom::new(cartridge))),
        mapper => Err(UnsupportedBoard {
            mapper,
            submapper: cartridge.submapper,
        }),
    }
}

/// NROM (mapper 0): 16 or 32 KiB of PRG ROM at `$8000`, the 16 KiB form
/// mirrored at `$C000`; 8 KiB of CHR ROM or CHR RAM; nametables fixed by the
/// header (NESdev wiki, "NROM", revision 23660).
///
/// PRG RAM at `$6000–$7FFF`, mirrored through the window, exists only when
/// the header says so: a NES 2.0 PRG RAM size, or the battery flag of an
/// iNES header ("Most emulators will map 8KiB of PRG-RAM ... if the battery
/// flag is set").
#[derive(Clone, Debug)]
pub struct Nrom {
    prg_rom: Vec<u8>,
    prg_ram: Vec<u8>,
    chr: Vec<u8>,
    chr_is_ram: bool,
    mirroring: Mirroring,
}

const CHR_RAM: usize = 8 * 1024;
const PRG_RAM_WINDOW: usize = 8 * 1024;

impl Nrom {
    pub fn new(cartridge: &Cartridge) -> Nrom {
        let ram = match cartridge.prg_ram {
            Some(size) => size.min(PRG_RAM_WINDOW),
            None if cartridge.battery => PRG_RAM_WINDOW,
            None => 0,
        };
        let chr_is_ram = cartridge.chr_rom.is_empty();
        Nrom {
            prg_rom: cartridge.prg_rom.clone(),
            prg_ram: vec![0; ram],
            chr: if chr_is_ram {
                vec![0; CHR_RAM]
            } else {
                cartridge.chr_rom.clone()
            },
            chr_is_ram,
            mirroring: cartridge.mirroring,
        }
    }
}

impl Mapper for Nrom {
    fn cpu_read(&mut self, address: u16) -> Option<u8> {
        match address {
            0x6000..=0x7FFF if !self.prg_ram.is_empty() => {
                Some(self.prg_ram[usize::from(address - 0x6000) % self.prg_ram.len()])
            }
            // A hand-built cartridge may have no PRG ROM; `from_ines` refuses
            // such an image, and the board answers nothing for it.
            0x8000..=0xFFFF if !self.prg_rom.is_empty() => {
                Some(self.prg_rom[usize::from(address - 0x8000) % self.prg_rom.len()])
            }
            _ => None,
        }
    }

    fn cpu_write(&mut self, address: u16, value: u8) {
        if (0x6000..=0x7FFF).contains(&address) && !self.prg_ram.is_empty() {
            let len = self.prg_ram.len();
            self.prg_ram[usize::from(address - 0x6000) % len] = value;
        }
    }

    fn ppu_read(&mut self, address: u16) -> u8 {
        self.chr[usize::from(address & 0x1FFF) % self.chr.len()]
    }

    fn ppu_write(&mut self, address: u16, value: u8) {
        if self.chr_is_ram {
            self.chr[usize::from(address & 0x1FFF)] = value;
        }
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cartridge::{Format, Region};

    fn cart(prg: usize, chr: usize, battery: bool, prg_ram: Option<usize>) -> Cartridge {
        Cartridge {
            format: Format::Ines,
            mapper: 0,
            submapper: 0,
            mirroring: Mirroring::Vertical,
            battery,
            prg_ram,
            region: Region::Ntsc,
            prg_rom: (0..prg).map(|i| (i >> 8) as u8).collect(),
            chr_rom: (0..chr).map(|i| i as u8).collect(),
        }
    }

    #[test]
    fn nrom_128_mirrors_its_prg_rom_at_c000() {
        let mut board = Nrom::new(&cart(0x4000, 0x2000, false, None));
        assert_eq!(board.cpu_read(0x8100), Some(0x01));
        assert_eq!(board.cpu_read(0xC100), Some(0x01));
        assert_eq!(board.cpu_read(0xFFFF), Some(0x3F));
    }

    #[test]
    fn nrom_256_maps_32_kib() {
        let mut board = Nrom::new(&cart(0x8000, 0x2000, false, None));
        assert_eq!(board.cpu_read(0xC100), Some(0x41));
    }

    #[test]
    fn prg_ram_exists_only_when_the_header_says_so() {
        let mut none = Nrom::new(&cart(0x4000, 0x2000, false, None));
        none.cpu_write(0x6000, 0x12);
        assert_eq!(none.cpu_read(0x6000), None, "open bus");

        let mut battery = Nrom::new(&cart(0x4000, 0x2000, true, None));
        battery.cpu_write(0x6001, 0x34);
        assert_eq!(battery.cpu_read(0x6001), Some(0x34));

        let mut small = Nrom::new(&cart(0x4000, 0x2000, false, Some(2048)));
        small.cpu_write(0x6002, 0x56);
        assert_eq!(
            small.cpu_read(0x6802),
            Some(0x56),
            "2 KiB mirrored in the window"
        );
    }

    #[test]
    fn chr_rom_ignores_writes_and_chr_ram_keeps_them() {
        let mut rom = Nrom::new(&cart(0x4000, 0x2000, false, None));
        rom.ppu_write(0x0010, 0xAA);
        assert_eq!(rom.ppu_read(0x0010), 0x10);
        let mut ram = Nrom::new(&cart(0x4000, 0, false, None));
        ram.ppu_write(0x1FFF, 0xAA);
        assert_eq!(ram.ppu_read(0x1FFF), 0xAA);
    }

    #[test]
    fn other_mappers_are_refused_by_number() {
        let mut c = cart(0x4000, 0x2000, false, None);
        c.mapper = 4;
        let error = board(&c).err().expect("refused");
        assert_eq!(error.mapper, 4);
        assert!(error.to_string().contains("mapper 4"));
    }
}
