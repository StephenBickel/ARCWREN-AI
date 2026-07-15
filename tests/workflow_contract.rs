use std::{collections::BTreeMap, fs, path::PathBuf};

use serde_yaml_ng::{Mapping, Value};

const CHECKOUT_ACTION: &str = "actions/checkout@11bd71901bbe5b1630ceea73d27597364c9af683";
const CHECKOUT_TAG: &str = "v4.2.2";

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read_workflow(name: &str) -> String {
    let path = repository_root().join(".github/workflows").join(name);
    fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()))
}

fn parse_workflow(workflow: &str) -> Result<Value, String> {
    serde_yaml_ng::from_str(workflow)
        .map_err(|error| format!("workflow must be valid YAML: {error}"))
}

fn value_map<'a>(value: &'a Value, context: &str) -> Result<&'a Mapping, String> {
    value
        .as_mapping()
        .ok_or_else(|| format!("{context} must be a mapping"))
}

fn field<'a>(mapping: &'a Mapping, name: &str, context: &str) -> Result<&'a Value, String> {
    mapping
        .get(Value::String(name.to_owned()))
        .ok_or_else(|| format!("{context} is missing `{name}`"))
}

fn string_field<'a>(mapping: &'a Mapping, name: &str, context: &str) -> Result<&'a str, String> {
    field(mapping, name, context)?
        .as_str()
        .ok_or_else(|| format!("{context}.{name} must be a string"))
}

fn job<'a>(jobs: &'a Mapping, name: &str) -> Result<&'a Mapping, String> {
    value_map(field(jobs, name, "jobs")?, &format!("jobs.{name}"))
}

fn steps<'a>(job: &'a Mapping, context: &str) -> Result<Vec<&'a Mapping>, String> {
    let steps = field(job, "steps", context)?
        .as_sequence()
        .ok_or_else(|| format!("{context}.steps must be a sequence"))?;

    steps
        .iter()
        .enumerate()
        .map(|(index, step)| value_map(step, &format!("{context}.steps[{index}]")))
        .collect()
}

fn run_commands<'a>(job: &'a Mapping, context: &str) -> Result<Vec<&'a str>, String> {
    steps(job, context)?
        .into_iter()
        .filter_map(|step| step.get(Value::String("run".to_owned())))
        .map(|run| {
            run.as_str()
                .map(str::trim)
                .ok_or_else(|| format!("{context} step `run` must be a string"))
        })
        .collect()
}

fn validate_common(root: &Mapping, workflow: &str) -> Result<(), String> {
    let permissions = value_map(
        field(root, "permissions", "workflow")?,
        "workflow.permissions",
    )?;
    if string_field(permissions, "contents", "workflow.permissions")? != "read" {
        return Err("workflow.permissions.contents must equal `read`".to_owned());
    }
    for (permission, access) in permissions {
        if access.as_str() == Some("write") {
            return Err(format!(
                "top-level permission {permission:?} must not grant write access"
            ));
        }
    }

    let concurrency = value_map(
        field(root, "concurrency", "workflow")?,
        "workflow.concurrency",
    )?;
    if field(concurrency, "cancel-in-progress", "workflow.concurrency")?.as_bool() != Some(true) {
        return Err("workflow.concurrency.cancel-in-progress must be true".to_owned());
    }

    let jobs = value_map(field(root, "jobs", "workflow")?, "workflow.jobs")?;
    if jobs.is_empty() {
        return Err("workflow.jobs must not be empty".to_owned());
    }

    let mut discovered_actions = BTreeMap::<String, usize>::new();
    for (job_name, job_value) in jobs {
        let job_name = job_name
            .as_str()
            .ok_or_else(|| "job names must be strings".to_owned())?;
        let context = format!("jobs.{job_name}");
        let job = value_map(job_value, &context)?;
        if let Some(permissions) = job.get(Value::String("permissions".to_owned())) {
            let permissions = value_map(permissions, &format!("{context}.permissions"))?;
            for (permission, access) in permissions {
                if access.as_str() == Some("write") {
                    let permission = permission.as_str().unwrap_or("<non-string-permission>");
                    return Err(format!(
                        "{context}.permissions.{permission} must not grant write access"
                    ));
                }
            }
        }
        let timeout = field(job, "timeout-minutes", &context)?
            .as_u64()
            .ok_or_else(|| format!("{context}.timeout-minutes must be a positive integer"))?;
        if timeout == 0 {
            return Err(format!(
                "{context}.timeout-minutes must be a positive integer"
            ));
        }

        for step in steps(job, &context)? {
            if let Some(action) = step.get(Value::String("uses".to_owned())) {
                let action = action
                    .as_str()
                    .ok_or_else(|| format!("{context} step `uses` must be a string"))?
                    .trim();
                validate_action_pin(action)?;
                if action != CHECKOUT_ACTION {
                    return Err(format!("unapproved workflow action: {action}"));
                }
                *discovered_actions.entry(action.to_owned()).or_default() += 1;
            }
        }
    }

    if discovered_actions.is_empty() {
        return Err("workflow must contain at least one structurally valid action step".to_owned());
    }
    for (action, occurrence_count) in discovered_actions {
        let tagged_line_count = workflow
            .lines()
            .filter(|line| action_line_has_expected_tag_comment(line, &action))
            .count();
        if tagged_line_count != occurrence_count {
            return Err("checkout action uses must have the exact `# v4.2.2` comment".to_owned());
        }
    }

    if value_contains_secrets_context(&Value::Mapping(root.clone())) {
        return Err("workflow values must not reference secrets context".to_owned());
    }

    Ok(())
}

