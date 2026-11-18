//! The processor: a state machine of cycles (decision-4).
//!
//! Every instruction is the cycle sequence of its addressing mode (MCS6500
//! Hardware Manual, MOS Technology, January 1976, Appendix A) with its
//! operation applied where the data is at hand. One [`Cpu::tick`] makes one
//! cycle, which is one bus call; everything an instruction holds between two
//! cycles is in [`CpuState`], so the processor can stop after any cycle.
//!
//! Interrupts (NESdev wiki, "CPU interrupts", revision 21632): the NMI input
//! goes through an edge detector and IRQ is a level; the decision to take
//! one is the state polled at the end of an instruction's second-to-last
//! cycle, so CLI, SEI and PLP change the I flag after the poll and RTI before
//! it. Interrupt sequences do not poll. The finer points (polling inside
//! branches, NMI hijacking BRK) and their hardware witness, blargg's
//! `cpu_interrupts_v2`, are milestone N1's.
//!
//! Reset (Programming Manual, January 1976, §9.1; Hardware Manual §1.4.1.2.11;
//! NESdev wiki, "CPU power up state", revision 22091, and "CPU interrupts"):
//! while RES is held the processor makes no progress and writes nothing; when
//! the line is released it runs the interrupt sequence with its three stack
//! writes turned into reads, so S drops by 3 and memory is untouched, then I
//! is set and PC loaded from `$FFFC`. One press gives one sequence however
//! long the line is held. Holding the line in the middle of an instruction is
//! milestone N1's; today the processor notices it at the next boundary.

use crate::bus::{Bus, Lines};
use crate::decode::{decode, Access, Instruction, Mode, Op};
use crate::state::flags::{
    BREAK, CARRY, DECIMAL, INTERRUPT_DISABLE, NEGATIVE, OVERFLOW, UNUSED, ZERO,
};
use crate::state::{CpuState, Event, Interrupt, Model, Sequence};

const NMI_VECTOR: u16 = 0xFFFA;
const RESET_VECTOR: u16 = 0xFFFC;
const IRQ_VECTOR: u16 = 0xFFFE;

/// A 6502-family processor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cpu {
    s: CpuState,
}

/// What one cycle of an instruction leaves to do.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Next {
    /// More cycles follow.
    Continue,
    /// This was the instruction's last cycle.
    Done,
}

impl Cpu {
    /// A processor just powered on: the next cycle starts the reset sequence.
    ///
    /// Registers start at the values the NESdev wiki's "CPU power up state"
    /// (revision 22091) measured on an RP2A03G: A, X, Y and S zero, P with
    /// only I (and bit 5) set; the reset sequence then leaves S at `$FD`.
    pub fn new(model: Model) -> Self {
        Cpu {
            s: CpuState {
                model,
                pc: 0,
                a: 0,
                x: 0,
                y: 0,
                s: 0,
                p: UNUSED | INTERRUPT_DISABLE,
                sequence: Sequence::Reset,
                opcode: 0,
                cycle: 0,
                address: 0,
                pointer: 0,
                data: 0,
                carry: false,
                nmi_line: false,
                nmi_pending: false,
                irq_line: false,
                poll_nmi: false,
                poll_irq: false,
                reset_line: false,
                reset_pending: true,
            },
        }
    }

    /// A copy of everything the processor holds.
    pub fn state(&self) -> CpuState {
        self.s
    }

    /// Replace everything the processor holds with `state`, which should come
    /// from [`state`](Self::state) or be at an instruction boundary
    /// (`cycle` 0).
    pub fn set_state(&mut self, state: CpuState) {
        self.s = state;
    }

    /// Run until an instruction, or an interrupt or reset sequence, completes.
    pub fn step<B: Bus + ?Sized>(&mut self, bus: &mut B) -> Event {
        loop {
            if let Some(event) = self.tick(bus) {
                return event;
            }
        }
    }

    /// Run exactly one cycle: one bus call, then the input lines are sampled.
    /// Returns what the cycle completed, if anything.
    pub fn tick<B: Bus + ?Sized>(&mut self, bus: &mut B) -> Option<Event> {
        let (event, polls) = if self.s.cycle == 0 {
            self.start(bus)
        } else {
            self.resume(bus)
        };
        let lines = bus.lines();
        self.sample(lines);
        if polls {
            self.poll();
        }
        event
    }

