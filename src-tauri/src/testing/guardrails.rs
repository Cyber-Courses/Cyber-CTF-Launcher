//! Guardrails: rules over the crate's shipped code (its tests aside, as [`super::source_scan`]
//! splits it) that turn a class of bug into a failing test. Each allow-list names a file and the
//! text of the line it allows, with the reason; an entry that no longer matches fails too, so a
//! list only shrinks. Process starts are clippy's (`disallowed-methods` in `clippy.toml`).
//!
//! - Isothrum's home is Isothrum's: no shipped code opens a path inside it (its runs' folders,
//!   its store, its keys); the launcher asks the runner.
//! - A shell script handed to `sh -c` is a literal, an isothrum-machine `RootScript` (Linux's
//!   installs), or quoted by `runtime::ssh::sh_quote`: never text spliced in by hand.
//! - A tool's output is parsed (JSON) only in the modules that drive that tool.
//!
//! And over its test code (and the TypeScript tests'), what makes a test pass or fail by luck
//! (tests/README.md, "Conventions"):
//!
//! - A test waits on a condition or an event, never on the clock: each `sleep` in a unit test
//!   (and each `await new Promise((r) => setTimeout(r, …))` in a TypeScript one) is listed with why
//!   it is no wait for something to happen. The scenarios under `integration_tests/` have their own
//!   count (`integration_tests::waits`).
//! - A free port is held until it is used: a listener bound to port 0 then let go (dropped right
//!   after its port is read, or a `.local_addr()` read off a listener the statement doesn't keep)
//!   hands its port to whichever process asks next.

use super::source_scan::{Source, crate_sources};
use std::path::Path;

/// A shipped line a rule finds: the file (under `src/`), the line number and its text.
#[derive(Debug)]
struct Hit {
    file: String,
    line: usize,
    text: String,
}

/// The crate's sources, each with its path under `src/` (with `/`).
fn sources() -> impl Iterator<Item = (String, Source)> {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    crate_sources().into_iter().map(move |s| (s.path.strip_prefix(&src).unwrap().to_string_lossy().replace('\\', "/"), s))
}

