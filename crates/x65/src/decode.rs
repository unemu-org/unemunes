//! Opcode decoding: which cycle sequence (addressing mode) and which
//! operation an opcode names.
//!
//! The documented instruction set of the NMOS 6502 is the MCS6500
//! Programming Manual (MOS Technology, January 1976), Appendix B (instructions
//! with their op codes and cycles) and Appendix D (op codes in hexadecimal
//! sequence), with the instruction descriptions of chapters 2 to 10; the cycle
//! sequence of each addressing mode is the MCS6500 Hardware Manual (January
//! 1976), Appendix A. An opcode the manuals do not document decodes to `None`
//! (decision-4: `Event::Unsupported` until milestone N1 adds the undocumented
//! opcodes with their own sources).

use crate::state::Model;

/// The cycle sequence of an instruction (Hardware Manual, Appendix A).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mode {
    /// A.1: single byte instructions, also the accumulator forms.
    Implied,
    /// A.2.1.
    Immediate,
    /// A.2.2, A.3.1, A.4.1.
    ZeroPage,
    /// A.2.6, A.3.5, A.4.3, indexed by X.
    ZeroPageX,
    /// A.2.6, A.3.5, indexed by Y.
    ZeroPageY,
    /// A.2.3, A.3.2, A.4.2.
    Absolute,
    /// A.2.5, A.3.4, A.4.4, indexed by X.
    AbsoluteX,
    /// A.2.5, A.3.4, indexed by Y.
    AbsoluteY,
    /// A.2.4, A.3.3: (zero page, X).
    IndirectX,
    /// A.2.7, A.3.6: (zero page), Y.
    IndirectY,
    /// A.5.8: conditional branches.
    Relative,
    /// A.5.6: JMP absolute.
    JumpAbsolute,
    /// A.5.6: JMP indirect.
    JumpIndirect,
    /// A.5.3.
    Jsr,
    /// A.5.7.
    Rts,
    /// A.5.5.
    Rti,
    /// A.5.4.
    Brk,
    /// A.5.1: PHA, PHP.
    Push,
    /// A.5.2: PLA, PLP.
    Pull,
}

/// The operation an instruction performs on its data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Op {
    // Read operations: internal execution on memory data (A.2).
    Adc,
    And,
    Bit,
    Cmp,
    Cpx,
    Cpy,
    Eor,
    Lda,
    Ldx,
    Ldy,
    Ora,
    Sbc,
    // Store operations (A.3).
    Sta,
    Stx,
    Sty,
    // Read-modify-write operations (A.4), and their accumulator forms.
    Asl,
    Dec,
    Inc,
    Lsr,
    Rol,
    Ror,
    // Single byte instructions (A.1).
    Clc,
    Cld,
    Cli,
    Clv,
    Dex,
    Dey,
    Inx,
    Iny,
    Nop,
    Sec,
    Sed,
    Sei,
    Tax,
    Tay,
    Tsx,
    Txa,
    Txs,
    Tya,
    // Branches (A.5.8): the flag tested and the value that takes the branch.
    Bcc,
    Bcs,
    Beq,
    Bmi,
    Bne,
    Bpl,
    Bvc,
    Bvs,
    // Stack operations (A.5.1, A.5.2).
    Pha,
    Php,
    Pla,
    Plp,
    // Control (A.5.3 to A.5.7).
    Jmp,
    Jsr,
    Rts,
    Rti,
    Brk,
}

/// How an instruction uses memory at its effective address.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Access {
    Read,
    Write,
    ReadModifyWrite,
}

impl Op {
    pub(crate) fn access(self) -> Access {
        match self {
            Op::Sta | Op::Stx | Op::Sty => Access::Write,
            Op::Asl | Op::Dec | Op::Inc | Op::Lsr | Op::Rol | Op::Ror => Access::ReadModifyWrite,
            _ => Access::Read,
        }
    }
}

/// A decoded instruction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Instruction {
    pub(crate) mode: Mode,
    pub(crate) op: Op,
}