    // ----- input lines -----

    fn sample(&mut self, lines: Lines) {
        if lines.nmi && !self.s.nmi_line {
            self.s.nmi_pending = true;
        }
        self.s.nmi_line = lines.nmi;
        self.s.irq_line = lines.irq;
        self.s.reset_line = lines.reset;
        if lines.reset {
            self.s.reset_pending = true;
        }
    }

    /// Latch the interrupt decision. Called after every cycle of an
    /// instruction except its last, so the last call is the one at the end of
    /// the second-to-last cycle.
    fn poll(&mut self) {
        self.s.poll_nmi = self.s.nmi_pending;
        self.s.poll_irq = self.s.irq_line && self.s.p & INTERRUPT_DISABLE == 0;
    }

    // ----- sequences -----

    /// The first cycle of the next sequence: an opcode fetch, or the first
    /// cycle of a reset or interrupt sequence. Returns the event and whether
    /// the cycle polls.
    fn start<B: Bus + ?Sized>(&mut self, bus: &mut B) -> (Option<Event>, bool) {
        if self.s.reset_line {
            // RES held: no progress, no write (§1.4.1.2.11).
            bus.read(self.s.pc);
            return (None, false);
        }
        let sequence = if self.s.reset_pending {
            self.s.reset_pending = false;
            Sequence::Reset
        } else if self.s.poll_nmi {
            self.s.nmi_pending = false;
            Sequence::Nmi
        } else if self.s.poll_irq {
            Sequence::Irq
        } else {
            Sequence::Instruction
        };
        self.s.poll_nmi = false;
        self.s.poll_irq = false;
        self.s.sequence = sequence;
        self.s.carry = false;

        if sequence != Sequence::Instruction {
            // The opcode fetch is made and ignored; PC does not advance
            // ("CPU interrupts": the sequence is BRK's without the PC
            // increments).
            bus.read(self.s.pc);
            self.s.opcode = 0x00;
            self.s.cycle = 1;
            return (None, false);
        }

        let opcode = bus.read(self.s.pc);
        self.s.opcode = opcode;
        if decode(self.s.model, opcode).is_none() {
            return (Some(Event::Unsupported { opcode }), false);
        }
        self.s.pc = self.s.pc.wrapping_add(1);
        self.s.cycle = 1;
        (None, true)
    }

    fn resume<B: Bus + ?Sized>(&mut self, bus: &mut B) -> (Option<Event>, bool) {
        let cycle = self.s.cycle;
        let next = match self.s.sequence {
            Sequence::Instruction => match decode(self.s.model, self.s.opcode) {
                Some(instruction) => self.instruction_cycle(bus, instruction, cycle),
                // Only a state restored from outside can get here: end the
                // sequence with a harmless read and report the opcode as
                // unsupported, as a fetch of it would.
                None => {
                    bus.read(self.s.pc);
                    self.s.cycle = 0;
                    let opcode = self.s.opcode;
                    return (Some(Event::Unsupported { opcode }), false);
                }
            },
            Sequence::Nmi | Sequence::Irq | Sequence::Reset => self.interrupt_cycle(bus, cycle),
        };
        match next {
            Next::Continue => {
                self.s.cycle = cycle + 1;
                (None, self.s.sequence == Sequence::Instruction)
            }
            Next::Done => {
                self.s.cycle = 0;
                let event = match self.s.sequence {
                    Sequence::Instruction => Event::Instruction {
                        opcode: self.s.opcode,
                    },
                    Sequence::Nmi => Event::Interrupt(Interrupt::Nmi),
                    Sequence::Irq => Event::Interrupt(Interrupt::Irq),
                    Sequence::Reset => Event::Reset,
                };
                (Some(event), false)
            }
        }
    }

