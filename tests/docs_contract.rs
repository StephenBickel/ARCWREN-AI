use std::{collections::BTreeSet, fs, path::PathBuf};

use carl::cli::Cli;
use clap::{CommandFactory, Parser, error::ErrorKind};

const PUBLIC_DOCS: &[&str] = &[
    "README.md",
    "CONTRIBUTING.md",
    "CODE_OF_CONDUCT.md",
    "SECURITY.md",
    "CHANGELOG.md",
    "docs/architecture.md",
    "docs/security.md",
    "docs/configuration.md",
    "docs/telegram.md",
    "docs/adr/0001-event-sourced-runtime.md",
    "docs/adr/0002-single-process-v1.md",
    "docs/adr/0003-no-undocumented-oauth.md",
];

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read_readme() -> String {
    let path = repository_root().join("README.md");
    fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()))
}

#[test]
fn public_project_documents_exist() {
    for relative_path in PUBLIC_DOCS {
        let path = repository_root().join(relative_path);
        assert!(
            path.is_file(),
            "required public document is missing: {}",
            path.display()
        );
    }
}

#[test]
fn readme_local_links_resolve_to_files() {
    let readme = read_readme();
    let mut local_link_count = 0;

    for raw_target in markdown_link_targets(&readme) {
        let target = raw_target.trim().trim_matches(['<', '>']);
        if target.is_empty()
            || target.starts_with('#')
            || target.starts_with("http://")
            || target.starts_with("https://")
            || target.starts_with("mailto:")
        {
            continue;
        }

        local_link_count += 1;
        let path_without_anchor = target
            .split_once('#')
            .map_or(target, |(path, _anchor)| path);
        let path = repository_root().join(path_without_anchor);
        assert!(
            path.is_file(),
            "README local link does not resolve: {target} ({})",
            path.display()
        );
    }

    assert!(
        local_link_count > 0,
        "README must link to at least one local project document"
    );
}

#[test]
fn fenced_arcwren_commands_match_the_clap_command_tree() {
    let readme = read_readme();
    validate_fenced_arcwren_commands(&readme).unwrap_or_else(|error| panic!("{error}"));
}

#[test]
fn fenced_arcwren_command_checker_rejects_unknown_option_only_invocations() {
    let markdown = "```sh\narcwren --bogus\n```";

    let error = validate_fenced_arcwren_commands(markdown)
        .expect_err("the docs checker must reject an unknown root option");

    assert!(error.contains("--bogus"));
}

fn validate_fenced_arcwren_commands(markdown: &str) -> Result<(), String> {
    let mut command = Cli::command();
    let clap_commands: BTreeSet<_> = command
        .get_subcommands()
        .map(|subcommand| subcommand.get_name().to_owned())
        .collect();
    let help = command.render_long_help().to_string();
    let documented_commands = fenced_arcwren_commands(markdown);

    if documented_commands.is_empty() {
        return Err("README must include at least one fenced arcwren command".to_owned());
    }

    for documented in documented_commands {
        let arguments: Vec<_> = documented.split_whitespace().collect();
        if arguments.first() != Some(&"arcwren") {
            return Err(format!(
                "fenced command does not begin with `arcwren`: `{documented}`"
            ));
        }

        match Cli::try_parse_from(arguments.iter().copied()) {
            Ok(_) => {}
            Err(error) if error.kind() == ErrorKind::DisplayHelp => {}
            Err(error) => {
                return Err(format!(
                    "README documents invalid arcwren invocation `{documented}`: {error}"
                ));
            }
        }

        let documented_top_level = arguments
            .iter()
            .skip(1)
            .find_map(|argument| clap_commands.get(*argument));
        if let Some(name) = documented_top_level
            && !help
                .lines()
                .any(|line| line.split_whitespace().next() == Some(name.as_str()))
        {
            return Err(format!(
                "arcwren --help does not expose documented command `{name}`"
            ));
        }
    }

    Ok(())
}

#[test]
fn readme_states_the_current_status_and_security_boundaries() {
    let normalized = read_readme()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();

    for required_statement in [
        "pre-alpha",
        "foundation",
        "not yet a usable end-user agent",
        "openai platform api key",
        "will not reuse codex or chatgpt credentials",
        "undocumented oauth",
        "not a complete security sandbox",
        "http/openai adapters",
        "runtime tool loop",
        "tui interaction",
        "telegram gateway",
        "only the five placeholder commands",
        "`serve`, `auth`, `pair`, `doctor`, and `sessions` return not-implemented errors",
        "clap's built-in `help` command displays help",
    ] {
        assert!(
            normalized.contains(required_statement),
            "README is missing critical statement fragment: {required_statement:?}"
        );
    }
}

fn markdown_link_targets(markdown: &str) -> Vec<&str> {
    markdown
        .match_indices("](")
        .filter_map(|(index, _)| {
            let remainder = &markdown[index + 2..];
            let end = remainder.find(')')?;
            Some(&remainder[..end])
        })
        .collect()
}

fn fenced_arcwren_commands(markdown: &str) -> Vec<&str> {
    let mut in_fence = false;
    let mut commands = Vec::new();

    for line in markdown.lines() {
        let line = line.trim();
        if line.starts_with("```") || line.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if !in_fence {
            continue;
        }

        let line = line.strip_prefix("$ ").unwrap_or(line);
        if line == "arcwren" || line.starts_with("arcwren ") {
            commands.push(line);
        }
    }

    commands
}
