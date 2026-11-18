//! The RP2C02 PPU, simple enough for NROM games: registers, the scroll
//! registers, background and sprites, sprite-0 hit, vertical blank and NMI.
//!
//! Sources, NESdev wiki: "PPU registers" (revision 24601), "PPU scrolling"
//! (23139), "PPU rendering" (23300), "PPU frame timing" (24376), "PPU sprite
//! evaluation" (22442), "PPU OAM" (23161), "PPU palettes" (24257), "PPU memory
//! map" (22765), "Mirroring" (24072), "PPU attribute tables" (21522).
//!
//! Simplified, for milestone N2a: the four memory fetches of a tile happen at
//! once on the tile's last dot, so a board that watches PPU addresses (MMC3)
//! would see the wrong timing; sprite evaluation happens at once on dot 257
//! and sets the overflow flag without the hardware's diagonal bug; reads and
//! writes of `$2007` during rendering, the warm-up after power-on, the decay
//! of the I/O latch and the `$2002` race at the start of vertical blank are
//! not modelled; `$2004` writes during rendering are ignored without the
//! address glitch they cause. Four-screen boards get the PPU's own 4 KiB here; their VRAM
//! belongs on the board (N2c).

use crate::cartridge::Mirroring;
use crate::mapper::Mapper;

/// Dots per scanline.
pub const DOTS: u16 = 341;
/// Scanlines per frame (NTSC).
pub const SCANLINES: u16 = 262;
/// Picture width in pixels.
pub const WIDTH: usize = 256;
/// Picture height in pixels.
pub const HEIGHT: usize = 240;

const PRE_RENDER: u16 = 261;
const VBLANK_LINE: u16 = 241;

// PPUCTRL ($2000).
const CTRL_INCREMENT_32: u8 = 0x04;
const CTRL_SPRITE_TABLE: u8 = 0x08;
const CTRL_BACKGROUND_TABLE: u8 = 0x10;
const CTRL_SPRITE_8X16: u8 = 0x20;
const CTRL_NMI: u8 = 0x80;
// PPUMASK ($2001).
const MASK_GREYSCALE: u8 = 0x01;
const MASK_BACKGROUND_LEFT: u8 = 0x02;
const MASK_SPRITES_LEFT: u8 = 0x04;
const MASK_BACKGROUND: u8 = 0x08;
const MASK_SPRITES: u8 = 0x10;
// PPUSTATUS ($2002).
const STATUS_OVERFLOW: u8 = 0x20;
const STATUS_SPRITE_0: u8 = 0x40;
const STATUS_VBLANK: u8 = 0x80;

/// A sprite chosen for the next scanline, with its pattern row fetched.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Sprite {
    x: u8,
    attributes: u8,
    low: u8,
    high: u8,
    zero: bool,
}

/// The PPU.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ppu {
    scanline: u16,
    dot: u16,
    frame: u64,

    ctrl: u8,
    mask: u8,
    status: u8,
    /// The value last driven on the PPU's CPU-side data bus.
    latch: u8,
    read_buffer: u8,

    // "PPU scrolling": current and temporary VRAM address, fine X, toggle.
    v: u16,
    t: u16,
    fine_x: u8,
    w: bool,

    oam: [u8; 256],
    oam_address: u8,
    vram: [u8; 4096],
    palette: [u8; 32],

    // Background pipeline: two tiles of pattern and palette bits.
    pattern_low: u16,
    pattern_high: u16,
    attribute_low: u16,
    attribute_high: u16,

    sprites: [Sprite; 8],
    sprite_count: usize,

    /// The picture being drawn and the last one finished: palette value in
    /// bits 0–5, emphasis in bits 6–8.
    drawing: Vec<u16>,
    finished: Vec<u16>,
    frame_done: bool,
}

impl Default for Ppu {
    fn default() -> Self {
        Ppu::new()
    }
}

