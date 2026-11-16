//! The `cyberctf` executable itself, as a script runs it: a data folder of its own, no window
//! system (no `DISPLAY`, no `WAYLAND_DISPLAY`), no tools on its PATH. Its JSON lines' shape and its
//! exit codes, and (Linux) that it links no GUI library: it must run where no display is.
#![allow(clippy::unwrap_used, clippy::panic, reason = "a test's helpers fail it by panicking, as its assertions do")]
#![expect(clippy::disallowed_methods, reason = "it runs the executable it tests, as a script does")]

use std::path::PathBuf;
use std::process::Command;

use serde_json::{Value, json};

/// A folder of the test's own, removed when it drops, the runner service the command line started
/// for its data folder shut down first: detached, it would serve a deleted folder until it idled
/// out, minutes after the test.
struct Scratch(PathBuf);

impl Scratch {
    /// A new, empty folder.
    fn new(tag: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!("cyberctf-cli-{tag}-{}-{}", std::process::id(), rand_suffix()));
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        Scratch(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let home = self.0.join("data").join("isothrum");
        if let Ok(runtime) = tokio::runtime::Runtime::new() {
            runtime.block_on(async {
                if let Ok(client) = isothrum_client::Client::connect_running(&home).await {
                    let _ = client.shutdown().await;
                }
            });
        }
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Some digits telling this test's folders from another run's.
fn rand_suffix() -> u128 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos())
}

/// What a run of `cyberctf` said.
struct Ran {
    code: Option<i32>,
    lines: Vec<Value>,
    stderr: String,
}

/// Runs `cyberctf <args>` on `scratch`'s data folder, with no display and an empty PATH.
fn cyberctf(scratch: &Scratch, args: &[&str]) -> Ran {
    let out = Command::new(env!("CARGO_BIN_EXE_cyberctf"))
        .args(args)
        .env_clear()
        .env("PATH", scratch.0.join("bin"))
        .env("HOME", &scratch.0)
        .env("USERPROFILE", &scratch.0)
        .env("CYBERCTF_DATA_DIR", scratch.0.join("data"))
        .output()
        .unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    let lines = stdout.lines().filter(|l| l.starts_with('{')).map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("{e}: {l}"))).collect();
    Ran { code: out.status.code(), lines, stderr: String::from_utf8(out.stderr).unwrap() }
}

/// Installs lab `id` with `spec` in `scratch`'s data folder.
fn install(scratch: &Scratch, id: &str, spec: &str) -> PathBuf {
    let dir = scratch.0.join("data").join("labs").join(id);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("isoloom.yml"), spec).unwrap();
    dir
}

#[test]
fn it_answers_in_versioned_json_lines_with_no_display() {
    let s = Scratch::new("version");
    let ran = cyberctf(&s, &["version", "--json"]);
    assert_eq!(ran.code, Some(0), "{}", ran.stderr);
    assert_eq!(ran.lines.len(), 1);
    let line = &ran.lines[0];
    assert_eq!((&line["v"], &line["type"], &line["command"]), (&json!(1), &json!("result"), &json!("version")));
    assert_eq!(line["data"]["version"], env!("CARGO_PKG_VERSION"));
}

#[test]
fn its_exit_codes_tell_usage_from_not_ready() {
    let s = Scratch::new("exits");
    assert_eq!(cyberctf(&s, &["labs", "fly"]).code, Some(2));
    let ran = cyberctf(&s, &["labs", "list", "--json"]);
    assert_eq!((ran.code, &ran.lines[0]["data"]), (Some(0), &json!([])));
    let ran = cyberctf(&s, &["labs", "show", "gone", "--runtime", "docker", "--json"]);
    assert_eq!(ran.code, Some(3));
    assert_eq!(ran.lines[0]["error"]["message"], "lab `gone` is not installed");
    let dir = install(&s, "web", "version: 1\nname: web\nmachines:\n  web: { app: { image: nginx:1.27 } }\n");
    // Destructive, off a terminal: refused without --yes, the lab left.
    let ran = cyberctf(&s, &["labs", "remove", "web", "--json"]);
    assert_eq!((ran.code, &ran.lines[0]["exit"]), (Some(2), &json!(2)));
    assert!(dir.is_dir());
    let ran = cyberctf(&s, &["labs", "remove", "web", "--yes", "--json"]);
    assert_eq!(ran.code, Some(0), "{:?} {}", ran.lines, ran.stderr);
    assert!(!dir.exists());
}

#[test]
fn the_setups_plan_runs_with_no_tool_on_path() {
    let s = Scratch::new("setup");
    let ran = cyberctf(&s, &["machine", "setup", "--for", "docker", "--dry-run", "--json"]);
    assert_eq!(ran.code, Some(0), "{}", ran.stderr);
    let plan = &ran.lines[0]["data"]["plan"];
    assert!(plan["steps"].is_array() && plan["items"].as_array().is_some_and(|i| i.iter().any(|i| i == "docker")), "{plan}");
}

