---
id: decision-1
title: Crate layout and the extractable 6502 crate x65
date: '2026-11-18'
status: accepted
---

Decider: maintainer. Settles [the design note](../docs/design/nes-core.md)
§14 Q1, Q2 and Q3, with the maintainer's decision of the same day that the
6502 is a crate of this workspace, written to be extractable. Design in §2
and §4.

## Context

unemunes is the second core on the `unemu-core-api` contract
(`decision-40 (unemupc)`, `decision-41 (unemupc)`). Its 6502 should serve a
later core too, a Commodore 64 for example, so it must not know the NES.
core-contract.md §13.5 (unemupc) states the rule for that crate from the
front end's side. The NES's other chips (the 2C02 PPU, the 2A03's APU, the
cartridge boards) exist only inside this console.

## Choice

- Four crates: `x65` (the 6502), `nes` (cartridges and mappers, PPU, APU,
  controllers, the console's bus and scheduler, saved state),
  `unemu-nes-core` (the contract adapter) and `unemunes` (the command line,
  the only crate that touches the host).
- `x65`, named after `x80` and `x86jit`, is guest-agnostic: memory through a
  `Bus` trait it defines, one call per bus cycle; IRQ, NMI, RDY and RESET as
  input lines it samples where the processor does; chip differences (no
  decimal arithmetic in the 2A03, the 6510's port, later CMOS instructions)
  as model parameters, never conditional compilation or a fork; plain-data
  state; its own tests against a published per-instruction test set and its
  own docs citing the processor's data sheets.
- `x65` is `#![no_std]`, uses no allocation, has no dependency and forbids
  unsafe code, like `x80-core` (`decision-4 (x80)`). It depends on no other
  crate of this workspace and not on the contract, so leaving for its own
  repository is a move of one directory.
- The PPU, the APU and the mappers are modules of `nes` behind traits, each
  with isolation tests on a mock bus.
- A boundary test enforces the dependency rules and keeps host interfaces of
  `std` out of every crate but `unemunes`.

## Alternatives

- A crate per chip from the start. Rejected for now: no reuse to gain, more
  manifests and boundaries to keep. Revisit when build time or a second user
  asks for it.
- `std` for `x65`. Rejected: a CPU needs nothing of `std`, and `no_std` widens
  who can take the crate later.
- The names `mos6502` (taken on crates.io) or `nes-cpu` (contradicts the
  purpose).

## Consequences

- Machine timing (DMA stalls, PPU and APU stepping) lives in `nes`, driven
  through `x65`'s bus calls and input lines.
- The adapter waits for contract 0.2 (milestone N3); `x65` and `nes` need
  nothing from unemupc.

## Trigger to revisit

- A second consumer of `x65` appears: move it to its own repository.
- Build time of `nes` or a reuse of one of its chips argues for splitting it.
