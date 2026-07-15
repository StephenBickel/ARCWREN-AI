# Architecture

Status: pre-alpha foundation. The [approved v1 design](superpowers/specs/2026-07-13-arcwren-v1-design.md) defines the target; this document separates current code from that target.

## System shape

ArcWren is designed as one Rust package with a library and one executable. Frontends create and consume provider-neutral events; they do not call model providers or tools directly.

```text
TUI (planned) --------+
                      +--> runtime (planned) --> provider boundary
Telegram (planned) ---+            |
                                   +--> policy (planned) --> tools (planned)
                                   |
                                   +--> event log (implemented) --> projections
```

This layout keeps user interfaces replaceable, provider wire formats contained, policy centralized, and scenario tests deterministic.

## Implemented foundation

- `events`: schema-versioned envelopes, provider-neutral event payloads, and stable serialized IDs.
- `error`: stable public error codes with sanitized user messages and separate internal detail.
- `runtime::budget`: hard counters for turn iterations and tool-call limits.
- `storage`: SQLite WAL, forward migrations verified by checksum, transactional append-only events, and durable session, memory, and approval lifecycle records.
- `providers`: a normalized provider request/event trait and a scripted adapter that replays sanitized JSON fixtures with cancellation support.
- `cli`: a Clap help shell reserving the planned top-level command names.

There is no production HTTP adapter, context assembler, turn state machine, policy evaluator, tool executor, TUI, configuration loader, diagnostics command, or Telegram transport yet.

## Planned turn boundary

The v1 runtime will validate and persist user input, assemble bounded context, stream normalized provider events, validate proposed tool calls, evaluate policy, persist approval decisions and tool results, then continue until a final answer, cancellation, or budget exhaustion. Consequential state transitions are to be persisted before a frontend exposes them.

Partially executed tools will not be resumed automatically after a crash: repeating a non-idempotent action is more dangerous than requiring the owner to review an interruption. The append-only log remains the audit and replay source while materialized projections serve normal reads. This decision is recorded in [ADR 0001](adr/0001-event-sourced-runtime.md).

## Ownership and process model

V1 permits one ArcWren process to own a data directory at a time. The interactive process and future headless `serve` mode are alternate owners, not concurrent daemons. This reduces locking, approval, cancellation, and credential-lifetime ambiguity while the public interfaces stabilize. See [ADR 0002](adr/0002-single-process-v1.md).

## Stable boundaries, unstable details

The event, provider, tool, policy, storage, and frontend boundaries are architectural commitments. Concrete configuration keys, provider request mappings, UI layout, and platform-specific process isolation remain subject to implementation and testing. Documentation must not present a target interface as current behavior.
