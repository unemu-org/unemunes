# NES core design

Status: **Accepted** by the maintainer on 2026-11-18 (`nes-1et`), with the
recommended answer to every question of §14. Durable choices are recorded as
[decision-1](<../../decisions/decision-1 - Crate-layout-and-the-extractable-6502-crate-x65.md>)
(crates and `x65`),
[decision-2](<../../decisions/decision-2 - Hardware-facts-need-a-written-source-and-a-running-witness.md>)
(provenance) and
[decision-3](<../../decisions/decision-3 - Oracles-by-licence-documents-in-the-shared-ledger-test-corpora-fetched.md>)
(oracles). The other answers (palette, power-on values, region) are design
choices of this note.

Markers: **READ** = read in a named source; **ASSUMED** = not verified;
**UNVERIFIED** = a claim that needs a pinned source before code relies on it.
Hardware facts below name the page or document they come from; pinning each
source by revision and SHA-256 is part of the bead that implements the fact
(§12).

## 0. The design in ten lines

1. Four kinds of crate: a guest-agnostic **6502 library** (working name `x65`),
   the **NES machine** (`nes`), the **contract adapter** (`unemu-nes-core`) and
   the **command line** (`unemunes`).
2. `x65` knows nothing of the NES: memory through a `Bus` trait, one bus call
   per CPU cycle, its own tests and oracles. It can leave the workspace for a
   C64 core without change.
3. Time is one **master clock** per region. Contract ticks are master-clock
   cycles at an exact `Rate`; contract steps are CPU cycles.
4. The CPU drives time: every CPU bus access advances the PPU and the APU to
   the moment of that access, so every register access sees exact PPU and APU
   state.
