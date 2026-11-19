//! Frame goldens from a cartridge built here from bytes of our own
//! (core-contract.md §13.6 step 16 (unemupc)): nothing proprietary, no
//! fetched oracle. The program initialises the console the usual way (wait
//! for two vertical blanks), writes a palette, fills the first nametable with
//! a checkerboard of two tiles and turns on the background; its NMI handler
//! counts frames at $01. The test checks the picture against the
//! checkerboard the program describes, and pins a digest of the RGB picture
//! so that any change to the PPU or the palette shows up here.
//!
//! `UNEMUNES_GOLDEN_PPM=<file>` writes the picture as a PPM image for a look.

use std::rc::Rc;

use unemu_core_api::test_support::{MemoryContent, MemoryStorage};
use unemu_core_api::{
    Content, ContentSource, Core, CreateRequest, DisplayId, Error, ErrorKind, Machine,
    OfferedContent, RunLimit, Stop,
};
use unemu_nes_core::{NesCore, PLATFORM};

const DIGEST: [u8; 32] = [0xC3; 32];

/// The program at $8000, assembled by hand (listing: address, label,
/// instruction).
const PROGRAM: &[u8] = &[
    0x78, // $8000 reset: SEI
    0xD8, // $8001 CLD
    0xA2, 0xFF, // $8002 LDX #$FF
    0x9A, // $8004 TXS
    0x2C, 0x02, 0x20, // $8005 vblank1: BIT $2002
    0x10, 0xFB, // $8008 BPL vblank1
    0x2C, 0x02, 0x20, // $800A vblank2: BIT $2002
    0x10, 0xFB, // $800D BPL vblank2
    0xA9, 0x3F, // $800F LDA #$3F
    0x8D, 0x06, 0x20, // $8011 STA $2006
    0xA9, 0x00, // $8014 LDA #$00
    0x8D, 0x06, 0x20, // $8016 STA $2006
    0xA2, 0x00, // $8019 LDX #$00
    0xBD, 0x6B, 0x80, // $801B pal: LDA palette,X
    0x8D, 0x07, 0x20, // $801E STA $2007
    0xE8, // $8021 INX
    0xE0, 0x04, // $8022 CPX #$04
    0xD0, 0xF5, // $8024 BNE pal
    0xA9, 0x20, // $8026 LDA #$20
    0x8D, 0x06, 0x20, // $8028 STA $2006
    0xA9, 0x00, // $802B LDA #$00
    0x8D, 0x06, 0x20, // $802D STA $2006
    0xA0, 0x00, // $8030 LDY #$00
    0xA2, 0x00, // $8032 row: LDX #$00
    0x84, 0x00, // $8034 col: STY $00
    0x8A, // $8036 TXA
    0x18, // $8037 CLC
    0x65, 0x00, // $8038 ADC $00
    0x29, 0x01, // $803A AND #$01
    0x8D, 0x07, 0x20, // $803C STA $2007
    0xE8, // $803F INX
    0xE0, 0x20, // $8040 CPX #$20
    0xD0, 0xF0, // $8042 BNE col
    0xC8, // $8044 INY
    0xC0, 0x1E, // $8045 CPY #$1E
    0xD0, 0xE9, // $8047 BNE row
    0xA2, 0x40, // $8049 LDX #$40
    0xA9, 0x00, // $804B LDA #$00
    0x8D, 0x07, 0x20, // $804D attr: STA $2007
    0xCA, // $8050 DEX
    0xD0, 0xFA, // $8051 BNE attr
    0xA9, 0x00, // $8053 LDA #$00
    0x8D, 0x05, 0x20, // $8055 STA $2005
    0x8D, 0x05, 0x20, // $8058 STA $2005
    0xA9, 0x0A, // $805B LDA #$0A
    0x8D, 0x01, 0x20, // $805D STA $2001
    0xA9, 0x80, // $8060 LDA #$80
    0x8D, 0x00, 0x20, // $8062 STA $2000
    0x4C, 0x65, 0x80, // $8065 loop: JMP loop
    0xE6, 0x01, // $8068 nmi: INC $01
    0x40, // $806A RTI
    0x0F, 0x16, 0x2A, 0x12, // $806B palette: .byte $0F,$16,$2A,$12
];
const NMI: u16 = 0x8068;
const RESET: u16 = 0x8000;
/// The palette the program writes to $3F00.
const PALETTE: [u8; 4] = [0x0F, 0x16, 0x2A, 0x12];