impl Ppu {
    /// A PPU at the start of a frame with its registers clear.
    pub fn new() -> Ppu {
        Ppu {
            scanline: 0,
            dot: 0,
            frame: 0,
            ctrl: 0,
            mask: 0,
            status: 0,
            latch: 0,
            read_buffer: 0,
            v: 0,
            t: 0,
            fine_x: 0,
            w: false,
            oam: [0; 256],
            oam_address: 0,
            vram: [0; 4096],
            palette: [0; 32],
            pattern_low: 0,
            pattern_high: 0,
            attribute_low: 0,
            attribute_high: 0,
            sprites: [Sprite::default(); 8],
            sprite_count: 0,
            drawing: vec![0; WIDTH * HEIGHT],
            finished: vec![0; WIDTH * HEIGHT],
            frame_done: false,
        }
    }

    /// Scanline and dot the next tick works on.
    pub fn position(&self) -> (u16, u16) {
        (self.scanline, self.dot)
    }

    /// Frames started since power-on.
    pub fn frame(&self) -> u64 {
        self.frame
    }

    /// Object attribute memory.
    pub fn oam(&self) -> &[u8; 256] {
        &self.oam
    }

    /// The NMI output: asserted while vertical blank is flagged and PPUCTRL
    /// enables NMI ("PPU registers", PPUCTRL bit 7).
    pub fn nmi(&self) -> bool {
        self.status & STATUS_VBLANK != 0 && self.ctrl & CTRL_NMI != 0
    }

    /// The last finished picture, `WIDTH × HEIGHT` values, rows top to
    /// bottom: palette value in bits 0–5, emphasis in bits 6–8.
    pub fn picture(&self) -> &[u16] {
        &self.finished
    }

    /// Whether a picture was finished since the last call.
    pub fn take_frame_done(&mut self) -> bool {
        std::mem::take(&mut self.frame_done)
    }

    fn rendering(&self) -> bool {
        self.mask & (MASK_BACKGROUND | MASK_SPRITES) != 0
    }

    /// Rendering is on and the PPU is on a visible or the pre-render line.
    fn rendering_line(&self) -> bool {
        self.rendering() && (self.scanline < HEIGHT as u16 || self.scanline == PRE_RENDER)
    }

    // ----- PPU memory ("PPU memory map", "Mirroring", "PPU palettes") -----

    fn nametable_index(address: u16, mirroring: Mirroring) -> usize {
        let a = usize::from(address & 0x0FFF);
        let table = a / 0x400;
        let physical = match mirroring {
            Mirroring::Horizontal => [0, 0, 1, 1][table],
            Mirroring::Vertical => [0, 1, 0, 1][table],
            _ => table,
        };
        physical * 0x400 + a % 0x400
    }

    fn palette_index(address: u16) -> usize {
        let i = usize::from(address & 0x1F);
        // $3F10, $3F14, $3F18 and $3F1C are $3F00, $3F04, $3F08 and $3F0C.
        if i & 0x13 == 0x10 {
            i & !0x10
        } else {
            i
        }
    }

    fn read_memory(&mut self, mapper: &mut dyn Mapper, address: u16) -> u8 {
        let address = address & 0x3FFF;
        match address {
            0x0000..=0x1FFF => mapper.ppu_read(address),
            0x2000..=0x3EFF => self.vram[Self::nametable_index(address, mapper.mirroring())],
            _ => self.palette[Self::palette_index(address)],
        }
    }

    fn write_memory(&mut self, mapper: &mut dyn Mapper, address: u16, value: u8) {
        let address = address & 0x3FFF;
        match address {
            0x0000..=0x1FFF => mapper.ppu_write(address, value),
            0x2000..=0x3EFF => {
                self.vram[Self::nametable_index(address, mapper.mirroring())] = value;
            }
            _ => self.palette[Self::palette_index(address)] = value & 0x3F,
        }
    }

    // ----- CPU-side registers ("PPU registers") -----

