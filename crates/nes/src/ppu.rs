//! The RP2C02 PPU, as far as the machine needs it today: its position in the
//! frame and its object memory.
//!
//! Timing (NESdev wiki, "PPU rendering", revision 23300): 262 scanlines of
//! 341 dots. The pre-render line's shorter odd frames apply only while
//! rendering is enabled, which comes with the picture (bead `nes-a3h.4`), as
//! do the other registers; until then `$2000–$2007` read as open bus except
//! `$2004`, and accept writes to `$2003` and `$2004` only (NESdev wiki, "PPU
//! registers", revision 24601).

/// Dots per scanline.
pub const DOTS: u16 = 341;
/// Scanlines per frame (NTSC).
pub const SCANLINES: u16 = 262;

/// The PPU.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ppu {
    scanline: u16,
    dot: u16,
    frame: u64,
    oam: [u8; 256],
    oam_address: u8,
}

impl Default for Ppu {
    fn default() -> Self {
        Ppu::new()
    }
}

impl Ppu {
    /// A PPU at the start of a frame.
    pub fn new() -> Ppu {
        Ppu {
            scanline: 0,
            dot: 0,
            frame: 0,
            oam: [0; 256],
            oam_address: 0,
        }
    }

    /// Advance one dot.
    pub fn tick(&mut self) {
        self.dot += 1;
        if self.dot == DOTS {
            self.dot = 0;
            self.scanline += 1;
            if self.scanline == SCANLINES {
                self.scanline = 0;
                self.frame += 1;
            }
        }
    }

    /// Scanline and dot the next tick starts from.
    pub fn position(&self) -> (u16, u16) {
        (self.scanline, self.dot)
    }

    /// Frames completed since power-on.
    pub fn frame(&self) -> u64 {
        self.frame
    }

    /// Object attribute memory.
    pub fn oam(&self) -> &[u8; 256] {
        &self.oam
    }

    /// A CPU read of register `index` (`address & 7`).
    pub fn read_register(&mut self, index: u16, open_bus: u8) -> u8 {
        match index {
            4 => self.oam[usize::from(self.oam_address)],
            _ => open_bus,
        }
    }

    /// A CPU write of register `index` (`address & 7`).
    pub fn write_register(&mut self, index: u16, value: u8) {
        match index {
            3 => self.oam_address = value,
            4 => {
                self.oam[usize::from(self.oam_address)] = value;
                self.oam_address = self.oam_address.wrapping_add(1);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frame_is_262_lines_of_341_dots() {
        let mut ppu = Ppu::new();
        for _ in 0..u32::from(DOTS) * u32::from(SCANLINES) - 1 {
            ppu.tick();
        }
        assert_eq!(ppu.position(), (261, 340));
        ppu.tick();
        assert_eq!((ppu.position(), ppu.frame()), ((0, 0), 1));
    }

    #[test]
    fn oam_data_writes_advance_the_address() {
        let mut ppu = Ppu::new();
        ppu.write_register(3, 0xFE);
        ppu.write_register(4, 0x11);
        ppu.write_register(4, 0x22);
        ppu.write_register(4, 0x33);
        assert_eq!(
            (ppu.oam()[0xFE], ppu.oam()[0xFF], ppu.oam()[0x00]),
            (0x11, 0x22, 0x33)
        );
    }
}
