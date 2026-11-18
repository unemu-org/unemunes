//! Cartridge images in the iNES and NES 2.0 formats.
//!
//! Layout (NESdev wiki, "INES", revision 23194, and "NES 2.0", revision
//! 24464): a 16-byte header starting with `NES` and `$1A`; an optional
//! 512-byte trainer; PRG ROM in 16 KiB units; CHR ROM in 8 KiB units, where 0
//! means the board has 8 KiB of CHR RAM instead. Byte 6 holds the nametable
//! arrangement, the battery and trainer flags and the mapper's low nibble;
//! byte 7 the mapper's high nibble and, when `byte 7 & $0C == $08`, the
//! NES 2.0 signature. NES 2.0 adds mapper bits 8 to 11 and the submapper
//! (byte 8), size high nibbles (byte 9), PRG RAM sizes (byte 10) and the
//! timing region (byte 12).
//!
//! Which flavour a header is follows the "INES" page's recommended order,
//! with one ruling: NES 2.0 when the signature is there; iNES when bytes 12 to
//! 15 are zero; otherwise an older or damaged header (tools wrote text such as
//! "DiskDude!" there), whose byte 7 is ignored. The page also treats a
//! signature whose sizes do not fit the image as an older header; this core
//! refuses such an image (truncated, or a size form it does not read)
//! instead, because reading it as an older header drops the mapper's high
//! bits and could run a board it does not have as NROM.

use std::fmt;

const HEADER: usize = 16;
const TRAINER: usize = 512;
const PRG_UNIT: usize = 16 * 1024;
const CHR_UNIT: usize = 8 * 1024;

/// How the board wires the PPU's nametables.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mirroring {
    /// Vertical arrangement (horizontal mirroring): `$2000` = `$2400`.
    Horizontal,
    /// Horizontal arrangement (vertical mirroring): `$2000` = `$2800`.
    Vertical,
    /// The board supplies all four nametables.
    FourScreen,
}

/// The console timing the image asks for.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Region {
    Ntsc,
    Pal,
    /// Runs on either.
    Multiple,
    Dendy,
}

/// Which header format an image uses.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Format {
    /// iNES with bytes 12 to 15 zero.
    Ines,
    /// NES 2.0.
    Nes20,
    /// An older iNES header or one with text in bytes 7 to 15; byte 7 is
    /// ignored.
    ArchaicInes,
}

/// A parsed cartridge image.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cartridge {
    pub format: Format,
    /// Mapper number (iNES 0 to 255, NES 2.0 0 to 4095).
    pub mapper: u16,
    /// NES 2.0 submapper, 0 otherwise.
    pub submapper: u8,
    pub mirroring: Mirroring,
    /// The board keeps its PRG RAM alive with a battery.
    pub battery: bool,
    /// Bytes of PRG RAM (work and battery-backed) the board has, when the
    /// header says so: NES 2.0 states it; iNES does not.
    pub prg_ram: Option<usize>,
    /// Timing region: NES 2.0 states it; iNES images are taken as NTSC.
    pub region: Region,
    pub prg_rom: Vec<u8>,
    /// CHR ROM, or empty when the board has CHR RAM.
    pub chr_rom: Vec<u8>,
}

/// Why bytes are not a usable cartridge image.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CartridgeError {
    /// The first four bytes are not `NES` and `$1A`.
    NotNes,
    /// The image is shorter than its header says.
    Truncated { expected: usize, actual: usize },
    /// The header declares no PRG ROM.
    NoPrgRom,
    /// A size in exponent form or above what this core reads.
    UnsupportedSize,
}

impl fmt::Display for CartridgeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CartridgeError::NotNes => f.write_str("not an iNES or NES 2.0 image"),
            CartridgeError::Truncated { expected, actual } => write!(
                f,
                "the image is {actual} bytes long, its header asks for {expected}"
            ),
            CartridgeError::NoPrgRom => f.write_str("the header declares no PRG ROM"),
            CartridgeError::UnsupportedSize => {
                f.write_str("the header declares a ROM size in a form this core does not read")
            }
        }
    }
}

impl std::error::Error for CartridgeError {}

