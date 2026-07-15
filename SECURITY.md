# Security Policy

ArcWren is pre-alpha foundation software and is not ready to execute end-user agent workloads. The runtime, production providers, tools, policy engine, TUI, and Telegram gateway described by the v1 design are not implemented.

## Supported versions

No stable release is currently supported. Security fixes are applied to the default development branch. When releases begin, this section will list supported versions explicitly.

## Reporting a vulnerability

Use the repository's private **Report a vulnerability** / private vulnerability reporting flow when it is available. Include the affected revision, impact, prerequisites, a minimal reproduction, and sanitized logs. Do not include real API keys, bot tokens, private conversations, or unrelated local data.

If private vulnerability reporting is unavailable, open a public issue containing only a request for a private maintainer contact. Do not disclose exploit details in that issue. Maintainers will acknowledge reports and coordinate validation, remediation, and disclosure as capacity permits; pre-alpha status means no response-time guarantee is offered.

## Security boundaries

The durable foundation currently provides versioned event contracts, typed sanitized user-facing errors, checksum-verified SQLite migrations, and deterministic provider tests without live credentials. These properties do not create a sandbox.

The planned threat model and enforcement boundaries are documented in [docs/security.md](docs/security.md). In particular, future shell execution will be constrained by policy and process controls, not a complete security sandbox. Never use ArcWren to execute code you would not trust under the host account.

Credential values must never be committed, placed in issues, or included in fixtures. OpenAI support will use an OpenAI Platform API key; copying Codex or ChatGPT credentials or using undocumented OAuth is out of scope.
