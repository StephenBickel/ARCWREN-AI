# ArcWren v1 Design

Status: approved direction; implementation pending written-spec review
Date: 2026-07-13

## Product thesis

ArcWren is a minimal, local-first agent harness written in Rust. It is useful to consumers as a personal workstation agent and legible to engineers as an example of disciplined harness design. Its value comes from a small deterministic core, explicit policies, replayable execution, and clean frontend boundaries rather than a large integration catalog.

Version 1 supports an interactive terminal UI and an owner-only Telegram gateway. Both frontends use the same runtime, session model, provider adapters, tools, policies, and event stream.

## Goals

- Install as one `arcwren` executable on macOS, Linux, and Windows.
- Run general workstation and personal-assistant tasks through a polished TUI.
- Continue conversations remotely through a paired Telegram bot.
- Support cloud and local models without coupling the runtime to one provider.
- Make every model response, proposed action, approval, tool result, error, and usage update inspectable.
- Persist sessions and explicit memories locally.
- Load simple instruction-based skills without implementing a plugin marketplace.
- Provide deterministic scenario tests that exercise the harness without live model calls.
- Establish a secure, well-maintained open-source repository with reproducible CI and releases.

## Non-goals

- Multi-user or group-chat operation.
- A bundled background-service manager, hosted control plane, web UI, or mobile app.
- Telegram webhooks, attachments, voice messages, and rich media.
- Multi-agent orchestration or autonomous delegation.
- A dynamic native-plugin ABI or public plugin marketplace.
- Browser or desktop GUI automation.
- Automatic self-modification or automatic skill generation.
- Undocumented OpenAI OAuth or reuse of another application's access tokens.
- Perfect sandboxing on every operating system. V1 enforces workspace, process, and policy boundaries and documents their limits.

## User experience

The executable exposes these primary commands:

- `arcwren`: start the interactive TUI.
- `arcwren serve`: run configured remote gateways headlessly; v1 supports Telegram.
- `arcwren auth`: configure model-provider credentials and store secrets in the OS credential store.
- `arcwren pair telegram`: create or replace the single authorized Telegram pairing.
- `arcwren doctor`: validate configuration, database migrations, provider connectivity, credential availability, workspace access, and Telegram connectivity.
- `arcwren sessions`: list, inspect, resume, export, or delete local sessions.

First run presents a short setup flow: select a provider, select or enter a model, test connectivity, select a workspace root, and optionally configure Telegram. Local endpoints may be used without credentials.

The TUI shows the conversation, streamed output, proposed tools, approvals, tool results, current model, active workspace, and turn budget. Detailed traces are available on demand rather than filling the normal chat view.

## Architecture

ArcWren starts as one Rust package with a library target and one binary target. Internal modules have explicit interfaces but are not split into separately versioned crates until external consumers justify that cost.

```text
TUI -----------------+
                     |
Telegram gateway --->+-- Agent runtime --> Provider --> Model
                           |
                           +--> Policy --> Tool execution
                           |
                           +--> Event log --> Sessions and memory
```

Primary modules:

- `runtime`: deterministic turn state machine, context assembly, streaming, cancellation, budgets, and tool-call iteration.
- `events`: provider-neutral input and output event types and stable serialization.
- `providers`: model-provider traits, capability discovery, OpenAI, and OpenAI-compatible adapters.
- `tools`: tool traits, JSON Schema validation, built-in tools, and normalized results.
- `policy`: capability evaluation, approval lifecycle, local and remote policies, and redaction.
- `storage`: SQLite migrations, repositories, append-only events, sessions, memory, Telegram state, and projections.
- `skills`: discovery, metadata validation, eligibility checks, and progressive instruction loading.
- `frontends::tui`: terminal rendering and local interaction.
- `frontends::telegram`: long polling, pairing, update deduplication, rendering, and approval callbacks.
- `config`: layered configuration, profiles, paths, and credential references.
- `diagnostics`: health checks and sanitized diagnostic output.