fn image() -> Vec<u8> {
    let mut prg = vec![0xEA; 0x4000];
    prg[..PROGRAM.len()].copy_from_slice(PROGRAM);
    let vectors = [NMI.to_le_bytes(), RESET.to_le_bytes(), NMI.to_le_bytes()].concat();
    prg[0x3FFA..].copy_from_slice(&vectors);
    // Tile 0: colour 1 everywhere (low plane set); tile 1: colour 2 (high
    // plane set).
    let mut chr = vec![0u8; 0x2000];
    chr[0..8].fill(0xFF);
    chr[24..32].fill(0xFF);
    let mut bytes = b"NES\x1A\x01\x01\x00\x00\0\0\0\0\0\0\0\0".to_vec();
    bytes.extend(prg);
    bytes.extend(chr);
    bytes
}

struct Source(Vec<u8>);

impl ContentSource for Source {
    fn open(&self, sha256: &[u8; 32], size: u64) -> Result<Content, Error> {
        if *sha256 == DIGEST && size == self.0.len() as u64 {
            Ok(Content::Bytes(self.0.clone()))
        } else {
            Err(Error::new(ErrorKind::Content, "no such content"))
        }
    }
}

fn machine() -> Box<dyn Machine> {
    let core: &dyn Core = &NesCore;
    let bytes = image();
    let reader = MemoryContent::new(bytes.clone());
    let offered = OfferedContent::new("checkerboard.nes", DIGEST, bytes.len() as u64, &reader);
    let identified = core.identify(&offered).unwrap().expect("an NROM image");
    let source = Source(bytes);
    let storage: Rc<dyn unemu_core_api::Storage> = Rc::new(MemoryStorage::new());
    core.create(&CreateRequest::new(
        PLATFORM,
        identified.config.as_document(),
        &source,
        storage,
    ))
    .expect("created")
}

/// FNV-1a over the pixels, little-endian.
fn digest(pixels: &[u32]) -> u64 {
    pixels
        .iter()
        .flat_map(|p| p.to_le_bytes())
        .fold(0xCBF2_9CE4_8422_2325, |h, b| {
            (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01B3)
        })
}

fn write_ppm(pixels: &[u32]) {
    let Some(path) = std::env::var_os("UNEMUNES_GOLDEN_PPM") else {
        return;
    };
    let mut out = b"P6\n256 240\n255\n".to_vec();
    for p in pixels {
        out.extend([(p >> 16) as u8, (p >> 8) as u8, *p as u8]);
    }
    std::fs::write(path, out).expect("write the PPM");
}

#[test]
fn the_checkerboard_is_drawn_and_matches_its_golden() {
    let mut m = machine();
    let mut frames = 0;
    let mut digests = Vec::new();
    while frames < 8 {
        let stop = m
            .run(RunLimit::steps(m.steps() + 1_000_000).with_stop_on_frame(true))
            .unwrap();
        assert_eq!(stop, Stop::Frame(DisplayId(0)));
        frames += 1;
        digests.push(digest(m.frame(DisplayId(0)).unwrap().pixels));
    }
    let frame = m.frame(DisplayId(0)).unwrap();
    write_ppm(frame.pixels);

    let colour = |value: u8| nes::palette::rgb(value, 0);
    for y in 0..240usize {
        for x in 0..256usize {
            let tile = (x / 8 + y / 8) & 1;
            let want = colour(PALETTE[1 + tile]);
            assert_eq!(frame.pixels[y * 256 + x], want, "pixel ({x}, {y})");
        }
    }
    // Frames 0 and 1 are the backdrop while the program waits for two
    // vertical blanks; it turns the background on during frame 2, so frame 2
    // is partly drawn; from frame 3 on the picture is whole and stays.
    assert_eq!(digests[0], digests[1]);
    assert_ne!(digests[2], digests[3]);
    assert!(digests[3..].iter().all(|d| *d == digests[7]));
    assert_eq!(
        digests[7], GOLDEN,
        "the pinned picture changed: {:#018X}",
        digests[7]
    );
}

/// The digest of the checkerboard, pinned for this revision (§13.6 step 16).
const GOLDEN: u64 = 0xFCDF_D186_11FE_D325;
