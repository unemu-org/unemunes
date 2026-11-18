---
id: decision-2
title: Hardware facts need a written source and a running witness
date: '2026-11-18'
status: accepted
---

Decider: maintainer. Settles [the design note](../docs/design/nes-core.md)
§14 Q4; sources in §12.

## Context

Nintendo published no data sheets for the RP2A03 or the RP2C02. Written
sources for the 6502 core are MOS Technology's manuals and the data sheets of
MOS and its second sources; for the rest of the console they are the NESdev
wiki and forum, whose general disclaimer declares the wiki's information
public domain. Die analysis exists as the Visual 2A03 and Visual 2C02
netlists; hardware measurement as test ROMs whose results were recorded on
consoles. x80's two-key rule (`decision-17 (x80)`) needs a manufacturer or
silicon key that the NES mostly lacks. Most NES emulators are GPL; this core is
MIT OR Apache-2.0.

## Choice

- A hardware fact enters the code with a **written source** and a **running
  witness**:
  - the written source is a manufacturer document cited by title, date,
    section and page, or a NESdev wiki page cited by its permanent revision
    link (`index.php?title=<Page>&oldid=<revision>`), or a named forum post;
  - the witness is a test on this emulator whose expected result comes from
    hardware or silicon: a test ROM verified on a console, a netlist
    observation, or a SingleStepTests vector.
- The code comment at the fact cites the source; the test names it too.
- **Never read another emulator's source code or documentation.** A fact
  known only from an emulator stays unverified until a source of the kinds
  above states it.
- When sources disagree, record both readings and the ruling; silicon and
  hardware measurement prevail over a written description.

## Alternatives

- x80's two-key rule unchanged. Rejected: unsatisfiable for most of the
  console without manufacturer documents.
- No formal rule. Rejected: copied facts and licence contamination are
  invisible until they cost a week.

## Consequences

- Every implementing bead pins its sources (revision ids, SHA-256 of scans)
  and adds witnesses.
- Facts with no source of the kinds above stay out, or behind an explicit
  unverified label approved by the maintainer.

## Trigger to revisit

- A fact the core needs has no written source at all, only measurements.
- Manufacturer documentation for the 2A03 or 2C02 becomes available.