Frontends may create input events and consume output events. They cannot call providers or tools directly. Provider-specific types do not escape the provider module. Tools do not access frontend state.

Only one ArcWren process may own a data directory at a time in v1. `arcwren serve` is the headless mode for users who want Telegram available after closing the TUI. Service-manager and container examples may keep that process running, but ArcWren does not install a system service automatically.

## Turn execution

Each turn follows an explicit state machine:

1. Validate and persist the user input.
2. Assemble system instructions, eligible skill instructions, relevant explicit memories, recent session history, tool definitions, and permission context.
3. Send a provider-neutral request through the selected model adapter.
4. Normalize provider streaming responses into ArcWren events.
5. Validate proposed tool calls against their schemas.
6. Ask the policy engine to allow, deny, or request approval.
7. Execute approved calls, bound their output, and persist normalized results.
8. Continue until the model returns a final response, the user cancels, or a budget is exhausted.

The runtime persists events before exposing consequential state transitions to a frontend. A crash leaves a traceable interrupted turn. V1 does not automatically resume a partially executed tool call, because doing so could repeat a non-idempotent action. The user may resume the session with an interruption summary.

Every turn has configurable hard limits:

- model/tool iteration count;
- total tool calls;
- wall-clock duration;
- per-tool duration;
- provider-reported token or monetary budget when available;
- tool input and output size;
- session context size.

Cancellation propagates through provider streams and child processes. A child process that ignores graceful cancellation is terminated after a short deadline.

## Providers and authentication

The provider interface accepts normalized messages, tool schemas, model settings, and cancellation. It emits text deltas, structured tool calls, usage updates, finish reasons, and typed errors.

V1 adapters:

- OpenAI Responses API using an OpenAI Platform API key.
- A configurable OpenAI-compatible adapter for services such as OpenRouter and self-hosted compatible endpoints.
- Presets for Ollama and LM Studio that use the compatible adapter and require no credential by default.

Adapters advertise capabilities such as streaming, structured tool calls, parallel tool calls, usage reporting, and context limits. The runtime degrades explicitly or refuses an unsupported configuration instead of silently changing semantics.

Secrets are referenced by name in configuration and stored in the operating system credential store. Plaintext environment-variable credentials are supported for automation but are never written into ArcWren configuration or traces.

The authentication interface permits `api_key`, `none`, and a future `oauth` implementation. OpenAI's documented ChatGPT login is specific to Codex clients; ArcWren does not copy Codex credentials or call undocumented authentication endpoints. A supported public OAuth flow can be added later without changing the provider interface.

## Built-in tools

V1 includes a small tool set:

- `fs.list`: list entries under the active workspace.
- `fs.read`: read bounded text files.
- `fs.search`: exact and regular-expression search under the workspace.
- `fs.patch`: apply an explicit patch with a previewable diff.
- `shell.exec`: run one command with a working directory, timeout, output cap, and cancellation.
- `web.fetch`: fetch an HTTP or HTTPS resource with size, redirect, and content-type limits.
- `memory.remember`: store an explicit user-approved memory.
- `memory.forget`: remove an explicit memory.

Tool schemas are generated from typed Rust inputs and included in provider requests. Tool results distinguish successful output, rejected policy decisions, timeouts, cancellation, and execution failures.

File tools canonicalize paths and remain under the configured workspace. Symlink traversal outside the workspace is rejected. Shell commands start in the workspace and receive a filtered environment that excludes provider and channel credentials. Tool output is truncated with an explicit marker and a reference to the full local trace when retained.

`web.fetch` blocks non-HTTP schemes and credentials embedded in URLs. Remote Telegram policy additionally denies loopback and private-network destinations by default.

## Skills

Skills are instruction bundles, not executable plugins. ArcWren discovers them from project-local `.arcwren/skills/` and user-level configuration directories.

