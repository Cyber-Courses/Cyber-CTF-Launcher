//! The command line driven in-process, on the app's own test app and scripted tools
//! (`platform::exec::fake`): every command's JSON shape and exit code, the confirmations, and the
//! guided setup run for real through its steps (installs answered by the fakes, none run).

use serde_json::{Value, json};

use super::Exit;
use crate::platform::exec::fake::{Reply, Tools, fail, ok};
use crate::testing::test_app::TestApp;

/// A container lab's spec.
const SPEC: &str = "version: 1\nname: web\nnetworks:\n  lab: { cidr: 10.30.0.0/24 }\nmachines:\n  web:\n    networks: { lab: 10 }\n    ports: { web: { port: 80, protocol: http } }\n    app: { image: nginx:1.27 }\n";

/// What a command line run said: its exit, its JSON lines (with `--json`) or its text, and stderr.
struct Ran {
    exit: Exit,
    lines: Vec<Value>,
    text: String,
    stderr: String,
}

impl Ran {
    /// The last JSON line: the result or the error.
    fn last(&self) -> &Value {
        self.lines.last().unwrap_or_else(|| panic!("no JSON line; stdout: {} stderr: {}", self.text, self.stderr))
    }

    /// The events' payloads.
    fn events(&self) -> Vec<&Value> {
        self.lines.iter().filter(|l| l["type"] == "event").map(|l| &l["event"]).collect()
    }
}

/// Runs `cyberctf <args>` on `t`'s folders, off a terminal.
async fn cyberctf(t: &TestApp, args: &[&str]) -> Ran {
    run_cyberctf(t, args, false).await
}

/// Runs `cyberctf <args>` on `t`'s folders, on a terminal (`--yes` then still prompts to elevate).
#[cfg(target_os = "linux")]
async fn on_a_terminal(t: &TestApp, args: &[&str]) -> Ran {
    run_cyberctf(t, args, true).await
}

/// Runs `cyberctf <args>` on `t`'s folders, on a terminal or not (`tty`).
async fn run_cyberctf(t: &TestApp, args: &[&str], tty: bool) -> Ran {
    let command_line = std::iter::once("cyberctf").chain(args.iter().copied()).map(std::ffi::OsString::from).collect();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let exit = super::run(command_line, tty, Some(t.dirs()), &mut out, &mut err).await;
    let text = String::from_utf8(out).unwrap();
    let lines =
        if args.contains(&"--json") { text.lines().map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("{e}: {l}"))).collect() } else { Vec::new() };
    Ran { exit, lines, text, stderr: String::from_utf8(err).unwrap() }
}

#[tokio::test]
async fn the_version_is_a_versioned_json_result() {
    let t = TestApp::new();
    let ran = cyberctf(&t, &["version", "--json"]).await;
    assert_eq!(ran.exit, Exit::Ok);
    let last = ran.last();
    assert_eq!((&last["v"], &last["type"], &last["command"]), (&json!(1), &json!("result"), &json!("version")));
    assert_eq!(last["data"]["version"], env!("CARGO_PKG_VERSION"));
    assert!(last["data"]["isothrum"].is_string() && last["data"]["isoloom"].is_string());
    assert_eq!(cyberctf(&t, &["version"]).await.text.trim(), format!("cyberctf {}", env!("CARGO_PKG_VERSION")));
}

#[tokio::test]
async fn the_shells_completions_name_every_command() {
    let t = TestApp::new();
    for shell in ["bash", "zsh", "fish", "powershell"] {
        let ran = cyberctf(&t, &["completions", shell]).await;
        assert_eq!(ran.exit, Exit::Ok, "{shell}");
        for command in ["labs", "attack-box", "machine", "setup", "servers", "doctor"] {
            assert!(ran.text.contains(command), "{shell} completes {command}");
        }
    }
}

#[tokio::test]
async fn a_wrong_command_line_is_a_usage_error_said_in_json_too() {
    let t = TestApp::new();
    let ran = cyberctf(&t, &["labs", "fly"]).await;
    assert_eq!(ran.exit, Exit::Usage);
    assert!(ran.stderr.contains("fly"), "{}", ran.stderr);
    let ran = cyberctf(&t, &["labs", "fly", "--json"]).await;
    assert_eq!(ran.exit, Exit::Usage);
    assert_eq!((&ran.last()["type"], &ran.last()["exit"]), (&json!("error"), &json!(2)));
    assert_eq!(cyberctf(&t, &["--help"]).await.exit, Exit::Ok);
}

