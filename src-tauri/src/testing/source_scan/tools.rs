//! The launcher is a thin layer over Isothrum (docs/contributing/boundaries.md): it runs no
//! container engine, Vagrant, Terraform, hypervisor or cloud CLI itself. A file of shipped code
//! that names one of those tools as the program it starts (the first argument of a call, the
//! head of an argv list, or a `&str` constant) is an offender, unless [`ALLOWED`] lists it.
//!
//! [`ALLOWED`] is today's offenders, each with the step of the migration to Isothrum that removes
//! it. It only shrinks: a file not on it fails the build, and so does an entry whose file no
//! longer offends (take it off).

/// The tools the launcher leaves to Isothrum and `isothrum-machine`.
const TOOLS: &str = r"docker|vagrant|terraform|tofu|VBoxManage|vboxmanage|prlctl|vmrun|utmctl|virsh|qemu-[a-z0-9_.-]+|aws|az|gcloud|doctl|linode-cli|oci";

/// The files allowed to start one of [`TOOLS`] until the migration step named removes them.
const ALLOWED: &[(&str, &str)] = &[("machine/tools/install.rs", "L-14: tool and hypervisor installs move to isothrum-machine (I-14)")];

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
