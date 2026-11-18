# unemunes

The NES emulator core of the unemu family: a library that emulates the Nintendo
Entertainment System and Famicom, and a small headless command line of the same
name. Front ends drive it through the `unemu-core-api` contract, the same
contract the PC core [unemupc](https://github.com/unemu-org/unemupc) implements.

The core embeds like a libretro core: it needs Rust's `std` with an allocator
and nothing else of the host. Frames, sound, input, game images (by SHA-256),
time and storage pass through the contract.

**Status:** early. The repository has its tooling and process documents; nothing
is emulated yet. See [status](backlog/docs/status.md).

## Game images

This repository never contains commercial game images or other proprietary
bytes. Bring your own legally obtained images. Open test ROMs are used as test
oracles under their own licences.

## Development

```sh
nix develop --no-update-lock-file
scripts/publication-guard.sh
```

See [commands](backlog/docs/commands.md) and [dev setup](backlog/docs/dev-setup.md).
Contributor and agent rules are in [AGENTS.md](AGENTS.md).

## Licence

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.