    /// Cycles 1 to 6 of a reset, NMI or IRQ sequence (Hardware Manual A.5.4;
    /// "CPU interrupts").
    fn interrupt_cycle<B: Bus + ?Sized>(&mut self, bus: &mut B, cycle: u8) -> Next {
        let reset = self.s.sequence == Sequence::Reset;
        match cycle {
            1 => {
                bus.read(self.s.pc);
                Next::Continue
            }
            2..=4 => {
                let value = match cycle {
                    2 => (self.s.pc >> 8) as u8,
                    3 => self.s.pc as u8,
                    // B is pushed clear by an interrupt.
                    _ => self.s.p & !BREAK | UNUSED,
                };
                if reset {
                    bus.read(self.stack_address());
                } else {
                    bus.write(self.stack_address(), value);
                }
                self.s.s = self.s.s.wrapping_sub(1);
                Next::Continue
            }
            5 => {
                self.s.data = bus.read(self.vector());
                self.s.p |= INTERRUPT_DISABLE;
                Next::Continue
            }
            _ => {
                let high = bus.read(self.vector().wrapping_add(1));
                self.s.pc = u16::from_le_bytes([self.s.data, high]);
                Next::Done
            }
        }
    }

    fn vector(&self) -> u16 {
        match self.s.sequence {
            Sequence::Nmi => NMI_VECTOR,
            Sequence::Reset => RESET_VECTOR,
            _ => IRQ_VECTOR,
        }
    }

