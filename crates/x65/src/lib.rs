//! `x65`: a guest-agnostic processor core for the MOS 6502 family.
//!
//! The crate models the processor and nothing around it. Memory and devices
//! belong to the embedder, which the processor reaches through a bus with one
//! access per clock cycle, so a machine can advance its other chips cycle by
//! cycle. What differs between chips of the family (the RP2A03 of the NES has
//! no decimal arithmetic, the 6510 of the Commodore 64 has an I/O port) is a
//! parameter of the model, never conditional compilation or a fork.
//!
//! Rules of the crate, from decision-1 and the design note
//! (`backlog/docs/design/nes-core.md` §4):
//!
//! - no knowledge of any machine: no picture or sound chip, no cartridge
//!   board, no memory map;
//! - `no_std`, no allocation, no dependency of any kind, no `unsafe`;
//! - state is plain data an embedder can save and restore;
//! - every behaviour cites its source (MCS6500 Microcomputer Family
//!   Programming Manual and Hardware Manual, MOS Technology, January 1976)
//!   and has a witness test (decision-2).
//!
//! The API is decision-4: a [`Bus`] with one call per cycle and the
//! [`Lines`] read after each access; a [`Cpu`] that is a state machine of
//! cycles, run one cycle at a time by [`Cpu::tick`] or one instruction at a
//! time by [`Cpu::step`]; and its whole state as plain data, [`CpuState`].
//!
//! Implemented today: the documented instructions of the [`Model::Rp2a03`],
//! the reset sequence, and NMI and IRQ polled at the end of an instruction's
//! second-to-last cycle. An undocumented opcode answers
//! [`Event::Unsupported`].

#![no_std]
#![forbid(unsafe_code)]

mod bus;
mod cpu;
mod decode;
mod state;

pub use crate::bus::{Bus, Lines};
pub use crate::cpu::Cpu;
pub use crate::state::{flags, CpuState, Event, Interrupt, Model, Sequence};