    /// A CPU read of register `index` (`address & 7`).
    pub fn read_register(&mut self, index: u16, mapper: &mut dyn Mapper) -> u8 {
        let value = match index {
            2 => {
                let value = self.status & 0xE0 | self.latch & 0x1F;
                self.status &= !STATUS_VBLANK;
                self.w = false;
                value
            }
            4 => self.oam[usize::from(self.oam_address)],
            7 => {
                let address = self.v & 0x3FFF;
                let value = if address >= 0x3F00 {
                    // Palette reads are not buffered; the buffer takes the
                    // nametable byte underneath, and bits 7-6 come from the
                    // latch.
                    self.read_buffer = self.read_memory(mapper, address - 0x1000);
                    let colour = self.read_memory(mapper, address);
                    let colour = if self.mask & MASK_GREYSCALE != 0 {
                        colour & 0x30
                    } else {
                        colour
                    };
                    colour | self.latch & 0xC0
                } else {
                    let buffered = self.read_buffer;
                    self.read_buffer = self.read_memory(mapper, address);
                    buffered
                };
                self.increment_v();
                value
            }
            _ => self.latch,
        };
        self.latch = value;
        value
    }

    /// A CPU write of register `index` (`address & 7`).
    pub fn write_register(&mut self, index: u16, value: u8, mapper: &mut dyn Mapper) {
        self.latch = value;
        match index {
            0 => {
                self.ctrl = value;
                self.t = self.t & !0x0C00 | u16::from(value & 3) << 10;
            }
            1 => self.mask = value,
            3 => self.oam_address = value,
            // Writes during rendering do not reach OAM ("PPU registers",
            // OAMDATA); the address glitch they cause is N2a's.
            4 if self.rendering_line() => {}
            4 => {
                // Byte 2's bits 2-4 do not exist ("PPU OAM").
                let value = if self.oam_address & 3 == 2 {
                    value & 0xE3
                } else {
                    value
                };
                self.oam[usize::from(self.oam_address)] = value;
                self.oam_address = self.oam_address.wrapping_add(1);
            }
            5 => {
                if self.w {
                    self.t = self.t & !0x73E0
                        | u16::from(value & 7) << 12
                        | u16::from(value & 0xF8) << 2;
                } else {
                    self.t = self.t & !0x001F | u16::from(value >> 3);
                    self.fine_x = value & 7;
                }
                self.w = !self.w;
            }
            6 => {
                if self.w {
                    self.t = self.t & 0xFF00 | u16::from(value);
                    self.v = self.t;
                } else {
                    self.t = self.t & 0x00FF | u16::from(value & 0x3F) << 8;
                }
                self.w = !self.w;
            }
            7 => {
                self.write_memory(mapper, self.v, value);
                self.increment_v();
            }
            _ => {}
        }
    }

    fn increment_v(&mut self) {
        let step = if self.ctrl & CTRL_INCREMENT_32 != 0 {
            32
        } else {
            1
        };
        self.v = self.v.wrapping_add(step) & 0x7FFF;
    }

    // ----- scroll register steps ("PPU scrolling") -----

    fn increment_coarse_x(&mut self) {
        if self.v & 0x001F == 31 {
            self.v &= !0x001F;
            self.v ^= 0x0400;
        } else {
            self.v += 1;
        }
    }

    fn increment_y(&mut self) {
        if self.v & 0x7000 != 0x7000 {
            self.v += 0x1000;
            return;
        }
        self.v &= !0x7000;
        let mut y = (self.v & 0x03E0) >> 5;
        if y == 29 {
            y = 0;
            self.v ^= 0x0800;
        } else if y == 31 {
            y = 0;
        } else {
            y += 1;
        }
        self.v = self.v & !0x03E0 | y << 5;
    }

    fn copy_horizontal(&mut self) {
        self.v = self.v & !0x041F | self.t & 0x041F;
    }

    fn copy_vertical(&mut self) {
        self.v = self.v & !0x7BE0 | self.t & 0x7BE0;
    }

    // ----- the dot -----

