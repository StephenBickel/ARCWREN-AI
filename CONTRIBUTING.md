# Contributing to Carl

Carl is a pre-alpha foundation. Contributions are welcome, but public documentation and code must distinguish implemented behavior from the approved v1 design.

## Before starting

- Read [CARL.md](CARL.md), the [Carl design](docs/superpowers/specs/2026-07-23-carl-top-tier-harness-design.md), and relevant records in `docs/adr/`.
- Discuss large scope or architecture changes before implementation.
- Report suspected vulnerabilities through the private process in [SECURITY.md](SECURITY.md), not a public issue.
- Never commit credentials, private traces, generated binaries, or personal data.

## Development workflow

Use the Rust toolchain pinned by `rust-toolchain.toml`. Keep `Cargo.lock` committed. Add a failing test before changing behavior, confirm the expected failure, implement the smallest change, and rerun the focused test before the full gate.

```sh
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
cargo test --doc
git diff --check
```

Normal tests must be deterministic and offline. Provider or channel behavior belongs behind an injected boundary with sanitized fixtures. Live tests, when they exist, must be opt-in and must not print credentials.

## Pull requests

Keep changes narrow and explain:

- the behavior or documentation contract being changed;
- the RED and GREEN test evidence;
- security consequences, including new data, network, credential, or process access;
- user-visible limitations and any deferred work.

Do not claim platform support, benchmark results, commands, or integrations without reproducible evidence. Update public docs and `CHANGELOG.md` when behavior changes.

By contributing, you agree that your contribution is licensed under the repository's MIT License and that you will follow the [Code of Conduct](CODE_OF_CONDUCT.md).
