# Status

Newest entries on top.

## 2026-11-18 — repository skeleton

The repository has its tooling and process documents: the flake dev shell,
pre-commit hooks, CI, the publication guard with its game-image check, and the
docs under `backlog/docs/`. There is no crate yet and nothing is emulated.

Agreed next steps, in order:

1. A design note before code: crate layout (6502, PPU, APU, cartridge and
   mappers from NROM, the bus and the timing model on one master clock, the
   contract implementation, the `unemunes` command line), test oracles and their
   licences, what the contract lacks for the NES, and what is out of scope at
   first.
2. The 6502 with its oracle tests.
3. A frame from a test ROM through the contract.