    /// One cycle (1 or later) of an instruction.
    fn instruction_cycle<B: Bus + ?Sized>(
        &mut self,
        bus: &mut B,
        instruction: Instruction,
        cycle: u8,
    ) -> Next {
        let op = instruction.op;
        match instruction.mode {
            // A.1: the byte after the opcode is read and ignored.
            Mode::Implied => {
                bus.read(self.s.pc);
                self.implied(op);
                Next::Done
            }
            // A.2.1.
            Mode::Immediate => {
                let value = self.fetch(bus);
                self.read_op(op, value);
                Next::Done
            }
            // A.2.2, A.3.1, A.4.1.
            Mode::ZeroPage => match cycle {
                1 => {
                    self.s.address = u16::from(self.fetch(bus));
                    Next::Continue
                }
                _ => self.memory(bus, op, cycle - 2),
            },
            // A.2.6, A.3.5, A.4.3: the base is read once and ignored; the
            // sum stays in page zero.
            Mode::ZeroPageX | Mode::ZeroPageY => match cycle {
                1 => {
                    self.s.pointer = self.fetch(bus);
                    Next::Continue
                }
                2 => {
                    bus.read(u16::from(self.s.pointer));
                    let index = if instruction.mode == Mode::ZeroPageX {
                        self.s.x
                    } else {
                        self.s.y
                    };
                    self.s.address = u16::from(self.s.pointer.wrapping_add(index));
                    Next::Continue
                }
                _ => self.memory(bus, op, cycle - 3),
            },
            // A.2.3, A.3.2, A.4.2.
            Mode::Absolute => match cycle {
                1 => {
                    self.s.address = u16::from(self.fetch(bus));
                    Next::Continue
                }
                2 => {
                    self.s.address |= u16::from(self.fetch(bus)) << 8;
                    Next::Continue
                }
                _ => self.memory(bus, op, cycle - 3),
            },
            // A.2.5, A.3.4, A.4.4.
            Mode::AbsoluteX | Mode::AbsoluteY => match cycle {
                1 => {
                    self.s.pointer = self.fetch(bus);
                    Next::Continue
                }
                2 => {
                    let high = self.fetch(bus);
                    let index = if instruction.mode == Mode::AbsoluteX {
                        self.s.x
                    } else {
                        self.s.y
                    };
                    self.index_into(high, index);
                    Next::Continue
                }
                _ => self.indexed(bus, op, cycle - 3),
            },
            // A.2.4, A.3.3.
            Mode::IndirectX => match cycle {
                1 => {
                    self.s.pointer = self.fetch(bus);
                    Next::Continue
                }
                2 => {
                    bus.read(u16::from(self.s.pointer));
                    self.s.pointer = self.s.pointer.wrapping_add(self.s.x);
                    Next::Continue
                }
                3 => {
                    self.s.address = u16::from(bus.read(u16::from(self.s.pointer)));
                    Next::Continue
                }
                4 => {
                    let high = bus.read(u16::from(self.s.pointer.wrapping_add(1)));
                    self.s.address |= u16::from(high) << 8;
                    Next::Continue
                }
                _ => self.memory(bus, op, cycle - 5),
            },
            // A.2.7, A.3.6.
            Mode::IndirectY => match cycle {
                1 => {
                    self.s.data = self.fetch(bus);
                    Next::Continue
                }
                2 => {
                    self.s.pointer = bus.read(u16::from(self.s.data));
                    Next::Continue
                }
                3 => {
                    let high = bus.read(u16::from(self.s.data.wrapping_add(1)));
                    self.index_into(high, self.s.y);
                    Next::Continue
                }
                _ => self.indexed(bus, op, cycle - 4),
            },
            Mode::Relative => self.branch(bus, op, cycle),
            // A.5.6.
            Mode::JumpAbsolute => match cycle {
                1 => {
                    self.s.data = self.fetch(bus);
                    Next::Continue
                }
                _ => {
                    let high = bus.read(self.s.pc);
                    self.s.pc = u16::from_le_bytes([self.s.data, high]);
                    Next::Done
                }
            },
            // A.5.6: the pointer's high byte is not carried into, so a pointer
            // at the end of a page wraps within it.
            Mode::JumpIndirect => match cycle {
                1 => {
                    self.s.address = u16::from(self.fetch(bus));
                    Next::Continue
                }
                2 => {
                    self.s.address |= u16::from(self.fetch(bus)) << 8;
                    Next::Continue
                }
                3 => {
                    self.s.data = bus.read(self.s.address);
                    Next::Continue
                }
                _ => {
                    let address = self.s.address & 0xFF00 | self.s.address.wrapping_add(1) & 0x00FF;
                    let high = bus.read(address);
                    self.s.pc = u16::from_le_bytes([self.s.data, high]);
                    Next::Done
                }
            },
            // A.5.3.
            Mode::Jsr => match cycle {
                1 => {
                    self.s.data = self.fetch(bus);
                    Next::Continue
                }
                2 => {
                    bus.read(self.stack_address());
                    Next::Continue
                }
                3 => {
                    self.push(bus, (self.s.pc >> 8) as u8);
                    Next::Continue
                }
                4 => {
                    self.push(bus, self.s.pc as u8);
                    Next::Continue
                }
                _ => {
                    let high = bus.read(self.s.pc);
                    self.s.pc = u16::from_le_bytes([self.s.data, high]);
                    Next::Done
                }
            },
            // A.5.7.
            Mode::Rts => match cycle {
                1 => {
                    bus.read(self.s.pc);
                    Next::Continue
                }
                2 => {
                    bus.read(self.stack_address());
                    self.s.s = self.s.s.wrapping_add(1);
                    Next::Continue
                }
                3 => {
                    self.s.data = bus.read(self.stack_address());
                    self.s.s = self.s.s.wrapping_add(1);
                    Next::Continue
                }
                4 => {
                    let high = bus.read(self.stack_address());
                    self.s.pc = u16::from_le_bytes([self.s.data, high]);
                    Next::Continue
                }
                _ => {
                    bus.read(self.s.pc);
                    self.s.pc = self.s.pc.wrapping_add(1);
                    Next::Done
                }
            },
            // A.5.5.
            Mode::Rti => match cycle {
                1 => {
                    bus.read(self.s.pc);
                    Next::Continue
                }
                2 => {
                    bus.read(self.stack_address());
                    self.s.s = self.s.s.wrapping_add(1);
                    Next::Continue
                }
                3 => {
                    let p = bus.read(self.stack_address());
                    self.set_p(p);
                    self.s.s = self.s.s.wrapping_add(1);
                    Next::Continue
                }
                4 => {
                    self.s.data = bus.read(self.stack_address());
                    self.s.s = self.s.s.wrapping_add(1);
                    Next::Continue
                }
                _ => {
                    let high = bus.read(self.stack_address());
                    self.s.pc = u16::from_le_bytes([self.s.data, high]);
                    Next::Done
                }
            },
            // A.5.4: the byte after BRK is fetched and skipped; B is pushed
            // set (Programming Manual §9.11).
            Mode::Brk => match cycle {
                1 => {
                    self.fetch(bus);
                    Next::Continue
                }
                2 => {
                    self.push(bus, (self.s.pc >> 8) as u8);
                    Next::Continue
                }
                3 => {
                    self.push(bus, self.s.pc as u8);
                    Next::Continue
                }
                4 => {
                    self.push(bus, self.s.p | BREAK | UNUSED);
                    Next::Continue
                }
                5 => {
                    self.s.data = bus.read(IRQ_VECTOR);
                    self.s.p |= INTERRUPT_DISABLE;
                    Next::Continue
                }
                _ => {
                    let high = bus.read(IRQ_VECTOR + 1);
                    self.s.pc = u16::from_le_bytes([self.s.data, high]);
                    Next::Done
                }
            },
            // A.5.1: PHP pushes B set (§8.11).
            Mode::Push => match cycle {
                1 => {
                    bus.read(self.s.pc);
                    Next::Continue
                }
                _ => {
                    let value = if op == Op::Php {
                        self.s.p | BREAK | UNUSED
                    } else {
                        self.s.a
                    };
                    self.push(bus, value);
                    Next::Done
                }
            },
            // A.5.2.
            Mode::Pull => match cycle {
                1 => {
                    bus.read(self.s.pc);
                    Next::Continue
                }
                2 => {
                    bus.read(self.stack_address());
                    self.s.s = self.s.s.wrapping_add(1);
                    Next::Continue
                }
                _ => {
                    let value = bus.read(self.stack_address());
                    if op == Op::Plp {
                        self.set_p(value);
                    } else {
                        self.s.a = value;
                        self.set_nz(value);
                    }
                    Next::Done
                }
            },
        }
    }

