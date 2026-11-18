//! The console: the CPU, its memory map, and the clock that drives the rest.
//!
//! The CPU drives time (design note §3): every CPU bus call is one CPU
//! cycle, twelve master-clock cycles on NTSC, during which the PPU makes three
//! dots (NESdev wiki, "Cycle reference chart", revision 24433). Within a cycle
//! the PPU's dots run first and the access completes after them; the exact
//! sub-cycle alignment is milestone N2a's.
//!
//! Memory map (NESdev wiki, "CPU memory map", revision 24447): 2 KiB of RAM
//! mirrored through `$1FFF`; the PPU's eight registers mirrored through
//! `$3FFF`; APU and I/O at `$4000–$4017`; test registers at `$4018–$401F`;
//! the cartridge above. A read nothing answers returns the last value on the
//! data bus; the controller ports drive only bits 4 to 0 and leave bits 7 to 5
//! from the bus (NESdev wiki, "Open bus behavior", revision 23814); the PPU's
//! registers answer with the PPU's own I/O latch. The APU is silent for now
//! (`apu`): `$4015` reads its status without touching open bus, and the
//! other APU registers read as open bus.
//!
//! OAM DMA (NESdev wiki, "DMA", revision 23450): a write to `$4014` makes the
//! DMA unit halt the CPU on its next read cycle; the halted cycle repeats the
//! CPU's read, an alignment cycle follows when that cycle is a put cycle, and
//! 256 get and put pairs copy the page to `$2004`: 513 or 514 cycles. Then the
//! CPU makes the read it was halted on. Which CPU cycles are get cycles
//! depends on how the CPU and APU powered up, so it is a config value. DMA
//! goes through the RDY input of the processor in milestone N1 (decision-4);
//! here the bus makes the halt itself.

use x65::{Bus, Cpu, CpuState, Event, Lines, Model};

use crate::apu::Apu;
use crate::cartridge::Cartridge;
use crate::controller::{Buttons, Controller};
use crate::mapper::{board, Mapper, UnsupportedBoard};
use crate::palette;
use crate::ppu::Ppu;

/// Master-clock cycles per CPU cycle (NTSC).
pub const MASTER_PER_CPU: u64 = 12;
/// PPU dots per CPU cycle (NTSC).
pub const DOTS_PER_CPU: u32 = 3;

/// What hardware leaves undefined at power-on, fixed so a run is a function of
/// its inputs (design note §14 Q7).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Config {
    /// The byte every internal RAM cell holds at power-on.
    pub ram_fill: u8,
    /// Whether DMA get cycles fall on odd CPU cycles (counted from power-on)
    /// instead of even ones.
    pub dma_get_on_odd: bool,
}

/// The console's bus: everything the CPU reaches.
pub(crate) struct Board {
    ram: [u8; 0x800],
    ppu: Ppu,
    apu: Apu,
    mapper: Box<dyn Mapper>,
    controllers: [Controller; 2],
    open_bus: u8,
    cpu_cycles: u64,
    oam_dma: Option<u8>,
    dma_get_on_odd: bool,
}

impl Board {
    fn new(mapper: Box<dyn Mapper>, config: Config) -> Board {
        Board {
            ram: [config.ram_fill; 0x800],
            ppu: Ppu::new(),
            apu: Apu::default(),
            mapper,
            controllers: [Controller::default(); 2],
            open_bus: 0,
            cpu_cycles: 0,
            oam_dma: None,
            dma_get_on_odd: config.dma_get_on_odd,
        }
    }

    /// One CPU cycle of time.
    fn clock(&mut self) {
        for _ in 0..DOTS_PER_CPU {
            self.ppu.tick(&mut *self.mapper);
        }
        self.cpu_cycles += 1;
    }

    fn is_get_cycle(&self) -> bool {
        (self.cpu_cycles % 2 == 1) == self.dma_get_on_odd
    }

    /// One read cycle by the CPU or the DMA unit.
    fn read_cycle(&mut self, address: u16) -> u8 {
        self.clock();
        if address == 0x4015 {
            // Inside the CPU: the external bus, and so open bus, is untouched.
            return self.apu.read_status(self.open_bus);
        }
        let value = match address {
            0x0000..=0x1FFF => self.ram[usize::from(address & 0x07FF)],
            0x2000..=0x3FFF => self.ppu.read_register(address & 7, &mut *self.mapper),
            0x4016 => self.open_bus & 0xE0 | self.controllers[0].read(),
            0x4017 => self.open_bus & 0xE0 | self.controllers[1].read(),
            0x4000..=0x401F => self.open_bus,
            _ => self.mapper.cpu_read(address).unwrap_or(self.open_bus),
        };
        self.open_bus = value;
        value
    }