fn validate_action_pin(action: &str) -> Result<(), String> {
    let (repository, revision) = action
        .split_once('@')
        .ok_or_else(|| format!("action must use owner/repo@SHA syntax: {action}"))?;
    let mut repository_parts = repository.split('/');
    let owner = repository_parts.next().unwrap_or_default();
    let name = repository_parts.next().unwrap_or_default();
    let is_repository = !owner.is_empty() && !name.is_empty() && repository_parts.next().is_none();
    let is_lower_hex_sha = revision.len() == 40
        && revision
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));

    if !is_repository || !is_lower_hex_sha {
        return Err(format!(
            "action must use owner/repo@ plus exactly 40 lowercase hexadecimal characters: {action}"
        ));
    }

    Ok(())
}

fn action_line_has_expected_tag_comment(line: &str, action: &str) -> bool {
    let line = line.trim_start();
    let Some(after_uses) = line
        .strip_prefix("- uses:")
        .or_else(|| line.strip_prefix("uses:"))
    else {
        return false;
    };
    let Some((raw_action, comment)) = after_uses.split_once('#') else {
        return false;
    };
    raw_action.trim() == action && comment.trim() == CHECKOUT_TAG
}

fn value_contains_secrets_context(value: &Value) -> bool {
    match value {
        Value::String(text) => {
            let compact: String = text
                .chars()
                .filter(|character| !character.is_whitespace())
                .collect();
            compact.contains("secrets.") || compact.contains("secrets[")
        }
        Value::Sequence(values) => values.iter().any(value_contains_secrets_context),
        Value::Mapping(mapping) => mapping.iter().any(|(key, value)| {
            value_contains_secrets_context(key) || value_contains_secrets_context(value)
        }),
        _ => false,
    }
}

fn triggers(root: &Mapping) -> Result<&Mapping, String> {
    value_map(field(root, "on", "workflow")?, "workflow.on")
}

fn require_trigger(triggers: &Mapping, name: &str) -> Result<(), String> {
    if triggers.contains_key(Value::String(name.to_owned())) {
        Ok(())
    } else {
        Err(format!("workflow.on is missing `{name}`"))
    }
}

fn require_commands(
    job: &Mapping,
    context: &str,
    expected_commands: &[&str],
) -> Result<(), String> {
    let steps = steps(job, context)?;
    for expected in expected_commands {
        let matching_steps: Vec<_> = steps
            .iter()
            .copied()
            .filter(|step| {
                step.get(Value::String("run".to_owned()))
                    .and_then(Value::as_str)
                    .map(str::trim)
                    == Some(*expected)
            })
            .collect();
        if matching_steps.is_empty() {
            return Err(format!(
                "{context} is missing exact step run command `{expected}`"
            ));
        }
        for step in matching_steps {
            validate_required_gating(step, &format!("{context} required command `{expected}`"))?;
        }
    }
    Ok(())
}

