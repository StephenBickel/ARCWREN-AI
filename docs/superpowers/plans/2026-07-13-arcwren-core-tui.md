# ArcWren Core and TUI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:executing-plans` to implement this plan task by task. Use `superpowers:test-driven-development` for every behavior change and `superpowers:verification-before-completion` before declaring the plan complete.

**Goal:** Deliver a minimal Rust agent harness whose deterministic runtime, durable event log, provider boundary, policy engine, built-in tools, skills, and TUI can complete and replay a tool-using conversation.

**Architecture:** A single `arcwren` package exposes a library and binary. The library owns provider-neutral domain types and a persisted turn state machine. Frontends only submit inputs and consume events; providers, tools, policy, and storage are injected behind narrow traits. SQLite is the durable source of truth, and the TUI is a thin reducer over runtime events.

**Tech stack:** Rust 1.97, Tokio, Clap, Serde, Reqwest, SQLite via Rusqlite, Ratatui/Crossterm, Tracing, Schemars, and the operating-system keyring. `Cargo.lock` is committed for reproducible application builds.

## Global constraints

- Keep one package and one executable; do not introduce a workspace or plugin ABI.
- Add a failing test before each behavior and observe the intended failure.
- Never make a live provider request in the default test suite.
- Persist every consequential transition before broadcasting it.
- Keep secrets out of configuration, events, logs, snapshots, and test fixtures.
- Run `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test --all-features` after every task cluster.

---

### Task 1: Bootstrap the package and CLI contract

**Files:**

- Create: `rust-toolchain.toml`
- Create: `Cargo.toml`
- Create: `src/lib.rs`
- Create: `src/main.rs`
- Create: `src/cli.rs`
- Create: `tests/cli_contract.rs`

**Step 1: Write the failing CLI contract test**

Test the real binary with `assert_cmd` and require `--help` to advertise `serve`, `auth`, `pair`, `doctor`, and `sessions`.

```rust
#[test]
fn help_exposes_the_v1_commands() {
    let mut command = assert_cmd::Command::cargo_bin("arcwren").unwrap();
    command.arg("--help").assert().success().stdout(
        predicates::str::contains("serve")
            .and(predicates::str::contains("auth"))
            .and(predicates::str::contains("pair"))
            .and(predicates::str::contains("doctor"))
            .and(predicates::str::contains("sessions")),
    );
}
```

**Step 2: Confirm the test fails**

Run: `cargo test --test cli_contract help_exposes_the_v1_commands`

Expected: failure because the package and binary do not exist yet.

**Step 3: Add the minimum package and parser**

Pin channel `1.97.0` with `rustfmt` and `clippy`. Define a `Cli` and `Command` enum with Clap. `main` parses input and routes each command to a temporary typed `NotImplemented` error; `--help` must still exit successfully. Export `cli` from the library.

**Step 4: Verify and commit**

Run:

```sh
cargo fmt --check
cargo test --test cli_contract
cargo clippy --all-targets -- -D warnings
git add Cargo.toml Cargo.lock rust-toolchain.toml src tests/cli_contract.rs
git commit -m "feat: bootstrap ArcWren CLI"
```

Expected: all checks pass.

---

### Task 2: Define stable events, IDs, errors, and turn budgets

**Files:**

- Create: `src/events.rs`
- Create: `src/error.rs`
- Create: `src/runtime/mod.rs`
- Create: `src/runtime/budget.rs`
- Modify: `src/lib.rs`
- Create: `tests/domain_contract.rs`

**Step 1: Write failing serialization and budget tests**

Require stable tagged JSON for `UserInput`, `AssistantTextDelta`, `ToolProposed`, `ApprovalRequested`, `ToolCompleted`, `TurnCompleted`, and `TurnInterrupted`. Require a budget to reject an iteration or tool call beyond its configured maximum with `ArcWrenError::BudgetExceeded`.

```rust
let encoded = serde_json::to_value(Event::UserInput {
    text: "hello".into(),
})?;
assert_eq!(encoded["type"], "user_input");
assert_eq!(encoded["schema_version"], 1);
```

**Step 2: Run the focused tests and observe missing-type failures**

Run: `cargo test --test domain_contract`

**Step 3: Implement the smallest typed domain**

Use UUID newtypes for session, turn, event, tool-call, and approval IDs. Give persisted event envelopes an ID, session ID, optional turn ID, sequence, schema version, timestamp, and flattened event payload. Define an exhaustive `ArcWrenError` enum with public error codes and sanitized user messages. Define `TurnBudget` and a mutable `BudgetTracker` with checked counters.

