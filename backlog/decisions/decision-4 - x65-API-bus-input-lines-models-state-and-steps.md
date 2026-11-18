---
id: decision-4
title: x65 API — bus, input lines, models, state and steps
date: '2026-11-18'
status: accepted
---

Decider: maintainer. Bead `nes-7fr.3`. Builds on decision-1 (the crate's
rules), the [design note](../docs/design/nes-core.md) §3 and §4, and the rule
for the processor crate in core-contract.md §13.5 (unemupc).

## Context

`x65` must let a machine advance its other chips cycle by cycle, take its
interrupt, ready and reset inputs where the processor samples them, keep its
state as plain data, and stay `no_std` without allocation or dependencies.
The NES machine drives the PPU and APU from the CPU's bus accesses (design
§3), so every register access sees exact chip state without the CPU knowing
the machine.

The contract counts work in **steps** of a unit the core names, and lets a
run pass its deadline by less than one step (core-contract.md §3.5
(unemupc)). The design note named the CPU cycle as the NES's step. A run that
stops at any cycle needs a processor that can stop in the middle of an
instruction. A processor written as straight-line code per instruction
cannot: adding that later means rewriting every instruction. The maintainer
asked (2026-11-18) that cycle stepping must never need such a remodel.

## Choice

1. **The bus.** One call is one CPU cycle; the embedder advances its clock in
   each call.

   ```rust
   pub trait Bus {
       fn read(&mut self, address: u16) -> u8;
       fn write(&mut self, address: u16, value: u8);
       /// The input lines as they are after the access just made.
       fn lines(&mut self) -> Lines;
   }

   #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
   pub struct Lines {
       pub nmi: bool,   // asserted (the pin is low)
       pub irq: bool,   // asserted (the pin is low)
       pub reset: bool, // asserted (the pin is low)
       pub halt: bool,  // RDY pulled low: the processor stops on a read cycle
   }
   ```

   `Bus` is object-safe, so an embedder can pass `&mut dyn Bus`; `x65` takes
   it generically so the inner loop is monomorphised. Every cycle the
   processor makes is one call, dummy reads and the double write of
   read-modify-write instructions included.

2. **The processor is a state machine of cycles inside.** Each instruction
   is a sequence of cycles built from addressing-mode templates (immediate,
   zero page, absolute, indexed, indirect, read-modify-write, stack, branch,
   interrupt sequence) and an operation; one cycle is one bus call. Everything
   an instruction holds between its cycles (the opcode, the cycle index, the
   address and data latches, pending interrupts) is a field of `CpuState`, so
   the processor can stop after any cycle and a saved state can be taken
   there. Two public ways to run it, from the first version:

   ```rust
   pub struct Cpu { /* private */ }

   impl Cpu {
       /// Power on: the next cycle starts the reset sequence.
       pub fn new(model: Model) -> Self;
       /// Run exactly one cycle: one bus call. `Some` when it completed an
       /// instruction or an interrupt or reset sequence.
       pub fn tick<B: Bus>(&mut self, bus: &mut B) -> Option<Event>;
       /// Run cycles until an instruction or an interrupt or reset sequence
       /// completes.
       pub fn step<B: Bus>(&mut self, bus: &mut B) -> Event;
       pub fn state(&self) -> CpuState;
       pub fn set_state(&mut self, state: CpuState);
   }

   /// Everything the processor holds: registers, flags, the instruction in
   /// flight and its latches, pending interrupts, the model. Plain data for
   /// saved states, valid after any cycle.
   #[derive(Clone, Copy, Debug, PartialEq, Eq)]
   pub struct CpuState { /* public fields */ }

   #[non_exhaustive]
   pub enum Model { Nmos6502, Rp2a03 }

   #[non_exhaustive]
   pub enum Event {
       Instruction { opcode: u8 },
       Interrupt(Interrupt),   // Nmi or Irq
       Reset,
       /// An opcode this model does not implement yet: one fetch cycle was
       /// made, nothing else changed, PC still points at the opcode.
       Unsupported { opcode: u8 },
   }
   ```

   `Model::Rp2a03` is the NMOS core with the D flag kept and no decimal
   arithmetic. Later differences (the 6510's port, CMOS instructions) are new
   model variants or model parameters.

3. **The NES's contract step is one instruction.** The machine names its
   step `instruction`: `steps()` counts instructions and interrupt or reset
   sequences, as the PC core counts retired instructions, and budgets read
   naturally. Ticks stay master-clock cycles, and sub-instruction timing is
   exact anyway, because the machine advances its chips in every bus call.
   The proof slice runs the machine by `step`; stopping a run on the exact
   cycle of its deadline is `tick` in the machine's run loop, with no change
   to `x65`.

4. **Scope by stage.** The proof slice implements the documented instructions
   of the `Rp2a03` model, the reset sequence, and NMI and IRQ taken at
   instruction boundaries; it ignores `halt`, and the machine makes its DMA
   stall itself. N1 adds the exact polling points, `halt` on read cycles,
   reset in the middle of an instruction, the `Nmos6502` model with decimal
   arithmetic and the undocumented opcodes, each with its source and witness
   (decision-2). Until then an undocumented opcode answers
   `Event::Unsupported`, as x80 does (`decision-3 (x80)`); the machine turns
   it into a `Guest` error.

## Alternatives

- **Straight-line code per instruction**, `step` only. Cheaper by perhaps a
  third today, and the natural way to write a 6502. Rejected: cycle stepping
  (two processors interleaved by the cycle, such as a C64 with an exact 1541
  drive; a cycle debugger) would need every instruction rewritten.
- **Instructions as `async fn` polled one cycle at a time.** Straight-line
  source with a compiler-built state machine. Rejected: the state in flight
  is an opaque future, not plain data a saved state can hold, and naming its
  type for a `no_std` struct without allocation needs unstable features.
- **Lines as setters on `Cpu`.** Rejected: the embedder changes the lines
  inside a bus call, while `step` holds the processor; the bus is where it
  can answer.
- **Public fields on `Cpu`.** Rejected: pending-interrupt latches have
  invariants; `CpuState` is the plain copy for saving.

## Consequences

- The design note's §3 and §9 change the NES's step unit from `cycle` to
  `instruction`; the unemupc supervisor is told, because the NTSC numbers it
  holds name `cycle`.
- The machine counts CPU cycles itself, from its bus calls, for its master
  clock.
- The CPU costs more to write now than straight-line code. In return the
  exact interrupt polling, `halt` on read cycles and reset in the middle of
  an instruction of N1 fall out of the cycle model rather than being bolted
  on.

## Trigger to revisit

- A model needs a line or an event this shape cannot carry.
- Per-cycle dispatch proves too slow for a machine (none expected: the NES
  runs the CPU at 1.79 MHz).
