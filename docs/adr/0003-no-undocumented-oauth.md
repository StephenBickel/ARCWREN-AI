# ADR 0003: No undocumented OAuth or credential reuse

- Status: Accepted
- Date: 2026-07-13

## Context

OpenAI's documented ChatGPT login is specific to supported clients such as Codex. Reusing another application's stored credentials or calling private authentication endpoints would create unstable behavior, unclear user consent, and a credential-handling risk.

## Decision

The planned OpenAI adapter will use a user-supplied OpenAI Platform API key. ArcWren will not read or copy Codex or ChatGPT credential stores and will not call undocumented OAuth endpoints. OpenAI-compatible local endpoints may use no credential. A future OAuth adapter is acceptable only when it uses a documented public flow intended for third-party clients.

Configuration will hold credential references rather than secret values. Secret values will be stored in the operating system credential store, with environment variables supported for automation but never copied into configuration or traces.

No production authentication or credential-store implementation exists yet.

## Consequences

- Setup requires an appropriate API key for OpenAI Platform access.
- ArcWren cannot promise a consumer ChatGPT subscription as provider authentication.
- Credential provenance and consent remain explicit.
- A supported OAuth flow can be added behind the existing authentication abstraction without changing provider-neutral runtime contracts.