**Step 4: Verify and commit**

Run: `cargo test --test domain_contract && cargo clippy --all-targets -- -D warnings`

Commit: `feat: define runtime event contract`

---

### Task 3: Implement the append-only SQLite event store

**Files:**

- Create: `migrations/0001_init.sql`
- Create: `src/storage/mod.rs`
- Create: `src/storage/schema.rs`
- Create: `src/storage/repository.rs`
- Modify: `src/lib.rs`
- Create: `tests/storage_contract.rs`

**Step 1: Write failing persistence tests**

Cover migration of a fresh temporary database, WAL mode, monotonic per-session sequence assignment, session creation/listing, append-and-read ordering, approval state, explicit memory state, and rollback when a transaction fails.

```rust
let first = store.append(session.id, None, Event::UserInput { text: "one".into() })?;
let second = store.append(session.id, None, Event::UserInput { text: "two".into() })?;
assert_eq!((first.sequence, second.sequence), (1, 2));
```

**Step 2: Confirm failure**

Run: `cargo test --test storage_contract`

**Step 3: Implement migrations and repositories**

Create tables for migrations, sessions, events, messages, memories, approvals, Telegram state, processed Telegram updates, and usage observations. Use SQLite transactions for sequence allocation and append. Enable foreign keys, WAL, and a busy timeout. Store event JSON with a schema version and reject unknown future versions with a typed storage error.

**Step 4: Verify and commit**

Run: `cargo test --test storage_contract && cargo test --doc`

Commit: `feat: add durable event storage`

---

### Task 4: Add the provider boundary and scripted provider

**Files:**

- Create: `src/providers/mod.rs`
- Create: `src/providers/scripted.rs`
- Modify: `src/lib.rs`
- Create: `tests/provider_contract.rs`
- Create: `tests/fixtures/provider/tool_then_answer.json`

**Step 1: Write the failing provider contract**

Define a normalized request containing messages, tool schemas, model settings, and a cancellation token. Require an async provider stream to yield text deltas, complete structured tool calls, usage, and a finish reason. The scripted provider must deterministically replay a JSON fixture and record requests for assertions.

**Step 2: Confirm failure**

Run: `cargo test --test provider_contract`

**Step 3: Implement provider-neutral traits and the fake**

Create `Provider`, `ProviderCapabilities`, `ModelRequest`, `ProviderEvent`, `ToolDefinition`, and typed provider errors. Keep provider-specific wire types private. The scripted provider must not sleep or call the network.

**Step 4: Verify and commit**

Run: `cargo test --test provider_contract && cargo clippy --all-targets -- -D warnings`

Commit: `feat: add provider abstraction and scripted adapter`

---

### Task 5: Add OpenAI Responses and compatible HTTP adapters

**Files:**

- Create: `src/providers/http.rs`
- Create: `src/providers/openai.rs`
- Create: `src/providers/compatible.rs`
- Create: `tests/provider_http_contract.rs`
- Create: `tests/fixtures/openai/*.json`

**Step 1: Write failing local-server contract tests**

Use a local mock HTTP server to assert endpoint, authorization redaction, request normalization, SSE parsing across chunk boundaries, structured tool-call assembly, usage parsing, `Retry-After`, authentication failure classification, and cancellation. Fixtures must be synthetic and contain no real credentials.

**Step 2: Confirm failure**

Run: `cargo test --test provider_http_contract`

**Step 3: Implement shared bounded HTTP and adapters**

The OpenAI adapter targets `/v1/responses`. The compatible adapter exposes a configurable base URL and model; include Ollama and LM Studio presets with no credential by default. Use the same normalized provider stream and explicit capability declarations. Retry only transport, 429, and 5xx failures before any tool call is emitted, with bounded attempts and cancellation-aware backoff.

**Step 4: Verify and commit**

Run: `cargo test --test provider_http_contract && cargo clippy --all-targets -- -D warnings`

Commit: `feat: support OpenAI and compatible providers`

---

### Task 6: Build the policy and approval engine

**Files:**

- Create: `src/policy/mod.rs`
- Create: `src/policy/redaction.rs`
- Create: `tests/policy_contract.rs`

**Step 1: Write the failing decision matrix**

