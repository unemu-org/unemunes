# Decisions

Architecture and durable project decisions, not tasks. Records are named
`decision-<n> - <Title>.md`. decision-1 settles the crate layout and the
extractable 6502 crate `x65`, decision-2 what a hardware fact needs (a written
source and a running witness), decision-3 where oracles live and when one may
be committed; all three settle questions of the
[design note](../docs/design/nes-core.md) §14. decision-4 fixes the `x65`
API: one bus call per cycle, a processor that is a state machine of cycles
run by `tick` or `step`, and the instruction as the NES's contract step.

```sh
backlog decision create "<title>"
ls backlog/decisions/
```

See [the decision guide](../docs/decisions.md) for when to create a record rather
than append a bead note.