impl Cartridge {
    /// Parse an iNES or NES 2.0 image.
    pub fn from_ines(bytes: &[u8]) -> Result<Cartridge, CartridgeError> {
        if bytes.len() < HEADER || bytes[0..4] != *b"NES\x1A" {
            return Err(CartridgeError::NotNes);
        }
        let h = &bytes[..HEADER];
        let trainer = h[6] & 0x04 != 0;
        let offset = HEADER + if trainer { TRAINER } else { 0 };

        // NES 2.0 sizes: byte 9 holds the high nibbles. A high nibble of $F
        // is the exponent form, which no image this core runs needs.
        let nes20_sizes = || -> Result<(usize, usize), CartridgeError> {
            let prg_high = usize::from(h[9] & 0x0F);
            let chr_high = usize::from(h[9] >> 4);
            if prg_high == 0x0F || chr_high == 0x0F {
                return Err(CartridgeError::UnsupportedSize);
            }
            Ok((
                (prg_high << 8 | usize::from(h[4])) * PRG_UNIT,
                (chr_high << 8 | usize::from(h[5])) * CHR_UNIT,
            ))
        };

        let format = if h[7] & 0x0C == 0x08 {
            Format::Nes20
        } else if h[12..16].iter().all(|&b| b == 0) && h[7] & 0x0C == 0x00 {
            Format::Ines
        } else {
            Format::ArchaicInes
        };

        let (prg_size, chr_size) = match format {
            Format::Nes20 => nes20_sizes()?,
            _ => (usize::from(h[4]) * PRG_UNIT, usize::from(h[5]) * CHR_UNIT),
        };
        if prg_size == 0 {
            return Err(CartridgeError::NoPrgRom);
        }
        let expected = offset + prg_size + chr_size;
        if bytes.len() < expected {
            return Err(CartridgeError::Truncated {
                expected,
                actual: bytes.len(),
            });
        }

        let low = u16::from(h[6] >> 4);
        let (mapper, submapper) = match format {
            Format::Nes20 => (
                u16::from(h[8] & 0x0F) << 8 | u16::from(h[7] & 0xF0) | low,
                h[8] >> 4,
            ),
            Format::Ines => (u16::from(h[7] & 0xF0) | low, 0),
            Format::ArchaicInes => (low, 0),
        };
        let mirroring = if h[6] & 0x08 != 0 {
            Mirroring::FourScreen
        } else if h[6] & 0x01 != 0 {
            Mirroring::Vertical
        } else {
            Mirroring::Horizontal
        };
        let (prg_ram, region) = match format {
            Format::Nes20 => {
                // Byte 10: volatile size in the low nibble, battery-backed in
                // the high one, each 64 << n bytes, 0 meaning none.
                let size = |n: u8| if n == 0 { 0 } else { 64usize << n };
                let ram = size(h[10] & 0x0F) + size(h[10] >> 4);
                let region = match h[12] & 0x03 {
                    0 => Region::Ntsc,
                    1 => Region::Pal,
                    2 => Region::Multiple,
                    _ => Region::Dendy,
                };
                (Some(ram), region)
            }
            _ => (None, Region::Ntsc),
        };

        let prg_rom = bytes[offset..offset + prg_size].to_vec();
        let chr_rom = bytes[offset + prg_size..offset + prg_size + chr_size].to_vec();
        Ok(Cartridge {
            format,
            mapper,
            submapper,
            mirroring,
            battery: h[6] & 0x02 != 0,
            prg_ram,
            region,
            prg_rom,
            chr_rom,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(header: [u8; 16], prg_units: usize, chr_units: usize) -> Vec<u8> {
        let mut bytes = header.to_vec();
        bytes.extend((0..prg_units * PRG_UNIT).map(|i| i as u8));
        bytes.extend((0..chr_units * CHR_UNIT).map(|i| (i as u8) ^ 0xFF));
        bytes
    }

    fn header(prg: u8, chr: u8, flags6: u8, flags7: u8) -> [u8; 16] {
        let mut h = [0u8; 16];
        h[0..4].copy_from_slice(b"NES\x1A");
        h[4] = prg;
        h[5] = chr;
        h[6] = flags6;
        h[7] = flags7;
        h
    }

    #[test]
    fn parses_an_ines_nrom_128_image() {
        let cart = Cartridge::from_ines(&image(header(1, 1, 0x00, 0x00), 1, 1)).unwrap();
        assert_eq!(cart.format, Format::Ines);
        assert_eq!(cart.mapper, 0);
        assert_eq!(cart.mirroring, Mirroring::Horizontal);
        assert_eq!(cart.prg_rom.len(), PRG_UNIT);
        assert_eq!(cart.chr_rom.len(), CHR_UNIT);
        assert_eq!(cart.prg_rom[1], 1);
        assert_eq!(cart.chr_rom[0], 0xFF);
        assert_eq!((cart.prg_ram, cart.region), (None, Region::Ntsc));
    }

    #[test]
    fn reads_flags_and_both_mapper_nibbles() {
        let cart = Cartridge::from_ines(&image(header(2, 0, 0x13, 0x40), 2, 0)).unwrap();
        assert_eq!(cart.mapper, 0x41);
        assert_eq!(cart.mirroring, Mirroring::Vertical);
        assert!(cart.battery);
        assert!(cart.chr_rom.is_empty(), "CHR size 0 means CHR RAM");
        let four = Cartridge::from_ines(&image(header(1, 1, 0x08, 0), 1, 1)).unwrap();
        assert_eq!(four.mirroring, Mirroring::FourScreen);
    }

    #[test]
    fn skips_the_trainer() {
        let mut bytes = header(1, 0, 0x04, 0).to_vec();
        bytes.extend([0xEE; TRAINER]);
        bytes.extend((0..PRG_UNIT).map(|i| i as u8));
        let cart = Cartridge::from_ines(&bytes).unwrap();
        assert_eq!(cart.prg_rom[0..2], [0, 1]);
    }

    #[test]
    fn ignores_byte_7_of_a_header_with_text_in_bytes_12_to_15() {
        let mut h = header(1, 1, 0x10, 0x00);
        h[7..16].copy_from_slice(b"DiskDude!");
        let cart = Cartridge::from_ines(&image(h, 1, 1)).unwrap();
        assert_eq!(cart.format, Format::ArchaicInes);
        assert_eq!(cart.mapper, 1, "not 0x41 from the 'D' of DiskDude");
    }

    #[test]
    fn reads_nes_2_0_fields() {
        let mut h = header(1, 1, 0x00, 0x08 | 0xA0);
        h[8] = 0x31; // submapper 3, mapper bits 8-11 = 1
        h[10] = 0x07; // 64 << 7 = 8 KiB of volatile PRG RAM
        h[12] = 0x01; // PAL
        let cart = Cartridge::from_ines(&image(h, 1, 1)).unwrap();
        assert_eq!(cart.format, Format::Nes20);
        assert_eq!(cart.mapper, 0x1A0);
        assert_eq!(cart.submapper, 3);
        assert_eq!(cart.prg_ram, Some(8 * 1024));
        assert_eq!(cart.region, Region::Pal);
    }

    #[test]
    fn a_nes_2_0_header_never_falls_back_to_an_older_reading() {
        // Mapper 16 in exponent form, and a truncated mapper $40 image: both
        // would read as NROM if byte 7 were ignored.
        let mut exponent = header(1, 1, 0x00, 0x08);
        exponent[9] = 0xF0;
        exponent[6] = 0x00;
        exponent[7] |= 0x10;
        assert_eq!(
            Cartridge::from_ines(&image(exponent, 1, 1)),
            Err(CartridgeError::UnsupportedSize)
        );
        let mut truncated = image(header(2, 1, 0x00, 0x48), 1, 1);
        truncated.truncate(HEADER + PRG_UNIT);
        assert!(matches!(
            Cartridge::from_ines(&truncated),
            Err(CartridgeError::Truncated { .. })
        ));
    }

    #[test]
    fn refuses_what_is_not_an_image() {
        assert_eq!(
            Cartridge::from_ines(b"NOT A ROM AT ALL"),
            Err(CartridgeError::NotNes)
        );
        assert_eq!(
            Cartridge::from_ines(&header(0, 1, 0, 0)),
            Err(CartridgeError::NoPrgRom)
        );
        let mut short = image(header(2, 1, 0, 0), 1, 0);
        short.truncate(1000);
        assert_eq!(
            Cartridge::from_ines(&short),
            Err(CartridgeError::Truncated {
                expected: HEADER + 2 * PRG_UNIT + CHR_UNIT,
                actual: 1000
            })
        );
    }
}
