//! The `cyberctf` executable itself, as a script runs it: a data folder of its own, no window
//! system (no `DISPLAY`, no `WAYLAND_DISPLAY`), no tools on its PATH. Its JSON lines' shape and its
//! exit codes, and (Linux) that it links no GUI library: it must run where no display is.
#![allow(clippy::unwrap_used, clippy::panic, reason = "a test's helpers fail it by panicking, as its assertions do")]

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};

/// A folder of the test's own, removed when it drops.
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
    assert!(Path::new(env!("CARGO_BIN_EXE_cyberctf")).is_file());
}