/// Every shipped line `found` picks, file by file.
fn shipped_lines(found: impl Fn(&str, &str) -> bool) -> Vec<Hit> {
    let mut hits = Vec::new();
    for (file, s) in sources() {
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
        "runtime/isothrum/deployment.rs",
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

/// Where a script handed to `sh -c` may be built from parts. (Linux's root scripts are
/// isothrum-machine's, built and checked there.)
const SHELL_ALLOWED: &[(&str, &str, &str)] = &[
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
    let hits = shipped_lines(|file, l| file != "runtime/ssh.rs" && spliced_shell(l));
    let wrong = unallowed(&hits, SHELL_ALLOWED);
    assert!(wrong.is_empty(), "`sh -c` given a script built by hand (use a literal, RootScript or sh_quote):\n{}", wrong.join("\n"));
}

/// The modules that drive a tool, where its output is read: the process layer and its tool
/// wrappers, Docker's CLI, the runner, Proxmox's API and Terraform's state.
const ADAPTERS: &[&str] = &["platform/exec", "runtime/docker/", "runtime/isothrum", "runtime/proxmox/", "runtime/terraform/"];

/// Shipped JSON parsing outside the adapters, each with what it reads. A tool's output among them
/// is marked so; the rest read the app's own files or answers that are no tool's.
const PARSE_ALLOWED: &[(&str, &str, &str)] = &[
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
    ("settings.rs", "serde_json::from_str::<TestRecord>(value)", "a test record 1.9 kept in the webview's storage"),
    ("settings.rs", "serde_json::from_str(value).unwrap_or_default()", "the servers' VM tests 1.9 kept in the webview's storage"),
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

/// A test line, with the lines after it (the rules that read a statement read a few).
struct TestLine<'a> {
    rest: &'a [&'a str],
}

/// Every unit test line `found` picks, file by file: the test code [`super::source_scan`] splits
/// off, outside `integration_tests/` (its scenarios have their own scan) and this file (its cases
/// below are no test's).
fn test_lines(found: impl Fn(&TestLine) -> bool) -> Vec<Hit> {
    let scanned = sources().filter(|(file, s)| !(file.starts_with("integration_tests") || file == "testing/guardrails.rs" || s.tests.is_empty()));
    scanned
        .flat_map(|(file, s)| {
            // The test code is the file's end: its first line's number is what comes before it.
            let before = std::fs::read_to_string(&s.path).unwrap().lines().count() - s.tests.lines().count();
            let lines: Vec<&str> = s.tests.lines().collect();
            (0..lines.len())
                .filter(|&i| !lines[i].trim().starts_with("//") && found(&TestLine { rest: &lines[i..] }))
                .map(|i| Hit { file: file.clone(), line: before + i + 1, text: lines[i].trim().to_owned() })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Whether `l` calls a `sleep` (`thread::sleep`, `tokio::time::sleep`, one imported), a
/// function's own definition aside.
fn sleeps(l: &str) -> bool {
    l.match_indices("sleep(").any(|(at, _)| !(l[..at].ends_with("fn ") || l[..at].ends_with('_')))
}

/// The sleeps the unit tests keep, each with why it is no wait for something to happen (a
/// stand-in that is slow on purpose, the poll inside a bounded wait). The two marked "wall clock"
/// are waits on a time, to replace with the condition they wait for.
const SLEEP_ALLOWED: &[(&str, &str, &str)] = &[
    ("app.rs", "tokio::time::sleep(every).await;", "every_pass's own interval: shipped code after the test module"),
    ("app.rs", "tokio::time::sleep(std::time::Duration::from_millis(5)).await;", "a poll under a 5 s timeout for the passes to count three"),
    ("app/quit.rs", "std::thread::sleep(Duration::from_millis(2500));", "wall clock: an absence (the relaunched app doesn't exit) takes time to see"),
    ("account/auth/tests.rs", "tokio::time::sleep(std::time::Duration::from_millis(10)).await;", "a poll under a 5 s timeout for the refresh to be saved"),
    ("cloud_cli/tests.rs", "tokio::time::sleep(Duration::from_millis(100)).await;", "wall clock: the sign-in under way before it is cancelled"),
    ("platform/exec/tests.rs", "tokio::time::sleep(Duration::from_millis(50)).await;", "a poll bounded to 300 looks for a child's pid"),
    ("platform/exec/fake.rs", "tokio::time::sleep(std::time::Duration::from_millis(100)).await;", "`gone`'s poll, bounded to 60 looks, for a process to end"),
    ("platform/identity.rs", "std::thread::sleep(std::time::Duration::from_millis(100));", "a poll to a 10 s deadline for a process's identity to go"),
    ("run_service/tests.rs", "tokio::time::sleep(Duration::from_millis(10)).await;", "polls for an operation's end and its first statuses, bounded"),
    ("runtime/inflight/lock.rs", "std::thread::sleep(Duration::from_millis(25));", "eventually's poll, bounded to 40 looks"),
    ("runtime/inflight/lock.rs", "std::thread::sleep(Duration::from_millis(20));", "a lock held a moment, so the threads overlap"),
    ("runtime/server/store.rs", "std::thread::sleep(std::time::Duration::from_millis(5));", "an update held a moment, so the threads overlap"),
    ("screen.rs", "tokio::time::sleep(std::time::Duration::from_millis(50)).await;", "a poll, bounded, for a viewer's pid to be reaped"),
    (
        "testing/service.rs",
        "tokio::time::sleep(Duration::from_millis(10)).await;",
        "polls bounded to 6000 looks: a test's runner service answering, a stopping one letting its home go",
    ),
    (
        "testing/service.rs",
        "tokio::time::sleep(GONE_EVERY).await;",
        "a service on the services' runtime looking for its home's folder to go, the test's end, for as long as it serves",
    ),
    ("screen/native.rs", "tokio::time::sleep(Duration::from_millis(20)).await;", "polls to a 120 s deadline for the viewer to start, then to warn"),
];

#[test]
fn a_unit_test_waits_on_a_condition_not_on_the_clock() {
    let hits = test_lines(|t| sleeps(t.rest[0]));
    let wrong = unallowed(&hits, SLEEP_ALLOWED);
    assert!(wrong.is_empty(), "a test sleeping (wait on the condition or event itself, a poll to a deadline, or name it here with why):\n{}", wrong.join("\n"));
}

/// The TypeScript tests' sleeps (`await new Promise((r) => setTimeout(r, …))`), each with why it
/// is no wait for something to happen; the files under the repository's root.
const TS_SLEEP_ALLOWED: &[(&str, &str, &str)] = &[
    ("src/components/app-shell.test.tsx", "await new Promise((r) => setTimeout(r, 50));", "wall clock: each screen's first render settling"),
    ("src/features/account/account.test.tsx", "await new Promise((r) => setTimeout(r, 0));", "one turn of the event loop, for a rejection to be reported"),
    ("tests/e2e/desktop/runner/run.test.ts", "await new Promise((r) => setTimeout(r, 100));", "a stand-in app's poll, bounded to 100 looks, for its server"),
    (
        "tests/e2e/desktop/runner/run.test.ts",
        "await new Promise((res) => setTimeout(res, 100));",
        "polls bounded to 100, 150 and 30 looks for a stand-in app's pids",
    ),
];

/// Every line of the TypeScript tests (`*.test.ts`, `*.test.tsx`, `*.test.mts`) under `src/`,
/// `scripts/` and `tests/` that `found` picks.
fn ts_test_lines(found: impl Fn(&str) -> bool) -> Vec<Hit> {
    fn walk(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if path.is_dir() && name != "node_modules" && !name.starts_with('.') {
                walk(&path, out);
            } else if [".test.ts", ".test.tsx", ".test.mts"].iter().any(|x| name.ends_with(x)) {
                out.push(path);
            }
        }
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut files = Vec::new();
    for dir in ["src", "scripts", "tests"] {
        walk(&root.join(dir), &mut files);
    }
    files.sort();
    let mut hits = Vec::new();
    for f in files {
        let file = f.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
        for (i, l) in std::fs::read_to_string(&f).unwrap().lines().enumerate() {
            let t = l.trim();
            if !t.starts_with("//") && found(t) {
                hits.push(Hit { file: file.clone(), line: i + 1, text: t.to_owned() });
            }
        }
    }
    hits
}

/// Whether `l` is a TypeScript sleep: a promise resolved by a timer and nothing else.
fn ts_sleeps(l: &str) -> bool {
    l.contains("new Promise(") && l.contains("=> setTimeout(")
}

#[test]
fn a_typescript_test_waits_on_a_condition_not_on_the_clock() {
    let hits = ts_test_lines(ts_sleeps);
    assert!(hits.len() > 1, "the TypeScript tests were read");
    let wrong = unallowed(&hits, TS_SLEEP_ALLOWED);
    assert!(
        wrong.is_empty(),
        "a TypeScript test sleeping (await the condition: findBy*, waitFor, vi's fake timers, or name it here with why):\n{}",
        wrong.join("\n")
    );
}

/// Whether the statement at `t` binds a socket to port 0 and lets it go before the port is used:
/// a `.local_addr()` read off the listener in the same statement (the listener a temporary), or
/// `drop(<it>)` within the next few lines.
fn lets_a_port_go(t: &TestLine) -> bool {
    const WITHIN: usize = 4;
    let l = t.rest[0];
    if !(l.contains("bind(") && (l.contains(":0\"") || l.contains(", 0))"))) {
        return false;
    }
    let statement = t.rest.iter().take(WITHIN).copied().collect::<Vec<_>>().join(" ");
    // From the bind to the statement's end (its `;`, or the block it opens).
    let read_off = statement.split_once("bind(").is_some_and(|(_, after)| after.split([';', '{']).next().unwrap_or_default().contains(".local_addr()"));
    let name = l.trim().strip_prefix("let ").map(|r| r.trim_start_matches("mut ")).and_then(|r| r.split([' ', ':', '=']).next()).filter(|n| !n.is_empty());
    let dropped = name.is_some_and(|n| t.rest.iter().skip(1).take(WITHIN).any(|l| l.contains(&format!("drop({n})"))));
    read_off || dropped
}

/// The ports the unit tests let go before they use them, each with why. "A port nothing listens
/// on" taken this way can be taken by a test beside it before it is used: to replace with a port
/// held but never accepting, or port 0 itself (which no connection reaches).
const PORT_ALLOWED: &[(&str, &str, &str)] = &[
    ("labs.rs", "let closed = tokio::net::TcpListener::bind(\"127.0.0.1:0\")", "a port nothing listens on, let go (to replace)"),
    ("runtime/launch/tests.rs", "let closed = tokio::net::TcpListener::bind(\"127.0.0.1:0\")", "a resolver that is down: a port let go (to replace)"),
    ("screen/bridge/tests.rs", "let free = TcpListener::bind(\"127.0.0.1:0\")", "a screen that doesn't answer: a port let go (to replace)"),
    ("account/api.rs", "let closed = tokio::net::TcpListener::bind(\"127.0.0.1:0\")", "the API unreachable: a port let go (to replace)"),
    ("account/auth/tokens.rs", "let closed = tokio::net::TcpListener::bind(\"127.0.0.1:0\")", "the sign-in server unreachable: a port let go (to replace)"),
];

#[test]
fn a_unit_test_holds_the_free_port_it_picked_until_it_uses_it() {
    let hits = test_lines(lets_a_port_go);
    let wrong = unallowed(&hits, PORT_ALLOWED);
    assert!(
        wrong.is_empty(),
        "a port picked by binding port 0, let go before it is used (keep the listener and hand it, or its port while held, to what uses it):\n{}",
        wrong.join("\n")
    );
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
    assert!(sleeps("std::thread::sleep(d);") && sleeps("tokio::time::sleep(d).await;") && sleeps("sleep(d).await;"));
    assert!(!sleeps("tokio::time::sleep_until(t).await;") && !sleeps("async fn sleep(d: Duration) {"));
    assert!(ts_sleeps("await new Promise((r) => setTimeout(r, 50));") && !ts_sleeps("setTimeout(() => done(), 0);"));
    let ports = [
        ("let l = TcpListener::bind(\"127.0.0.1:0\").unwrap();\nlet port = l.local_addr().unwrap().port();\ndrop(l);", true),
        ("let closed = TcpListener::bind(\"127.0.0.1:0\").await.unwrap().local_addr().unwrap();", true),
        ("let l = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();\nserve(l);", false),
        ("if let Ok(l) = TcpListener::bind((Ipv6Addr::UNSPECIFIED, 0)) {\n    let port = l.local_addr().unwrap().port();", false),
        ("let held = TcpListener::bind((\"127.0.0.1\", port)).unwrap();\ndrop(held);", false),
    ];
    for (code, lets_go) in ports {
        let lines: Vec<&str> = code.lines().collect();
        assert_eq!(lets_a_port_go(&TestLine { rest: &lines }), lets_go, "{code}");
    }
}