    fn write_cycle(&mut self, address: u16, value: u8) {
        self.clock();
        self.open_bus = value;
        match address {
            0x0000..=0x1FFF => self.ram[usize::from(address & 0x07FF)] = value,
            0x2000..=0x3FFF => self
                .ppu
                .write_register(address & 7, value, &mut *self.mapper),
            0x4014 => self.oam_dma = Some(value),
            0x4016 => {
                for pad in &mut self.controllers {
                    pad.write_strobe(value & 1 != 0);
                }
            }
            0x4000..=0x4013 | 0x4015 | 0x4017 => self.apu.write_register(address, value),
            0x4000..=0x401F => {}
            _ => self.mapper.cpu_write(address, value),
        }
    }

    /// The OAM DMA the CPU's read at `halted` is waiting for.
    fn oam_dma(&mut self, page: u8, halted: u16) {
        self.read_cycle(halted);
        if !self.is_get_cycle() {
            self.read_cycle(halted);
        }
        for low in 0..=255u8 {
            let value = self.read_cycle(u16::from_le_bytes([low, page]));
            self.write_cycle(0x2004, value);
        }
    }
}

impl Bus for Board {
    fn read(&mut self, address: u16) -> u8 {
        if let Some(page) = self.oam_dma.take() {
            self.oam_dma(page, address);
        }
        self.read_cycle(address)
    }

    fn write(&mut self, address: u16, value: u8) {
        self.write_cycle(address, value);
    }

    fn lines(&mut self) -> Lines {
        Lines {
            nmi: self.ppu.nmi(),
            ..Lines::default()
        }
    }
}

/// A Nintendo Entertainment System (NTSC).
pub struct Nes {
    cpu: Cpu,
    board: Board,
    palette: Vec<u32>,
}

impl Nes {
    /// A console with `cartridge` inserted, just powered on: the first step
    /// runs the CPU's reset sequence.
    ///
    /// The console is NTSC whatever region the image states; choosing the
    /// machine for a region is the caller's.
    pub fn new(cartridge: &Cartridge, config: Config) -> Result<Nes, UnsupportedBoard> {
        Ok(Nes {
            cpu: Cpu::new(Model::Rp2a03),
            board: Board::new(board(cartridge)?, config),
            palette: palette::table(),
        })
    }

    /// Run one CPU instruction, or one interrupt or reset sequence, with
    /// everything it takes along (DMA, PPU dots).
    pub fn step(&mut self) -> Event {
        self.cpu.step(&mut self.board)
    }

    /// The CPU's state.
    pub fn cpu(&self) -> CpuState {
        self.cpu.state()
    }

    /// Replace the CPU's state.
    pub fn set_cpu(&mut self, state: CpuState) {
        self.cpu.set_state(state);
    }

    /// CPU cycles since power-on.
    pub fn cpu_cycles(&self) -> u64 {
        self.board.cpu_cycles
    }

    /// Master-clock cycles since power-on.
    pub fn master_cycles(&self) -> u64 {
        self.board.cpu_cycles * MASTER_PER_CPU
    }

    /// The PPU.
    pub fn ppu(&self) -> &Ppu {
        &self.board.ppu
    }

    /// Internal RAM.
    pub fn ram(&self) -> &[u8; 0x800] {
        &self.board.ram
    }

    /// Whether the PPU finished a picture since the last call.
    pub fn take_frame_done(&mut self) -> bool {
        self.board.ppu.take_frame_done()
    }

    /// The last finished picture as `0x00RRGGBB`, `ppu::WIDTH × ppu::HEIGHT`
    /// pixels, rows top to bottom, through the computed NTSC palette.
    pub fn picture_rgb(&self, out: &mut Vec<u32>) {
        out.clear();
        out.extend(
            self.board
                .ppu
                .picture()
                .iter()
                .map(|&p| self.palette[usize::from(p & 0x1FF)]),
        );
    }

    /// Append the sound made since the last call, at `apu::RATE`, to `out`.
    pub fn drain_audio(&mut self, out: &mut Vec<[i16; 2]>) {
        let master = self.master_cycles();
        self.board.apu.drain(master, out);
    }

    /// Drop the sound made since the last call, as when nobody takes it.
    pub fn skip_audio(&mut self) {
        let master = self.master_cycles();
        self.board.apu.skip(master);
    }

