# Carl

Carl is Stephen Bickel's personal, local-first Rust coding agent and an open-source agent harness built around a deterministic core, replayable events, explicit policy boundaries, and interchangeable model providers.

Carl's personality and operating principles are part of the repository; see the
[public operating contract](CARL.md). The name is personal rather than an acronym:
Carl is Stephen's middle name and his grandfather's name.

## Terminal and Telegram workflow

The intended v1 experience is one continuous session across a local terminal UI and an owner-only Telegram bot: begin a task at the workstation, inspect or approve proposed actions locally, then resume the same persisted session from a paired private chat. This is a **design preview, not captured output or a runnable demo**; neither frontend is implemented yet.

## Status: pre-alpha foundation

> [!WARNING]
> Carl is currently a **pre-alpha foundation** and is not yet a usable end-user agent. Only the CLI help shell is usable. The HTTP/OpenAI adapters, runtime tool loop, built-in tools, TUI interaction, and Telegram gateway are not implemented. Command names shown by help reserve the planned interface. Only the five placeholder commands `serve`, `auth`, `pair`, `doctor`, and `sessions` return not-implemented errors; Clap's built-in `help` command displays help.

The repository is being developed in public so the storage, event, provider, and policy boundaries can be reviewed before consequential tool execution exists.

## Features

Implemented and covered by deterministic tests:

- versioned, provider-neutral events plus stable IDs and typed, sanitized errors;
- hard turn-budget accounting primitives;
- SQLite WAL storage with checksum-verified forward migrations;
- append-only session events and durable session, memory, and approval lifecycles;
- a provider trait and deterministic scripted provider for offline contract tests;
- a Clap command/help shell for the planned top-level interface.

The approved v1 design adds a shared runtime loop, OpenAI and OpenAI-compatible HTTP adapters, bounded workspace tools, a TUI, explicit memory, and an owner-only Telegram gateway. These are roadmap items, not current capabilities.

## Quick start

The project currently requires the Rust toolchain declared in `rust-toolchain.toml`. Build the foundation, run its tests, and inspect the only supported CLI behavior:

```sh
cargo build --locked
cargo test --all-features --locked
cargo run --locked -- --help
```

If the binary is already on `PATH`, the equivalent help command is:

```sh
carl --help
```

Do not rely on `serve`, `auth`, `pair`, `doctor`, or `sessions` yet; they are placeholders.

## Architecture

Both planned frontends feed one provider-neutral event stream and are forbidden from calling providers or tools directly:

```text
TUI (planned) --------+
                      +--> runtime --> provider
Telegram (planned) ---+       |
                              +--> policy --> tools
                              |
                              +--> append-only event log --> projections
```

Today, the event model, storage layer, budget primitives, provider boundary, and scripted adapter exist. The runtime, policy evaluator, tools, production adapters, and frontends remain planned. See the [architecture guide](docs/architecture.md), the [approved Carl design](docs/superpowers/specs/2026-07-23-carl-top-tier-harness-design.md), and the decisions on [event-sourced execution](docs/adr/0001-event-sourced-runtime.md) and a [single-process v1](docs/adr/0002-single-process-v1.md).

## Security model

Carl treats model output, remote messages, fetched content, and tool arguments as untrusted. The v1 design requires workspace-confined file access, bounded output, explicit approval for consequential actions, credential redaction, and stricter remote policy. Most of those enforcement layers are not implemented yet; see the [security model](docs/security.md) and [security policy](SECURITY.md).

**Shell isolation is policy- and process-based in the v1 design; it is not a complete security sandbox.** A future `shell.exec` tool must not be treated as containment for hostile code, even after its workspace, timeout, environment-filtering, and cancellation controls are implemented.

## Provider setup

Provider configuration is not implemented. The approved design calls for the OpenAI Responses API to use an **OpenAI Platform API key**, plus an OpenAI-compatible adapter for local or third-party endpoints. Carl will not reuse Codex or ChatGPT credentials and will not call undocumented OAuth endpoints. See the [configuration guide](docs/configuration.md) and the [authentication ADR](docs/adr/0003-no-undocumented-oauth.md).

## Telegram pairing

The Telegram gateway is not implemented. The v1 target uses outbound long polling with no public listener and permits exactly one paired owner in one private chat. Pairing will use a short-lived, one-time code; re-pairing invalidates the previous owner. Group, channel, guest, and unpaired updates will be rejected before model invocation. The planned flow and remote approval rules are documented in the [Telegram guide](docs/telegram.md).

## Development

Start with [CONTRIBUTING.md](CONTRIBUTING.md). The local quality gate is:

```sh
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
cargo test --doc
```

Public behavior should be developed test-first with deterministic fixtures; normal tests must not require live model or Telegram credentials. Changes follow the [Code of Conduct](CODE_OF_CONDUCT.md), and notable work is recorded in the [changelog](CHANGELOG.md).

## Roadmap

- [x] Provider-neutral domain contracts, budgets, and durable event storage
- [x] Provider interface and deterministic scripted provider
- [ ] Production HTTP/OpenAI-compatible adapters
- [ ] Runtime tool/approval loop, policy engine, and bounded built-in tools
- [ ] Interactive TUI and session operations
- [ ] Owner-only Telegram long-polling gateway
- [ ] Cross-platform CI and checksummed releases

The [approved design](docs/superpowers/specs/2026-07-23-carl-top-tier-harness-design.md) is the source of truth for v1 scope; checkboxes here describe repository state, not release promises.

## License

Carl is available under the [MIT License](LICENSE).
