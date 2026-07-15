# ADR 0001: Event-sourced runtime

- Status: Accepted for v1
- Date: 2026-07-13

## Context

An agent turn can stream provider output, request approval, execute consequential tools, fail, or be cancelled. A chat transcript alone cannot explain which proposal was approved, what arguments ran, or where an interrupted turn stopped. Debugging and deterministic scenario tests need a provider-neutral record.

## Decision

Use an append-only, schema-versioned event stream as the durable execution record. Persist consequential state transitions before exposing them to a frontend. Maintain query-efficient projections that can be rebuilt from events, and keep oversized artifacts outside SQLite behind content-addressed references.

Do not automatically resume a partially executed tool after a crash. Record the interruption and require an explicit user decision, because implicit retry could duplicate a non-idempotent action.

The versioned event types, append-only SQLite storage, and lifecycle persistence are implemented. The complete runtime state machine and projection rebuild path remain planned.

## Consequences

- Replays, audits, crash diagnosis, and deterministic tests have a stable source.
- Schema evolution must be explicit and backward-aware.
- Events and exports are sensitive data and require redaction and retention controls.
- Projection code and storage volume add complexity.
- Runtime actions must respect persistence ordering, even when that adds latency.