#[tokio::test]
async fn the_installed_labs_are_listed_with_how_each_runs() {
    let t = TestApp::new();
    let _tools = Tools::rules(&[]);
    let ran = cyberctf(&t, &["labs", "list", "--json"]).await;
    assert_eq!((ran.exit, &ran.last()["data"], &ran.last()["command"]), (Exit::Ok, &json!([]), &json!("labs.list")));
    t.lab("web", &[("isoloom.yml", SPEC)]);
    t.lab("ad", &[("isoloom.yml", "version: 1\nname: ad\nmachines:\n  dc: { os: windows-server@2022 }\n")]);
    let ran = cyberctf(&t, &["labs", "list", "--json"]).await;
    let labs = ran.last()["data"].as_array().unwrap().clone();
    let brief: Vec<(Value, Value, Value)> = labs.iter().map(|l| (l["id"].clone(), l["runtime"].clone(), l["running"].clone())).collect();
    assert_eq!(brief, [(json!("ad"), json!("VM"), json!(false)), (json!("web"), json!("DOCKER"), json!(false))]);
    assert_eq!(labs[1]["name"], "web");
    assert!(cyberctf(&t, &["labs", "list"]).await.text.contains("web"));
}

#[tokio::test]
async fn a_labs_details_mask_its_passwords_unless_asked() {
    use crate::runtime::attack_box::{self, Cpu, Kind};
    let t = TestApp::new();
    let _tools = Tools::rules(&[]);
    let dir = t.lab("web", &[("isoloom.yml", SPEC)]);
    attack_box::add(&dir, Kind::Container { image: "kalilinux/kali-rolling" }, Cpu::Lab, None, None).unwrap();
    let password = crate::commands::labs::sign_in_in(&t.dirs(), "web").unwrap().unwrap().password;
    let ran = cyberctf(&t, &["labs", "show", "web", "--json"]).await;
    assert_eq!(ran.exit, Exit::Ok, "{}", ran.text);
    let data = &ran.last()["data"];
    assert_eq!((&data["runtime"], &data["status"]["running"]), (&json!("DOCKER"), &json!(false)));
    assert_eq!(data["attackBox"], json!({ "user": attack_box::PLAYER, "password": "********" }));
    assert!(!ran.text.contains(&password));
    let shown = cyberctf(&t, &["labs", "show", "web", "--show-secrets", "--json"]).await;
    assert_eq!(shown.last()["data"]["attackBox"]["password"], password);
    assert!(!cyberctf(&t, &["labs", "show", "web"]).await.text.contains(&password), "the text masks it too");
}

#[tokio::test]
async fn an_operation_on_a_lab_that_isnt_installed_is_not_ready_and_a_bad_id_is_a_usage_error() {
    let t = TestApp::new();
    let tools = Tools::rules(&[]);
    let not_installed: [&[&str]; 3] = [
        &["labs", "stop", "gone", "--yes", "--json"],
        &["labs", "resume", "gone", "--runtime", "vm", "--json"],
        &["attack-box", "start", "gone", "--runtime", "docker", "--json"],
    ];
    for args in not_installed {
        let ran = cyberctf(&t, args).await;
        assert_eq!(ran.exit, Exit::NotReady, "{args:?}: {:?}", ran.lines);
        assert_eq!(ran.last()["error"]["message"], "lab `gone` is not installed");
    }
    let ran = cyberctf(&t, &["labs", "status", "../x", "--runtime", "docker", "--json"]).await;
    assert_eq!((ran.exit, &ran.last()["error"]["message"]), (Exit::Usage, &json!("invalid lab id `../x`")));
    // Refused before the run service: an image that could reach a command line.
    t.lab("web", &[("isoloom.yml", SPEC)]);
    let ran = cyberctf(&t, &["attack-box", "start", "web", "--image", "x;y", "--json"]).await;
    assert_eq!((ran.exit, &ran.last()["error"]["message"]), (Exit::Usage, &json!("invalid attack-box image `x;y`")));
    assert!(tools.calls().is_empty(), "{:?}", tools.calls());
}

#[tokio::test]
async fn a_destructive_command_needs_yes_off_a_terminal() {
    let t = TestApp::new();
    let _tools = Tools::rules(&[]);
    let dir = t.lab("web", &[("isoloom.yml", SPEC)]);
    for args in [&["labs", "remove", "web", "--json"][..], &["labs", "stop", "web", "--json"], &["storage", "clean", "--json"]] {
        let ran = cyberctf(&t, args).await;
        assert_eq!(ran.exit, Exit::Usage, "{args:?}");
        assert!(ran.last()["error"]["message"].as_str().unwrap().ends_with("this needs --yes when not run on a terminal"), "{:?}", ran.last());
    }
    assert!(dir.is_dir(), "nothing removed");
    let ran = cyberctf(&t, &["labs", "remove", "web", "--yes", "--json"]).await;
    assert_eq!((ran.exit, &ran.last()["data"]["left"]), (Exit::Ok, &json!([])));
    assert!(!dir.exists());
}