Each skill contains a `SKILL.md` file with a small metadata header: name, description, optional platform constraints, required executables, and required credential references. ArcWren loads only metadata during discovery and loads full instructions when the model or user selects the skill. Ineligible skills are visible in diagnostics with a concrete reason.

V1 does not install skills from the internet, execute skill-owned setup code, or allow a skill to bypass policy. Skills may describe how to use existing tools; the normal tool schemas and policy checks still govern every action.

## Sessions, events, and memory

SQLite is the sole durable store. It runs in WAL mode with versioned forward migrations and transactional writes.

Core tables represent:

- sessions and their selected profile, model, workspace, and frontend;
- append-only typed events with schema versions and timestamps;
- materialized messages for efficient context assembly;
- explicit memories with provenance and lifecycle state;
- pending and resolved approvals;
- Telegram pairing, update offset, and processed update identifiers;
- sanitized usage and cost observations.

The append-only event stream is the source for debugging and replay. Materialized projections may be rebuilt. Binary blobs and oversized tool output remain in a content-addressed local artifact directory referenced from events.

Memory is explicit in v1. The agent may propose a memory, but it is written only through `memory.remember`, which requires approval. ArcWren does not silently summarize all conversations into long-term memory.

## Telegram gateway

`arcwren serve` uses Telegram long polling. It does not require a public URL or inbound port. The bot token is held in the OS credential store, and the last confirmed update offset is persisted so restarts do not replay acknowledged updates.

Pairing is single-owner:

1. `arcwren pair telegram` creates a short-lived one-time code.
2. The owner sends the code to the bot in a private chat.
3. ArcWren records the Telegram user and private-chat identifiers.
4. The code expires and cannot be reused.
5. Re-pairing invalidates the previous owner.

All group, supergroup, channel, guest, and unpaired-user updates are ignored without invoking the model. V1 accepts text messages and supported callback buttons only.

Each Telegram private chat maps to a persistent ArcWren session. `/new`, `/sessions`, `/resume`, `/cancel`, `/status`, and `/help` provide minimal session control. The gateway sends an initial status message, then rate-limits message edits while output streams. Long output is split without breaking code blocks where practical.

Telegram uses a stricter remote policy:

- read-only workspace operations and bounded public web fetches may be allowed automatically;
- file patches, shell execution, memory changes, and other consequential actions require inline approval;
- approval messages show the exact operation, workspace-relative targets, risk category, and expiration;
- approvals are bound to one proposed call and cannot authorize modified arguments;
- secrets and raw credential-bearing environment values are never rendered;
- pending approvals expire and default to denial;
- repeated callbacks and Telegram updates are idempotently deduplicated.

## Policy and security

Policy evaluates a typed capability request against frontend, workspace, tool, arguments, and profile. The decision is one of `allow`, `ask`, or `deny`, with a machine-readable reason.

Default local policy allows bounded reads and asks before writes, shell execution, memory changes, or outbound actions. Default Telegram policy is stricter as described above. Users may tighten policies in configuration; loosening high-risk remote defaults requires an explicit setting and a startup warning.

ArcWren redacts known credentials before events reach storage or frontends. Diagnostics include configuration keys and credential presence, never credential values. Logs and exported traces are treated as sensitive and receive a visible warning.

Shell isolation is policy and process based in v1, not a complete security sandbox. The documentation states this plainly. Platform-specific sandbox backends may be added later behind the same execution interface.

## Errors and recovery

Errors are typed by domain: configuration, authentication, provider, rate limit, policy, validation, tool, storage, channel, timeout, and cancellation. User messages remain concise while traces preserve sanitized structured details and causal chains.

Retry behavior is conservative:

- retry transient provider and Telegram transport failures with bounded exponential backoff and jitter;
- honor server-provided retry delays;
- do not retry authentication, validation, or policy failures;
- do not implicitly retry tool calls;
- persist Telegram update progress only after the update is safely handled;
- open a circuit after repeated provider or channel failures and surface it through `doctor` and `/status`.