    /// The cycles at the effective address, for modes whose address is final
    /// when computed. `step` counts from the first of them.
    fn memory<B: Bus + ?Sized>(&mut self, bus: &mut B, op: Op, step: u8) -> Next {
        match op.access() {
            Access::Read => {
                let value = bus.read(self.s.address);
                self.read_op(op, value);
                Next::Done
            }
            Access::Write => {
                bus.write(self.s.address, self.store_value(op));
                Next::Done
            }
            Access::ReadModifyWrite => self.modify(bus, op, step),
        }
    }

    /// Read-modify-write (A.4): read, write the unmodified value back, write
    /// the result.
    fn modify<B: Bus + ?Sized>(&mut self, bus: &mut B, op: Op, step: u8) -> Next {
        match step {
            0 => {
                self.s.data = bus.read(self.s.address);
                Next::Continue
            }
            1 => {
                bus.write(self.s.address, self.s.data);
                self.s.data = self.modify_op(op, self.s.data);
                Next::Continue
            }
            _ => {
                bus.write(self.s.address, self.s.data);
                Next::Done
            }
        }
    }

    /// Put `low + index` in the address latch under `high` without the
    /// carry, and remember whether a carry is owed (A.2.5, A.2.7).
    fn index_into(&mut self, high: u8, index: u8) {
        let (low, carry) = self.s.pointer.overflowing_add(index);
        self.s.address = u16::from_le_bytes([low, high]);
        self.s.carry = carry;
    }

    /// The cycles after an indexed address was formed without its carry
    /// (absolute indexed, indirect indexed). A read without a carry is done in
    /// one cycle; otherwise the first cycle reads the uncorrected address and
    /// is ignored.
    fn indexed<B: Bus + ?Sized>(&mut self, bus: &mut B, op: Op, step: u8) -> Next {
        if step == 0 {
            let value = bus.read(self.s.address);
            if self.s.carry {
                self.s.address = self.s.address.wrapping_add(0x100);
                self.s.carry = false;
            } else if op.access() == Access::Read {
                self.read_op(op, value);
                return Next::Done;
            }
            return Next::Continue;
        }
        self.memory(bus, op, step - 1)
    }