const fn i(mode: Mode, op: Op) -> Option<Instruction> {
    Some(Instruction { mode, op })
}

/// The instruction an opcode names on `model`, or `None` for an opcode the
/// model does not implement.
pub(crate) fn decode(model: Model, opcode: u8) -> Option<Instruction> {
    match model {
        Model::Rp2a03 => documented(opcode),
    }
}

/// The 151 documented opcodes of the NMOS 6502 (Programming Manual,
/// Appendices B and D).
fn documented(opcode: u8) -> Option<Instruction> {
    use Mode::*;
    use Op::*;
    match opcode {
        0x00 => i(Mode::Brk, Op::Brk),
        0x01 => i(IndirectX, Ora),
        0x05 => i(ZeroPage, Ora),
        0x06 => i(ZeroPage, Asl),
        0x08 => i(Push, Php),
        0x09 => i(Immediate, Ora),
        0x0A => i(Implied, Asl),
        0x0D => i(Absolute, Ora),
        0x0E => i(Absolute, Asl),
        0x10 => i(Relative, Bpl),
        0x11 => i(IndirectY, Ora),
        0x15 => i(ZeroPageX, Ora),
        0x16 => i(ZeroPageX, Asl),
        0x18 => i(Implied, Clc),
        0x19 => i(AbsoluteY, Ora),
        0x1D => i(AbsoluteX, Ora),
        0x1E => i(AbsoluteX, Asl),
        0x20 => i(Mode::Jsr, Op::Jsr),
        0x21 => i(IndirectX, And),
        0x24 => i(ZeroPage, Bit),
        0x25 => i(ZeroPage, And),
        0x26 => i(ZeroPage, Rol),
        0x28 => i(Pull, Plp),
        0x29 => i(Immediate, And),
        0x2A => i(Implied, Rol),
        0x2C => i(Absolute, Bit),
        0x2D => i(Absolute, And),
        0x2E => i(Absolute, Rol),
        0x30 => i(Relative, Bmi),
        0x31 => i(IndirectY, And),
        0x35 => i(ZeroPageX, And),
        0x36 => i(ZeroPageX, Rol),
        0x38 => i(Implied, Sec),
        0x39 => i(AbsoluteY, And),
        0x3D => i(AbsoluteX, And),
        0x3E => i(AbsoluteX, Rol),
        0x40 => i(Mode::Rti, Op::Rti),
        0x41 => i(IndirectX, Eor),
        0x45 => i(ZeroPage, Eor),
        0x46 => i(ZeroPage, Lsr),
        0x48 => i(Push, Pha),
        0x49 => i(Immediate, Eor),
        0x4A => i(Implied, Lsr),
        0x4C => i(JumpAbsolute, Jmp),
        0x4D => i(Absolute, Eor),
        0x4E => i(Absolute, Lsr),
        0x50 => i(Relative, Bvc),
        0x51 => i(IndirectY, Eor),
        0x55 => i(ZeroPageX, Eor),
        0x56 => i(ZeroPageX, Lsr),
        0x58 => i(Implied, Cli),
        0x59 => i(AbsoluteY, Eor),
        0x5D => i(AbsoluteX, Eor),
        0x5E => i(AbsoluteX, Lsr),
        0x60 => i(Mode::Rts, Op::Rts),
        0x61 => i(IndirectX, Adc),
        0x65 => i(ZeroPage, Adc),
        0x66 => i(ZeroPage, Ror),
        0x68 => i(Pull, Pla),
        0x69 => i(Immediate, Adc),
        0x6A => i(Implied, Ror),
        0x6C => i(JumpIndirect, Jmp),
        0x6D => i(Absolute, Adc),
        0x6E => i(Absolute, Ror),
        0x70 => i(Relative, Bvs),
        0x71 => i(IndirectY, Adc),
        0x75 => i(ZeroPageX, Adc),
        0x76 => i(ZeroPageX, Ror),
        0x78 => i(Implied, Sei),
        0x79 => i(AbsoluteY, Adc),
        0x7D => i(AbsoluteX, Adc),
        0x7E => i(AbsoluteX, Ror),
        0x81 => i(IndirectX, Sta),
        0x84 => i(ZeroPage, Sty),
        0x85 => i(ZeroPage, Sta),
        0x86 => i(ZeroPage, Stx),
        0x88 => i(Implied, Dey),
        0x8A => i(Implied, Txa),
        0x8C => i(Absolute, Sty),
        0x8D => i(Absolute, Sta),
        0x8E => i(Absolute, Stx),
        0x90 => i(Relative, Bcc),
        0x91 => i(IndirectY, Sta),
        0x94 => i(ZeroPageX, Sty),
        0x95 => i(ZeroPageX, Sta),
        0x96 => i(ZeroPageY, Stx),
        0x98 => i(Implied, Tya),
        0x99 => i(AbsoluteY, Sta),
        0x9A => i(Implied, Txs),
        0x9D => i(AbsoluteX, Sta),
        0xA0 => i(Immediate, Ldy),
        0xA1 => i(IndirectX, Lda),
        0xA2 => i(Immediate, Ldx),
        0xA4 => i(ZeroPage, Ldy),
        0xA5 => i(ZeroPage, Lda),
        0xA6 => i(ZeroPage, Ldx),
        0xA8 => i(Implied, Tay),
        0xA9 => i(Immediate, Lda),
        0xAA => i(Implied, Tax),
        0xAC => i(Absolute, Ldy),
        0xAD => i(Absolute, Lda),
        0xAE => i(Absolute, Ldx),
        0xB0 => i(Relative, Bcs),
        0xB1 => i(IndirectY, Lda),
        0xB4 => i(ZeroPageX, Ldy),
        0xB5 => i(ZeroPageX, Lda),
        0xB6 => i(ZeroPageY, Ldx),
        0xB8 => i(Implied, Clv),
        0xB9 => i(AbsoluteY, Lda),
        0xBA => i(Implied, Tsx),
        0xBC => i(AbsoluteX, Ldy),
        0xBD => i(AbsoluteX, Lda),
        0xBE => i(AbsoluteY, Ldx),
        0xC0 => i(Immediate, Cpy),
        0xC1 => i(IndirectX, Cmp),
        0xC4 => i(ZeroPage, Cpy),
        0xC5 => i(ZeroPage, Cmp),
        0xC6 => i(ZeroPage, Dec),
        0xC8 => i(Implied, Iny),
        0xC9 => i(Immediate, Cmp),
        0xCA => i(Implied, Dex),
        0xCC => i(Absolute, Cpy),
        0xCD => i(Absolute, Cmp),
        0xCE => i(Absolute, Dec),
        0xD0 => i(Relative, Bne),
        0xD1 => i(IndirectY, Cmp),
        0xD5 => i(ZeroPageX, Cmp),
        0xD6 => i(ZeroPageX, Dec),
        0xD8 => i(Implied, Cld),
        0xD9 => i(AbsoluteY, Cmp),
        0xDD => i(AbsoluteX, Cmp),
        0xDE => i(AbsoluteX, Dec),
        0xE0 => i(Immediate, Cpx),
        0xE1 => i(IndirectX, Sbc),
        0xE4 => i(ZeroPage, Cpx),
        0xE5 => i(ZeroPage, Sbc),
        0xE6 => i(ZeroPage, Inc),
        0xE8 => i(Implied, Inx),
        0xE9 => i(Immediate, Sbc),
        0xEA => i(Implied, Nop),
        0xEC => i(Absolute, Cpx),
        0xED => i(Absolute, Sbc),
        0xEE => i(Absolute, Inc),
        0xF0 => i(Relative, Beq),
        0xF1 => i(IndirectY, Sbc),
        0xF5 => i(ZeroPageX, Sbc),
        0xF6 => i(ZeroPageX, Inc),
        0xF8 => i(Implied, Sed),
        0xF9 => i(AbsoluteY, Sbc),
        0xFD => i(AbsoluteX, Sbc),
        0xFE => i(AbsoluteX, Inc),
        _ => None,
    }
}
