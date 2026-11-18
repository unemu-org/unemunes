# Deliberately deferred

Do not implement these unprompted. Open work is tracked in beads.

| Item | Why deferred | Revisit |
| --- | --- | --- |
| Contract adapter (`Core`, `Machine`) | The contract gets its own repository first (`decision-40 (unemupc)`, §11 Q1 of its core-contract design); a git pin of unemupc would give a second copy of the crate | The unemupc supervisor names the repository and the revision to pin |
| A 6502 library in its own repository | Maintainer decision, 2026-11-18: a crate of this workspace first, written to be extractable | A second consumer, such as a C64 core |
| A libretro wrapper | `decision-41 (unemupc)` §9: not before contract 1.0 | Someone wants the core in a libretro front end |
| `no_std` and a C ABI | `decision-41 (unemupc)` §9: no embedder needs them | A target without `std`, or an embedder not written in Rust |
