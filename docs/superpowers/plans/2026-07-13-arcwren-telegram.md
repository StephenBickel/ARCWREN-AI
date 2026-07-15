# ArcWren Telegram Gateway Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:executing-plans` to implement this plan task by task. Use `superpowers:test-driven-development` for every behavior change and `superpowers:verification-before-completion` before declaring the plan complete.

**Goal:** Add an owner-only Telegram long-polling frontend that uses the same persisted runtime as the TUI and safely supports session control, streamed answers, approvals, denial, and cancellation.

**Architecture:** A raw Telegram Bot API client sits behind a transport trait so contract tests use a local fake. The gateway validates chat type and owner identity before runtime invocation, persists update progress after safe handling, and routes approved private text and callbacks into runtime commands. It never calls a provider or tool directly.

**Tech stack:** Existing ArcWren core, Tokio, Reqwest, Serde, SQLite, and deterministic mock HTTP fixtures.

## Global constraints

- Use long polling only; do not open a public listener or implement webhooks.
- Ignore group, channel, guest, malformed, and unsupported updates before model invocation.
- Persist the confirmed offset only after an update is safely handled.
- Bind each callback to one persisted approval and make duplicate delivery idempotent.
- Never render credentials, raw environment values, or unbounded tool arguments.

---

### Task 1: Define Telegram wire types and a testable Bot API client

**Files:**

- Create: `src/frontends/telegram/mod.rs`
- Create: `src/frontends/telegram/api.rs`
- Create: `src/frontends/telegram/types.rs`
- Create: `tests/telegram_api_contract.rs`
- Create: `tests/fixtures/telegram/*.json`

**Step 1: Write failing transport tests**

Assert `getUpdates` sends the persisted offset and allowed update types, `sendMessage` escapes content correctly, `editMessageText` handles rate-limit responses, `answerCallbackQuery` is called, API-level `ok: false` becomes a typed channel error, and the bot token is redacted from errors.

**Step 2: Confirm failure**

Run: `cargo test --test telegram_api_contract`

**Step 3: Implement the minimal client**

Model only fields ArcWren consumes. Put the token in the URL at the final request boundary and sanitize it from every error. Support injected base URL for tests. Implement bounded, cancellation-aware retry for transport, 429, and 5xx responses and honor Telegram's retry delay.

**Step 4: Verify and commit**

Run: `cargo test --test telegram_api_contract`

Commit: `feat: add Telegram Bot API client`

---

### Task 2: Implement expiring single-owner pairing

**Files:**

- Create: `src/frontends/telegram/pairing.rs`
- Create: `src/commands/pair.rs`
- Modify: `src/cli.rs`
- Modify: `src/main.rs`
- Create: `tests/telegram_pairing.rs`

**Step 1: Write failing pairing tests**

Use injected clock and random source. Cover one-time code creation, hashed-at-rest storage, private-chat-only claim, expiry, single use, re-pair invalidation, wrong-code behavior, and owner/chat persistence. Assert logs and events never contain the plaintext bot token or active pairing code.

**Step 2: Confirm failure**

Run: `cargo test --test telegram_pairing`

**Step 3: Implement pairing**

`arcwren pair telegram` creates a short-lived human-readable code and stores only a keyed hash plus expiry. The next matching private text update atomically installs owner user/chat IDs and consumes the code. Re-pairing invalidates the prior owner.

**Step 4: Verify and commit**

Run: `cargo test --test telegram_pairing`

Commit: `feat: secure Telegram owner pairing`

---

### Task 3: Add durable long polling and admission control

**Files:**

- Create: `src/frontends/telegram/gateway.rs`
- Create: `src/frontends/telegram/admission.rs`
- Modify: `src/storage/repository.rs`
- Create: `tests/telegram_gateway.rs`

**Step 1: Write failing gateway tests**