/// The executable must start where no window system is: on Linux it links no GTK nor web view
/// (`ldd`), which would make the loader refuse it on a server without them.
#[cfg(target_os = "linux")]
#[test]
fn it_links_no_gui_library() {
    let ldd = Command::new("ldd").arg(env!("CARGO_BIN_EXE_cyberctf")).output().unwrap();
    let libs = String::from_utf8(ldd.stdout).unwrap();
    for gui in ["gtk", "gdk", "webkit", "javascriptcore", "soup", "cairo", "pango", "X11", "wayland"] {
        assert!(!libs.to_lowercase().contains(&gui.to_lowercase()), "cyberctf links {gui}:\n{libs}");
    }
    assert!(std::path::Path::new(env!("CARGO_BIN_EXE_cyberctf")).is_file());
}

/// A Docker the test scripts, on the scratch PATH (which holds nothing else, so it uses the
/// shell's builtins only): Compose's answers for one project with `web` (port 80 published) and
/// Isoloom's probe, up after an `up` and gone after a `down`, kept in `state`; every call
/// appended to `calls`.
#[cfg(unix)]
fn scripted_docker(scratch: &Scratch) {
    use std::os::unix::fs::PermissionsExt;
    let dir = scratch.0.display();
    let script = format!(
        r#"#!/bin/sh
echo "$*" >>"{dir}/calls"
state=down; [ -f "{dir}/state" ] && read -r state <"{dir}/state"
case " $* " in
  *" compose version"*) echo 2.29.7 ;;
  *" config --services"*) printf 'web\nisoloom-probe\n' ;;
  *" config --format json"*) echo '{{"services":{{"web":{{"ports":[{{"target":80,"host_ip":"127.0.0.1","published":"","protocol":"tcp"}}]}},"isoloom-probe":{{}}}},"networks":{{"lab":{{"name":"cyberctf-lab_lab"}}}}}}' ;;
  *" up "*) echo up >"{dir}/state" ;;
  *" down"*) echo down >"{dir}/state" ;;
  *" ps "*)
    if [ "$state" = up ]; then
      echo '{{"Service":"web","State":"running","Health":"","ExitCode":0,"Publishers":[{{"URL":"127.0.0.1","PublishedPort":41873,"TargetPort":80,"Protocol":"tcp"}}]}}'
      echo '{{"Service":"isoloom-probe","State":"running","Health":"","ExitCode":0,"Publishers":[]}}'
    fi ;;
  *" container ls"*) [ "$state" = up ] && echo c1 ;;
  " version "*|*" version --format"*) echo 27.3.1 ;;
  *" info"*) echo 27.3.1 ;;
esac
exit 0
"#
    );
    let docker = scratch.0.join("bin").join("docker");
    std::fs::write(&docker, script).unwrap();
    std::fs::set_permissions(&docker, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// A container lab installed here starts, reads as running, and stops, through the command line
/// alone (its runner service started from `cyberctf` itself), on a scripted Docker: what a script
/// on a server without the app does.
#[cfg(unix)]
#[test]
fn a_lab_starts_runs_and_stops_from_the_command_line_alone() {
    let s = Scratch::new("start");
    scripted_docker(&s);
    // A lab of the player's own, from its folder: installed and started, no catalogue, no sign-in.
    let folder = s.0.join("my-lab");
    std::fs::create_dir_all(&folder).unwrap();
    let spec = "version: 1\nname: lab\nnetworks:\n  lab: { cidr: 10.44.0.0/24 }\nmachines:\n  web:\n    networks: { lab: 10 }\n    ports: { site: { port: 80, protocol: http, publish: 8080 } }\n    app: { image: nginx:1.27 }\n";
    std::fs::write(folder.join("isoloom.yml"), spec).unwrap();
    let started = cyberctf(&s, &["labs", "install", &folder.display().to_string(), "--json"]);
    let calls = std::fs::read_to_string(s.0.join("calls")).unwrap_or_default();
    assert_eq!(started.code, Some(0), "{:?}\n{}\ndocker was asked:\n{calls}", started.lines, started.stderr);
    assert!(calls.contains(" up "), "Compose brought it up:\n{calls}");
    let id = started.lines.iter().find_map(|l| l["event"]["labId"].as_str()).unwrap_or_default().to_owned();
    assert!(id.starts_with("local-"), "{:?}", started.lines);

    let listed = cyberctf(&s, &["labs", "list", "--json"]);
    assert_eq!(listed.code, Some(0), "{}", listed.stderr);
    assert!(listed.lines[0]["data"].as_array().is_some_and(|l| l.iter().any(|lab| lab["id"] == id.as_str())), "{:?}", listed.lines);
    let status = cyberctf(&s, &["labs", "status", &id, "--json"]);
    assert_eq!(status.code, Some(0), "{}", status.stderr);
    assert_eq!(status.lines[0]["data"]["status"]["running"], true, "{:?}", status.lines);

    let stopped = cyberctf(&s, &["labs", "stop", &id, "--yes", "--json"]);
    assert_eq!(stopped.code, Some(0), "{:?}\n{}", stopped.lines, stopped.stderr);
    assert!(std::fs::read_to_string(s.0.join("calls")).unwrap().contains(" down"), "Compose took it down");
    let status = cyberctf(&s, &["labs", "status", &id, "--json"]);
    assert_eq!(status.lines[0]["data"]["status"]["running"], false, "{:?}", status.lines);
}