    /// Branches. The offset is fetched; a taken branch then reads the next
    /// opcode's address while it adds the offset to PCL, and a page crossing
    /// costs one more cycle that reads the target without its carry.
    ///
    /// Sources disagree here (decision-2). The Hardware Manual's table A.5.8
    /// puts the target without carry on the address bus in T2 and the target
    /// with carry in T3. The SingleStepTests trace reads PC + 2 in T2 and the
    /// target without carry in T3, for example `BNE` at `$7711` with offset
    /// `$C6`: `$7711`, `$7712`, `$7713`, `$77D9`, then `$76D9`; the NESdev
    /// wiki's "CPU interrupts" (revision 21632) describes the same cycles
    /// when it places the interrupt polls. Ruling: follow the measured trace;
    /// the manual's table reads as describing addresses one cycle early.
    fn branch<B: Bus + ?Sized>(&mut self, bus: &mut B, op: Op, cycle: u8) -> Next {
        match cycle {
            1 => {
                self.s.data = self.fetch(bus);
                if self.branch_taken(op) {
                    Next::Continue
                } else {
                    Next::Done
                }
            }
            2 => {
                bus.read(self.s.pc);
                let target = self.s.pc.wrapping_add(self.s.data as i8 as u16);
                self.s.address = target;
                self.s.carry = target & 0xFF00 != self.s.pc & 0xFF00;
                self.s.pc = self.s.pc & 0xFF00 | target & 0x00FF;
                if self.s.carry {
                    Next::Continue
                } else {
                    Next::Done
                }
            }
            _ => {
                bus.read(self.s.pc);
                self.s.pc = self.s.address;
                self.s.carry = false;
                Next::Done
            }
        }
    }

    fn branch_taken(&self, op: Op) -> bool {
        let p = self.s.p;
        match op {
            Op::Bcc => p & CARRY == 0,
            Op::Bcs => p & CARRY != 0,
            Op::Bne => p & ZERO == 0,
            Op::Beq => p & ZERO != 0,
            Op::Bpl => p & NEGATIVE == 0,
            Op::Bmi => p & NEGATIVE != 0,
            Op::Bvc => p & OVERFLOW == 0,
            Op::Bvs => p & OVERFLOW != 0,
            _ => false,
        }
    }

    // ----- bus helpers -----

    /// Read the byte at PC and advance PC.
    fn fetch<B: Bus + ?Sized>(&mut self, bus: &mut B) -> u8 {
        let value = bus.read(self.s.pc);
        self.s.pc = self.s.pc.wrapping_add(1);
        value
    }

    fn stack_address(&self) -> u16 {
        0x0100 | u16::from(self.s.s)
    }

    fn push<B: Bus + ?Sized>(&mut self, bus: &mut B, value: u8) {
        bus.write(self.stack_address(), value);
        self.s.s = self.s.s.wrapping_sub(1);
    }

    // ----- operations (Programming Manual, chapters 2 to 10) -----

    fn set_p(&mut self, value: u8) {
        self.s.p = value & !BREAK | UNUSED;
    }

    fn set_flag(&mut self, flag: u8, on: bool) {
        if on {
            self.s.p |= flag;
        } else {
            self.s.p &= !flag;
        }
    }

    fn set_nz(&mut self, value: u8) {
        self.set_flag(ZERO, value == 0);
        self.set_flag(NEGATIVE, value & 0x80 != 0);
    }

    fn store_value(&self, op: Op) -> u8 {
        match op {
            Op::Stx => self.s.x,
            Op::Sty => self.s.y,
            _ => self.s.a,
        }
    }

    fn read_op(&mut self, op: Op, value: u8) {
        match op {
            Op::Lda => {
                self.s.a = value;
                self.set_nz(value);
            }
            Op::Ldx => {
                self.s.x = value;
                self.set_nz(value);
            }
            Op::Ldy => {
                self.s.y = value;
                self.set_nz(value);
            }
            Op::And => {
                self.s.a &= value;
                self.set_nz(self.s.a);
            }
            Op::Ora => {
                self.s.a |= value;
                self.set_nz(self.s.a);
            }
            Op::Eor => {
                self.s.a ^= value;
                self.set_nz(self.s.a);
            }
            Op::Adc => self.add(value),
            // §2.2.2: subtraction adds the complement.
            Op::Sbc => self.add(!value),
            Op::Cmp => self.compare(self.s.a, value),
            Op::Cpx => self.compare(self.s.x, value),
            Op::Cpy => self.compare(self.s.y, value),
            // §4.2.2: Z from A AND M; N and V are bits 7 and 6 of M.
            Op::Bit => {
                self.set_flag(ZERO, self.s.a & value == 0);
                self.set_flag(NEGATIVE, value & 0x80 != 0);
                self.set_flag(OVERFLOW, value & 0x40 != 0);
            }
            _ => {}
        }
    }

