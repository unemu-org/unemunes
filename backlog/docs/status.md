# Status

Newest entries on top.

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
