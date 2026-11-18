# Development setup

## Toolchain

The default flake shell selects Rust **1.98.1**, the same as x80 and x86jit,
with rust-src, rust-analyzer, clippy and rustfmt. `flake.lock` keeps the same
input pins as x80. The shell adds nextest, cargo-deny, pre-commit and
Backlog.md. No Nix package output is defined yet.

```sh
nix develop --no-update-lock-file
```

The first run may need network access to fetch the pinned inputs.

## Local activation

Run these by hand once per clone:

```sh
direnv allow
scripts/skills-bootstrap.sh
pre-commit install --install-hooks
```

Uncached hook environments may download tools. Configure GPG signing and a
matching e-mail and key UID yourself. The UID hook checks consistency only when
signing is already enabled; it does not enforce or configure signing.

Canonical vendored skills belong under `.agents/skills/`; the bootstrap script
links them into ignored `.claude/skills/`.

## Beads

Install bd (≥ 1.3.1) on the host; it is not in this shell. The issue prefix is
`nes`. Never open a newer schema with an older tool.

Until the database is initialized, there is no tracker in this clone. After it
is, a fresh clone uses the `bd bootstrap` recovery flow; `bd init` would mint a
new project identity. `bd init` and the first remote setup create commits and
need explicit maintainer authorization.

The database is ignored by git; synchronization targets only the configured
`origin`, with push authorization. See [working on tasks](working-on-tasks.md).
