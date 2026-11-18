//! The bundle's contents and its redaction, from a data folder, log folder, settings and machine
//! check seeded with secrets in every source.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::collect::{Operation, Runner};
use super::{Sources, write};
use crate::platform::AppDirs;

/// A lab's spec, as small as one reads.
const SPEC: &str = "version: 1\nname: invoice-portal\nnetworks:\n  lab: { cidr: 10.30.0.0/24 }\nmachines:\n  web:\n    networks: { lab: 10 }\n    app: { image: nginx:1.27 }\n";

/// The launch token a lab's run was launched with (no pattern takes it: only its value does).
const LAUNCH_TOKEN: &str = "ltk-9f8e7d6c5b4a3921";
/// A flag a lab's run was launched with, of no flag shape.
const JOB_FLAG: &str = "a1b2c3d4e5f60718";
/// The player's e-mail address.
const EMAIL: &str = "ada.lovelace@example.org";

/// Every secret seeded somewhere: none may be in the bundle.
const SECRETS: [&str; 9] = [
    LAUNCH_TOKEN,
    JOB_FLAG,
    EMAIL,
    "FLAG{seeded_in_the_log}",
    "eyJhbGciOiJSUzI1NiJ9.eyJzdWIiOiIxMjM0NSJ9.c2lnbmF0dXJl",
    "hunter2-proxmox-password",
    "AKIAIOSFODNN7EXAMPLE",
    "192.0.2.44",
    "Garage Proxmox",
];

/// A seeded computer: a home folder holding the app's data and log folders.
struct Seeded {
    _root: tempfile::TempDir,
    home: PathBuf,
    data: PathBuf,
    logs: PathBuf,
}

impl Drop for Seeded {
    fn drop(&mut self) {
        // The store this process keeps open on the data folder goes before the folder.
        crate::store::forget(&self.data);
        let _ = crate::runtime::launch_values::forget_lab("web");
    }
}

impl Seeded {
    fn new() -> Seeded {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home").join("ada");
        let (data, logs) = (home.join("app-data"), home.join("app-logs"));
        let seeded = Seeded { home, data, logs, _root: root };
        seeded.seed();
        seeded
    }

    fn file(path: PathBuf, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    /// A line naming every kind of secret, and a path under the home folder.
    fn secret_line(&self) -> String {
        format!(
            "user {EMAIL} token {LAUNCH_TOKEN} flag {JOB_FLAG} found FLAG{{seeded_in_the_log}} Authorization: Bearer eyJhbGciOiJSUzI1NiJ9.eyJzdWIiOiIxMjM0NSJ9.c2lnbmF0dXJl PROXMOX_PASSWORD=hunter2-proxmox-password key AKIAIOSFODNN7EXAMPLE at {}\n",
            self.home.join("notes.txt").display()
        )
    }

    fn seed(&self) {
        let line = self.secret_line();
        Seeded::file(self.data.join("labs/web/isoloom.yml"), SPEC);
        Seeded::file(self.data.join("labs/web/.cyberctf-commit"), "0123abcd\n");
        Seeded::file(self.data.join("labs/local-notes/isoloom.yml"), &SPEC.replace("invoice-portal", "notes-app"));
        // The values web's run was launched with, kept while it lasts (`runtime::launch_values`).
        let launched = [("CTF_LAUNCH_TOKEN", LAUNCH_TOKEN), ("CTF_FLAG_MAIN", JOB_FLAG)].map(|(n, v)| (n.to_owned(), v.to_owned()));
        crate::runtime::launch_values::keep("cyberctf-web", &launched);
        let runner_home = crate::runtime::paths::runner_home_in(&self.data);
        Seeded::file(runner_home.join("service.log"), &format!("isothrum serve: listening\n{line}"));
        let host = json!({ "id": "srv1", "name": "Garage Proxmox", "provider": "proxmox", "host": "192.0.2.44", "port": 8006, "username": "root@pam" });
        let host: crate::runtime::server::profile::HostProfile = serde_json::from_value(host).unwrap();
        crate::runtime::server::store::update(&self.sources().dirs, |s| {
            s.hosts.push(host);
            s.default = Some("srv1".into());
            Ok(())
        })
        .unwrap();
        Seeded::file(self.logs.join("cyberctf.log"), &format!("[info] Cyber CTF app starting\n{line}"));
    }

    fn sources(&self) -> Sources {
        let settings = BTreeMap::from([
            ("attackImage".to_owned(), "cyberctf/attack-box".to_owned()),
            ("vmProvider".to_owned(), "qemu".to_owned()),
            ("sharedFolder".to_owned(), self.home.join("CyberCTF").join("shared").display().to_string()),
            ("note".to_owned(), self.secret_line()),
        ]);
        Sources {
            dirs: AppDirs::new(Some(self.data.clone()), Some(self.data.join("caches"))),
            log_dir: Some(self.logs.clone()),
            home: Some(self.home.clone()),
            settings,
        }
    }

    /// The bundle written, as its files' paths and texts.
    fn bundle(&self) -> BTreeMap<String, String> {
        self.bundle_with(Ok(Runner::default()))
    }

    /// [`Seeded::bundle`] with what the runner service says of the runs (`runner`).
    fn bundle_with(&self, runner: crate::error::Result<Runner>) -> BTreeMap<String, String> {
        let report = json!({ "os": "linux", "docker": { "installed": true, "version": "27.1" }, "ovftoolLocation": { "path": self.home.join("bin").join("ovftool").display().to_string() }, "hint": self.secret_line() });
        let out = self.home.join("diagnostics.tar.gz");
        write(&self.sources(), &report, runner, &out).unwrap();
        unpack(&out)
    }
}

/// The path under the home folder made of `parts`, as the bundle writes it: the home as `~`, the
/// rest in this OS's separators (`~\notes.txt` on Windows), as the seeded paths were.
fn tilde(parts: &[&str]) -> String {
    parts.iter().fold(PathBuf::from("~"), |path, part| path.join(part)).display().to_string()
}

/// The files of the bundle at `path`, by their path inside its folder.
fn unpack(path: &Path) -> BTreeMap<String, String> {
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(std::fs::File::open(path).unwrap()));
    let mut files = BTreeMap::new();
    for entry in archive.entries().unwrap() {
        let mut entry = entry.unwrap();
        let name = entry.path().unwrap().to_string_lossy().into_owned();
        let mut text = String::new();
        entry.read_to_string(&mut text).unwrap();
        files.insert(name.strip_prefix("cyberctf-diagnostics/").unwrap_or_else(|| panic!("outside the bundle's folder: {name}")).to_owned(), text);
    }
    files
}

