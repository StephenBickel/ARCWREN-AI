# Telegram Gateway

The Telegram gateway is an approved v1 design target and is **not implemented**. Current `serve` and `pair` command names are placeholders and must not be used as setup instructions.

## Intended boundary

The gateway will call the Telegram Bot API through outbound long polling. It will not expose a public listener or support webhooks in v1. The bot token will live in the operating system credential store and must be redacted from errors, events, logs, and diagnostics.

The gateway is a frontend only: it may create input events and render output events, but it may not call providers or tools directly.

## Planned owner pairing

V1 permits one owner and one private chat:

1. The local owner requests a short-lived, one-time pairing code.
2. The owner sends that code to the bot in a private Telegram chat.
3. Carl atomically records the Telegram user and private-chat identifiers and consumes the code.
4. Re-pairing invalidates the prior owner.

Only a keyed representation of the active code should be stored. Expired, reused, group, channel, guest, and unpaired updates must be ignored before invoking a model. Pairing behavior requires deterministic clock, randomness, persistence, and admission-control tests before release.

## Planned session controls

The owner-only private chat is designed to support `/new`, `/sessions`, `/resume`, `/cancel`, `/status`, and `/help`. These are Telegram message commands, not current `carl` CLI commands. Ordinary private text will map to the owner's persistent Carl session.

Streaming will use a bounded initial status message followed by rate-limited edits and safe splitting of long final output. Persisted update offsets and processed identifiers will prevent acknowledged updates from replaying after restart.

## Remote approvals

Remote policy is stricter than local policy. Bounded reads and public web fetches may be allowed automatically; file patches, shell execution, memory changes, and other consequential operations require an inline approval showing a sanitized exact operation, workspace-relative target, risk category, and expiry.

Each callback will refer to one opaque persisted approval. Approval is owner-only, argument-bound, expiring, and atomically resolved before runtime continuation. Duplicate or forged callbacks must not execute a tool.

## Current status

There is no Telegram transport, token loading, pairing store integration, admission controller, renderer, callback handler, or gateway loop in the current codebase. Do not provide a real bot token to this revision. The [security model](security.md) and [approved Carl design](superpowers/specs/2026-07-23-carl-top-tier-harness-design.md) define the controls implementation must prove.
