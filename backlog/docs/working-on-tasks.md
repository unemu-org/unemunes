# Working on tasks

Open work uses beads (`bd`, prefix `nes`). Backlog.md stores docs and decisions
only. These rules adapt the shared rules of x86jit, x80 and unemupc.

## Roles and session start

The maintainer decides scope, priorities and outward-facing actions. The supervisor
scopes, claims, reviews, runs gates and closes beads. Workers implement a narrow
brief or perform read-only review; they never claim or close beads, commit, push,
merge or change design documents outside their brief. Commits and pushes require
the maintainer's request.

At session start, read `bd memories`, then `bd list --status in_progress` and
`bd show <id>` before task work. `bd <command> --help` supplies version-specific
flags. Repository rules override generic `bd prime` advice. A sandbox unable to
open the beads lock takes the relevant rules from the supervisor's brief.

## Statuses and epics

| Status | Meaning |
| --- | --- |
| `open` | Scoped, not started; default for new beads |
| `in_progress` | Claimed before implementation |
| `closed` | Landed on `main`; the close reason names the commit |

Blocked work is derived from dependencies; `bd ready` excludes open blockers.
Priorities run P0 (highest) through P4. Each theme gets an epic; task children use
`--parent`, `--no-inherit-labels` and their own labels.

```sh
bd ready --exclude-type epic
bd list --type epic
bd list --parent <epic-id>
bd show <id>
bd create --title "<title>" --description "<why and what>" --acceptance "<criteria>" \
  --type task --priority P2 --parent <epic-id> --no-inherit-labels --labels <theme>
bd update <id> --claim
bd update <id> --append-notes "MEASURED: <command> → <result>"
bd close <id> --reason "<commit> — <what landed>"
```

Never use `--notes` on an existing bead: it overwrites the field. Label evidence
`MEASURED` (command and result), `READ` (file:line), `ASSUMED` (not checked).
Discovered work gets a separate bead with `--deps discovered-from:<id>`. Do not
close before the change reaches `main`.

## Definition of Done

Before landing, the supervisor verifies:

1. Requested scope and docs agree; hardware facts cite their source by name and
   section, and a test witnesses each implemented fact.
2. `cargo fmt --all --check` and clippy with all targets and features and
   `-D warnings` pass.
3. Workspace tests and doctests pass.
4. `cargo deny --all-features check licenses bans sources` passes.
5. `scripts/publication-guard.sh` passes.
6. A fresh session reviews the final change; findings are verified against
   evidence before they are acted on.

Never treat an unavailable gate as green or bypass hooks to land a change.

## Git flow and publication

One bead, one branch `<type>/<bead-or-topic>`, one worktree outside the main
checkout. The supervisor squash-merges one Conventional, signed commit per bead.
Never commit or push without an explicit maintainer request. Push only to the
configured `origin`. Repository creation, mirroring and publishing belong to the
maintainer.

The beads database is ignored by git. `bd dolt push` to `refs/dolt/data` on
`origin` is a push and needs authorization; `origin` is public, so bead history
is published at once. Setup commands can commit on their own; read their help and
get the authorization first.

Everything is English and publish-safe, bead history and memories included.
Durable decisions go in `backlog/decisions/`; bead-bound choices are append-notes.
See [decisions](decisions.md).

## Session handoff

At session end, append completed work, next step, blockers and commands/results:

```sh
bd update <id> --append-notes "Session pause <date>. Done: <X>. Next: <Y>. Blocker: <Z|none>."
```

The next session reads the bead before rediscovering state from the diff.
