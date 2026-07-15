use std::{collections::BTreeMap, fs, path::PathBuf};

use serde_yaml_ng::{Mapping, Value};

const CHECKOUT_ACTION: &str = "actions/checkout@11bd71901bbe5b1630ceea73d27597364c9af683";

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
            .filter(|line| action_line_has_tag_comment(line, &action))
            .count();
        if tagged_line_count != occurrence_count {
            return Err(format!(
                "every `{action}` use must have a human tag comment; found {tagged_line_count} comments for {occurrence_count} uses"
            ));
        }
    }

    if value_contains_text(&Value::Mapping(root.clone()), "secrets.") {
        return Err("workflow values must not reference repository secrets".to_owned());
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

fn action_line_has_tag_comment(line: &str, action: &str) -> bool {
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
    let tag = comment.trim();
    raw_action.trim() == action
        && tag
            .strip_prefix('v')
            .and_then(|version| version.chars().next())
            .is_some_and(|character| character.is_ascii_digit())
}

fn value_contains_text(value: &Value, needle: &str) -> bool {
    match value {
        Value::String(text) => text.contains(needle),
        Value::Sequence(values) => values
            .iter()
            .any(|value| value_contains_text(value, needle)),
        Value::Mapping(mapping) => mapping.iter().any(|(key, value)| {
            value_contains_text(key, needle) || value_contains_text(value, needle)
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
    let commands = run_commands(job, context)?;
    for expected in expected_commands {
        if !commands.contains(expected) {
            return Err(format!(
                "{context} is missing exact step run command `{expected}`"
            ));
        }
    }
    Ok(())
}

fn validate_ci_workflow(workflow: &str) -> Result<(), String> {
    let document = parse_workflow(workflow)?;
    let root = value_map(&document, "workflow")?;
    validate_common(root, workflow)?;

    let triggers = triggers(root)?;
    require_trigger(triggers, "push")?;
    require_trigger(triggers, "pull_request")?;

    let jobs = value_map(field(root, "jobs", "workflow")?, "workflow.jobs")?;
    let quality = job(jobs, "quality")?;
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
    require_trigger(triggers, "schedule")?;
    require_trigger(triggers, "workflow_dispatch")?;

    let jobs = value_map(field(root, "jobs", "workflow")?, "workflow.jobs")?;
    let security = job(jobs, "security")?;
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
fn security_workflow_runs_scheduled_and_manual_dependency_checks() {
    let workflow = read_workflow("security.yml");
    validate_security_workflow(&workflow).unwrap_or_else(|error| panic!("{error}"));
}