Table-test `allow`, `ask`, and `deny` across local and Telegram frontends. Reads may be allowed; patch, shell, memory mutation, and remote outbound action must ask by default. Assert an approval binds to a canonical hash of tool name plus arguments and cannot approve modified arguments. Assert known secret values are redacted before serialization.

**Step 2: Confirm failure**

Run: `cargo test --test policy_contract`

**Step 3: Implement policy values and approval lifecycle**

Keep policy evaluation pure. Persist pending/resolved approvals through storage. Use constant-time comparison for approval binding where practical, expire approvals by injected clock, and default expiration to denial.

**Step 4: Verify and commit**

Run: `cargo test --test policy_contract`

Commit: `feat: enforce capability policy and approvals`

---

### Task 7: Implement the tool registry and workspace-safe file tools

**Files:**

- Create: `src/tools/mod.rs`
- Create: `src/tools/fs.rs`
- Create: `src/tools/path_guard.rs`
- Create: `tests/file_tools.rs`

**Step 1: Write failing schema and boundary tests**

Test generated JSON Schema, bounded output, normal read/list/search/patch behavior, `..` traversal rejection, absolute outside-path rejection, and symlink escape rejection. Patch tests must verify an exact preview before applying.

**Step 2: Confirm failure**

Run: `cargo test --test file_tools`

**Step 3: Implement a typed registry**

Define a `Tool` trait, typed `ToolContext`, normalized `ToolOutcome`, and registry lookup. Canonicalize the workspace once, resolve existing ancestors for new paths, reject escape, cap file/search output, and apply only explicit unified patches with an atomic replacement.

**Step 4: Verify and commit**

Run: `cargo test --test file_tools && cargo clippy --all-targets -- -D warnings`

Commit: `feat: add workspace-safe file tools`

---

### Task 8: Add shell, web, and explicit-memory tools

**Files:**

- Create: `src/tools/shell.rs`
- Create: `src/tools/web.rs`
- Create: `src/tools/memory.rs`
- Create: `tests/shell_tool.rs`
- Create: `tests/web_tool.rs`
- Create: `tests/memory_tool.rs`

**Step 1: Write focused failing tests**

For shell, cover workspace working directory, filtered environment, timeout, cancellation, process termination, stdout/stderr caps, and exit status. For web, cover only HTTP(S), no URL credentials, redirect/size/type caps, and private-network denial under remote policy. For memory, cover approved remember/forget and provenance.

**Step 2: Observe the failures**

Run: `cargo test --test shell_tool --test web_tool --test memory_tool`

**Step 3: Implement one tool at a time**

Use argument arrays rather than an implicit shell by default. Remove variables whose names match credential patterns and all configured secret values. Resolve DNS before remote fetch and reject loopback/private/link-local targets. Store memories transactionally through the repository.

**Step 4: Verify and commit**

Run: `cargo test --test shell_tool --test web_tool --test memory_tool`

Commit: `feat: add bounded execution and memory tools`

---

### Task 9: Implement the deterministic turn state machine

**Files:**

- Create: `src/runtime/agent.rs`
- Create: `src/runtime/context.rs`
- Create: `src/runtime/approval.rs`
- Modify: `src/runtime/mod.rs`
- Create: `tests/runtime_scenarios.rs`
- Create: `tests/fixtures/scenarios/*.yaml`

**Step 1: Add failing executable scenarios**

Fixtures must cover direct answer, tool-then-answer, denied tool, pending approval and resume, cancellation, budget exhaustion, provider failure, tool failure, and interrupted persisted turn. Assert the exact ordered event kinds and that a consequential tool never runs before its approval event is durable.

```yaml
name: tool_then_answer
input: "Read README.md"
provider:
  - tool_call: { name: fs.read, arguments: { path: README.md } }
  - text: "The file says ..."
expect_events:
  - user_input
  - tool_proposed
  - policy_decided
  - tool_started
  - tool_completed
  - assistant_text_delta
  - turn_completed
```

**Step 2: Confirm failure**

Run: `cargo test --test runtime_scenarios`

**Step 3: Implement the loop**

Build context from persisted messages, eligible skills, approved memories, policy summary, and tool schemas. Drive provider events into persisted ArcWren events; validate tool arguments; evaluate policy; suspend on approval; execute with cancellation and budgets; append results; and continue until final or terminal error. Broadcast only after append succeeds.

**Step 4: Verify and commit**

Run: `cargo test --test runtime_scenarios && cargo test --all-features`

