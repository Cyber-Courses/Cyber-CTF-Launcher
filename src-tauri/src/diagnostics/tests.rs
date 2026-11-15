//! The bundle's contents and its redaction, from a data folder, log folder, settings and machine
//! check seeded with secrets in every source.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::{Sources, write};
use crate::platform::AppDirs;

/// A lab's spec, as small as one reads.
const SPEC: &str = "version: 1\nname: invoice-portal\nnetworks:\n  lab: { cidr: 10.30.0.0/24 }\nmachines:\n  web:\n    networks: { lab: 10 }\n    app: { image: nginx:1.27 }\n";

/// The launch token a running job carries (no pattern takes it: only its value does).
const LAUNCH_TOKEN: &str = "ltk-9f8e7d6c5b4a3921";
/// A flag a running job gives its lab, of no flag shape.
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
        Seeded::file(self.data.join("deploys/web.status"), "ok");
        Seeded::file(self.data.join("deploys/web.log"), &format!("==> Starting\n{line}✓ Lab is running\n"));
        Seeded::file(self.data.join("deploys/api.status"), r#"error: {"code":"portInUse","message":"Host port 8000 is already in use"}"#);
        let job = json!({ "lab_id": "api", "op": "launch", "launch": { "labId": "api", "env": [{ "name": "CTF_LAUNCH_TOKEN", "value": LAUNCH_TOKEN }, { "name": "CTF_FLAG_MAIN", "value": JOB_FLAG }] } });
        Seeded::file(self.data.join("deploys/api.json"), &job.to_string());
        Seeded::file(self.data.join("deploys/api.log"), &line);
        let host = json!({ "id": "srv1", "name": "Garage Proxmox", "provider": "proxmox", "host": "192.0.2.44", "port": 8006, "username": "root@pam" });
        let host: crate::runtime::server::profile::HostProfile = serde_json::from_value(host).unwrap();
        crate::runtime::server::store::update(&self.sources().dirs, |s| {
            s.hosts.push(host);
            s.default = Some("srv1".into());
            Ok(())
        })
        .unwrap();
        Seeded::file(self.logs.join("cyberctf.log"), &format!("[info] Cyber CTF app starting\n{line}"));
        Seeded::file(self.logs.join("deploy-api.log"), &line);
    }

    fn sources(&self) -> Sources {
        let settings = BTreeMap::from([
            ("attackImage".to_owned(), "cyberctf/attack-box".to_owned()),
            ("vmProvider".to_owned(), "qemu".to_owned()),
            ("sharedFolder".to_owned(), self.home.join("CyberCTF/shared").display().to_string()),
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
        let report = json!({ "os": "linux", "docker": { "installed": true, "version": "27.1" }, "ovftoolLocation": { "path": self.home.join("bin/ovftool").display().to_string() }, "hint": self.secret_line() });
        let out = self.home.join("diagnostics.tar.gz");
        write(&self.sources(), &report, &out).unwrap();
        unpack(&out)
    }
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
    assert!(log.contains("~/notes.txt") && log.contains("[redacted]"), "the line is kept, its secrets replaced: {log}");
    assert!(files["host-report.json"].contains("~/bin/ovftool"), "{}", files["host-report.json"]);
}

#[test]
fn the_bundle_holds_the_summary_the_report_and_the_logs() {
    let files = Seeded::new().bundle();
    let names: Vec<&str> = files.keys().map(String::as_str).collect();
    for expected in ["README.txt", "summary.json", "host-report.json", "logs/cyberctf.log", "logs/deploy-api.log", "operations/web.log", "operations/api.log"] {
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
    assert_eq!(summary["settings"]["window"]["sharedFolder"], "~/CyberCTF/shared");
    assert_eq!(summary["settings"]["servers"], json!([{ "provider": "proxmox", "default": true }]), "a server's kind only");
    assert_eq!(
        summary["labs"],
        json!([
            { "id": "local-notes", "slug": "notes-app", "commit": null, "imported": true },
            { "id": "web", "slug": "invoice-portal", "commit": "0123abcd", "imported": false },
        ])
    );
    let operations: BTreeMap<String, (Value, Value)> =
        summary["operations"].as_array().unwrap().iter().map(|o| (o["id"].as_str().unwrap().to_owned(), (o["outcome"].clone(), o["code"].clone()))).collect();
    assert_eq!(operations["web"], (json!("succeeded"), Value::Null));
    assert_eq!(operations["api"], (json!("failed"), json!("portInUse")));
    assert_eq!(summary["problems"], json!([]), "everything could be read");
}

#[test]
fn an_empty_data_folder_still_makes_a_bundle() {
    let root = tempfile::tempdir().unwrap();
    let sources = Sources { dirs: AppDirs::new(Some(root.path().join("data")), None), log_dir: None, home: None, settings: BTreeMap::new() };
    let out = root.path().join("d.tar.gz");
    write(&sources, &json!({}), &out).unwrap();
    let files = unpack(&out);
    assert_eq!(files.keys().map(String::as_str).collect::<Vec<_>>(), ["README.txt", "host-report.json", "summary.json"]);
    let summary: Value = serde_json::from_str(&files["summary.json"]).unwrap();
    assert_eq!((summary["labs"].clone(), summary["operations"].clone()), (json!([]), json!([])));
}

#[test]
fn operations_are_listed_newest_first_and_capped() {
    use super::collect::{Operation, newest};
    let op = |id: &str, at: Option<&str>| Operation {
        source: "launcher",
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