5. DMA (sprite DMA and the sample channel's fetches) stalls the CPU through
   its RDY input, which `x65` samples as the processor does; the machine's
   DMA unit drives it.
6. Cartridges are a `Mapper` trait over iNES and NES 2.0 images; NROM first,
   then MMC1, UxROM, CNROM, AxROM and MMC3.
7. The PPU produces palette indices with emphasis bits; RGB comes from one
   palette table at `frame()`.
8. The APU mixes, filters and divides its CPU-rate signal by a whole number
   inside the core, and describes the divided rate exactly (contract 0.2).
9. Everything that is undefined on hardware at power-on (RAM contents, the
   CPU/PPU clock alignment) is a config value with a fixed default, so a run
   is a function of its inputs.
10. The core starts with NTSC, two standard controllers and saved state;
    PAL and Dendy, the Zapper, the Disk System and expansion sound follow as
    separate work.

## 1. Goals, scope and non-goals

**Goal line.** An NTSC NES or Famicom with an RP2A03 CPU/APU and an RP2C02
PPU, running licensed cartridges of the common mappers, frame-exact and
deterministic, driven headless by `unemunes` and by the `unemu` front end
through the contract. Accuracy target: every bus-visible event at the right
CPU cycle and every pixel at the right PPU dot, measured by the open test ROMs
of §11. Not a transistor-level simulation.

**First out of scope**, each a later bead or epic, not a half-implementation:

| Item | Why later |
| --- | --- |
| PAL and Dendy | Different dividers, line counts and APU tables (§3). The region is a config field from the first day; only NTSC is implemented |
| Famicom Disk System | Needs the Disk System BIOS, proprietary firmware the core must take by digest (a firmware catalogue on the front end's side) |
| Zapper | The light-gun device itself; contract 0.2 already has the absolute pointer it needs (§9) |
| Expansion sound (VRC6, VRC7, N163, 5B, MMC5, FDS) | Mapper-specific synthesis; after the base APU |
| Vs. System, PlayChoice-10, Famicom keyboard, microphone | Hardware variants and input devices without a contract shape yet |
| NSF player, debugger, tracing, rewind, run-ahead | Front-end or tooling features over a working core |

## 2. Crates

| Crate | Layer (unemupc's names) | Depends on | Holds |
| --- | --- | --- | --- |
| `x65` | CPU library | nothing | NMOS 6502 models, `Bus` trait, interrupt lines, undocumented opcodes |
| `nes` | emulation | `x65` | cartridge and mappers, PPU, APU, controllers, the console's bus and scheduler, saved state |
| `unemu-nes-core` | emulation (adapter) | `nes`, `unemu-core-api` | `Core` and `Machine` over `nes`; config document parsing |
| `unemunes` | host (binary) | `nes`, later `unemu-nes-core` | the headless command line (§10); the only crate that reads files or prints |

Why one `nes` crate and not a crate per chip: the PPU, the APU and the mappers
exist only inside this console, and splitting them buys no reuse. They stay
modules behind traits (`Mapper`, the PPU's and APU's register interfaces), each
with isolation tests on a mock bus, and can split when build time or reuse asks
for it. `x65` is separate because it is meant to leave (maintainer decision,
2026-11-18). Settled by §14 Q3 and decision-1.

A boundary test in the spirit of unemupc's `scripts/crate-boundaries.sh` keeps
the rules: `x65` has no dependency; `nes` depends only on `x65`; no crate but
`unemunes` names `std::fs`, `std::env`, `std::process`, `std::time`,
`std::thread` or the standard streams.

## 3. Time and scheduling

**Clocks** (READ: NESdev wiki, "Cycle reference chart", revision 24433 of
2026-10-12, which "Clock rate" redirects to; "PPU rendering", revision 23300;
"PPU frame timing", revision 24376):

| Region | Master clock | CPU divider | PPU divider | Lines × dots |
| --- | --- | --- | --- | --- |
| NTSC | 236 250 000 / 11 Hz (≈ 21.477 MHz) | 12 | 4 | 262 × 341; with rendering enabled, the pre-render line is one dot shorter on odd frames |
| PAL | 26 601 712.5 Hz | 16 | 5 | 312 × 341 (UNVERIFIED: line count not yet read) |
| Dendy | as PAL | 15 | 5 | 312 × 341 (UNVERIFIED: as PAL) |

Contract mapping: `MachineDescription::clock` is the master clock as an exact
`Rate` (NTSC `236250000/11`); `Ticks` count master-clock cycles; `steps()` counts
CPU cycles, `step_unit` is `"cycle"`. NTSC has exactly three PPU dots per CPU
cycle; PAL has 3.2, which master-clock accounting handles without fractions.

**Who drives time.** Every 6502 cycle is one bus access, a read or a write,
including the dummy accesses (READ: MCS6500 Hardware Manual, the cycle tables;
to pin). So the CPU drives the machine: the machine's `Bus` implementation,
before it performs an access, advances the PPU by the dots and the APU by the
cycle that elapse up to that access. Every register access then sees the PPU
and APU exactly as they are at that cycle, without catch-up heuristics. It is
the simplest scheme that is exact at CPU-cycle granularity; optimisations
(running the PPU ahead when no access can observe it) come later, behind the
same tests.

**Sub-cycle phase.** The position of the PPU inside a CPU cycle at power-on
differs between consoles and changes some test results (UNVERIFIED: NESdev
wiki, "CPU/PPU clock alignment"). The core fixes one alignment as a config
value with a default, so results never depend on chance (§14 Q7).

**Interrupts.** The 6502 samples NMI (edge) and IRQ (level) at fixed points of
each instruction; the `Bus` trait reports the lines after each access, and
`x65` decides when they are taken. The sources (PPU vblank NMI; APU frame
counter, DMC and mapper IRQs) live in `nes`.

**DMA.** Sprite DMA (`$4014`) and the DMC's sample fetches halt the CPU on its
next read cycle through the 6502 core's RDY input, and their length depends
on the parity of the CPU cycle (READ: NESdev wiki, "DMA"; to pin). RDY is an
input of `x65`, as §13.5 (unemupc) requires of the processor crate: on a read
cycle with RDY low the processor holds its address and repeats the read
(UNVERIFIED: the exact NMOS behaviour, to cite in N1). The machine's DMA unit
drives RDY and makes its transfers in those cycles, so the repeated reads
and their side effects on registers such as `$4016` and `$2007` happen as on
the console. The same input serves a C64's video chip stealing cycles.

**Run limits.** `Machine::run` checks its limits at CPU-cycle boundaries: a
deadline is met within one step, as contract §3.5 allows. `Stop::Frame` fires
when the PPU finishes the last visible line (a fixed dot, chosen in the
implementing bead and documented there).

**Determinism.** No host clock and no randomness anywhere. Power-on RAM and
the clock alignment come from the config. The PPU's open-bus latch decays
over time on hardware (UNVERIFIED: NESdev wiki, "Open bus behavior"); if
modelled, it decays in master-clock ticks.

## 4. The 6502 library (`x65`)

**Models.** The NMOS 6502 with decimal mode, and the RP2A03's core, which keeps
the D flag but does no decimal arithmetic (READ: NESdev wiki, "CPU"; to pin).
The 65C02 and later CMOS parts are out of scope. A C64's 6510 is the NMOS
model plus an I/O port the embedder provides.

**API sketch** (not a commitment; a decision in N1 fixes it):

```rust
/// One bus access per call; every call is one CPU cycle.
pub trait Bus {
    fn read(&mut self, address: u16) -> u8;
    fn write(&mut self, address: u16, value: u8);
    /// Input lines after the access just performed.
    fn lines(&self) -> Lines;
}

pub struct Lines { pub irq: bool, pub nmi: bool, pub ready: bool, pub reset: bool }

impl Cpu {
    pub fn new(model: Model) -> Self;
    /// Run one instruction or one interrupt sequence.
    pub fn step<B: Bus>(&mut self, bus: &mut B) -> Event;
    pub fn reset<B: Bus>(&mut self, bus: &mut B);
}
```

`Bus` is generic, not `dyn`: the contract's object-safety rule binds the
contract crate, not a CPU library, and the CPU's inner loop wants
monomorphisation. Rules inside `x65`:

- Exactly one bus call per cycle, dummy reads and the double writes of
  read-modify-write instructions included; a test compares the per-cycle bus
  trace with SingleStepTests (§11).
- Interrupt timing details are part of the model: the delay after a taken
  branch, NMI hijacking a BRK, the polling point (UNVERIFIED: each needs its
  source when implemented).
- Undocumented opcodes: the stable ones are implemented; the unstable ones
  (ANE, LXA, SHA, SHX, SHY, TAS) use constants the model names and their
  sources state; the JAM opcodes stop the CPU in a typed state.
- What differs between chips of the family (no decimal arithmetic in the
  2A03, the 6510's I/O port, later the CMOS instructions) is a parameter of
  the model, never conditional compilation or a fork (§13.5 (unemupc)).
- The CPU's state is plain data a machine saves and restores.
- `#![no_std]`, no allocation, no dependency, `#![forbid(unsafe_code)]`, like
  `x80-core` (decision-4 (x80)); settled by §14 Q2 and decision-1.

## 5. The PPU

A 2C02 stepped one dot at a time: the eight registers at `$2000–$2007`
mirrored through `$3FFF`, the internal scroll registers (`v`, `t`, fine X, the
write toggle), the background and sprite pipelines, sprite evaluation with its
overflow behaviour, sprite-0 hit, palette RAM and OAM (READ: NESdev wiki,
"PPU registers", "PPU scrolling", "PPU rendering", "PPU sprite evaluation";
to pin). Pattern and nametable fetches go through the cartridge (`Mapper`),
which sees every PPU address, as MMC3's scanline counter needs.

Output: a 256×240 buffer of 9-bit values (6-bit palette index and 3 emphasis
bits). `frame()` converts it to `0x00RRGGBB` through one 512-entry table. The
NES has no RGB palette in hardware: the PPU emits a composite signal, so the
table is either computed from the documented signal model or taken from a
measured palette with a licence that allows it; §14 Q6 chose the computed one. The picture is
256×240 with 8:7 pixels, so `Frame::aspect` is `(256 × 8) : (240 × 7)` =
`128:105`. Cropping the overscan is presentation, the front end's.

## 6. The APU

Two pulse channels, triangle, noise, the delta-modulation channel (DMC) with
its DMA, the frame counter in its 4- and 5-step modes with the frame IRQ,
length counters, envelopes and sweeps (READ: NESdev wiki, "APU" and its
sub-pages; to pin). The channels combine through the non-linear mixer and the
console's output filters, then the core resamples to the rate it describes.

**Output rate.** Contract 0.2 settles it (core-contract.md §13.2 (unemupc)):
a machine states the rate natural to it, between 8 000 and 192 000 Hz, and
never resamples for a host device; a generator that runs faster filters and
divides by a whole number inside the core. The APU runs at the CPU clock,
19 687 500/11 Hz on NTSC, so the core low-pass filters and keeps one frame in
40: **984 375/22 Hz** (about 44.7 kHz), the contract's own example. One output
frame is then exactly 480 master-clock ticks, so the contract's rule holds by
construction. Contract 0.2's rule, as the unemupc supervisor sent its final
wording on 2026-11-18: frame `i` is the sound at virtual time
`i × clock / rate` ticks from creation; at every return of `run`, once
`drain_audio` has been called, the next frame it hands over has index
`floor(now() × rate / clock)` (here `now() / 480`), within one frame; frames
not drained when sound is switched off, or before `load_state`, are dropped;
after `load_state` the index follows the loaded `now()`. The low-pass filter is written here from the
signal-processing literature, not taken from another emulator (§12). The
front end converts to the device's rate.

## 7. Cartridges and mappers

Images are iNES or NES 2.0 (READ: NESdev wiki, "INES" and "NES 2.0"; to pin).
The header gives the mapper, PRG and CHR sizes, mirroring, battery and,
for NES 2.0, the region and RAM sizes. A `Mapper` trait sees CPU reads and
writes at `$4020–$FFFF`, every PPU address, and CPU cycles for mappers that
count them; it drives the IRQ line and the nametable mapping.

Order: NROM (0) first; then MMC1 (1), UxROM (2), CNROM (3), AxROM (7) and
MMC3 (4), each with holy mapperel or a comparable open test (§11). Others by
demand. Headers known to be wrong in circulating images would need a game
database; none is planned until a bead shows the need and its licence is
checked.

Battery-backed PRG RAM goes into one contract storage record, `battery`
(contract 0.2, `Storage::open`), opened only when the cartridge has such
memory, written with `replace` and `commit` at `finish` and whenever `run`
returns with the memory changed and ten virtual seconds gone since the last
write (core-contract.md §13.2 (unemupc)). A failed write surfaces from `run`
as `Storage` or `StorageUncertain` and leaves the machine `Failed`; after an
uncertain commit the record holds the old or the new memory, whole, and the
front end recreates the machine (final 0.2 wording, 2026-11-18).

## 8. The machine

- **Memory map:** 2 KiB RAM mirrored to `$1FFF`, PPU registers, APU and I/O at
  `$4000–$4017`, the cartridge above (READ: NESdev wiki, "CPU memory map";
  to pin). Open-bus reads return the last value on the data bus.
- **Controllers:** the standard pad through the `$4016` strobe and the serial
  reads of `$4016` and `$4017` (READ: NESdev wiki, "Standard controller"; to
  pin). Two ports, contract `InputKind::Pad` with Up, Down, Left, Right, A, B,
  Select and Start.
- **Reset:** `ResetKind::Soft` is the reset button: the CPU's reset sequence
  and the documented partial resets of PPU and APU. `ResetKind::Hard` is power
  off and on: every volatile part from its power-on value; battery RAM stays.
- **Saved state:** from the first adapter release. The core's own versioned
  binary format, written by hand, no serialisation crate. `state_identity`
  names the core's release, the hash of the config document and the
  cartridge's SHA-256.

## 9. The contract

The adapter implements contract **0.2**, pinned by the exact revision of the
`v0.2.0` tag of github.com/unemu-org/unemu-core-api (MIT OR Apache-2.0; only
`v0.1.0` exists on 2026-11-18), with a gitignored `.cargo/config.toml`
`[patch]` for a local checkout and never a path dependency. It follows the
checklist of
core-contract.md §13.5 (unemupc), accepted on 2026-11-18. What 0.2 settles for
the NES, from the gaps this note raised:

| Need | Contract 0.2 (core-contract.md §12.5, §13.2 (unemupc)) |
| --- | --- |
| Battery RAM | A `Storage` record per cartridge, with the write rule of §7 |
| Sound | The rate rule of §6: divide by a whole number inside the core |
| Opening a file | `Core::identify` recognises an image and writes the config document; `Core::validate` returns the content the config needs. No provider crate on the front end's side |
| The config document | The core's own: `ConfigDocumentBuf` with a media type in the core's namespace, versioned, and a format the core chooses. A small key=value form parsed by hand keeps `toml` and `serde` out of the core |
| The Zapper | `InputEvent::PointerPosition`, a position in 65 536ths of the whole picture, and `InputKind::AbsolutePointer` |
| Overscan | `Frame::visible`, the safe part of the picture; `aspect` stays the whole picture's |

The description: core id `unemunes`; one platform, id `nes`, with the region
in the config; the clock `236250000/11` on NTSC; step unit `cycle`; one
display of 256×240 at `128:105`; the sound rate of §6; one `Pad` port per
controller in player order; the cartridge as a slot that is not removable.

The config document holds the region, the cartridge's SHA-256, size and
format, the devices in the controller ports, and the power-on values of
§14 Q7. A gap the adapter finds in 0.2 goes to the unemupc supervisor as a
request for 0.3.

## 10. The command line `unemunes`

A headless tool, with the words, output style and exit codes of `unemupc`'s
command line where they overlap (decision-41 (unemupc), item 8). It starts
in N2 over `nes`, reading files itself, as a development tool; in N4 it moves
onto the adapter and reads and keeps files through `unemu-core-files` from
the contract's repository, so it runs a machine exactly as a front end does.

| Command | Does |
| --- | --- |
| `unemunes info ROM` | Print the header: format, mapper, sizes, mirroring, battery, region, SHA-256 |
| `unemunes run ROM --frames N` | Run headless; optionally write the last frame as a PPM image and print frame and audio hashes |
| `unemunes test ROM` | Run a test ROM that reports through the `$6000` protocol of blargg's suites and exit with its result |

PPM needs no dependency; PNG output can come later in the binary only.

## 11. Tests and oracles

| Layer | Oracle | Checks |
| --- | --- | --- |
| `x65` | SingleStepTests (6502 and the NES variant) | Registers, memory and the per-cycle bus trace of every opcode |
| `x65` | Klaus Dormann's functional and decimal tests | Whole-instruction-set behaviour |
| `nes` CPU path | nestest and its log | CPU behaviour from `$C000`, cycle counts |
| `nes` | blargg's CPU, PPU and APU suites | Timing, interrupts, DMA, PPU flags, APU behaviour, through the `$6000` protocol |
| `nes` | AccuracyCoin | A broad accuracy sweep in one cartridge |
| `nes` mappers | holy mapperel | Mapper banking and mirroring |
| goldens | the open ROMs above | SHA-256 of frames and sound at fixed frames |

Every component also has isolation tests on a mock bus, with expected values
derived from the cited source, not from the implementation.

**Licences** (READ on 2026-11-18 for `nes-1et`: licence files and source
headers, cross-checked against the GitHub API where a repository exists). The
rule: an oracle is committed only under a licence that allows redistribution
and is compatible with MIT OR Apache-2.0, beside its licence text and listed
in the publication guard's allowlist. Everything else is fetched at test time,
pinned by SHA-256, into an ignored directory, as x80 handles its GPL
exercisers.

| Oracle | Source | Licence | Treatment |
| --- | --- | --- | --- |
| SingleStepTests 65x02, `6502/v1` and `nes6502/v1` | github.com/SingleStepTests/65x02 | MIT ("Copyright (c) 2024 Thomas Harte et al") | Fetch: about 1 GB of JSON, data to fetch rather than commit. `nes6502` is the 6502 without decimal mode; each test lists the per-cycle bus activity. The archived `ProcessorTests` repository has no licence and is not used |
| Klaus Dormann's functional and interrupt tests | github.com/Klaus2m5/6502_65C02_functional_tests | GPL-3.0-or-later | Fetch only, never vendored |
| Bruce Clark's decimal test (in the same repository) | as above | Public domain, by its header | Could be committed; fetched with the rest for simplicity |
| nestest and nestest.log (Kevin Horton, 2004) | qmtpro.com, mirrored on nickmass.com | No licence found | Fetch only, pinned (`f67d55fd…0004` for the ROM, `627c8e18…7540` for the log) |
| blargg's suites (Shay Green, 2005–2010) | archived site; the christopherpow/nes-test-roms mirror | No licence found in any readme, source or forum post | Fetch only, pinned. The mirror has no licence either, and its uploader doubted his own permission |
| `cpu_dummy_writes` (Joel Yliluoma, on blargg's framework) | bisqwit.iki.fi | No licence found | Fetch only |
| AccuracyCoin (Chris Siebert, 2025) | github.com/100thCoin/AccuracyCoin | MIT | May be committed (the ROM and its licence; never the bundled assembler binary). Fetching pinned is enough at first |
| holy mapperel (Damian Yerrick, v0.02) | github.com/pinobatch/holy-mapperel | zlib | May be committed with its licence |
| 240p test suite, NES port | github.com/pinobatch/240p-test-mini | GPL-2.0-or-later | Not an automated oracle; manual checks only, fetched |

**Where oracles live:** documents in `unemu-org/oracles`, test corpora fetched by a script here (§14 Q5, decision-3).

## 12. Provenance

The NES has almost no manufacturer documentation: Nintendo published no
datasheets for the 2A03 or the 2C02. What exists:

- **Manufacturer documents** for the 6502 core: the MCS6500 Microcomputer
  Family Programming Manual and Hardware Manual (MOS Technology, January
  1976; scans on bitsavers.org and archive.org) and the 6500 data sheets of
  MOS (1976, 1985), Rockwell (1979) and Synertek (1979). Copyrighted: cited,
  never copied into the repository.
- **Die and netlist analysis:** the transistor-level Visual 2A03 and Visual
  2C02 simulations. Their simulator code is MIT; their netlist data carries
  no licence, and the Visual 6502's layout data is CC BY-NC-SA, so neither
  is committed.
- **Hardware measurement:** test ROMs whose results were recorded on
  consoles, and their authors' write-ups.
- **Third-party written analysis:** the NESdev wiki and forum. The wiki's
  general disclaimer (revision 17363) declares its information public
  domain; a page is cited as
  `https://www.nesdev.org/w/index.php?title=<Page>&oldid=<revision>`.

Proposal: a fact enters the core with a written statement (a manual, or a
NESdev wiki page pinned by revision id) **and** a witness that runs on the
emulator (a test ROM verified on hardware, a netlist observation, or a
SingleStepTests vector). Never read another emulator's source code or its
documentation: most NES emulators are GPL, this core is MIT OR Apache-2.0, and
a copied mistake is invisible. This is lighter than x80's two-key rule
(decision-17 (x80)), because the NES has no manufacturer key to turn; settled by
§14 Q4 and decision-2.

## 13. Milestones (proposed epics)

Numbered as core-contract.md §13.6 (unemupc) numbers them, so both sides
speak of the same thing. N1 and N2 need nothing from unemupc; N3 needs the
contract's repository to read and its `v0.2.0` tag to pin.

| Milestone | Done when |
| --- | --- |
| N0 Design | This note accepted; decisions recorded |
| N1 `x65` | Both models pass SingleStepTests and Dormann's tests; boundary test; docs |
| N2a First frame | NROM machine with PPU background and sprites; the nestest log matches; `unemunes run` writes a test ROM's frame; first goldens |
| N2b Sound | APU with DMC and DMA; blargg's APU tests; sound goldens |
| N2c Mappers | MMC1, UxROM, CNROM, AxROM, MMC3; battery RAM behind a seam that becomes the storage record in N3 |
| N3 Adapter | `unemu-nes-core` on contract 0.2 by the checklist of §13.5 (unemupc); saved state |
| N4 Command line | `unemunes` on the adapter and `unemu-core-files` |
| N5 Hand-over | The first revision for the front end, with the hand-over of §13.5 item 15 (unemupc) |
| N6 Later | Each on its own: the Zapper, PAL and Dendy, the Disk System, accuracy beyond the suites of N2 |

Each failing test of the suites in §11 becomes a bead of its milestone.

**The proof slice comes first** (maintainer decision, 2026-11-18). What
unemunes has to prove is that the contract is generic enough for one front
end to show both the PC and the NES. So before the milestones above, one
slice (epic `nes-a3h`) builds the smallest core that does that: `x65` with the
documented instructions of the RP2A03 model and plain NMI, IRQ and reset;
`nes` with NROM, the controllers, OAM DMA as a plain stall and a simple PPU
(background, sprites, sprite-0 hit, vblank NMI, the computed palette);
silence at 984 375/22 Hz; and the adapter on contract 0.2 without saved state
or battery memory. Its own goldens come from a test ROM built from our bytes.
The architecture is the one above; only the order changes. The slice starts
when contract `v0.2.0` is tagged. N1 to N4 then complete what the slice left
out.

## 14. Questions answered by the maintainer

Answered on 2026-11-18: Q1 (a), Q2 (a), Q3 (a), Q4 (a), Q5 (c), Q6 (a),
Q7 (a), Q8 (a), the recommendation of each. The options are kept for the
record.

**Q1. The 6502 crate's name.** (a) `x65`, after `x80` and `x86jit`;
(b) `mos6502` or similar, which collides with existing crates on crates.io;
(c) `nes-cpu`, which contradicts its purpose. Recommendation: **(a)**.

**Q2. `no_std` for `x65`.** (a) `no_std`, no allocation, like `x80-core`;
(b) `std` like the rest of the core. Recommendation: **(a)**: it costs
nothing for a CPU and widens who can take it later.

**Q3. Crate granularity.** (a) `x65`, `nes`, `unemu-nes-core`, `unemunes`,
with PPU, APU and mappers as modules; (b) a crate per chip from the start.
Recommendation: **(a)** (§2).

**Q4. Provenance rule.** (a) The rule of §12: a written statement plus a
running witness, no emulator sources; (b) x80's two-key rule unchanged;
(c) no formal rule. Recommendation: **(a)**.

**Q5. Where oracles live.** (a) In `unemu-org/oracles`, the family's
provenance ledger, as a third citing project: documents vendored where the
licence allows, test ROMs pinned fetch-only; (b) a fetch script in this
repository, like x80's CPU-test fetcher; (c) both: documents in
`oracles`, test corpora by script here. Recommendation: **(c)**: citations
belong in the ledger, hundreds of megabytes of test vectors do not.

**Q6. Palette.** (a) Computed from the documented composite signal model, with
citations; (b) a measured palette under a licence that allows it.
Recommendation: **(a)**, with (b) as an option if a suitably licensed
measurement exists.

**Q7. Power-on values.** RAM contents and the CPU/PPU alignment are undefined
on hardware. (a) Config fields with fixed defaults (RAM zeroed, one
documented alignment); (b) fixed in the core, not configurable.
Recommendation: **(a)**: tests that probe them need to choose.

**Q8. Region scope.** (a) NTSC only until N6, the region already a config
field; (b) PAL alongside NTSC from N2. Recommendation: **(a)**.
