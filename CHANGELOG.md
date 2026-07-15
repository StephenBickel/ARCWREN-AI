# Changelog

All notable changes to ArcWren will be recorded here. The project is pre-alpha and has not made a supported release.

## Unreleased

### Added

- Public project, contribution, conduct, security, architecture, configuration, and Telegram documentation.
- Architecture decision records for event-sourced execution, the single-process v1 boundary, and documented authentication only.
- A documentation contract covering required public files, local README links, CLI command names, and critical status/security statements.
- A provider-neutral event, identifier, error, and budget foundation.
- SQLite WAL persistence with append-only events and checksum-verified migrations.
- A provider interface and deterministic scripted provider for offline tests.
