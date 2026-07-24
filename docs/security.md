# Security Model

Status: design plus partial foundation. Carl is not currently a usable agent, and the controls described as planned below are not enforcement claims.

## Trust model

The local owner and host operating system define the trust root. Model output, tool arguments, remote messages, provider responses, fetched content, repository files, and skill instructions are untrusted inputs. The SQLite database, exported traces, and logs may contain sensitive conversation or filesystem metadata even after credential redaction.

Carl does not attempt to defend a host account from an attacker who already controls that account or its operating system. It also does not promise perfect containment of arbitrary native commands.

## Implemented properties

- public error codes and user messages are separated from sanitized internal detail;
- events use stable types and schema versions;
- SQLite migrations are forward-only and checksum-verified, and the store rejects unknown future schemas;
- session events are append-only and lifecycle changes are transactional;
- the scripted provider supports deterministic tests without a network or live credentials.

These properties improve auditability and failure behavior. They do not implement provider authentication, redaction of all runtime data, filesystem confinement, approvals, or process isolation.

## Planned v1 controls

The approved design requires:

- canonical workspace-relative file access with symlink escape rejection;
- typed `allow`, `ask`, or `deny` policy decisions for every tool proposal;
- exact, expiring approvals bound to one call and one argument set;
- filtered child-process environments, deadlines, output caps, and cancellation;
- bounded HTTP(S) fetches, with private-network destinations denied remotely by default;
- known-credential redaction before events reach storage or frontends;
- a stricter Telegram policy and admission checks before model invocation;
- fail-closed behavior for incompatible storage and storage-write failures.

Every one of these controls requires implementation and adversarial tests before it can be claimed as present.

## Shell boundary

**Shell isolation in v1 is policy- and process-based; it is not a complete security sandbox.** Workspace selection, a filtered environment, timeouts, cancellation, and approvals reduce accidental harm but do not neutralize hostile programs running as the same OS user. Platform sandbox backends may be added later. Until then, never approve a command you would not run directly under the host account.

## Credentials and authentication

The target design stores secret references in configuration and values in the operating system credential store, with environment variables supported for automation. Environment credentials must not be copied into configuration, events, logs, diagnostics, fixtures, or exported traces.

OpenAI access will use an OpenAI Platform API key. Carl will not reuse Codex or ChatGPT credentials and will not call undocumented OAuth endpoints. See [ADR 0003](adr/0003-no-undocumented-oauth.md).

## Remote channel boundary

The planned Telegram gateway uses outbound long polling and one paired owner. Group, channel, guest, and unpaired updates must be discarded before provider or tool invocation. Duplicate updates and approval callbacks must be persisted and deduplicated so retries cannot duplicate consequential work. See the [Telegram design guide](telegram.md).

For vulnerability reporting, follow the private process in the repository [security policy](../SECURITY.md).
