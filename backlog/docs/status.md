# Status

Newest entries on top.

## 2026-11-20 — proof slice done; contract 0.3

The proof slice (epic `nes-a3h`) is done: the unemupc front end pins
`unemu-nes-core` and opens an NROM image beside the PC. The core has the
documented 6502 instructions with NMI, IRQ and reset (`x65`); iNES with NROM,
two pads and a simple PPU with sprites, sprite-0 hit and an NTSC palette; a
silent APU at 984 375/22 Hz; and the adapter with Soft and Hard reset and stop
on frame. Not yet: other mappers, undocumented opcodes (a guest error), sound,
dot-accurate PPU, saved state, battery memory and the command line.

`unemu-nes-core` now states contract 0.3 and pins `unemu-core-api` v0.3.0
(commit `0d490fcb0b97320ac785b12f84a5c49e72f28b1d`); the only change for a
core is the version it states.

## 2026-11-18 — design accepted

The maintainer accepted the [design note](design/nes-core.md) with the
recommended answer to every question; decision-1 to decision-3 record the
crate layout and `x65`, the provenance rule and the oracle policy. Contract
0.2 is being written in the `unemu-core-api` repository; the adapter (N3)
waits for its `v0.2.0` tag. Nothing is emulated yet.

Next: the proof slice (design note §13, epic `nes-a3h`), the smallest core
that the unemupc front end can open beside the PC. It starts when contract
`v0.2.0` is tagged.

## 2026-11-18 — repository skeleton

The repository has its tooling and process documents: the flake dev shell,
pre-commit hooks, CI, the publication guard with its game-image check, and the
docs under `backlog/docs/`. There is no crate yet.