    /// Work one dot, then move to the next.
    pub fn tick(&mut self, mapper: &mut dyn Mapper) {
        let (line, dot) = (self.scanline, self.dot);
        let visible = line < HEIGHT as u16;

        if line == VBLANK_LINE && dot == 1 {
            self.status |= STATUS_VBLANK;
        }
        if line == PRE_RENDER && dot == 1 {
            self.status &= !(STATUS_VBLANK | STATUS_SPRITE_0 | STATUS_OVERFLOW);
        }

        if self.rendering() && (visible || line == PRE_RENDER) {
            if visible && (1..=256).contains(&dot) {
                self.draw(usize::from(dot - 1), usize::from(line));
            }
            if (1..=256).contains(&dot) || (321..=336).contains(&dot) {
                self.shift();
                if dot % 8 == 0 {
                    self.fetch_tile(mapper);
                    self.increment_coarse_x();
                }
            }
            if dot == 256 {
                self.increment_y();
            }
            if dot == 257 {
                self.copy_horizontal();
                if visible {
                    self.evaluate_sprites(mapper, line);
                } else {
                    self.sprite_count = 0;
                }
            }
            if line == PRE_RENDER && (280..=304).contains(&dot) {
                self.copy_vertical();
            }
            // "PPU registers", OAMADDR: set to 0 on dots 257-320.
            if (257..=320).contains(&dot) {
                self.oam_address = 0;
            }
        } else if visible && (1..=256).contains(&dot) {
            // With rendering off the backdrop is $3F00, or the palette entry
            // v points at when v is in $3F00-$3FFF ("PPU palettes").
            let entry = if self.v & 0x3F00 == 0x3F00 {
                Self::palette_index(self.v)
            } else {
                0
            };
            let backdrop = self.colour(self.palette[entry]);
            self.drawing[usize::from(line) * WIDTH + usize::from(dot - 1)] = backdrop;
        }

        // Advance. With rendering on, the pre-render line of an odd frame
        // skips its last dot, "jumping directly from (339,261) to (0,0)".
        let skip = line == PRE_RENDER && dot == 339 && self.frame % 2 == 1 && self.rendering();
        self.dot += 1;
        if self.dot == DOTS || skip {
            self.dot = 0;
            self.scanline += 1;
            if self.scanline == HEIGHT as u16 {
                std::mem::swap(&mut self.drawing, &mut self.finished);
                self.frame_done = true;
            }
            if self.scanline == SCANLINES {
                self.scanline = 0;
                self.frame += 1;
            }
        }
    }

    fn shift(&mut self) {
        self.pattern_low <<= 1;
        self.pattern_high <<= 1;
        self.attribute_low <<= 1;
        self.attribute_high <<= 1;
    }

    /// The tile at `v`: its nametable byte, attribute bits and pattern row,
    /// into the low byte of the shifters ("PPU rendering", "PPU attribute
    /// tables").
    fn fetch_tile(&mut self, mapper: &mut dyn Mapper) {
        let v = self.v;
        let tile = self.read_memory(mapper, 0x2000 | v & 0x0FFF);
        let attribute_address = 0x23C0 | v & 0x0C00 | (v >> 4) & 0x38 | (v >> 2) & 0x07;
        let shift = (v >> 4) & 4 | v & 2;
        let palette = self.read_memory(mapper, attribute_address) >> shift & 3;
        let table = if self.ctrl & CTRL_BACKGROUND_TABLE != 0 {
            0x1000
        } else {
            0
        };
        let row = table | u16::from(tile) << 4 | (v >> 12) & 7;
        let low = self.read_memory(mapper, row);
        let high = self.read_memory(mapper, row + 8);
        let fill = |bit: u8| if palette & bit != 0 { 0x00FF } else { 0 };
        self.pattern_low = self.pattern_low & 0xFF00 | u16::from(low);
        self.pattern_high = self.pattern_high & 0xFF00 | u16::from(high);
        self.attribute_low = self.attribute_low & 0xFF00 | fill(1);
        self.attribute_high = self.attribute_high & 0xFF00 | fill(2);
    }

