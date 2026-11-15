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

/// Whether `w` is the shell word at byte `i` of `script`.
fn word_at(script: &str, i: usize, w: &str) -> bool {
    let bytes = script.as_bytes();
    script[i..].starts_with(w) && (i == 0 || b" \t;(|&".contains(&bytes[i - 1])) && bytes.get(i + w.len()).is_none_or(|c| b" \t;)".contains(c))
}

/// Follows the keywords at byte `i` (outside quotes): a `case`/`esac` inside a substitution, and
/// at the top level an `if`'s or a loop's condition, which `then`/`do` ends.
fn keyword(script: &str, i: usize, stack: &mut [Frame], condition: &mut bool) {
    let word = |w: &str| word_at(script, i, w);
    match stack.last_mut() {
        Some(Frame::Substitution { cases }) if word("case") => *cases += 1,
        Some(Frame::Substitution { cases }) if word("esac") => *cases = cases.saturating_sub(1),
        None if ["if", "elif", "while", "until"].into_iter().any(word) => *condition = true,
        None if word("then") || word("do") => *condition = false,
        _ => {}
    }
}

/// Where `script` chains with `&&` at its top level (outside quotes and `$(…)`, and outside an
/// `if`'s or a loop's condition, where `set -e` doesn't apply anyway), as byte offsets.
fn top_level_chains(script: &str) -> Vec<usize> {
    let bytes = script.as_bytes();
    let mut stack: Vec<Frame> = Vec::new();
    let mut condition = false;
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
            b'&' if stack.is_empty() && !condition && bytes.get(i + 1) == Some(&b'&') => {
                found.push(i);
                i += 1;
            }
            c if !quoted && c.is_ascii_lowercase() => keyword(script, i, &mut stack, &mut condition),
            _ => {}
        }
        i += 1;
    }
    found
}

/// The chains kept on purpose, each with why: what is before the `&&` in the command it ends.
const ALLOWED: &[(&str, &str)] = &[(
    "usermod -aG \"$g\" ",
    "joining the group that opens /dev/kvm is best-effort: when it fails the install goes on, and only the note that it was done is left out",
)];

/// Whether the chain at `at` in `script` is one of [`ALLOWED`].
fn allowed(script: &str, at: usize) -> bool {
    let command = &script[script[..at].rfind([';', '\n']).map_or(0, |i| i + 1)..at];
    ALLOWED.iter().any(|(before, _)| command.contains(before))
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
    let start = src.find("fn linux_plan(").expect("the Linux removal plan");
    let body = &src[start..start + src[start..].find("\n}\n").expect("its end")];
    body.split('"').skip(1).step_by(2).map(|lit| ("removal".to_owned(), lit.to_owned())).collect()
}

#[test]
fn no_root_script_chains_with_and_at_its_top_level() {
    let chained: Vec<String> = install_scripts()
        .into_iter()
        .chain(removal_scripts())
        .filter(|(_, s)| top_level_chains(s).into_iter().any(|at| !allowed(s, at)))
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
    assert_eq!(top_level_chains(r#"c="$(. /etc/os-release && echo "$ID")"; echo 'a && b'; awk '$1=="x"&&p'"#), Vec::<usize>::new());
    assert_eq!(top_level_chains(r#"if [ -n "$a" ] && b; then c; fi; while x && y; do z; done"#), Vec::<usize>::new(), "a condition's chain");
    assert_eq!(top_level_chains("if a; then b && c; fi").len(), 1, "a chain in an if's body counts");
    let kvm = r#"if getent group "$g" >/dev/null; then usermod -aG "$g" ada && echo "Added"; fi"#;
    assert!(top_level_chains(kvm).into_iter().all(|at| allowed(kvm, at)), "the kvm group's chain is allowed");
    assert_eq!(top_level_chains(r#"echo "\"" && true"#).len(), 1, "an escaped quote doesn't open a string");
    let case = r#"b="$(. /etc/os-release && case " $ID " in *ubuntu*) ;; *debian*) echo "x" ;; esac)"; a && b"#;
    assert_eq!(top_level_chains(case).len(), 1, "a case pattern's `)` doesn't end the substitution");
}
