//! The services the flag contract runs against, local and started for the test only:
//! CyberBackend's e2e server (`test/e2e/server.ts`: the real API on a throwaway Neo4j it starts and
//! removes, its own signing key, test players' tokens it signs itself) and cyberctf-web's opt-in
//! submission test (`test/e2e/submit-objective.e2e.test.ts`: the lab page's real server action).
//! Never production: no production URL or player token is ever used here.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex, PoisonError};

use serde::{Deserialize, Serialize};

/// A Node tool of a checkout (`node_modules/.bin/<name>`, `.cmd` on Windows).
fn node_tool(checkout: &Path, name: &str) -> std::path::PathBuf {
    checkout.join("node_modules").join(".bin").join(if cfg!(windows) { format!("{name}.cmd") } else { name.to_owned() })
}

/// What the backend says once it serves.
#[derive(Deserialize)]
struct Ready {
    url: String,
    players: BTreeMap<String, String>,
    labs: BTreeMap<String, String>,
}

/// CyberBackend's e2e server, running until dropped.
pub(super) struct Backend {
    child: Child,
    ready: Ready,
    /// Everything it printed after it was ready (its log), for the leak check.
    output: Arc<Mutex<String>>,
}

impl Backend {
    /// Starts the server of the CyberBackend checkout `checkout`, and waits until it serves.
    pub(super) fn start(checkout: &Path) -> Backend {
        let mut child = Command::new(node_tool(checkout, "tsx"))
            .arg("test/e2e/server.ts")
            .current_dir(checkout)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap_or_else(|e| panic!("start CyberBackend's e2e server in {}: {e}", checkout.display()));
        let output = Arc::new(Mutex::new(String::new()));
        let mut stdout = BufReader::new(child.stdout.take().unwrap());
        let mut first = String::new();
        stdout.read_line(&mut first).unwrap();
        collect(child.stderr.take().unwrap(), &output);
        let ready: Ready = serde_json::from_str(&first).unwrap_or_else(|e| {
            let _ = child.kill();
            panic!("CyberBackend's e2e server didn't start ({e}): {first}{}", read_all(&output))
        });
        collect(stdout, &output);
        Backend { child, ready, output }
    }

    /// The API's origin (`http://127.0.0.1:<port>`).
    pub(super) fn url(&self) -> &str {
        &self.ready.url
    }

    /// Test player `name`'s access token.
    pub(super) fn token(&self, name: &str) -> &str {
        self.ready.players.get(name).unwrap_or_else(|| panic!("no test player {name}"))
    }

    /// The id of the published lab `name`.
    pub(super) fn lab(&self, name: &str) -> &str {
        self.ready.labs.get(name).unwrap_or_else(|| panic!("no test lab {name}"))
    }

    /// What it printed since it was ready.
    pub(super) fn output(&self) -> String {
        read_all(&self.output)
    }
}

impl Drop for Backend {
    /// Closing its input stops it: it removes its database's container, then exits.
    fn drop(&mut self) {
        drop(self.child.stdin.take());
        let _ = self.child.wait();
    }
}

/// Appends what `stream` prints to `into`, on its own thread, until it closes.
fn collect(stream: impl Read + Send + 'static, into: &Arc<Mutex<String>>) {
    let into = Arc::clone(into);
    std::thread::spawn(move || {
        for line in BufReader::new(stream).lines().map_while(std::result::Result::ok) {
            let mut all = into.lock().unwrap_or_else(PoisonError::into_inner);
            all.push_str(&line);
            all.push('\n');
        }
    });
}

fn read_all(text: &Arc<Mutex<String>>) -> String {
    text.lock().unwrap_or_else(PoisonError::into_inner).clone()
}

/// What the backend must answer to one submission from the lab page.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum Expect {
    Correct,
    Wrong,
}

/// One answer the player submits from the lab page.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Submission {
    pub(super) lab_id: String,
    pub(super) key: String,
    pub(super) answer: String,
    pub(super) expect: Expect,
}

/// Submits `submissions` as player `token` through cyberctf-web's lab page action (its opt-in e2e
/// test, in checkout `web`) to the backend at `url`: whether every answer was judged as expected,
/// and the test's output.
pub(super) fn submit_from_the_lab_page(web: &Path, url: &str, token: &str, submissions: &[Submission]) -> (bool, String) {
    let out = Command::new(node_tool(web, "vitest"))
        .args(["run", "test/e2e/submit-objective.e2e.test.ts"])
        .current_dir(web)
        .env("CYBERCTF_E2E_BACKEND_URL", url)
        .env("CYBERCTF_E2E_TOKEN", token)
        .env("CYBERCTF_E2E_SUBMISSIONS", serde_json::to_string(submissions).unwrap())
        .stdin(Stdio::null())
        .output()
        .unwrap_or_else(|e| panic!("run cyberctf-web's e2e test in {}: {e}", web.display()));
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    (out.status.success(), text)
}
