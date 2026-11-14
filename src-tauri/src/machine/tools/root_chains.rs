//! No `&&` chain at a root script's top level. Every root script runs under `set -e`
//! ([`crate::platform::root_script::PRELUDE`]), and `set -e` ignores a failure anywhere in an
//! `a && b` list but its last command: `apt-get update && apt-get install x` that fails its update
//! goes on as if it hadn't. A root script's steps are `;`-separated, so the first failure stops
//! it; a command that may fail says so itself (`|| true`, `if`). Inside a command substitution, a
//! quoted string or an awk program, `&&` is not the script's chain and is left alone.
//!
//! Every root script the setup makes is checked: each `RootScript` the install scripts return
//! (for a player and for none), and the removal scripts' literals.

use super::scripts::{
    awscli_script, azurecli_script, docker_dnf_script, docker_script, gcloud_script, libvirt_dnf_script, libvirt_script, qemu_dnf_script, qemu_script,
    terraform_dnf_script, terraform_script, vagrant_dnf_script, vagrant_script, virtualbox_dnf_script, virtualbox_script, vmware_utility_script,
};

/// What the lexer is inside: a command substitution `$(…)` (with the `case` blocks open in it,
/// whose patterns end in a `)` that isn't the substitution's), or a double-quoted string.
enum Frame {
    Substitution { cases: usize },
    Quoted,
}

/// Where `script` chains with `&&` at its top level (outside quotes and `$(…)`), as byte offsets.
fn top_level_chains(script: &str) -> Vec<usize> {
    let bytes = script.as_bytes();
    let word_at = |i: usize, w: &str| {
        script[i..].starts_with(w) && (i == 0 || b" \t;(|&".contains(&bytes[i - 1])) && bytes.get(i + w.len()).is_none_or(|c| b" \t;)".contains(c))
    };
    let mut stack: Vec<Frame> = Vec::new();
    let mut found = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let quoted = matches!(stack.last(), Some(Frame::Quoted));
        match bytes[i] {
            b'\\' => i += 1,
            b'\'' if !quoted => i += script[i + 1..].find('\'').map_or(bytes.len(), |end| end + 1),
            b'"' if quoted => {
                stack.pop();
            }
            b'"' => stack.push(Frame::Quoted),
            b'$' if bytes.get(i + 1) == Some(&b'(') => {
                stack.push(Frame::Substitution { cases: 0 });
                i += 1;
            }
            b')' if !quoted && matches!(stack.last(), Some(Frame::Substitution { cases: 0 })) => {
                stack.pop();
            }
            b'c' if !quoted && word_at(i, "case") => {
                if let Some(Frame::Substitution { cases }) = stack.last_mut() {
                    *cases += 1;
                }
            }
            b'e' if !quoted && word_at(i, "esac") => {
                if let Some(Frame::Substitution { cases }) = stack.last_mut() {
                    *cases = cases.saturating_sub(1);
                }
            }
            b'&' if stack.is_empty() && bytes.get(i + 1) == Some(&b'&') => {
                found.push(i);
                i += 1;
            }
            _ => {}
        }
        i += 1;
    }
    found
}

/// Every root script the install steps make, by name.
fn install_scripts() -> Vec<(String, String)> {
    let mut out: Vec<(&str, String)> = vec![
        ("terraform", terraform_script().into_string()),
        ("vagrant", vagrant_script().into_string()),
        ("vmware_utility", vmware_utility_script().into_string()),
        ("virtualbox", virtualbox_script().into_string()),
        ("awscli", awscli_script().into_string()),
        ("azurecli", azurecli_script().into_string()),
        ("gcloud", gcloud_script().into_string()),
        ("vagrant_dnf", vagrant_dnf_script().into_string()),
        ("terraform_dnf", terraform_dnf_script().into_string()),
    ];
    for user in [Some("ada"), None] {
        out.extend([
            ("docker", docker_script(user).into_string()),
            ("libvirt", libvirt_script(user).into_string()),
            ("qemu", qemu_script(user).into_string()),
            ("docker_dnf", docker_dnf_script(user).into_string()),
            ("virtualbox_dnf", virtualbox_dnf_script(user).into_string()),
            ("libvirt_dnf", libvirt_dnf_script(user).into_string()),
            ("qemu_dnf", qemu_dnf_script(user).into_string()),
        ]);
    }
    out.into_iter().map(|(n, s)| (n.to_owned(), s)).collect()
}

/// The file's text, from the crate's sources.
fn source(path: &str) -> String {
    std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join(path)).unwrap()
}

/// The removal scripts: the string literals of the Linux removal plan (plain literals there).
fn removal_scripts() -> Vec<(String, String)> {
    let src = source("machine/tools/uninstall.rs");
    let start = src.find("fn linux_plan_with").expect("the Linux removal plan");
    let body = &src[start..start + src[start..].find("\n}\n").expect("its end")];
    body.split('"').skip(1).step_by(2).map(|lit| ("removal".to_owned(), lit.to_owned())).collect()
}

#[test]
fn no_root_script_chains_with_and_at_its_top_level() {
    let chained: Vec<String> = install_scripts()
        .into_iter()
        .chain(removal_scripts())
        .filter(|(_, s)| !top_level_chains(s).is_empty())
        .map(|(name, s)| format!("{name}: {s}"))
        .collect();
    assert!(chained.is_empty(), "root scripts chaining with && (set -e ignores a failure before the last command; use `;`):\n{}", chained.join("\n"));
}

/// Every install script that returns a `RootScript` is in [`install_scripts`]: a new one is
/// checked as soon as it exists.
#[test]
fn every_root_script_function_is_checked() {
    let src = source("machine/tools/scripts.rs");
    let listed = install_scripts().into_iter().map(|(n, _)| n).collect::<std::collections::BTreeSet<_>>();
    let made: Vec<String> = src
        .lines()
        .filter_map(|l| l.strip_prefix("pub fn ")?.split_once("_script(").map(|(n, rest)| (n.to_owned(), rest.contains("-> RootScript"))))
        .filter(|(_, root)| *root)
        .map(|(n, _)| n)
        .collect();
    let unchecked: Vec<&String> = made.iter().filter(|n| !listed.contains(*n)).collect();
    assert!(!made.is_empty() && unchecked.is_empty(), "root scripts no test checks for && chains: {unchecked:?}");
}

#[test]
fn only_a_top_level_and_chain_counts() {
    assert_eq!(top_level_chains("apt-get update && apt-get install -y x"), [15]);
    assert!(top_level_chains(r#"c="$(. /etc/os-release && echo "$ID")"; echo 'a && b'; awk '$1=="x"&&p'"#).is_empty());
    assert_eq!(top_level_chains(r#"echo "\"" && true"#).len(), 1, "an escaped quote doesn't open a string");
    let case = r#"b="$(. /etc/os-release && case " $ID " in *ubuntu*) ;; *debian*) echo "x" ;; esac)"; a && b"#;
    assert_eq!(top_level_chains(case).len(), 1, "a case pattern's `)` doesn't end the substitution");
}
