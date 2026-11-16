//! Guardrails: rules over the crate's shipped code (its tests aside, as [`super::source_scan`]
//! splits it) that turn a class of bug into a failing test. Each allow-list names a file and the
//! text of the line it allows, with the reason; an entry that no longer matches fails too, so a
//! list only shrinks. Process starts are clippy's (`disallowed-methods` in `clippy.toml`).
//!
//! - Isothrum's home is Isothrum's: no shipped code opens a path inside it (its runs' folders,
//!   its store, its keys); the launcher asks the runner.
//! - A shell script handed to `sh -c` is a literal, a [`crate::platform::root_script::RootScript`],
//!   or quoted by `runtime::ssh::sh_quote`: never text spliced in by hand.
//! - A tool's output is parsed (JSON) only in the modules that drive that tool.

use super::source_scan::crate_sources;
use std::path::Path;

/// A shipped line a rule finds: the file (under `src/`), the line number and its text.
#[derive(Debug)]
struct Hit {
    file: String,
    line: usize,
    text: String,
}

/// Every shipped line `found` picks, file by file.
fn shipped_lines(found: impl Fn(&str, &str) -> bool) -> Vec<Hit> {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut hits = Vec::new();
    for s in crate_sources() {
        let file = s.path.strip_prefix(&src).unwrap().to_string_lossy().replace('\\', "/");
        for (i, l) in s.code.lines().enumerate() {
            let t = l.trim();
            if !t.starts_with("//") && found(&file, t) {
                hits.push(Hit { file: file.clone(), line: i + 1, text: t.to_owned() });
            }
        }
    }
    hits
}

/// The hits no entry of `allowed` (file, text in the line, reason) covers, and the entries that
/// cover no hit any more (stale: remove them).
fn unallowed(hits: &[Hit], allowed: &[(&str, &str, &str)]) -> Vec<String> {
    let covers = |h: &Hit, (file, text, _): &(&str, &str, &str)| h.file == *file && h.text.contains(text);
    let mut out: Vec<String> = hits.iter().filter(|h| !allowed.iter().any(|a| covers(h, a))).map(|h| format!("{}:{}: {}", h.file, h.line, h.text)).collect();
    out.extend(
        allowed
            .iter()
            .filter(|a| !hits.iter().any(|h| covers(h, a)))
            .map(|(f, t, _)| format!("stale allow-list entry (nothing matches it now: remove it): {f}: {t}")),
    );
    out
}

/// What names a path inside Isothrum's home (`docs/reference/state.md` of Isothrum): its runs'
/// folders, its store, its locks, its key, its provider cache, and the 2.x files.
const HOME_PATHS: &[&str] = &[
    "join(\"runs\")",
    "\"runs/",
    "state.db",
    "slots.lock",
    "join(\"ssh\")",
    "join(\"id_ed25519\")",
    "terraform-plugins",
    "record.json",
    "store.sqlite",
    "events.jsonl",
    "seen.json",
    "anonymous-volumes",
];

/// Shipped code that still opens paths inside Isothrum's home, each with why, until Isothrum's
/// API gives what it reads there.
const HOME_ALLOWED: &[(&str, &str, &str)] = &[
    ("runtime/ssh.rs", "dirs.data()?.join(\"ssh\")", "the launcher's own key folder, which is also the runner's (its home is the app data folder)"),
    ("runtime/ssh.rs", "ssh_dir(dirs)?.join(\"id_ed25519\")", "the launcher's own key, the same file as the runner's"),
    (
        "runtime/isothrum/opened.rs",
        "home.join(\"ssh\").join(\"id_ed25519\")",
        "a Terraform run's key for its module's machines and its jump host: the runner gives none of its key's path (asked of Isothrum)",
    ),
];

#[test]
fn shipped_code_opens_no_path_inside_isothrums_home() {
    let hits = shipped_lines(|_, l| HOME_PATHS.iter().any(|p| l.contains(p)));
    let wrong = unallowed(&hits, HOME_ALLOWED);
    assert!(wrong.is_empty(), "paths inside Isothrum's home (ask the runner instead):\n{}", wrong.join("\n"));
}

/// Where a script handed to `sh -c` may be built from parts: the root scripts' own module, and
/// the one place a pkexec step takes a [`crate::platform::root_script::RootScript`].
const SHELL_ALLOWED: &[(&str, &str, &str)] = &[
    ("machine/tools/install.rs", "format!(\"'{brew}' trust azure/azure-cli", "macOS: Homebrew's own path, found by the app, single-quoted"),
    ("machine/tools/scripts.rs", "args: vec![\"-c\".into(), script]", "macOS: the cask fetch; Homebrew's path and fixed cask names, single-quoted"),
    ("machine/tools/vmware_utility.rs", "args: vec![\"-c\".into(), mac_script(", "macOS: the utility's install, a pinned file name and digest"),
    ("machine/tools/install.rs", "\"-c\".into(),", "macOS: QEMU's brew update then install, Homebrew's own path single-quoted"),
    ("machine/selftest/vm.rs", "Step::new(&[\"ssh\", \"-c\", &uname_command()])", "vagrant ssh's -c: the self-test's fixed probe, run in the test VM"),
    ("platform.rs", "args([\"-n\", \"-c\", script])", "a test helper (cfg(test)): sh -n only parses the script"),
    (
        "runtime/isothrum/checks.rs",
        "[\"sh\".to_owned(), \"-c\".to_owned(), self.runner_code(check)?]",
        "a lab's check runner, whole as Isoloom generated it (or its command), run in the lab's VM",
    ),
    ("runtime/terminal_launch.rs", "args([flag, \"sh\", \"-c\", &run])", "the attack shell's line, quoted word by word with shell_words"),
    ("screen/rdp.rs", "vec![\"-c\".into(), file]", "Remmina's -c is its connection file, not a shell"),
];

