//! The launcher is a thin layer over Isothrum (docs/contributing/boundaries.md): it runs no
//! container engine, Vagrant, Terraform, hypervisor or cloud CLI itself. A file of shipped code
//! that names one of those tools as the program it starts (the first argument of a call, the
//! head of an argv list, or a `&str` constant) is an offender, unless [`ALLOWED`] lists it.
//!
//! [`ALLOWED`] is today's offenders, each with the step of the migration to Isothrum that removes
//! it. It only shrinks: a file not on it fails the build, and so does an entry whose file no
//! longer offends (take it off).
//!
//! Every process the launcher starts goes through Isothrum's runner (`platform::processes`,
//! `platform::exec`): a file of shipped code that starts one any other way (`Command::new`, a
//! pseudo-terminal's `spawn_command`, `ShellExecuteExW`, `CreateProcessW`) is an offender too,
//! unless [`OWN_WINDOWS`] lists it.

/// The tools the launcher leaves to Isothrum and `isothrum-machine`.
const TOOLS: &str = r"docker|vagrant|terraform|tofu|VBoxManage|vboxmanage|prlctl|vmrun|utmctl|virsh|qemu-[a-z0-9_.-]+|aws|az|gcloud|doctl|linode-cli|oci";

/// The files allowed to start one of [`TOOLS`] until the migration step named removes them.
const ALLOWED: &[(&str, &str)] = &[];

/// The files that start a process outside the runner, each with why: a window of the player's own
/// that must outlive the call that opens it and the app (the runner ends a tool with its tree),
/// or a start the runner can't make. It only shrinks, as [`ALLOWED`] does.
const OWN_WINDOWS: &[(&str, &str)] = &[
    ("screen.rs", "the Remote Desktop client, the player's own window, given the password on its input"),
    ("screen/native.rs", "the hypervisor's console app, the player's own window, left running"),
    ("runtime/terminal_launch.rs", "the player's terminal window; moves to isothrum-machine's terminal"),
    ("terminal.rs", "the in-app terminal's shell, on a pseudo-terminal"),
    ("platform/proxy.rs", "macOS's proxy settings (scutil), read once before the async runtime runs"),
    ("platform/sys/windows/elevation.rs", "the one UAC prompt that joins Hyper-V Administrators (ShellExecuteExW runas)"),
];

/// A process started in `code` other than through the runner, 1-based lines; comments aside.
fn spawns(code: &str) -> Vec<usize> {
    let start = regex::Regex::new(r"\bCommand::new\(|\bspawn_command\(|\bShellExecuteExW\(|\bCreateProcessW\(").unwrap();
    let code: String = code.lines().map(|l| if l.trim_start().starts_with("//") { "" } else { l }).flat_map(|l| [l, "\n"]).collect();
    let mut lines: Vec<usize> = start.find_iter(&code).map(|m| code[..m.end()].lines().count()).collect();
    lines.dedup();
    lines
}

/// A tool started in `code`: the first argument of a call that starts or finds a program, the
/// head of a (program, args) pair, or a `&str` constant.
fn started() -> regex::Regex {
    let callee = "Command::new|run|run_read|run_read_env|run_cli|run_env|run_env_timed|stream|background|launch_detached|as_root|probe|sign_in|resolve";
    let tool = format!(r#""(?:{TOOLS})(?:\.exe)?""#);
    regex::Regex::new(&format!(r"\b(?:{callee})\(\s*{tool}\s*[,)]|[(\[]\s*{tool}\s*,\s*&\[|&(?:'static\s+)?str\s*=\s*{tool}\s*;")).unwrap()
}

/// Each line of `code` that starts one of [`TOOLS`], 1-based; comments aside.
fn starts(code: &str) -> Vec<usize> {
    let code: String = code.lines().map(|l| if l.trim_start().starts_with("//") { "" } else { l }).flat_map(|l| [l, "\n"]).collect();
    let mut lines: Vec<usize> = started().find_iter(&code).map(|m| code[..m.end()].lines().count()).collect();
    lines.dedup();
    lines
}

#[test]
fn the_tool_rule_sees_a_call_a_pair_and_a_constant_and_nothing_else() {
    assert_eq!(starts("run_cli(\"aws\", &[\"sts\"]).await"), [1]);
    assert_eq!(starts("let x = 1;\nexec::run(\n    \"docker\",\n    &[\"ps\"],\n)"), [3]);
    assert_eq!(starts("Command::new(\"VBoxManage.exe\")"), [1]);
    assert_eq!(starts("Cloud::Azure => (\"az\", &[\"login\"]),"), [1]);
    assert_eq!(starts("const VAGRANT: &str = \"vagrant\";"), [1]);
    assert_eq!(starts("// Command::new(\"az\") can't find it"), [] as [usize; 0]);
    assert_eq!(starts("if target == \"docker\" { out.join(\"vagrant\"); c[\"aws\"]; Some(\"oci\") }"), [] as [usize; 0]);
    assert_eq!(starts("run(\"dockerd-rootless\", &[])"), [] as [usize; 0]);
}

#[test]
fn no_code_outside_the_allow_list_runs_a_tool_isothrum_runs() {
    let (found, stale) = super::beyond_allow_list(starts, &[], ALLOWED);
    assert_eq!(found, [] as [String; 0], "these start a tool themselves: go through Isothrum or isothrum-machine");
    assert_eq!(stale, [] as [String; 0], "these no longer start a tool: take them off ALLOWED");
}

#[test]
fn the_spawn_rule_sees_every_way_to_start_a_process_and_nothing_else() {
    assert_eq!(spawns("std::process::Command::new(\"sh\")"), [1]);
    assert_eq!(spawns("let c = tokio::process::Command::new(p);\npair.slave.spawn_command(cmd)"), [1, 2]);
    assert_eq!(spawns("if ShellExecuteExW(&raw mut info) == 0 {}"), [1]);
    assert_eq!(spawns("ShellCommand::new(\"docker\", [\"ps\"], dir)"), [] as [usize; 0]);
    assert_eq!(spawns("// Command::new(\"az\")"), [] as [usize; 0]);
}

#[test]
fn every_process_the_app_starts_goes_through_the_runner() {
    let (found, stale) = super::beyond_allow_list(spawns, &["platform.rs"], OWN_WINDOWS);
    assert_eq!(found, [] as [String; 0], "these start a process themselves: run it through platform::exec (Isothrum's runner)");
    assert_eq!(stale, [] as [String; 0], "these no longer start a process themselves: take them off OWN_WINDOWS");
}
