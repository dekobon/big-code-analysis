use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

use crate::cli::{Action, Mode, Scope, parse_args};

fn workflow() -> String {
    fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../.github/workflows/benchmark.yml"),
    )
    .unwrap()
}

fn gate_script(yaml: &str) -> String {
    let after = yaml
        .split_once("      - name: Complexity-class gate\n")
        .unwrap()
        .1;
    let body = after.split_once("        run: |\n").unwrap().1;
    body.lines()
        .take_while(|line| line.is_empty() || line.starts_with("          "))
        .map(|line| line.strip_prefix("          ").unwrap_or(line))
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn issue_tail_lines() -> usize {
    let yaml = workflow();
    let command = yaml
        .lines()
        .find(|line| line.trim_start().starts_with("tail -n "))
        .unwrap();
    command.split_whitespace().nth(2).unwrap().parse().unwrap()
}

#[test]
fn workflow_reproduction_command_selects_the_confirmation_scope() {
    let yaml = workflow();
    let echo = yaml
        .lines()
        .find(|line| line.contains("Reproduce locally with"))
        .unwrap()
        .trim();
    let output = Command::new("bash").args(["-c", echo]).output().unwrap();
    assert!(output.status.success());
    let prose = String::from_utf8(output.stdout).unwrap();
    let command = prose.split('`').nth(1).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let stub = dir.path().join("make");
    fs::write(&stub, "#!/bin/bash\nset -eu\nprintf '%s\\n' \"$@\"\n").unwrap();
    fs::set_permissions(&stub, fs::Permissions::from_mode(0o755)).unwrap();
    let path = std::env::join_paths(
        std::iter::once(dir.path().to_path_buf())
            .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
    )
    .unwrap();
    let invocation = Command::new("bash")
        .args(["-c", command])
        .env("PATH", path)
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(invocation.status.success());
    let arguments = String::from_utf8(invocation.stdout).unwrap();
    let mut lines = arguments.lines();
    assert_eq!(lines.next(), Some("bench-scaling"));
    let flags = lines.next().unwrap().strip_prefix("BENCH_ARGS=").unwrap();
    assert_eq!(lines.next(), None);
    let Action::Run(args) = parse_args(
        std::iter::once("--bench")
            .chain(flags.split_whitespace())
            .map(str::to_owned),
    )
    .unwrap() else {
        panic!("run")
    };
    assert_eq!(args.scope, Scope::Legacy);
    assert_eq!(args.rounds, 21);
}

#[test]
fn inherited_workflow_retries_only_legacy_and_preserves_harness_errors() {
    let script = gate_script(&workflow());
    for (exits, expected_exit, expected_rounds, confirmation) in [
        ("0", 0, vec![11], false),
        ("1 0", 0, vec![11, 21], true),
        ("1 1", 1, vec![11, 21], true),
        ("2", 2, vec![11], false),
        ("1 2", 2, vec![11, 21], false),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let stub = dir.path().join("cargo");
        fs::write(&stub, "#!/bin/bash\nset -eu\nprintf '%s\\n' \"$*\" >> \"$CALLS\"\nread -r -a codes <<< \"$CODES\"\nn=$(wc -l < \"$CALLS\")\necho simulated-scaling\nexit \"${codes[n-1]}\"\n").unwrap();
        fs::set_permissions(&stub, fs::Permissions::from_mode(0o755)).unwrap();
        let calls = dir.path().join("calls");
        let path = std::env::join_paths(
            std::iter::once(dir.path().to_path_buf())
                .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
        )
        .unwrap();
        let output = Command::new("bash")
            .args(["-c", &script])
            .current_dir(dir.path())
            .env("PATH", path)
            .env("CALLS", &calls)
            .env("CODES", exits)
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(expected_exit),
            "{exits}: {} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let calls = fs::read_to_string(calls).unwrap();
        let parsed: Vec<_> = calls
            .lines()
            .map(|call| {
                let argv: Vec<_> = call.split_whitespace().collect();
                let separator = argv.iter().position(|arg| *arg == "--").unwrap();
                assert_eq!(
                    &argv[..separator],
                    [
                        "bench",
                        "-p",
                        "big-code-analysis-bench",
                        "--bench",
                        "scaling"
                    ]
                );
                let args = std::iter::once("--bench".to_owned())
                    .chain(argv[separator + 1..].iter().map(|s| (*s).to_owned()));
                let Action::Run(args) = parse_args(args).unwrap() else {
                    panic!("run")
                };
                assert_eq!(args.scope, Scope::Legacy);
                assert_eq!(args.mode, Mode::Gate);
                args.rounds
            })
            .collect();
        assert_eq!(parsed, expected_rounds);
        assert_eq!(
            dir.path().join("target/bench/scaling-confirm.log").exists(),
            confirmation
        );
    }
}
