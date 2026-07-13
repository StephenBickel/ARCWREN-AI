# ArcWren Repository and Release Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:executing-plans` to implement this plan task by task. Use `superpowers:verification-before-completion` before declaring the plan complete.

**Goal:** Turn ARCWREN-AI into a credible open-source project with clear architecture and security documentation, hardened GitHub automation, reproducible cross-platform checks, checksummed releases, and enforceable repository policy.

**Architecture:** Repository configuration is versioned wherever GitHub permits. CI separates fast required checks from scheduled security work. Workflow permissions are read-only by default and every third-party action is pinned to an immutable commit SHA. GitHub-side settings are applied only after the workflows exist on the default branch.

**Tech stack:** GitHub Actions, Cargo, rustfmt, Clippy, cargo-deny, cargo-audit, release-plz or cargo-dist only if its generated workflow remains reviewable and SHA-pinned, GitHub CLI/API, Markdown.

## Global constraints

- Do not commit tokens, personal email addresses, generated binaries, or private traces.
- Do not claim platform support until CI proves it.
- Pin every action, including first-party GitHub actions, to a full commit SHA with a version comment.
- Keep workflow `permissions: contents: read` unless one job has a documented narrower elevation.
- Apply branch protection only after required check names have run on the default branch.

---

### Task 1: Write the public project surface

**Files:**

- Create: `README.md`
- Create: `CONTRIBUTING.md`
- Create: `CODE_OF_CONDUCT.md`
- Create: `SECURITY.md`
- Create: `CHANGELOG.md`
- Create: `docs/architecture.md`
- Create: `docs/security.md`
- Create: `docs/configuration.md`
- Create: `docs/telegram.md`
- Create: `docs/adr/0001-event-sourced-runtime.md`
- Create: `docs/adr/0002-single-process-v1.md`
- Create: `docs/adr/0003-no-undocumented-oauth.md`

**Step 1: Add a failing documentation smoke check**

Create a script or test that verifies all README-local links exist and every documented command appears in `arcwren --help`.

Run: `cargo test --test docs_contract`

Expected: failure because public docs do not exist.

**Step 2: Write concise, truthful documentation**

README order: one-sentence thesis, terminal/Telegram demo, status warning, features, quick start, architecture, security model, provider setup, Telegram pairing, development, roadmap, license. Explain that OpenAI uses API keys and that ArcWren will not reuse Codex/ChatGPT credentials or undocumented OAuth. Document shell isolation limits prominently.

**Step 3: Verify and commit**

Run:

```sh
cargo test --test docs_contract
cargo run -- --help
git diff --check
```

Commit: `docs: publish ArcWren project guide`

---

### Task 2: Add contribution and ownership automation

**Files:**

- Create: `.github/CODEOWNERS`
- Create: `.github/PULL_REQUEST_TEMPLATE.md`
- Create: `.github/ISSUE_TEMPLATE/bug.yml`
- Create: `.github/ISSUE_TEMPLATE/feature.yml`
- Create: `.github/ISSUE_TEMPLATE/config.yml`
- Create: `.github/dependabot.yml`
- Create: `.github/labeler.yml`
- Create: `.github/release.yml`

**Step 1: Add and run static schema checks**

Validate YAML parsing, required issue-form fields, valid Dependabot ecosystems/directories/schedules, and that CODEOWNERS names the repository owner.

**Step 2: Implement templates and dependency grouping**

Collect version, platform, provider, reproduction, sanitized logs, and security redirect. Group compatible Cargo patch/minor updates and GitHub Actions updates weekly; keep major updates separate.

**Step 3: Verify and commit**

Run: `ruby -e 'require "yaml"; Dir[".github/**/*.{yml,yaml}"].each { |f| YAML.load_file(f); puts f }'`

Commit: `chore: add community project metadata`

---

### Task 3: Add hardened required CI

**Files:**

- Create: `.github/workflows/ci.yml`
- Create: `.github/workflows/security.yml`
- Create: `deny.toml`
- Modify: `Cargo.toml`

**Step 1: Pin action SHAs from upstream releases**

