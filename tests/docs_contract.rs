use std::{collections::BTreeSet, fs, path::PathBuf};

use arcwren::cli::Cli;
use clap::CommandFactory;

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
    let mut command = Cli::command();
    let clap_commands: BTreeSet<_> = command
        .get_subcommands()
        .map(|subcommand| subcommand.get_name().to_owned())
        .collect();
    let help = command.render_long_help().to_string();
    let documented_commands = fenced_arcwren_commands(&readme);

    assert!(
        !documented_commands.is_empty(),
        "README must include at least one fenced arcwren command"
    );

    for documented in documented_commands {
        let mut arguments = documented.split_whitespace();
        assert_eq!(arguments.next(), Some("arcwren"));

        let Some(name) = arguments
            .next()
            .filter(|argument| !argument.starts_with('-'))
        else {
            continue;
        };

        assert!(
            clap_commands.contains(name),
            "README documents unknown arcwren command `{name}` in `{documented}`"
        );
        assert!(
            help.contains(name),
            "arcwren --help does not expose documented command `{name}`"
        );
    }
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