fn validate_required_gating(mapping: &Mapping, context: &str) -> Result<(), String> {
    if mapping.contains_key(Value::String("if".to_owned())) {
        return Err(format!("{context} must not define `if`"));
    }
    if let Some(continue_on_error) = mapping.get(Value::String("continue-on-error".to_owned()))
        && continue_on_error.as_bool() != Some(false)
    {
        return Err(format!("{context} must not set `continue-on-error: true`"));
    }
    Ok(())
}

fn validate_ci_workflow(workflow: &str) -> Result<(), String> {
    let document = parse_workflow(workflow)?;
    let root = value_map(&document, "workflow")?;
    validate_common(root, workflow)?;

    let triggers = triggers(root)?;
    let push = value_map(field(triggers, "push", "workflow.on")?, "workflow.on.push")?;
    let push_branches = field(push, "branches", "workflow.on.push")?
        .as_sequence()
        .ok_or_else(|| "workflow.on.push.branches must be a sequence".to_owned())?;
    if !push_branches
        .iter()
        .any(|branch| branch.as_str() == Some("main"))
    {
        return Err("workflow.on.push.branches must include `main`".to_owned());
    }
    require_trigger(triggers, "pull_request")?;

    let jobs = value_map(field(root, "jobs", "workflow")?, "workflow.jobs")?;
    let quality = job(jobs, "quality")?;
    validate_required_gating(quality, "jobs.quality")?;
    if string_field(quality, "name", "jobs.quality")? != "Quality" {
        return Err("jobs.quality.name must equal `Quality`".to_owned());
    }
    if string_field(quality, "runs-on", "jobs.quality")? != "ubuntu-latest" {
        return Err("jobs.quality.runs-on must equal `ubuntu-latest`".to_owned());
    }
    require_commands(
        quality,
        "jobs.quality",
        &[
            "cargo fmt --check",
            "cargo clippy --all-targets --all-features -- -D warnings",
            "cargo test --doc",
            "cargo deny check",
        ],
    )?;

    let test = job(jobs, "test")?;
    validate_required_gating(test, "jobs.test")?;
    if string_field(test, "name", "jobs.test")? != "Test (${{ matrix.os }})" {
        return Err("jobs.test.name must retain the stable matrix check name".to_owned());
    }
    if string_field(test, "runs-on", "jobs.test")? != "${{ matrix.os }}" {
        return Err("jobs.test.runs-on must use matrix.os".to_owned());
    }
    let strategy = value_map(field(test, "strategy", "jobs.test")?, "jobs.test.strategy")?;
    let matrix = value_map(
        field(strategy, "matrix", "jobs.test.strategy")?,
        "jobs.test.strategy.matrix",
    )?;
    if let Some(exclude) = matrix.get(Value::String("exclude".to_owned())) {
        let exclude = exclude.as_sequence().ok_or_else(|| {
            "jobs.test.strategy.matrix.exclude must be absent or empty".to_owned()
        })?;
        if !exclude.is_empty() {
            return Err("jobs.test.strategy.matrix.exclude must be absent or empty".to_owned());
        }
    }
    let operating_systems = field(matrix, "os", "jobs.test.strategy.matrix")?
        .as_sequence()
        .ok_or_else(|| "jobs.test.strategy.matrix.os must be a sequence".to_owned())?;
    for expected in ["ubuntu-latest", "macos-latest", "windows-latest"] {
        if !operating_systems
            .iter()
            .any(|value| value.as_str() == Some(expected))
        {
            return Err(format!(
                "jobs.test.strategy.matrix.os is missing `{expected}`"
            ));
        }
    }
    require_commands(test, "jobs.test", &["cargo test --all-features"])?;

    let all_feature_test_count = jobs
        .iter()
        .map(|(name, value)| {
            let name = name.as_str().unwrap_or("<non-string-job>");
            let job = value_map(value, &format!("jobs.{name}"))?;
            Ok(run_commands(job, &format!("jobs.{name}"))?
                .into_iter()
                .filter(|command| *command == "cargo test --all-features")
                .count())
        })
        .collect::<Result<Vec<usize>, String>>()?
        .into_iter()
        .sum::<usize>();
    if all_feature_test_count != 1 {
        return Err(format!(
            "CI must run `cargo test --all-features` exactly once per matrix expansion, found {all_feature_test_count} step definitions"
        ));
    }

    Ok(())
}

