---
id: decision-3
title: Oracles by licence — documents in the shared ledger, test corpora fetched
date: '2026-11-18'
status: accepted
---

Decider: maintainer. Settles [the design note](../docs/design/nes-core.md)
§14 Q5; licences in §11.

## Context

The family keeps a provenance ledger, `unemu-org/oracles`: pinned reference
documents, vendored where their licence allows, fetch-only otherwise, each
with the fact it witnesses. NES test oracles differ in size and licence: the
SingleStepTests 65x02 corpus is MIT but about 1 GB; AccuracyCoin is MIT and
holy mapperel zlib; Klaus Dormann's tests are GPL-3.0; nestest and blargg's
suites carry no licence at all (READ 2026-11-18, design note §11).

## Choice

- **Documents** cited as written sources (decision-2) go into
  `unemu-org/oracles`, with this repository as a citing project, pinned and
  vendored only where their licence allows.
- **Test corpora and test ROMs** are fetched by a script in this repository,
  each pinned by URL and SHA-256, into an ignored directory. Tests that need
  them skip with a clear message when they are absent, and CI fetches them.
- A test ROM may be committed only under a licence that allows redistribution
  and is compatible with MIT OR Apache-2.0, beside its licence text, at a path
  the publication guard allowlists. Content without such a licence, or under
  the GPL, is never committed.
- Commercial game images are never used as oracles.

## Alternatives

- Everything in `unemu-org/oracles`. Rejected: a gigabyte of test vectors is
  data to fetch, not a source anyone cites by section.
- Everything fetched by this repository. Rejected: citations belong in the
  family's ledger, where `verify --upstream` checks them.

## Consequences

- Adding NES documents to `unemu-org/oracles` is a change in that repository,
  under its rules, made when a bead first needs the document.
- The fetch script and its pins are part of milestone N1.

## Trigger to revisit

- A test corpus becomes small enough to commit and its licence allows it.
- An author grants a licence to a test ROM that has none today.