#[test]
fn no_seeded_secret_and_no_home_path_is_in_the_bundle() {
    let seeded = Seeded::new();
    let files = seeded.bundle();
    let home = seeded.home.display().to_string();
    for (name, text) in &files {
        for secret in SECRETS {
            assert!(!text.contains(secret), "{secret} is in {name}:\n{text}");
        }
        assert!(!text.contains(&home), "the home folder is in {name}:\n{text}");
    }
    let log = &files["logs/cyberctf.log"];
    assert!(log.contains(&tilde(&["notes.txt"])) && log.contains("[redacted]"), "the line is kept, its secrets replaced: {log}");
    let report: Value = serde_json::from_str(&files["host-report.json"]).unwrap();
    assert_eq!(report["ovftoolLocation"]["path"], tilde(&["bin", "ovftool"]), "{report}");
}

/// Saving a bundle never reads the store, where a run's launch values are kept (on macOS a
/// keychain read may be a prompt): it replaces those this process holds, and after a restart, with
/// web's values only in the store, it still asks nothing, and the shapes the logs' redaction knows
/// are still replaced.
#[test]
fn saving_a_bundle_reads_nothing_from_the_keychain() {
    use crate::runtime::{launch_values, server::secrets};
    let seeded = Seeded::new();
    let before = secrets::reads();
    let files = seeded.bundle();
    assert_eq!(secrets::reads(), before, "the bundle read the store");
    assert!(files.values().all(|text| !text.contains(LAUNCH_TOKEN) && !text.contains(JOB_FLAG)), "the values held here are replaced");

    launch_values::restart();
    let line = format!("CTF_LAUNCH_TOKEN={LAUNCH_TOKEN} CTF_FLAG_MAIN={JOB_FLAG}\n");
    Seeded::file(seeded.logs.join("cyberctf.log"), &line);
    let before = secrets::reads();
    let files = seeded.bundle();
    assert_eq!(secrets::reads(), before, "after a restart, the bundle read the store");
    assert_eq!(files["logs/cyberctf.log"], "CTF_LAUNCH_TOKEN=[redacted] CTF_FLAG_MAIN=[redacted]\n");
    assert!(secrets::get_named("launch-values:cyberctf-web").unwrap().is_some(), "web's values are still kept, for its resume");
}

