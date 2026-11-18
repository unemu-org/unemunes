# Commands

Run these inside the dev shell (`nix develop --no-update-lock-file`, or direnv).
The Rust gates need a Cargo workspace; it arrives with the first crate, and
until then CI skips them.

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

## Hooks

```sh
pre-commit run --all-files
```

The pre-commit stage runs the generic file checks, the publication guard,
`cargo fmt --check` and the GPG UID check; the pre-push stage runs clippy.
