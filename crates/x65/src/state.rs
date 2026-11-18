//! The processor's state as plain data, and what a cycle reports
//! (decision-4).

/// Bits of the status register `P`.
///
/// Bit 5 has no flip-flop and reads as 1; bit 4 (B) exists only in the copy
/// of `P` that PHP and BRK push, where it is 1, while an interrupt sequence
/// pushes it as 0 (MCS6500 Programming Manual, MOS Technology, January 1976,
/// §3.4, §3.5, §8.11 and §9.11; NESdev wiki "Status flags", revision 22145;
/// witnessed by the PHP, PLP and BRK traces of SingleStepTests). The `p`
/// of [`CpuState`] therefore always has bit 5 set and bit 4 clear.
pub mod flags {
    pub const CARRY: u8 = 0x01;
    pub const ZERO: u8 = 0x02;
    pub const INTERRUPT_DISABLE: u8 = 0x04;
    pub const DECIMAL: u8 = 0x08;
    pub const BREAK: u8 = 0x10;
    pub const UNUSED: u8 = 0x20;
    pub const OVERFLOW: u8 = 0x40;
    pub const NEGATIVE: u8 = 0x80;
}

/// A member of the 6502 family. Differences between chips are model
/// parameters, never conditional compilation (decision-1).
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Model {
    /// The 6502 core of the Ricoh RP2A03 (NES, NTSC): the NMOS 6502 with the
    /// D flag kept and no decimal arithmetic. The NMOS 6502 with decimal
    /// arithmetic follows as its own model in milestone N1.
    Rp2a03,
}

/// An interrupt the processor took.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Interrupt {
    Nmi,
    Irq,
}

/// What a cycle completed.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Event {
    /// An instruction finished.
    Instruction { opcode: u8 },
    /// An interrupt sequence finished: the handler's first instruction is
    /// next.
    Interrupt(Interrupt),
    /// The reset sequence finished: the program at the reset vector is next.
    Reset,
    /// An opcode this model does not implement yet. The cycle fetched it and
    /// changed nothing else: the program counter still points at it, so the
    /// next cycle fetches it again.
    Unsupported { opcode: u8 },
}

/// What is in flight.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Sequence {
    Instruction,
    Nmi,
    Irq,
    Reset,
}

/// Everything the processor holds, as plain data a machine can save and
/// restore after any cycle.
///
/// The fields after `p` describe the sequence in flight and the interrupt
/// inputs; restore only values that [`Cpu::state`](crate::Cpu::state)
/// returned. The struct is deliberately exhaustive: a new field is a
/// compile error in every saved-state writer, not a field silently left out.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CpuState {
    pub model: Model,
    pub pc: u16,
    pub a: u8,
    pub x: u8,
    pub y: u8,
    /// Stack pointer; the stack is page 1.
    pub s: u8,
    /// Status register, bit 5 set and bit 4 clear ([`flags`]).
    pub p: u8,

    /// The sequence in flight; meaningful while `cycle` is not 0.
    pub sequence: Sequence,
    /// The opcode in flight.
    pub opcode: u8,
    /// Cycles made of the sequence in flight; 0 at a boundary.
    pub cycle: u8,
    /// Address latch.
    pub address: u16,
    /// Zero-page pointer or index base latch.
    pub pointer: u8,
    /// Data latch.
    pub data: u8,
    /// A page-boundary carry still to be added to the address latch.
    pub carry: bool,

    /// The NMI line as last sampled, for the edge detector.
    pub nmi_line: bool,
    /// The edge detector's output: an NMI waits to be taken.
    pub nmi_pending: bool,
    /// The IRQ line as last sampled.
    pub irq_line: bool,
    /// The interrupt decision polled at the end of the cycle before an
    /// instruction's last one: take an NMI next.
    pub poll_nmi: bool,
    /// As `poll_nmi`, for an IRQ.
    pub poll_irq: bool,
    /// The RES line as last sampled: while it is asserted the processor
    /// holds at the instruction boundary.
    pub reset_line: bool,
    /// A reset waits to run once the line is released.
    pub reset_pending: bool,
}
