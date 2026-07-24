# ADR 0002: Single-process v1

- Status: Accepted for v1
- Date: 2026-07-13

## Context

Concurrent terminal, gateway, and background processes sharing one data directory would require distributed ownership of sessions, approvals, cancellation, migrations, and credentials. That coordination would expand the failure surface before the runtime contracts are stable.

## Decision

Permit exactly one Carl process to own a data directory at a time in v1. The future interactive TUI and headless `serve` mode are alternate entry points to the same runtime, not concurrent peers. External service managers may keep the headless process alive, but Carl will not install a system service automatically.

The SQLite store exists today; the process-level data-directory lock, TUI, and headless gateway remain to be implemented.

## Consequences

- Storage, approval, migration, and shutdown semantics remain understandable.
- A user must stop one mode before starting the other for the same data directory.
- Remote continuity requires the headless mode to remain running.
- A supervised daemon and thin clients may be reconsidered after runtime and event interfaces stabilize.
