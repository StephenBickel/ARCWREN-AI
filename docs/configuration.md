# Configuration

Configuration loading is not implemented. This page documents design constraints, not accepted file names, keys, environment variables, or working setup commands.

## Planned model

V1 will layer explicit command-line choices over a selected profile and local defaults. A profile is expected to select a provider, model, workspace root, turn budgets, policy posture, and credential reference. Exact key names and platform paths will be documented only after a validated configuration schema exists.

The planned configuration boundary separates non-secret choices from secret values:

| Setting | Intended location |
| --- | --- |
| Provider, model, endpoint, workspace, budgets | Carl configuration |
| Credential reference name | Carl configuration |
| API keys and Telegram bot tokens | OS credential store |
| Automation-only credentials | Process environment, never copied to config or traces |

## Providers

The approved v1 provider targets are:

- OpenAI Responses API authenticated with an OpenAI Platform API key;
- a configurable OpenAI-compatible endpoint for third-party or self-hosted services;
- local-compatible presets such as Ollama and LM Studio, with no credential by default.

None of these production adapters exists yet. The repository currently contains only the provider-neutral trait and deterministic scripted provider.

Carl will not reuse Codex or ChatGPT credentials or use undocumented OAuth. A future documented public OAuth flow may implement the existing authentication boundary without changing provider-neutral runtime types.

## Workspace, policy, and budgets

The future workspace root will be the default boundary for file and shell tools, not proof of OS-level containment. Profiles will be able to tighten local and remote policy. Turn settings will impose hard bounds on iterations, tool calls, duration, provider usage where reported, tool input/output size, and context size.

Remote defaults will be stricter than local defaults. Any option that weakens high-risk remote policy must be explicit and produce a startup warning.

## Current operation

There is no first-run setup, credential storage, provider connectivity check, or configuration file parser today. Use the CLI help shell only; do not place real secrets in guessed files or variables. This page will gain concrete, tested examples when the loader and `auth`/`doctor` behavior are implemented.