fn validate_security_workflow(workflow: &str) -> Result<(), String> {
    let document = parse_workflow(workflow)?;
    let root = value_map(&document, "workflow")?;
    validate_common(root, workflow)?;

    let triggers = triggers(root)?;
    let schedule = field(triggers, "schedule", "workflow.on")?
        .as_sequence()
        .ok_or_else(|| {
            "workflow.on.schedule must contain a valid five-field cron entry".to_owned()
        })?;
    let has_valid_cron = schedule.iter().any(|entry| {
        entry
            .as_mapping()
            .and_then(|entry| entry.get(Value::String("cron".to_owned())))
            .and_then(Value::as_str)
            .is_some_and(|cron| cron.split_whitespace().count() == 5)
    });
    if !has_valid_cron {
        return Err("workflow.on.schedule must contain a valid five-field cron entry".to_owned());
    }
    require_trigger(triggers, "workflow_dispatch")?;

    let jobs = value_map(field(root, "jobs", "workflow")?, "workflow.jobs")?;
    let security = job(jobs, "security")?;
    validate_required_gating(security, "jobs.security")?;
    if string_field(security, "runs-on", "jobs.security")? != "ubuntu-latest" {
        return Err("jobs.security.runs-on must equal `ubuntu-latest`".to_owned());
    }
    require_commands(
        security,
        "jobs.security",
        &["cargo audit", "cargo deny check"],
    )
}

fn assert_ci_workflow(workflow: &str) {
    validate_ci_workflow(workflow).unwrap_or_else(|error| panic!("{error}"));
}

fn replace_in_workflow(name: &str, from: &str, to: &str) -> String {
    let workflow = read_workflow(name);
    assert!(
        workflow.contains(from),
        "test fixture source was not found in {name}: {from:?}"
    );
    workflow.replacen(from, to, 1)
}

fn assert_ci_rejected(workflow: &str, expected_error: &str) {
    let error = validate_ci_workflow(workflow).expect_err("CI workflow must be rejected");
    assert!(
        error.contains(expected_error),
        "unexpected CI validation error: {error}"
    );
}

fn assert_security_rejected(workflow: &str, expected_error: &str) {
    let error =
        validate_security_workflow(workflow).expect_err("security workflow must be rejected");
    assert!(
        error.contains(expected_error),
        "unexpected security validation error: {error}"
    );
}

#[test]
fn ci_workflow_enforces_required_cross_platform_checks() {
    assert_ci_workflow(&read_workflow("ci.yml"));
}

#[test]
fn checker_rejects_multiline_unpinned_uses() {
    let workflow = r#"
name: Adversarial

on:
  workflow_dispatch:

permissions:
  contents: read

concurrency:
  group: adversarial
  cancel-in-progress: true

jobs:
  test:
    runs-on: ubuntu-latest
    timeout-minutes: 5
    steps:
      - uses: actions/checkout@11bd71901bbe5b1630ceea73d27597364c9af683 # v4.2.2
      - name: Hidden unpinned action
        uses: >-
          example/action@main
"#;

    let document = parse_workflow(workflow).unwrap();
    let root = value_map(&document, "workflow").unwrap();
    let error = validate_common(root, workflow)
        .expect_err("checker must reject a multiline unpinned action");
    assert!(
        error.contains("exactly 40 lowercase hexadecimal characters"),
        "unexpected structural validation error: {error}"
    );
}

#[test]
fn checker_rejects_comment_only_requirements_and_step_timeouts() {
    let workflow = r#"
name: Adversarial

on:
  workflow_dispatch:

permissions:
  contents: read

concurrency:
  group: adversarial
  cancel-in-progress: true

jobs:
  decoy:
    steps:
      - uses: actions/checkout@11bd71901bbe5b1630ceea73d27597364c9af683 # v4.2.2
      - name: First decoy
        timeout-minutes: 1
        run: echo first
      - name: Second decoy
        timeout-minutes: 1
        run: echo second

# pull_request:
# push:
# name: Quality
# name: Test (${{ matrix.os }})
# ubuntu-latest
# macos-latest
# windows-latest
# cargo fmt --check
# cargo clippy --all-targets --all-features -- -D warnings
# cargo test --all-features
# cargo test --doc
# cargo deny check
"#;

    let error = validate_ci_workflow(workflow)
        .expect_err("checker must reject comment-only requirements and step-level timeouts");
    assert!(
        error.contains("jobs.decoy is missing `timeout-minutes`"),
        "unexpected structural validation error: {error}"
    );
}