    /// Set the buttons held on controller `port` (0 or 1).
    pub fn set_buttons(&mut self, port: usize, buttons: Buttons) {
        self.board.controllers[port].buttons = buttons;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cartridge::{Format, Mirroring, Region};
    use crate::mapper::Nrom;

    fn cartridge() -> Cartridge {
        Cartridge {
            format: Format::Ines,
            mapper: 0,
            submapper: 0,
            mirroring: Mirroring::Horizontal,
            battery: false,
            prg_ram: None,
            region: Region::Ntsc,
            prg_rom: (0..0x4000).map(|i| (i >> 8) as u8 | 0x80).collect(),
            chr_rom: vec![0; 0x2000],
        }
    }

    fn board(config: Config) -> Board {
        Board::new(Box::new(Nrom::new(&cartridge())), config)
    }

    #[test]
    fn ram_is_mirrored_four_times() {
        let mut b = board(Config::default());
        b.write(0x0123, 0x42);
        for address in [0x0123, 0x0923, 0x1123, 0x1923] {
            assert_eq!(b.read(address), 0x42);
        }
    }

    #[test]
    fn ram_starts_with_the_configured_fill() {
        let mut b = board(Config {
            ram_fill: 0xFF,
            ..Config::default()
        });
        assert_eq!(b.read(0x0700), 0xFF);
    }

    #[test]
    fn ppu_registers_repeat_every_eight_bytes() {
        let mut b = board(Config::default());
        b.write(0x3FFB, 0x05); // $2003
        b.write(0x200C, 0x99); // $2004
        assert_eq!(b.ppu.oam()[5], 0x99);
    }

    #[test]
    fn unmapped_reads_return_the_last_bus_value() {
        let mut b = board(Config::default());
        assert_eq!(b.read(0x8005), 0x80);
        assert_eq!(b.read(0x4018), 0x80, "test registers");
        assert_eq!(b.read(0x5000), 0x80, "no PRG RAM, nothing at $5000");
        b.write(0x0000, 0x37);
        assert_eq!(b.read(0x4000), 0x37, "APU registers read as open bus");
    }

    #[test]
    fn controller_ports_drive_bits_4_to_0_only() {
        let mut b = board(Config::default());
        b.controllers[0].buttons = Buttons::A;
        b.controllers[1].buttons = Buttons::B;
        b.write(0x4016, 1);
        b.write(0x4016, 0);
        b.write(0x0010, 0x40);
        b.read(0x0010); // $40 on the bus, as after the high byte of LDA $4016
        assert_eq!(b.read(0x4016), 0x41, "bits 7-5 from the bus, A in bit 0");
        assert_eq!(b.read(0x4017), 0x40, "port 2 first reports A = 0");
        assert_eq!(b.read(0x4017), 0x41, "then B = 1");
    }

    #[test]
    fn oam_dma_copies_a_page_in_513_or_514_cycles() {
        // The halt lands on the cycle after the write; an alignment cycle
        // follows when the cycle after the halt is a put cycle.
        for (get_on_odd, write_cycle, expected) in [
            (false, 0, 513),
            (false, 1, 514),
            (true, 0, 514),
            (true, 1, 513),
        ] {
            let mut b = board(Config {
                dma_get_on_odd: get_on_odd,
                ..Config::default()
            });
            for i in 0..=255u8 {
                b.write(0x0200 + u16::from(i), i ^ 0x5A);
            }
            if b.cpu_cycles % 2 != write_cycle {
                b.read(0x8000);
            }
            b.write(0x4014, 0x02);
            let before = b.cpu_cycles;
            b.read(0x8000);
            let spent = b.cpu_cycles - before - 1;
            assert_eq!(
                spent, expected,
                "get on odd {get_on_odd}, $4014 written on cycle parity {write_cycle}"
            );
            // Byte 2 of each entry keeps only its implemented bits.
            let want = |i: u8| {
                if i % 4 == 2 {
                    (i ^ 0x5A) & 0xE3
                } else {
                    i ^ 0x5A
                }
            };
            assert!((0..=255u8).all(|i| b.ppu.oam()[usize::from(i)] == want(i)));
        }
    }

    #[test]
    fn apu_status_keeps_open_bus() {
        let mut b = board(Config::default());
        b.write(0x0000, 0xFF);
        b.read(0x0000);
        assert_eq!(b.read(0x4015), 0x20, "silent, bit 5 from open bus");
        assert_eq!(b.read(0x4018), 0xFF, "the $4015 read left open bus alone");
    }

    #[test]
    fn the_ppu_makes_three_dots_per_cpu_cycle() {
        let mut b = board(Config::default());
        for _ in 0..200 {
            b.read(0x8000);
        }
        assert_eq!(b.ppu.position(), (1, 600 - 341));
    }

    #[test]
    fn a_console_boots_through_the_reset_vector() {
        let mut nes = Nes::new(&cartridge(), Config::default()).unwrap();
        assert_eq!(nes.step(), Event::Reset);
        // The reset vector reads $FFFC/$FFFD of the 16 KiB image: $BF, $BF.
        assert_eq!(nes.cpu().pc, 0xBFBF);
        assert_eq!(nes.cpu_cycles(), 7);
        assert_eq!(nes.master_cycles(), 84);
    }
}