#[tokio::test]
async fn a_labs_last_log_is_read_back() {
    let t = TestApp::new();
    let deploys = crate::runtime::deploys_dir(&t.dirs()).unwrap();
    std::fs::write(deploys.join("web.log"), "==> Starting\n✗ it failed\n").unwrap();
    let ran = cyberctf(&t, &["labs", "logs", "web", "--follow", "--json"]).await;
    assert_eq!(ran.exit, Exit::Ok);
    assert_eq!(ran.last()["data"], json!({ "lab": "web", "running": false, "log": "==> Starting\n✗ it failed\n" }));
    assert_eq!(cyberctf(&t, &["labs", "logs", "other"]).await.text.trim(), "");
}

#[tokio::test]
async fn the_saved_servers_are_listed_and_a_default_needs_a_name() {
    let t = TestApp::new();
    t.host("pve", "proxmox", json!({}));
    let ran = cyberctf(&t, &["servers", "list", "--json"]).await;
    assert_eq!(ran.last()["data"]["hosts"][0]["id"], "pve");
    let ran = cyberctf(&t, &["servers", "set-default", "pve", "--json"]).await;
    assert_eq!(ran.exit, Exit::Ok, "{:?}", ran.lines);
    assert_eq!(cyberctf(&t, &["servers", "show", "pve", "--json"]).await.last()["data"]["default"], true);
    assert_eq!(cyberctf(&t, &["servers", "set-default", "--json"]).await.exit, Exit::Usage);
    assert_eq!(cyberctf(&t, &["servers", "show", "nope", "--json"]).await.exit, Exit::NotReady);
}

#[tokio::test]
async fn an_unknown_tool_is_a_usage_error_and_a_plugin_isnt_removed_alone() {
    let t = TestApp::new();
    let tools = Tools::rules(&[]);
    assert_eq!(cyberctf(&t, &["machine", "tools", "install", "nmap", "--yes", "--json"]).await.exit, Exit::Usage);
    assert_eq!(cyberctf(&t, &["machine", "tools", "remove", "plugin:vagrant-qemu", "--yes", "--json"]).await.exit, Exit::Usage);
    assert!(tools.calls().is_empty(), "{:?}", tools.calls());
}

/// A bare Linux machine for the setup's tests: apt, nothing else, until a root step installs
/// Docker (it then answers as installed and running), or fails it when `install_fails`.
#[cfg(target_os = "linux")]
fn bare_linux(install_fails: bool) -> (Tools, std::rc::Rc<std::cell::Cell<bool>>) {
    let installed = std::rc::Rc::new(std::cell::Cell::new(false));
    let docker = installed.clone();
    let tools = Tools::new(move |program, args| {
        let line = args.join(" ");
        match program {
            "apt" | "apt-get" | "dnf" => ok("apt 2.9.8 (amd64)\n"),
            "pkexec" | "sudo" if line.starts_with("-n true") => fail("sudo: a password is required"),
            "pkexec" | "sudo" | "env" if line.contains("docker") => {
                if install_fails {
                    return fail("E: Unable to locate package docker-ce");
                }
                docker.set(true);
                ok("Setting up docker-ce (5:29.1.0) ...\n")
            }
            "docker" if docker.get() => docker_answer(&line),
            "id" | "getent" => ok("player : player docker\n"),
            _ => None,
        }
    });
    (tools, installed)
}