    /// §2.2.1 and §2.2.3: binary addition with carry. The RP2A03 has no
    /// decimal arithmetic, so D is ignored.
    fn add(&mut self, value: u8) {
        let a = self.s.a;
        let sum = u16::from(a) + u16::from(value) + u16::from(self.s.p & CARRY);
        let result = sum as u8;
        self.set_flag(CARRY, sum > 0xFF);
        self.set_flag(OVERFLOW, (a ^ result) & (value ^ result) & 0x80 != 0);
        self.s.a = result;
        self.set_nz(result);
    }

    /// §4.2.1: compare sets C when the register is not less than memory.
    fn compare(&mut self, register: u8, value: u8) {
        self.set_flag(CARRY, register >= value);
        self.set_nz(register.wrapping_sub(value));
    }

    /// Chapter 10: shifts, rotates, increment and decrement.
    fn modify_op(&mut self, op: Op, value: u8) -> u8 {
        let carry_in = self.s.p & CARRY;
        let result = match op {
            Op::Asl => {
                self.set_flag(CARRY, value & 0x80 != 0);
                value << 1
            }
            Op::Lsr => {
                self.set_flag(CARRY, value & 0x01 != 0);
                value >> 1
            }
            Op::Rol => {
                self.set_flag(CARRY, value & 0x80 != 0);
                value << 1 | carry_in
            }
            Op::Ror => {
                self.set_flag(CARRY, value & 0x01 != 0);
                value >> 1 | carry_in << 7
            }
            Op::Inc => value.wrapping_add(1),
            Op::Dec => value.wrapping_sub(1),
            _ => value,
        };
        self.set_nz(result);
        result
    }

    /// Single byte instructions (A.1); the shifts act on A.
    fn implied(&mut self, op: Op) {
        match op {
            Op::Asl | Op::Lsr | Op::Rol | Op::Ror => {
                self.s.a = self.modify_op(op, self.s.a);
            }
            Op::Clc => self.set_flag(CARRY, false),
            Op::Sec => self.set_flag(CARRY, true),
            Op::Cli => self.set_flag(INTERRUPT_DISABLE, false),
            Op::Sei => self.set_flag(INTERRUPT_DISABLE, true),
            Op::Clv => self.set_flag(OVERFLOW, false),
            Op::Cld => self.set_flag(DECIMAL, false),
            Op::Sed => self.set_flag(DECIMAL, true),
            Op::Tax => {
                self.s.x = self.s.a;
                self.set_nz(self.s.x);
            }
            Op::Txa => {
                self.s.a = self.s.x;
                self.set_nz(self.s.a);
            }
            Op::Tay => {
                self.s.y = self.s.a;
                self.set_nz(self.s.y);
            }
            Op::Tya => {
                self.s.a = self.s.y;
                self.set_nz(self.s.a);
            }
            Op::Tsx => {
                self.s.x = self.s.s;
                self.set_nz(self.s.x);
            }
            // §8.8: TXS changes no flag.
            Op::Txs => self.s.s = self.s.x,
            Op::Inx => {
                self.s.x = self.s.x.wrapping_add(1);
                self.set_nz(self.s.x);
            }
            Op::Iny => {
                self.s.y = self.s.y.wrapping_add(1);
                self.set_nz(self.s.y);
            }
            Op::Dex => {
                self.s.x = self.s.x.wrapping_sub(1);
                self.set_nz(self.s.x);
            }
            Op::Dey => {
                self.s.y = self.s.y.wrapping_sub(1);
                self.set_nz(self.s.y);
            }
            _ => {}
        }
    }
}
