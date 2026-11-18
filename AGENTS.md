# AGENTS.md — unemunes

Canonical repository instructions for coding agents. `CLAUDE.md` imports this file.

## What this is

`unemunes` is the **NES emulator core** of the unemu family: a library that
emulates the Nintendo Entertainment System and Famicom, and a small headless
command line of the same name. A front end (`unemu`, developed in unemupc for
now) drives it through the contract crate `unemu-core-api`, as it drives the PC
core; see `decision-40 (unemupc)` and `decision-41 (unemupc)`. The NES is the
second platform on that contract, chosen because it differs completely from the
PC: it proves the contract before the contract reaches 1.0.

It is a research project like its siblings: the priorities are **testability**,
**swappability** and **dense documentation**.

The 6502 CPU is a crate of this workspace, written so it can leave later as its
own library (for example for a C64 core): it knows nothing of the NES. It talks
to memory through a bus trait, has no PPU, APU or mapper types, and carries its
own tests, oracles and docs (maintainer decision, 2026-11-18).

Nothing is emulated yet. The next step is a design note before code; see
[status](backlog/docs/status.md).

## Language

- **Talk to the maintainer in Polish.**
- **Everything committed is English**: code, comments, docs, commit messages,
  branch names, bead and decision titles, identifiers.

## The contract and the front end

- The contract crate `unemu-core-api` is **not copied or vendored** here. It
  gets its own repository on the unemupc side; this workspace depends on it
  pinned to the revision the unemupc supervisor names, and only from the step
  that implements the contract. The CPU, PPU, APU and cartridge crates do not
  depend on it.
- A change the NES needs from the contract (an absolute pointer for a light gun,
  `Core::validate`, the audio rate) is brought to the unemupc supervisor session
  and lands there, so both cores stay on one copy.
- The front end's work (opening a ROM, the machine view) is unemupc's.

## Embeddability (decision-41 (unemupc))

A core is a library that touches the host only through what its embedder hands
it. Every crate of the core needs Rust's `std` with an allocator and nothing
else of the host:

- no crate for windowing, graphics, host audio, host input, network or async;
- no host clock and no randomness; no thread it starts; no process-wide mutable
  state (immutable tables are fine);
- no file, path, directory, environment variable, standard stream or process
  exit; ROMs arrive as content by SHA-256, battery memory goes to the
  contract's storage;
- `#![forbid(unsafe_code)]` in every crate.

Only the command line binary touches the host, to read files and print.

## Engineering hard rules

1. **Testability-first.** No component lands without isolation tests on a mock
   bus. No machine-level feature lands without a golden or test-ROM test.
2. **Swappability-first.** Components sit behind traits (the CPU's bus, the
   cartridge's mapper, the picture and sound outputs); the machine composes them.
3. **Docs-first.** Every module carries a doc-comment describing the modelled
   hardware and citing its source by name and section.
4. **Determinism.** No wall clock in the execution path. Virtual time is counted
   in cycles of one master clock; every component ticks off it.

## No commercial ROMs, ever

Never commit a commercial game image, firmware dump or any proprietary byte:
not as a fixture, not compressed, not in a test. Hashes and titles are fine.
Open test ROMs may be committed only with their licence text, after the licence
was checked, and only at a path that `scripts/publication-guard.sh` allowlists.
The guard rejects NES, FDS, UNIF and NSF images by name and by signature.

## Where things live — load on demand

| Need | Path or command | When to load |
| --- | --- | --- |
| Feature status | [status](backlog/docs/status.md) | Session start; "does X work?" |
| Lifecycle, gates, git flow, handoff | [working on tasks](backlog/docs/working-on-tasks.md) | Before tracked work |
| Rust and commit conventions | [conventions](backlog/docs/conventions.md) | Before changing code |
| Build and verification | [commands](backlog/docs/commands.md) | Running checks |
| Shell, direnv, hooks and bd setup | [dev setup](backlog/docs/dev-setup.md) | Onboarding or tooling work |
| Deliberate deferrals | [deferred](backlog/docs/deferred.md) | Before adding something that "seems missing" |
| Decision capture | [decisions guide](backlog/docs/decisions.md), [records](backlog/decisions/) | Durable choices |
| Backlog doc/decision workflow | [backlog skill](.agents/skills/backlog/SKILL.md) | Using the Backlog.md CLI |

Vendored skills live under `.agents/skills/`; `scripts/skills-bootstrap.sh` links
them into ignored `.claude/skills/`.

## Working on tasks

Open work uses beads (`bd`, prefix `nes`); design, guides and decisions live in
`backlog/`. The supervisor scopes, claims, reviews and closes beads; workers
implement a narrow brief and report evidence. See
[working on tasks](backlog/docs/working-on-tasks.md). Cross-repository ids carry
a tag: `decision-41 (unemupc)`, `upc-dfs.26 (unemupc)`.

## Hard rules

- **Never commit without an explicit maintainer request.** Plan acceptance is
  not commit acceptance. Push only when requested, only to the configured
  `origin`. `bd init` and the first `bd dolt push` commit on their own; check
  `git log -1` after any bd setup command.
- **Write as if the repository were public.** English, publish-safe: no
  proprietary bytes, no private host, private path or personal setup. Never
  create, mirror or publish a repository; that is the maintainer's step.
- **Add only what the task requires.** No speculative framework, no
  opportunistic cleanup, no work a later bead owns.
- **Project knowledge lives under `backlog/`**, apart from root entry points,
  licences and bootstrap files. Open work lives in beads, not in markdown.
- **Never bypass hooks.** Never report a skipped gate as passed.

## Code ownership

Maintainer / sole decider: Mikołaj Mikołajczyk.