Resolve immutable SHAs for checkout, Rust toolchain/cache, and artifact upload. Record the human-readable tag in a comment. Do not use moving tags in `uses:`.

**Step 2: Implement checks**

Required Linux jobs: formatting, Clippy with warnings denied, all-feature tests, docs, cargo-deny. Matrix job: tests on Linux, macOS, and Windows. Scheduled/manual security workflow: cargo-audit and dependency review where event-compatible. Add explicit concurrency cancellation and timeouts.

**Step 3: Validate locally**

Run:

```sh
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
cargo test --doc
cargo deny check
cargo audit
```

Use `actionlint` when installed or run it from a pinned release binary whose checksum is verified.

**Step 4: Commit**

Commit: `ci: add cross-platform quality and security gates`

---

### Task 4: Build reproducible release automation

**Files:**

- Create: `.github/workflows/release.yml`
- Create: `release.toml` or generated release configuration
- Create: `scripts/check-release-artifacts.sh`
- Modify: `README.md`

**Step 1: Define supported targets from proven CI**

Start with macOS Apple Silicon/Intel, Linux x86_64/aarch64 where the build strategy is reproducible, and Windows x86_64. If a target is not green, omit it rather than publishing an unverified asset.

**Step 2: Implement tag-triggered builds**

Require tags matching `v*.*.*`, compile locked release binaries, archive license/readme/binary, create SHA-256 checksums, attach source archive, and generate release notes. Elevate `contents: write` only in the publishing job. Ensure forks and pull requests cannot publish.

**Step 3: Verify without publishing**

Run local release builds for the host and execute the artifact checker. Validate workflow syntax and pinned actions. Do not create a public tag during bootstrap.

**Step 4: Commit**

Commit: `ci: add checksummed release pipeline`

---

### Task 5: Validate a clean checkout and publish the bootstrap branch

**Files:**

- Modify only files needed to fix reproducibility failures.

**Step 1: Run the complete local gate**

Run:

```sh
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
cargo test --doc
cargo deny check
cargo audit
git diff --check
git status --short
```

**Step 2: Verify from a clean clone or clean worktree**

Build and test using only tracked files and documented prerequisites. Confirm README commands. Inspect `git ls-files` for secrets and generated artifacts.

**Step 3: Push the feature branch and open a pull request**

Push `codex/arcwren-v1`, open a non-draft pull request with design, testing, security, and release sections, and wait for every required workflow to finish. Fix failures on the branch and rerun until green.

**Step 4: Merge only when green**

Squash merge the bootstrap pull request and delete the feature branch. Record the resulting default-branch commit for ruleset configuration.

---

### Task 6: Apply and verify GitHub repository settings

**External state:** `StephenBickel/ARCWREN-AI` repository settings and rulesets.

**Step 1: Configure repository metadata**

Set description to: `A minimal, local-first Rust agent harness with a replayable runtime, terminal UI, and owner-only Telegram gateway.`

Set homepage only after a real project site exists. Add topics: `rust`, `ai-agent`, `agent-harness`, `tui`, `telegram-bot`, `local-first`, `llm`, `open-source`. Keep issues enabled; disable wiki and projects. Enable automatic branch deletion. Enable squash merge only and use pull-request titles as squash commit titles.

**Step 2: Harden Actions**

Set default workflow token permissions to read-only and disable approval of pull requests by workflows. Restrict allowed actions to GitHub-owned plus the exact third-party actions already pinned in versioned workflows, or require full-SHA pinning if the repository API supports that setting.

**Step 3: Create the `main` ruleset**

Require pull requests, at least one approval, dismissal of stale approvals, conversation resolution, linear history, and the exact successful CI checks observed on the default branch. Block force pushes and deletion. Allow repository administrators to bypass only for emergencies, not always.

**Step 4: Verify settings through the API**

Read back repository metadata, Actions permissions, merge configuration, branch/ruleset state, topics, and the latest default-branch workflow conclusions. Save no credentials or raw API responses containing private data.

Expected: settings match the plan and all required checks are active and green.