/// Whether `-c` on line `l` hands a shell a script that isn't a literal.
fn spliced_shell(l: &str) -> bool {
    l.match_indices("\"-c\"").any(|(at, _)| {
        let rest = l[at + 4..].trim_start_matches(".into()").trim_start_matches(')').trim_start_matches(".arg(").trim_start_matches([',', ' ']);
        !(rest.starts_with('"') || rest.starts_with("r\"") || rest.starts_with("r#\""))
    })
}

#[test]
fn no_shell_script_is_spliced_by_hand() {
    // A `-c` whose script comes on a later line is read from there: `-c` and its argument.
    let hits = shipped_lines(|file, l| file != "platform/root_script.rs" && file != "runtime/ssh.rs" && spliced_shell(l));
    let wrong = unallowed(&hits, SHELL_ALLOWED);
    assert!(wrong.is_empty(), "`sh -c` given a script built by hand (use a literal, RootScript or sh_quote):\n{}", wrong.join("\n"));
}

/// The modules that drive a tool, where its output is read: the cloud CLIs, the process layer
/// and its tool wrappers, Docker's CLI, the runner, Proxmox's API and Terraform's state.
const ADAPTERS: &[&str] = &["cloud_cli.rs", "platform/exec", "runtime/docker/", "runtime/isothrum", "runtime/proxmox/", "runtime/terraform/"];

/// Shipped JSON parsing outside the adapters, each with what it reads. A tool's output among them
/// is marked so; the rest read the app's own files or answers that are no tool's.
const PARSE_ALLOWED: &[(&str, &str, &str)] = &[
    ("machine/system.rs", "serde_json::from_str(json.trim())", "tool output: `az version`; belongs in cloud_cli"),
    ("runtime/server/azure_size.rs", "serde_json::from_str(json)", "tool output: `az vm list-sizes`; belongs in cloud_cli"),
    ("runtime/server/azure_size.rs", "serde_json::from_str::<Vec<Value>>(out)", "tool output: `az vm list-usage`; belongs in cloud_cli"),
    ("app.rs", "serde_json::from_str(json)", "the webview's stored settings"),
    ("platform/policy.rs", "serde_json::from_str::<serde_json::Value>(text)", "the administrator's policy file"),
    ("runtime/attack_box.rs", "serde_json::from_str::<File>(IMAGE_FILE)", "the bundled attack box image file"),
    ("runtime/server/secrets.rs", "serde_json::from_str(&raw)", "the app's own secrets file"),
    ("app/updates.rs", "serde_json::from_str::<serde_json::Value>(receipt)", "the installer's receipt file"),
    ("labs/offline.rs", "serde_json::from_str(&raw)", "the app's record of a lab kept offline"),
    ("labs/ctf.rs", "serde_json::from_str(&text)", "a lab's own metadata file"),
    ("account/auth/store.rs", "serde_json::from_str(raw)", "the app's stored sign-in"),
    ("account/auth/tokens.rs", "serde_json::from_slice(&URL_SAFE_NO_PAD.decode(payload)", "a token's claims (JWT payload)"),
    ("account/auth/tokens.rs", "serde_json::from_str::<serde_json::Value>(&body)", "the sign-in server's HTTP answer"),
    ("account/api.rs", "serde_json::from_str(text)", "the backend's HTTP answer"),
    ("store.rs", "serde_json::from_str(text)", "the launcher's own store's JSON columns"),
    ("english.rs", "serde_json::from_str(source)", "the webview's bundled English messages"),
    ("diagnostics/collect.rs", "serde_json::from_str::<Value>(body)", "an earlier version's deploy verdict file, left in the data folder"),
    ("diagnostics/collect.rs", "serde_json::from_slice::<Value>(&std::fs::read(dir.join(n))", "an earlier version's deploy job files, left in the data folder"),
];

#[test]
fn tool_output_is_parsed_only_in_the_adapters() {
    let parses =
        |file: &str, l: &str| !ADAPTERS.iter().any(|a| file.starts_with(a)) && (l.contains("serde_json::from_str") || l.contains("serde_json::from_slice"));
    let hits = shipped_lines(parses);
    let wrong = unallowed(&hits, PARSE_ALLOWED);
    assert!(wrong.is_empty(), "JSON parsed outside a tool's adapter (move it there, or add it with what it reads):\n{}", wrong.join("\n"));
}

#[test]
fn the_guardrails_see_what_they_look_for() {
    assert!(spliced_shell("args: vec![\"-c\".into(), format!(\"x {y}\")],"));
    assert!(spliced_shell(".arg(\"-c\").arg(script)"));
    assert!(!spliced_shell("step(\"sh\", &[\"-c\", \"rm -rf \\\"$HOME/x\\\"\"])"));
    assert!(!spliced_shell("[\"-c\", \"qemu:///system\", \"list\"]"));
    let hit = |file: &str, text: &str| Hit { file: file.into(), line: 1, text: text.into() };
    assert_eq!(unallowed(&[hit("a.rs", "x.join(\"runs\")")], &[]).len(), 1);
    assert_eq!(unallowed(&[hit("a.rs", "x.join(\"runs\")")], &[("a.rs", "x.join", "why")]), Vec::<String>::new());
    assert!(unallowed(&[], &[("a.rs", "x.join", "why")])[0].starts_with("stale"), "an entry nothing matches is stale");
}