Cover startup from persisted offset, ordered update handling, empty polls, graceful cancellation, duplicate update IDs, crash before commit, restart after commit, group/channel rejection, unpaired-user rejection, unsupported update rejection, and owner private-text admission. Use a provider spy to prove rejected updates never invoke the runtime.

**Step 2: Confirm failure**

Run: `cargo test --test telegram_gateway`

**Step 3: Implement the poll loop**

Fetch with `timeout`, classify before routing, write processed update and next offset in the same transaction after safe handling, and bound retry/circuit state. Unsupported and unauthorized updates are safe no-ops but still advance the offset once classified.

**Step 4: Verify and commit**

Run: `cargo test --test telegram_gateway`

Commit: `feat: add durable Telegram polling`

---

### Task 4: Route Telegram sessions and render streamed answers

**Files:**

- Create: `src/frontends/telegram/commands.rs`
- Create: `src/frontends/telegram/render.rs`
- Create: `src/frontends/telegram/router.rs`
- Create: `tests/telegram_sessions.rs`

**Step 1: Write failing session and rendering tests**

Cover stable owner-to-session mapping, `/new`, `/sessions`, `/resume`, `/cancel`, `/status`, `/help`, ordinary text turns, initial status message, rate-limited edits, finalization, Telegram length splitting, Markdown fallback, and runtime cancellation. Inject the clock so edit coalescing is deterministic.

**Step 2: Confirm failure**

Run: `cargo test --test telegram_sessions`

**Step 3: Implement routing and renderer**

Map commands to storage/runtime APIs. Create one status message, coalesce streamed deltas into bounded edits, split final output without cutting UTF-8, and fall back to plain text if formatting is rejected. Store Telegram message identifiers only as channel metadata, not in provider context.

**Step 4: Verify and commit**

Run: `cargo test --test telegram_sessions`

Commit: `feat: route ArcWren sessions through Telegram`

---

### Task 5: Implement inline approvals with callback idempotency

**Files:**

- Create: `src/frontends/telegram/approvals.rs`
- Modify: `src/frontends/telegram/router.rs`
- Create: `tests/telegram_approvals.rs`

**Step 1: Write failing security tests**

Cover allow/deny buttons, exact sanitized operation rendering, owner-only callbacks, approval expiry, argument binding, forged callback data, approval for another session, duplicate callback delivery, callback after resolution, and restart with a pending approval. Use a counting tool fake to prove at-most-once execution.

**Step 2: Confirm failure**

Run: `cargo test --test telegram_approvals`

**Step 3: Implement opaque callbacks**

Use short opaque callback identifiers mapped to persisted approval IDs. Atomically transition pending approval to allowed/denied before resuming the runtime. Duplicate or expired callbacks acknowledge with a safe status and never execute a tool again.

**Step 4: Verify and commit**

Run: `cargo test --test telegram_approvals`

Commit: `feat: add idempotent Telegram approvals`

---

### Task 6: Wire `serve`, diagnostics, and end-to-end Telegram proof

**Files:**

- Create: `src/commands/serve.rs`
- Modify: `src/commands/mod.rs`
- Modify: `src/diagnostics.rs`
- Modify: `src/main.rs`
- Create: `tests/e2e_telegram.rs`

**Step 1: Write a failing full gateway journey**

Drive the actual `serve` command against a fake Bot API: pair owner, reject guest/group updates, start a session, request a shell action, approve it once, stream the final response, cancel another turn, restart, and prove offsets and session state resume without duplication.

**Step 2: Confirm failure**

Run: `cargo test --test e2e_telegram`

**Step 3: Wire production dependencies**

Load token from credential store, acquire the single-process data-directory lock, build the existing runtime, start the polling gateway, expose sanitized health through `doctor`, and perform graceful shutdown on Ctrl-C.

**Step 4: Run Telegram plan verification**

Run:

```sh
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
cargo test --test e2e_telegram
```

Expected: all checks pass without a live Telegram token or network call.

Commit: `feat: complete Telegram v1 gateway`