#[test]
fn checker_rejects_if_on_a_required_job() {
    let workflow = replace_in_workflow(
        "ci.yml",
        "  quality:\n    name: Quality\n",
        "  quality:\n    name: Quality\n    if: ${{ false }}\n",
    );

    assert_ci_rejected(&workflow, "jobs.quality must not define `if`");
}

#[test]
fn checker_rejects_continue_on_error_on_a_required_job() {
    let workflow = replace_in_workflow(
        "ci.yml",
        "  quality:\n    name: Quality\n",
        "  quality:\n    name: Quality\n    continue-on-error: true\n",
    );

    assert_ci_rejected(
        &workflow,
        "jobs.quality must not set `continue-on-error: true`",
    );
}

#[test]
fn checker_rejects_if_on_a_required_command_step() {
    let workflow = replace_in_workflow(
        "ci.yml",
        "      - name: Check formatting\n        run: cargo fmt --check\n",
        "      - name: Check formatting\n        if: ${{ false }}\n        run: cargo fmt --check\n",
    );

    assert_ci_rejected(
        &workflow,
        "required command `cargo fmt --check` must not define `if`",
    );
}

#[test]
fn checker_rejects_continue_on_error_on_a_required_command_step() {
    let workflow = replace_in_workflow(
        "ci.yml",
        "      - name: Check formatting\n        run: cargo fmt --check\n",
        "      - name: Check formatting\n        continue-on-error: true\n        run: cargo fmt --check\n",
    );

    assert_ci_rejected(
        &workflow,
        "required command `cargo fmt --check` must not set `continue-on-error: true`",
    );
}

#[test]
fn checker_rejects_matrix_exclusions_that_remove_required_operating_systems() {
    let workflow = replace_in_workflow(
        "ci.yml",
        "      matrix:\n        os: [ubuntu-latest, macos-latest, windows-latest]\n",
        "      matrix:\n        os: [ubuntu-latest, macos-latest, windows-latest]\n        exclude:\n          - os: ubuntu-latest\n          - os: macos-latest\n          - os: windows-latest\n",
    );

    assert_ci_rejected(
        &workflow,
        "jobs.test.strategy.matrix.exclude must be absent or empty",
    );
}

#[test]
fn checker_rejects_job_level_write_permissions() {
    let workflow = replace_in_workflow(
        "ci.yml",
        "  quality:\n    name: Quality\n",
        "  quality:\n    name: Quality\n    permissions:\n      contents: write\n",
    );

    assert_ci_rejected(
        &workflow,
        "jobs.quality.permissions.contents must not grant write access",
    );
}

#[test]
fn checker_rejects_indexed_secret_context_with_whitespace() {
    let workflow = replace_in_workflow(
        "ci.yml",
        "      - name: Check formatting\n        run: cargo fmt --check\n",
        "      - name: Check formatting\n        env:\n          TOKEN: ${{ secrets [ 'TOKEN' ] }}\n        run: cargo fmt --check\n",
    );

    assert_ci_rejected(
        &workflow,
        "workflow values must not reference secrets context",
    );
}

#[test]
fn checker_rejects_empty_security_schedule() {
    let workflow = replace_in_workflow(
        "security.yml",
        "  schedule:\n    - cron: \"17 6 * * 1\"\n",
        "  schedule: []\n",
    );

    assert_security_rejected(
        &workflow,
        "workflow.on.schedule must contain a valid five-field cron entry",
    );
}

#[test]
fn checker_rejects_ci_push_without_main_branch() {
    let workflow = replace_in_workflow(
        "ci.yml",
        "  push:\n    branches: [main]\n",
        "  push:\n    branches: [develop]\n",
    );

    assert_ci_rejected(&workflow, "workflow.on.push.branches must include `main`");
}

#[test]
fn checker_rejects_misleading_checkout_tag_comment() {
    let workflow = replace_in_workflow("ci.yml", "# v4.2.2", "# v9.9.9");

    assert_ci_rejected(
        &workflow,
        "checkout action uses must have the exact `# v4.2.2` comment",
    );
}

#[test]
fn security_workflow_runs_scheduled_and_manual_dependency_checks() {
    let workflow = read_workflow("security.yml");
    validate_security_workflow(&workflow).unwrap_or_else(|error| panic!("{error}"));
}