    /// Choose the sprites of the next scanline and fetch their pattern rows
    /// ("PPU sprite evaluation": OAM order, at most 8; a sprite whose Y is
    /// `y` shows on scanlines `y + 1` onwards).
    fn evaluate_sprites(&mut self, mapper: &mut dyn Mapper, line: u16) {
        let height: u16 = if self.ctrl & CTRL_SPRITE_8X16 != 0 {
            16
        } else {
            8
        };
        self.sprite_count = 0;
        for n in 0..64 {
            let (y, tile, attributes, x) = (
                self.oam[n * 4],
                self.oam[n * 4 + 1],
                self.oam[n * 4 + 2],
                self.oam[n * 4 + 3],
            );
            let row = line.wrapping_sub(u16::from(y));
            if row >= height {
                continue;
            }
            if self.sprite_count == 8 {
                self.status |= STATUS_OVERFLOW;
                break;
            }
            let row = if attributes & 0x80 != 0 {
                height - 1 - row
            } else {
                row
            };
            let address = if height == 16 {
                let table = u16::from(tile & 1) << 12;
                let tile = u16::from(tile & 0xFE) + row / 8;
                table | tile << 4 | row & 7
            } else {
                let table = if self.ctrl & CTRL_SPRITE_TABLE != 0 {
                    0x1000
                } else {
                    0
                };
                table | u16::from(tile) << 4 | row
            };
            let mut low = self.read_memory(mapper, address);
            let mut high = self.read_memory(mapper, address + 8);
            if attributes & 0x40 != 0 {
                low = low.reverse_bits();
                high = high.reverse_bits();
            }
            self.sprites[self.sprite_count] = Sprite {
                x,
                attributes,
                low,
                high,
                zero: n == 0,
            };
            self.sprite_count += 1;
        }
    }

    /// The pixel at `(x, y)`: background and sprite combined by priority
    /// ("PPU rendering").
    fn draw(&mut self, x: usize, y: usize) {
        let bit = 15 - u16::from(self.fine_x);
        let mut background = 0u8;
        if self.mask & MASK_BACKGROUND != 0 && (x >= 8 || self.mask & MASK_BACKGROUND_LEFT != 0) {
            let p = (self.pattern_low >> bit & 1) | (self.pattern_high >> bit & 1) << 1;
            let a = (self.attribute_low >> bit & 1) | (self.attribute_high >> bit & 1) << 1;
            if p != 0 {
                background = (a << 2 | p) as u8;
            }
        }

        let mut sprite = None;
        if self.mask & MASK_SPRITES != 0 && (x >= 8 || self.mask & MASK_SPRITES_LEFT != 0) {
            for s in &self.sprites[..self.sprite_count] {
                let dx = x.wrapping_sub(usize::from(s.x));
                if dx >= 8 {
                    continue;
                }
                let b = 7 - dx;
                let p = (s.low >> b & 1) | (s.high >> b & 1) << 1;
                if p != 0 {
                    sprite = Some((*s, p));
                    break;
                }
            }
        }

        let index = match sprite {
            Some((s, p)) => {
                if s.zero && background != 0 && x != 255 {
                    self.status |= STATUS_SPRITE_0;
                }
                if background == 0 || s.attributes & 0x20 == 0 {
                    0x10 | (s.attributes & 3) << 2 | p
                } else {
                    background
                }
            }
            None => background,
        };
        let colour = self.colour(self.palette[Self::palette_index(u16::from(index))]);
        self.drawing[y * WIDTH + x] = colour;
    }

