use std::{fs, path::PathBuf};

const CHECKOUT: &str = "actions/checkout@11bd71901bbe5b1630ceea73d27597364c9af683 # v4.2.2";

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read_workflow(name: &str) -> String {
    let path = repository_root().join(".github/workflows").join(name);
    fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()))
}

fn assert_hardened(workflow: &str, expected_job_count: usize) {
    assert!(
        workflow.contains("\npermissions:\n  contents: read\n"),
        "workflow must set top-level read-only contents permission"
    );
    assert!(
        !workflow.contains("write"),
        "workflow must not grant write permission"
    );
    assert!(
        workflow.contains("\nconcurrency:\n") && workflow.contains("cancel-in-progress: true"),
        "workflow must cancel superseded runs"
    );
    assert_eq!(
        workflow.matches("timeout-minutes:").count(),
        expected_job_count,
        "every job must have one explicit timeout"
    );
    assert!(
        !workflow.contains("secrets."),
        "workflow must not expose repository secrets"
    );

    let action_uses: Vec<_> = workflow
        .lines()
        .map(str::trim)
        .filter_map(|line| line.strip_prefix("- uses: "))
        .collect();
    assert!(
        !action_uses.is_empty(),
        "workflow must check out the repository"
    );
    assert!(
        action_uses.iter().all(|action| *action == CHECKOUT),
        "every action must be an approved immutable pin; found {action_uses:?}"
    );
}

#[test]
fn ci_workflow_enforces_required_cross_platform_checks() {
    let workflow = read_workflow("ci.yml");
    assert_hardened(&workflow, 2);

    for trigger in ["pull_request:", "push:"] {
        assert!(
            workflow.contains(trigger),
            "CI trigger is missing: {trigger}"
        );
    }
    for check_name in ["name: Quality", "name: Test (${{ matrix.os }})"] {
        assert!(
            workflow.contains(check_name),
            "stable required check name is missing: {check_name}"
        );
    }
    for runner in ["ubuntu-latest", "macos-latest", "windows-latest"] {
        assert!(
            workflow.contains(runner),
            "test matrix is missing runner: {runner}"
        );
    }
    for command in [
        "cargo fmt --check",
        "cargo clippy --all-targets --all-features -- -D warnings",
        "cargo test --all-features",
        "cargo test --doc",
        "cargo deny check",
    ] {
        assert!(
            workflow.contains(command),
            "CI is missing exact required command: {command}"
        );
    }
    assert_eq!(
        workflow.matches("cargo test --all-features").count(),
        1,
        "the matrix must run the all-feature suite without a duplicate Linux test step"
    );
}

#[test]
fn security_workflow_runs_scheduled_and_manual_dependency_checks() {
    let workflow = read_workflow("security.yml");
    assert_hardened(&workflow, 1);

    for trigger in ["schedule:", "workflow_dispatch:"] {
        assert!(
            workflow.contains(trigger),
            "security trigger is missing: {trigger}"
        );
    }
    for command in ["cargo audit", "cargo deny check"] {
        assert!(
            workflow.contains(command),
            "security workflow is missing exact command: {command}"
        );
    }
}
