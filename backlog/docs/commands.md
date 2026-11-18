# Commands

Run these inside the dev shell (`nix develop --no-update-lock-file`, or direnv).

## Gates

```sh
scripts/publication-guard.sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo nextest run --workspace --all-features --no-fail-fast
cargo test --workspace --doc --all-features
cargo deny --all-features check licenses bans sources
```

CI (`.github/workflows/ci.yml`) runs the publication guard and the same Rust
gates on Ubuntu, each gate even after an earlier one failed.

## Publication guard

`scripts/publication-guard.sh` scans tracked and untracked, non-ignored files
for private paths, e-mail addresses, private IPv4 addresses, game images (NES,
FDS, UNIF and NSF, by name and by signature) and Cargo.lock sources outside
crates.io and github.com. It needs only bash 3.2 and POSIX tools, so it runs
without the dev shell. Set `PUBLICATION_GUARD_PRIVATE_HOSTS` (comma-separated)
locally to check private names too; CI never sets it.

## Test oracles

Oracles that are not committed (decision-3) are fetched into the ignored
directory `.oracles/`, each file pinned by URL and SHA-256 in
`scripts/oracles.manifest`:

```sh
scripts/fetch-oracles.sh list                 # sets and file counts
scripts/fetch-oracles.sh fetch sst-nes6502    # about 1.1 GB
scripts/fetch-oracles.sh fetch nestest
scripts/fetch-oracles.sh verify               # offline check of what is present
```

A download that does not match its pin is removed and fails the run. Tests
that need a set skip with a message naming the fetch command when it is
absent.

## Hooks

```sh
pre-commit run --all-files
```

The pre-commit stage runs the generic file checks, the publication guard,
`cargo fmt --check` and the GPG UID check; the pre-push stage runs clippy.