    /// A palette value with greyscale and emphasis applied.
    fn colour(&self, value: u8) -> u16 {
        let value = if self.mask & MASK_GREYSCALE != 0 {
            value & 0x30
        } else {
            value & 0x3F
        };
        u16::from(value) | u16::from(self.mask >> 5) << 6
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A board with 8 KiB of CHR RAM and a fixed arrangement.
    struct Chr {
        chr: Vec<u8>,
        mirroring: Mirroring,
    }

    impl Mapper for Chr {
        fn cpu_read(&mut self, _: u16) -> Option<u8> {
            None
        }
        fn cpu_write(&mut self, _: u16, _: u8) {}
        fn ppu_read(&mut self, address: u16) -> u8 {
            self.chr[usize::from(address & 0x1FFF)]
        }
        fn ppu_write(&mut self, address: u16, value: u8) {
            self.chr[usize::from(address & 0x1FFF)] = value;
        }
        fn mirroring(&self) -> Mirroring {
            self.mirroring
        }
    }

    fn chr(mirroring: Mirroring) -> Chr {
        Chr {
            chr: vec![0; 0x2000],
            mirroring,
        }
    }

    fn write_vram(ppu: &mut Ppu, board: &mut Chr, address: u16, bytes: &[u8]) {
        ppu.write_register(6, (address >> 8) as u8, board);
        ppu.write_register(6, address as u8, board);
        for &b in bytes {
            ppu.write_register(7, b, board);
        }
    }

    fn run_frames(ppu: &mut Ppu, board: &mut Chr, frames: usize) {
        for _ in 0..frames {
            loop {
                ppu.tick(board);
                if ppu.take_frame_done() {
                    break;
                }
            }
        }
    }

    #[test]
    fn status_read_clears_vblank_and_the_toggle() {
        let mut board = chr(Mirroring::Horizontal);
        let mut ppu = Ppu::new();
        ppu.status = STATUS_VBLANK;
        ppu.write_register(5, 0x10, &mut board);
        assert!(ppu.w);
        assert_eq!(ppu.read_register(2, &mut board) & 0x80, 0x80);
        assert_eq!(ppu.read_register(2, &mut board) & 0x80, 0);
        assert!(!ppu.w);
    }

    #[test]
    fn scroll_and_address_writes_fill_t_v_and_fine_x() {
        // The worked example of "PPU scrolling" § Summary: $2000 = 0, $2005 =
        // $7D then $5E, $2006 = $3D then $F0.
        let mut board = chr(Mirroring::Horizontal);
        let mut ppu = Ppu::new();
        ppu.write_register(0, 0x00, &mut board);
        ppu.write_register(5, 0x7D, &mut board);
        assert_eq!((ppu.t, ppu.fine_x), (0x000F, 5));
        ppu.write_register(5, 0x5E, &mut board);
        assert_eq!(ppu.t, 0x616F);
        ppu.write_register(6, 0x3D, &mut board);
        assert_eq!(ppu.t, 0x3D6F);
        ppu.write_register(6, 0xF0, &mut board);
        assert_eq!((ppu.t, ppu.v), (0x3DF0, 0x3DF0));
    }

    #[test]
    fn data_reads_are_buffered_except_palette_reads() {
        let mut board = chr(Mirroring::Vertical);
        let mut ppu = Ppu::new();
        write_vram(&mut ppu, &mut board, 0x2000, &[0x11, 0x22]);
        write_vram(&mut ppu, &mut board, 0x3F00, &[0x0F]);
        ppu.write_register(6, 0x20, &mut board);
        ppu.write_register(6, 0x00, &mut board);
        ppu.read_register(7, &mut board); // the stale buffer
        assert_eq!(ppu.read_register(7, &mut board), 0x11);
        assert_eq!(ppu.read_register(7, &mut board), 0x22);
        ppu.write_register(6, 0x3F, &mut board);
        ppu.write_register(6, 0x00, &mut board);
        assert_eq!(
            ppu.read_register(7, &mut board) & 0x3F,
            0x0F,
            "not buffered"
        );
    }

    #[test]
    fn increment_32_steps_down_a_column() {
        let mut board = chr(Mirroring::Vertical);
        let mut ppu = Ppu::new();
        ppu.write_register(0, CTRL_INCREMENT_32, &mut board);
        write_vram(&mut ppu, &mut board, 0x2000, &[1, 2]);
        assert_eq!((ppu.vram[0], ppu.vram[32]), (1, 2));
    }

    #[test]
    fn palette_backdrop_mirrors_and_nametable_arrangements() {
        let mut board = chr(Mirroring::Horizontal);
        let mut ppu = Ppu::new();
        write_vram(&mut ppu, &mut board, 0x3F10, &[0x2A]);
        assert_eq!(ppu.palette[0], 0x2A, "$3F10 is $3F00");
        write_vram(&mut ppu, &mut board, 0x2400, &[0x55]);
        assert_eq!(ppu.vram[0], 0x55, "horizontal mirroring: $2400 is $2000");
        let mut vertical = chr(Mirroring::Vertical);
        write_vram(&mut ppu, &mut vertical, 0x2800, &[0x66]);
        assert_eq!(ppu.vram[0], 0x66, "vertical mirroring: $2800 is $2000");
    }

    #[test]
    fn greyscale_applies_to_palette_reads() {
        let mut board = chr(Mirroring::Horizontal);
        let mut ppu = Ppu::new();
        write_vram(&mut ppu, &mut board, 0x3F01, &[0x16]);
        ppu.write_register(1, MASK_GREYSCALE, &mut board);
        ppu.write_register(6, 0x3F, &mut board);
        ppu.write_register(6, 0x01, &mut board);
        assert_eq!(ppu.read_register(7, &mut board) & 0x3F, 0x10);
    }

    #[test]
    fn oam_address_is_cleared_while_rendering_and_writes_are_ignored() {
        let mut board = chr(Mirroring::Horizontal);
        let mut ppu = Ppu::new();
        ppu.write_register(3, 0x40, &mut board);
        ppu.write_register(1, MASK_SPRITES, &mut board);
        while ppu.position() != (10, 300) {
            ppu.tick(&mut board);
        }
        assert_eq!(ppu.oam_address, 0);
        ppu.write_register(4, 0x77, &mut board);
        assert_eq!((ppu.oam()[0], ppu.oam_address), (0, 0));
    }

    #[test]
    fn backdrop_follows_v_into_the_palette_with_rendering_off() {
        let mut board = chr(Mirroring::Horizontal);
        let mut ppu = Ppu::new();
        write_vram(&mut ppu, &mut board, 0x3F00, &[0x0F, 0x21]);
        ppu.write_register(6, 0x3F, &mut board);
        ppu.write_register(6, 0x01, &mut board);
        run_frames(&mut ppu, &mut board, 1);
        assert_eq!(ppu.picture()[0], 0x21);
    }

    #[test]
    fn oam_byte_2_keeps_only_its_implemented_bits() {
        let mut board = chr(Mirroring::Horizontal);
        let mut ppu = Ppu::new();
        ppu.write_register(3, 2, &mut board);
        ppu.write_register(4, 0xFF, &mut board);
        assert_eq!(ppu.oam()[2], 0xE3);
    }

    #[test]
    fn vblank_and_nmi_follow_the_frame() {
        let mut board = chr(Mirroring::Horizontal);
        let mut ppu = Ppu::new();
        ppu.write_register(0, CTRL_NMI, &mut board);
        while ppu.position() != (241, 2) {
            assert!(!ppu.nmi());
            ppu.tick(&mut board);
        }
        assert!(ppu.nmi(), "set at scanline 241, dot 1");
        while ppu.position() != (261, 2) {
            ppu.tick(&mut board);
        }
        assert!(!ppu.nmi(), "cleared at the pre-render line, dot 1");
    }

    #[test]
    fn odd_frames_are_one_dot_shorter_while_rendering() {
        let mut board = chr(Mirroring::Horizontal);
        let mut ppu = Ppu::new();
        ppu.write_register(1, MASK_BACKGROUND, &mut board);
        let mut lengths = Vec::new();
        for _ in 0..4 {
            let frame = ppu.frame();
            let mut dots = 0u32;
            while ppu.frame() == frame {
                ppu.tick(&mut board);
                dots += 1;
            }
            lengths.push(dots);
        }
        // "PPU frame timing": 89342 dots, 89341 on odd frames.
        assert_eq!(lengths, [89342, 89341, 89342, 89341]);
    }

    /// Tile 1 is solid colour 1; the nametable has tile 1 in column 0 only.
    fn striped(ppu: &mut Ppu, board: &mut Chr) {
        for row in 0..8 {
            board.chr[16 + row] = 0xFF;
        }
        for y in 0..30u16 {
            write_vram(ppu, board, 0x2000 + y * 32, &[1]);
        }
        write_vram(ppu, board, 0x3F00, &[0x0F, 0x16]);
    }

    #[test]
    fn background_draws_the_nametable_and_scrolls_by_fine_x() {
        let mut board = chr(Mirroring::Vertical);
        let mut ppu = Ppu::new();
        striped(&mut ppu, &mut board);
        ppu.write_register(1, MASK_BACKGROUND | MASK_BACKGROUND_LEFT, &mut board);
        ppu.write_register(6, 0, &mut board);
        ppu.write_register(6, 0, &mut board);
        run_frames(&mut ppu, &mut board, 2);
        let row = &ppu.picture()[100 * WIDTH..101 * WIDTH];
        assert!(row[0..8].iter().all(|&p| p == 0x16), "tile 1 in column 0");
        assert!(row[8..].iter().all(|&p| p == 0x0F), "backdrop elsewhere");

        // Fine X = 3: the stripe moves 3 pixels left.
        ppu.write_register(5, 3, &mut board);
        ppu.write_register(5, 0, &mut board);
        run_frames(&mut ppu, &mut board, 2);
        let row = &ppu.picture()[100 * WIDTH..101 * WIDTH];
        assert!(row[0..5].iter().all(|&p| p == 0x16));
        assert_eq!(row[5], 0x0F);
    }

    #[test]
    fn left_column_clipping_hides_the_first_eight_pixels() {
        let mut board = chr(Mirroring::Vertical);
        let mut ppu = Ppu::new();
        striped(&mut ppu, &mut board);
        ppu.write_register(1, MASK_BACKGROUND, &mut board);
        run_frames(&mut ppu, &mut board, 2);
        assert!(ppu.picture()[50 * WIDTH..50 * WIDTH + 8]
            .iter()
            .all(|&p| p == 0x0F));
    }

    #[test]
    fn sprites_draw_one_line_below_their_y_and_sprite_0_hits() {
        let mut board = chr(Mirroring::Vertical);
        let mut ppu = Ppu::new();
        striped(&mut ppu, &mut board);
        write_vram(&mut ppu, &mut board, 0x3F11, &[0x2A]);
        // Sprite 0: tile 1 at x = 4, y = 20, so it shows on lines 21-28.
        ppu.write_register(3, 0, &mut board);
        for b in [20, 1, 0, 4] {
            ppu.write_register(4, b, &mut board);
        }
        for _ in 4..256 {
            ppu.write_register(4, 0xFF, &mut board);
        }
        // Point v at the top-left so the first frame scrolls from there.
        ppu.write_register(6, 0, &mut board);
        ppu.write_register(6, 0, &mut board);
        ppu.write_register(
            1,
            MASK_BACKGROUND | MASK_BACKGROUND_LEFT | MASK_SPRITES | MASK_SPRITES_LEFT,
            &mut board,
        );
        while ppu.position().0 != 20 {
            ppu.tick(&mut board);
        }
        assert_eq!(ppu.status & STATUS_SPRITE_0, 0);
        run_frames(&mut ppu, &mut board, 1);
        assert_ne!(ppu.status & STATUS_SPRITE_0, 0, "overlaps the stripe");
        let at = |x: usize, y: usize| ppu.picture()[y * WIDTH + x];
        assert_eq!(at(4, 20), 0x16, "line 20 is above the sprite");
        assert_eq!(at(4, 21), 0x2A);
        assert_eq!(at(11, 28), 0x2A);
        assert_eq!(at(11, 29), 0x0F);
    }

    #[test]
    fn the_ninth_sprite_on_a_line_sets_overflow() {
        let mut board = chr(Mirroring::Vertical);
        let mut ppu = Ppu::new();
        ppu.write_register(3, 0, &mut board);
        for n in 0..64u8 {
            let y = if n < 9 { 50 } else { 0xF0 };
            for b in [y, 0, 0, n.wrapping_mul(8)] {
                ppu.write_register(4, b, &mut board);
            }
        }
        ppu.write_register(1, MASK_SPRITES, &mut board);
        run_frames(&mut ppu, &mut board, 1);
        assert_ne!(ppu.status & STATUS_OVERFLOW, 0);
    }
}
