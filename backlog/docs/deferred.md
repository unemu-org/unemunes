# Deliberately deferred

Do not implement these unprompted. Open work is tracked in beads.

| Item | Why deferred | Revisit |
| --- | --- | --- |
| Contract adapter (`Core`, `Machine`) | The adapter targets contract 0.2 (storage objects, `identify` and `validate`, the pointer, the visible area). The contract's repository, github.com/unemu-org/unemu-core-api, has only `v0.1.0` so far (commit `4e2dd8325b54ce652bf4884fd85da602b14ae726`), enough to read and prototype against | The `v0.2.0` tag exists and the unemupc supervisor names its revision; milestone N3 of the design note |
| A 6502 library in its own repository | Maintainer decision, 2026-11-18: a crate of this workspace first, written to be extractable | A second consumer, such as a C64 core |
| A libretro wrapper | `decision-41 (unemupc)` §9: not before contract 1.0 | Someone wants the core in a libretro front end |
| `no_std` and a C ABI | `decision-41 (unemupc)` §9: no embedder needs them | A target without `std`, or an embedder not written in Rust |
