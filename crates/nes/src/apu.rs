//! The 2A03's APU, silent for now (bead `nes-a3h.5`; the sound itself is
//! milestone N2b).
//!
//! What the stub keeps true already: the registers `$4000–$4013`, `$4015` and
//! `$4017` accept writes; `$4015` reads as no channel playing and no
//! interrupt, with bit 5 from open bus, and the read does not change open
//! bus, because the register is inside the CPU (NESdev wiki, "APU", revision
//! 24458, "$4015 read"); the frame counter never raises its IRQ.
//!
//! The output rate follows contract 0.2's rule for a generator faster than
//! 192 kHz (design note §6): the APU runs at the CPU clock, and the core keeps
//! one frame in 40 CPU cycles, 480 master-clock cycles, so the rate is
//! (236 250 000 / 11) / 480 = 984 375 / 22 Hz. Frame `i` is the sound at
//! master cycle `480 × i`, so the next frame to hand over after master cycle
//! `m` is `m / 480`.

/// Master-clock cycles per output frame.
pub const MASTER_PER_FRAME: u64 = 480;
/// The output rate as numerator and denominator: 984 375 / 22 Hz.
pub const RATE: (u64, u64) = (984_375, 22);

/// The APU.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Apu {
    /// The index of the next output frame to hand over.
    next_frame: u64,
}

impl Apu {
    /// A CPU write of `$4000–$4013`, `$4015` or `$4017`.
    pub fn write_register(&mut self, _address: u16, _value: u8) {}

    /// A CPU read of `$4015`: the status, with bit 5 from `open_bus`.
    pub fn read_status(&mut self, open_bus: u8) -> u8 {
        open_bus & 0x20
    }

    /// Append the frames of sound from the last call up to master cycle
    /// `master` (exclusive of the frame that starts after it) to `out`.
    pub fn drain(&mut self, master: u64, out: &mut Vec<[i16; 2]>) {
        let until = master / MASTER_PER_FRAME;
        if until > self.next_frame {
            out.extend(core::iter::repeat_n(
                [0, 0],
                (until - self.next_frame) as usize,
            ));
            self.next_frame = until;
        }
    }

    /// Drop the sound up to master cycle `master` without handing it over,
    /// as when nobody takes it; also after restoring a state at `master`.
    pub fn skip(&mut self, master: u64) {
        self.next_frame = master / MASTER_PER_FRAME;
    }

    /// The index of the next frame `drain` hands over.
    pub fn next_frame(&self) -> u64 {
        self.next_frame
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rate_is_the_master_clock_over_480() {
        // 236 250 000 / 11 / 480 = 984 375 / 22.
        assert_eq!(236_250_000 * RATE.1, 11 * 480 * RATE.0);
    }

    #[test]
    fn drain_follows_the_position_rule() {
        let mut apu = Apu::default();
        let mut out = Vec::new();
        apu.drain(479, &mut out);
        assert!(out.is_empty());
        apu.drain(480 * 10 + 7, &mut out);
        assert_eq!(out.len(), 10);
        assert!(out.iter().all(|f| *f == [0, 0]));
        assert_eq!(apu.next_frame(), (480 * 10 + 7) / 480);
    }

    #[test]
    fn skipped_sound_is_dropped_not_owed() {
        let mut apu = Apu::default();
        let mut out = Vec::new();
        apu.skip(480 * 100);
        apu.drain(480 * 103, &mut out);
        assert_eq!(out.len(), 3);
    }

    #[test]
    fn status_reads_silence_with_bit_5_from_open_bus() {
        let mut apu = Apu::default();
        assert_eq!(apu.read_status(0xFF), 0x20);
        assert_eq!(apu.read_status(0x40), 0x00);
    }
}