/// What an installed, running Docker Engine answers to `docker <line>`.
#[cfg(target_os = "linux")]
fn docker_answer(line: &str) -> Option<Reply> {
    if line.starts_with("--version") || line.starts_with("version") {
        ok("Docker version 29.1.0, build 1\n")
    } else if line.starts_with("compose") {
        ok("Docker Compose version v2.40.0\n")
    } else if line.starts_with("context") {
        ok("{\"Name\":\"default\",\"Current\":true,\"DockerEndpoint\":\"unix:///var/run/docker.sock\"}\n")
    } else {
        ok("{\"ServerVersion\":\"29.1.0\",\"OperatingSystem\":\"Debian GNU/Linux 13 (trixie)\",\"Name\":\"host\"}\n")
    }
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn the_setups_plan_is_printed_and_nothing_runs_in_a_dry_run() {
    let t = TestApp::new();
    let (tools, installed) = bare_linux(false);
    let ran = cyberctf(&t, &["machine", "setup", "--for", "docker", "--dry-run", "--json"]).await;
    assert_eq!(ran.exit, Exit::Ok, "{:?}", ran.lines);
    let data = &ran.last()["data"];
    assert_eq!(data["ran"], false);
    let steps = data["plan"]["steps"].as_array().unwrap();
    assert_eq!((&steps[0]["item"], &steps[0]["kind"]), (&json!("docker"), &json!("install")), "{steps:?}");
    assert!(steps[0]["does"].as_str().unwrap().starts_with("would install "), "{steps:?}");
    assert!(!installed.get() && !tools.called("pkexec"), "{:?}", tools.calls());
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn a_failing_setup_step_stops_the_setup_with_its_name_its_output_and_the_setups_id() {
    let t = TestApp::new();
    let (_tools, installed) = bare_linux(true);
    let ran = cyberctf(&t, &["machine", "setup", "--for", "docker", "--json"]).await;
    assert_eq!(ran.exit, Exit::Usage, "off a terminal it needs --yes");
    assert!(!installed.get());
    // On a terminal with --yes: the prompts' elevation (pkexec), which the fakes answer.
    let tty = on_a_terminal(&t, &["machine", "setup", "--for", "docker", "--yes", "--json"]).await;
    assert_eq!(tty.exit, Exit::Failed, "{:?}", tty.lines);
    let error = &tty.last()["error"];
    let message = error["message"].as_str().unwrap();
    assert!(message.starts_with("step docker failed: "), "{message}");
    assert!(message.contains("E: Unable to locate package docker-ce"), "the output's tail: {message}");
    assert!(error["operation"].as_str().unwrap().starts_with("setup-"), "{error}");
    let events = tty.events();
    let kinds: Vec<(&str, &str)> = events.iter().map(|e| (e["event"].as_str().unwrap(), e["item"].as_str().unwrap())).collect();
    assert_eq!(kinds.first(), Some(&("started", "docker")), "{kinds:?}");
    assert_eq!(kinds.last(), Some(&("ended", "docker")), "it stopped there: {kinds:?}");
    assert_eq!(events.last().unwrap()["outcome"], "failed");
    assert!(events.iter().all(|e| e["operation"] == error["operation"]), "every event carries the setup's id");
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn an_unattended_setup_that_cant_elevate_is_not_ready_before_anything_runs() {
    if crate::platform::elevation::is_root() {
        return;
    }
    let t = TestApp::new();
    let (tools, installed) = bare_linux(false);
    let ran = cyberctf(&t, &["machine", "setup", "--for", "docker", "--yes", "--json"]).await;
    assert_eq!(ran.exit, Exit::NotReady, "{:?}", ran.lines);
    assert!(ran.last()["error"]["message"].as_str().unwrap().starts_with("Nothing can be installed unattended here"), "{:?}", ran.last());
    assert!(tools.called("sudo -n true"), "{:?}", tools.calls());
    assert!(!installed.get() && !tools.called("pkexec"), "no install ran: {:?}", tools.calls());
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn the_setup_installs_through_its_steps_and_a_rerun_finds_nothing_to_do() {
    let t = TestApp::new();
    let (tools, installed) = bare_linux(false);
    let Ran { exit, lines, .. } = on_a_terminal(&t, &["machine", "setup", "--for", "docker", "--yes", "--json"]).await;
    assert!(installed.get(), "the install step ran: {:?}", tools.calls());
    let ended: Vec<&Value> = lines.iter().filter(|l| l["event"]["event"] == "ended").map(|l| &l["event"]).collect();
    assert_eq!((&ended[0]["item"], &ended[0]["outcome"]), (&json!("docker"), &json!("done")), "{lines:?}");
    // Done, or stopped before a new login the docker group needs: either way, nothing failed.
    assert!(matches!(exit, Exit::Ok | Exit::NotReady), "{exit:?}: {lines:?}");
    // Run again: Docker is there and runs, so the plan has no install left.
    let again = cyberctf(&t, &["machine", "setup", "--for", "docker", "--dry-run", "--json"]).await;
    let steps = again.last()["data"]["plan"]["steps"].as_array().unwrap().clone();
    assert!(steps.iter().all(|s| s["kind"] != "install"), "{steps:?}");
}
