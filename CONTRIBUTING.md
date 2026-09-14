# Contributing

Thanks for taking the time to contribute.

## Development setup

```bash
git clone https://github.com/anaxo-io/orderbook-rs
cd orderbook-rs
cargo test --all-features
```

The toolchain is pinned in `rust-toolchain.toml`; `rustup` picks it up automatically.

## Before opening a pull request

```bash
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
cargo test --no-default-features
cargo doc --no-deps --all-features
```

All five must pass. CI runs the same commands plus an MSRV check against Rust 1.85 and `cargo deny check`.

## Guidelines

- **Tests are the contract.** `tests/property_tests.rs` encodes the book invariants (sortedness, best bid/ask, deduplication, zero-quantity removal). If a change requires weakening a property, explain why in the pull request.
- **No `unsafe`.** The crate is `#![forbid(unsafe_code)]`.
- **No new dependencies** without a reason that cannot be met by the standard library or an existing dependency.
- **Benchmarks** live in `benches/`. If a change targets performance, include before/after numbers from `cargo bench`.
- **Public items need docs.** The crate builds with `#![warn(missing_docs)]` and CI treats rustdoc warnings as errors.

## Commit messages

[Conventional Commits](https://www.conventionalcommits.org/): `feat:`, `fix:`, `docs:`, `perf:`, `refactor:`, `test:`, `chore:`.

## Licence

Contributions are dual-licensed under MIT and Apache-2.0, matching the crate.