Storage write failures stop the turn before additional consequential actions. Corrupt or incompatible databases fail closed with recovery instructions rather than attempting destructive repair.

## Testing and evaluation

The core is designed around injected clocks, identifiers, provider streams, tool executors, and storage so tests are deterministic.

Required test layers:

- unit tests for state transitions, budgets, schemas, policies, path boundaries, redaction, and error classification;
- provider contract tests using recorded sanitized fixtures;
- storage migration and crash-recovery tests against temporary databases;
- Telegram contract tests with a local fake Bot API transport;
- integration tests for provider-to-tool-to-final-response loops;
- end-to-end tests that drive the actual binary with a scripted provider and no live LLM;
- opt-in live smoke tests excluded from normal CI.

Scenario fixtures describe input events, scripted provider responses, expected tool proposals, policy decisions, and expected event traces. These fixtures form an executable harness evaluation suite and make behavioral regressions reviewable.

CI must run formatting, Clippy with warnings denied, tests, documentation tests, dependency auditing, and license/policy checks. Core validation runs on Linux, macOS, and Windows. GitHub Actions dependencies are pinned to immutable commit SHAs.

## Repository and release configuration

The repository will include:

- a concise README with the thesis, architecture, security model, demo, and quick start;
- `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md`, `SECURITY.md`, and `CHANGELOG.md`;
- issue forms, a pull-request template, and ownership metadata;
- Dependabot configuration for Cargo and GitHub Actions;
- CI, security-audit, and release workflows;
- Rust formatting, lint, minimum-supported-version, and dependency-policy configuration;
- architecture decision records for consequential harness choices.

Repository metadata will use a specific description and relevant Rust, agent, TUI, Telegram, local-first, and AI topics. Issues remain enabled; the unused wiki and projects features are disabled initially. Discussions may be enabled when there is a user community to support.

Only squash merging remains enabled, merged branches are deleted automatically, and a `main` ruleset requires pull requests and passing CI after the bootstrap commit lands. Actions permissions are narrowed, workflow tokens default to read-only, and third-party actions are SHA-pinned.

Tagged releases produce checksummed binaries for supported targets and a source archive. Release automation is reproducible and does not require committing generated binaries. `cargo install` remains a supported installation path.

## V1 acceptance criteria

V1 is complete only when all of the following are verified:

- A clean checkout builds and tests on supported CI platforms.
- A user can complete first-run setup with either OpenAI API access or an unauthenticated local compatible endpoint.
- The TUI can stream a response, request approval, execute a tool, cancel a turn, resume a session, and inspect its trace.
- Sessions, events, explicit memories, and interrupted-turn state survive restart.
- File tools cannot escape the configured workspace in boundary and symlink tests.
- Shell tools enforce timeout, cancellation, output, working-directory, and environment rules.
- A Telegram owner can pair, start or resume a session, receive streamed output, approve or deny a proposed action, and cancel a turn.
- Unpaired users and non-private Telegram chats cannot invoke the model or tools.
- Duplicate Telegram updates and callbacks do not duplicate turns or actions.
- Provider, storage, and Telegram failure scenarios produce typed, actionable errors.
- The deterministic scenario suite exercises successful, denied, cancelled, budget-exhausted, retryable, and interrupted turns.
- README installation and demo instructions are verified from a clean environment.
- Repository rules, CI, dependency automation, security policy, and release workflow are active and validated.

## Deferred evolution

Likely post-v1 additions include a stable subprocess/MCP extension protocol, a supervised daemon and thin clients, Telegram webhooks and media, additional messaging frontends, platform sandbox backends, more provider-native adapters, supported provider OAuth flows, and opt-in derived memory. These additions must preserve the frontend/runtime, provider, tool, policy, and event boundaries established in v1.