#[test]
fn the_bundle_holds_the_summary_the_report_and_the_logs() {
    let files = Seeded::new().bundle();
    let names: Vec<&str> = files.keys().map(String::as_str).collect();
    for expected in ["README.txt", "summary.json", "host-report.json", "logs/cyberctf.log", "runner/service.log"] {
        assert!(names.contains(&expected), "{expected} missing from {names:?}");
    }
    let report: Value = serde_json::from_str(&files["host-report.json"]).unwrap();
    assert_eq!(report["docker"]["version"], "27.1");
    let summary: Value = serde_json::from_str(&files["summary.json"]).unwrap();
    assert_eq!(summary["bundle"], 1);
    assert_eq!(summary["app"]["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(summary["system"]["os"], std::env::consts::OS);
    assert_eq!(summary["system"]["arch"], std::env::consts::ARCH);
    assert_eq!(summary["settings"]["window"]["vmProvider"], "qemu");
    assert_eq!(summary["settings"]["window"]["sharedFolder"], tilde(&["CyberCTF", "shared"]));
    assert_eq!(summary["settings"]["servers"], json!([{ "provider": "proxmox", "default": true }]), "a server's kind only");
    assert_eq!(
        summary["labs"],
        json!([
            { "id": "local-notes", "slug": "notes-app", "commit": null, "imported": true },
            { "id": "web", "slug": "invoice-portal", "commit": "0123abcd", "imported": false },
        ])
    );
    assert_eq!(summary["operations"], json!([]), "the runner recorded none");
    assert_eq!(summary["problems"], json!([]), "everything could be read");
}

/// The runner service runs the labs' operations: the bundle holds each run's last one in the
/// summary and each lab's run's journal (what `cyberctf labs logs <lab> --json` lists), read from
/// a service that took web's run down, its launch values as secrets.
#[tokio::test]
async fn the_bundle_holds_the_runner_services_operations() {
    use isothrum_runner::fake::{FakeProcesses, Reply};
    let seeded = Seeded::new();
    let dirs = seeded.sources().dirs;
    let home = crate::runtime::paths::runner_home(&dirs).unwrap();
    let module = seeded.data.join("module-web");
    Seeded::file(module.join("main.tf"), "variable \"region\" {\n  default = \"eu-west-3\"\n}\n");
    let record = json!({ "version": 0, "name": "cyberctf-web", "project": module, "format": "terraform", "cloud": "aws", "state": "ready", "expires_at": 1 });
    crate::testing::runner::save_run(&home, &record);
    let mut config = isothrum_runner::RunnerConfig::new(&home);
    config.processes = std::sync::Arc::new(FakeProcesses::model(|call| match call.args.first().map(String::as_str) {
        Some("version") => Reply::ok(r#"{"terraform_version":"1.10.5"}"#),
        Some("show") => Reply::ok(r#"{"values":{"root_module":{"resources":[]}}}"#),
        Some("output") => Reply::ok("{}"),
        _ => Reply::ok(""),
    }));
    crate::testing::service::serve_home(config);
    let record = crate::runtime::isothrum::records(&home).await.unwrap().into_iter().find(|r| r.name.as_str() == "cyberctf-web").unwrap();
    let launched = [("CTF_LAUNCH_TOKEN".to_owned(), LAUNCH_TOKEN.to_owned())];
    crate::runtime::isothrum::destroy_expired(&home, &record, &launched).await.unwrap();

    let files = seeded.bundle_with(super::collect::runner_operations(&dirs).await);
    let journal: Value = serde_json::from_str(&files["operations/web.json"]).unwrap();
    let ops: Vec<(&Value, &Value)> = journal.as_array().unwrap().iter().map(|o| (&o["op"], &o["ended"]["outcome"])).collect();
    assert_eq!(ops, [(&json!("down"), &json!("succeeded"))], "{journal}");
    let summary: Value = serde_json::from_str(&files["summary.json"]).unwrap();
    let last = &summary["operations"][0];
    assert_eq!(
        (&last["source"], &last["id"], &last["lab"], &last["op"], &last["outcome"]),
        (&json!("isothrum"), &json!("cyberctf-web"), &json!("web"), &json!("down"), &json!("succeeded"))
    );
    assert_eq!(summary["problems"], json!([]), "everything could be read");
    for (name, text) in &files {
        for secret in SECRETS {
            assert!(!text.contains(secret), "{secret} is in {name}:\n{text}");
        }
    }
}

#[test]
fn an_empty_data_folder_still_makes_a_bundle() {
    let root = tempfile::tempdir().unwrap();
    let sources = Sources { dirs: AppDirs::new(Some(root.path().join("data")), None), log_dir: None, home: None, settings: BTreeMap::new() };
    let out = root.path().join("d.tar.gz");
    write(&sources, &json!({}), Ok(Runner::default()), &out).unwrap();
    let files = unpack(&out);
    assert_eq!(files.keys().map(String::as_str).collect::<Vec<_>>(), ["README.txt", "host-report.json", "summary.json"]);
    let summary: Value = serde_json::from_str(&files["summary.json"]).unwrap();
    assert_eq!((summary["labs"].clone(), summary["operations"].clone()), (json!([]), json!([])));
}

#[test]
fn operations_are_listed_newest_first_and_capped() {
    use super::collect::newest;
    let op = |id: &str, at: Option<&str>| Operation {
        source: "isothrum",
        id: id.into(),
        lab: id.into(),
        op: None,
        outcome: "succeeded",
        code: None,
        at: at.map(Into::into),
    };
    let listed = newest(vec![op("old", Some("2026-11-01T00:00:00Z")), op("none", None), op("new", Some("2026-11-14T00:00:00Z"))], 2);
    assert_eq!(listed.iter().map(|o| o.id.as_str()).collect::<Vec<_>>(), ["new", "old"]);
}
