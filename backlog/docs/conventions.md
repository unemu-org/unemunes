# Conventions

Rust edition 2021. Development toolchain: Rust 1.99.1, which the contract crate needs (x80 and
x86jit pin 1.98.1), from the flake. The declared `rust-version` stays at or below the front
end's toolchain, because the front end compiles this core.

- Every crate is `#![forbid(unsafe_code)]` and keeps the embeddability rules of
  [AGENTS.md](../../AGENTS.md#embeddability-decision-41-unemupc). Only the
  command line binary touches the host.
- The 6502 crate has no NES knowledge and no dependency on another crate of
  this workspace.
- Use snake_case modules, one primary concept per file. Cross-crate consumers
  use deliberate public APIs.
- Prefer typed enums for public failures; library errors implement
  `std::error::Error`. Do not add error or extension machinery speculatively.
- Names explain what; comments explain constraints. Hardware behaviour cites its
  source by name and section, and a test witnesses it. Expected values in tests
  stay independent of the helpers they test.
- File discovered work as a separate bead rather than widening a change.

Use Conventional Commits (`feat:`, `fix:`, `docs:`, `refactor:`, `chore:`,
`test:`, `ci:`, `build:`), one signed squash commit per bead. Never commit or
push without an explicit maintainer request; never bypass hooks.

Formatting and clippy with `-D warnings` must pass; see [commands](commands.md).
When a fact or a contract is uncertain, record the question rather than guess.