Commit: `feat: implement replayable agent runtime`

---

### Task 10: Load progressive instruction skills

**Files:**

- Create: `src/skills/mod.rs`
- Create: `src/skills/metadata.rs`
- Create: `tests/skills_contract.rs`
- Create: `tests/fixtures/skills/eligible/SKILL.md`
- Create: `tests/fixtures/skills/ineligible/SKILL.md`

**Step 1: Write failing discovery tests**

Require project skills to override user skills with the same normalized name, metadata-only discovery, lazy instruction loading, platform/executable/credential eligibility, duplicate-name diagnostics, and rejection of malformed frontmatter.

**Step 2: Confirm failure**

Run: `cargo test --test skills_contract`

**Step 3: Implement discovery and lazy loading**

Parse only the bounded frontmatter during discovery. Never execute skill-owned code. Return explicit eligibility reasons and load the body only when selected for context assembly.

**Step 4: Verify and commit**

Run: `cargo test --test skills_contract`

Commit: `feat: add progressive instruction skills`

---

### Task 11: Add layered configuration and credential storage

**Files:**

- Create: `src/config/mod.rs`
- Create: `src/config/credentials.rs`
- Create: `src/config/paths.rs`
- Create: `tests/config_contract.rs`

**Step 1: Write failing precedence and secrecy tests**

Require defaults < user config < project config < environment < CLI. Assert serialized config stores credential references, never values. Test API-key, none, missing credential, and future OAuth variants. Use an in-memory credential store in tests.

**Step 2: Confirm failure**

Run: `cargo test --test config_contract`

**Step 3: Implement config and credential traits**

Use platform config/data directories, explicit profile structs, and a `CredentialStore` trait backed by the OS keyring in production. Support environment variables for automation without persisting them. Keep `OAuth` typed but return `unsupported` for OpenAI until a documented third-party flow exists.

**Step 4: Verify and commit**

Run: `cargo test --test config_contract`

Commit: `feat: add secure provider configuration`

---

### Task 12: Build the TUI as a pure event reducer

**Files:**

- Create: `src/frontends/mod.rs`
- Create: `src/frontends/tui/mod.rs`
- Create: `src/frontends/tui/app.rs`
- Create: `src/frontends/tui/render.rs`
- Create: `src/frontends/tui/input.rs`
- Create: `tests/tui_contract.rs`
- Modify: `src/main.rs`

**Step 1: Write failing reducer and render tests**

Drive the TUI model with events and assert conversation, streamed text, approval panel, trace panel, active model/workspace, budget display, cancel command, and terminal restoration. Render into Ratatui's test backend; assert semantic buffer regions rather than full ANSI output.

**Step 2: Confirm failure**

Run: `cargo test --test tui_contract`

**Step 3: Implement the thin frontend**

Keep runtime work in Tokio tasks and UI state in a pure reducer. Map input to runtime commands. Provide approval keys, trace toggle, cancellation, session resume, and a non-interactive test mode. Install a panic hook and always restore the terminal.

**Step 4: Verify and commit**

Run: `cargo test --test tui_contract && cargo test --all-features`

Commit: `feat: add interactive terminal frontend`

---

### Task 13: Finish local commands and end-to-end proof

**Files:**

- Create: `src/diagnostics.rs`
- Create: `src/commands/mod.rs`
- Create: `src/commands/auth.rs`
- Create: `src/commands/doctor.rs`
- Create: `src/commands/sessions.rs`
- Modify: `src/main.rs`
- Create: `tests/e2e_scripted.rs`

**Step 1: Write a failing binary-level journey**

In a temporary data directory, run a scripted conversation that proposes a read, accepts approval, finishes, lists the session, exports its sanitized trace, and resumes it. Add `doctor --json` assertions for stable machine-readable checks. No network or real keyring access is allowed.

**Step 2: Confirm failure**

Run: `cargo test --test e2e_scripted`

**Step 3: Implement command routing and diagnostics**

Add secure credential prompts, provider connectivity checks behind an explicit flag, database/workspace checks, sanitized diagnostics, session list/inspect/resume/export/delete, structured exit codes, and scripted test hooks enabled only in tests.

**Step 4: Run the core plan verification**

Run:

```sh
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
cargo test --doc
cargo run -- --help
cargo run -- doctor --json
```

Expected: all checks pass; `doctor` may report missing optional live credentials as `not_configured`, not as a crash.

Commit: `feat: complete local ArcWren workflow`
