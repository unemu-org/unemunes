//! The bus the processor drives (decision-4).
//!
//! The 6502 makes exactly one memory access in every clock cycle, a read or a
//! write, including the accesses whose data it ignores (MCS6500 Hardware
//! Manual, MOS Technology, January 1976, §3.1.2 and Appendix A). `x65` calls
//! [`Bus::read`] or [`Bus::write`] once per cycle, so the embedder advances its
//! own clock and its other chips in each call.

/// What the processor is connected to. One call of [`read`](Bus::read) or
/// [`write`](Bus::write) is one CPU cycle; after it the processor asks for the
/// input lines.
///
/// The trait is object-safe: an embedder may pass `&mut dyn Bus`.
pub trait Bus {
    /// One read cycle at `address`.
    fn read(&mut self, address: u16) -> u8;

    /// One write cycle of `value` at `address`.
    fn write(&mut self, address: u16, value: u8);

    /// The input lines as they are after the access just made.
    fn lines(&mut self) -> Lines;
}

/// The processor's input lines, each `true` while asserted. The pins are
/// active low; these fields say whether a line is active, so
/// [`Lines::default()`] is "nothing asserted".
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Lines {
    /// Non-maskable interrupt request. The processor reacts to the change
    /// from not asserted to asserted.
    pub nmi: bool,
    /// Interrupt request, by level, masked by the I flag.
    pub irq: bool,
    /// Reset.
    pub reset: bool,
    /// The RDY input pulled low: the processor stops in its next read cycle
    /// (Hardware Manual §3.1.2). Not modelled yet (decision-4, N1): the
    /// processor ignores it.
    pub halt: bool,
}
