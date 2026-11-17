use super::*;
use crate::error::{Code, Wire};
use crate::testing::test_app::TestApp;

/// The labs the store keeps a failure for.
fn failed(t: &TestApp) -> Vec<String> {
    crate::store::open(&t.dirs()).unwrap().read(crate::store::autostop::failed).unwrap()
}

/// Keeps `failures` (lab, error) as a sweep would.
fn record(t: &TestApp, failures: &[(&str, &Wire)]) {
    let rows: Vec<(String, String, Wire)> =
        failures.iter().map(|(id, w)| ((*id).to_owned(), format!("Auto-stop couldn't destroy {id}"), (*w).clone())).collect();
    crate::store::open(&t.dirs()).unwrap().write(|tx| crate::store::autostop::replace(tx, &rows)).unwrap();
}

#[test]
fn a_recorded_reap_failure_is_read_back_for_the_lab_page() {
    // What a sweep couldn't destroy is kept per lab and shown on its page, so a lab billing past
    // its auto-stop never looks stopped.
    let t = TestApp::new();
    let wire = Wire { code: Code::Other, message: "the token has expired".into() };
    record(&t, &[("web", &wire)]);
    assert_eq!(last_failure(&t.dirs(), "web"), Some(wire));
    assert_eq!(last_failure(&t.dirs(), "other"), None, "a lab that never failed has nothing");
}

#[test]
fn a_sweeps_failures_replace_the_last_ones() {
    // The next sweep reads these before replacing them, so a lab that fails again isn't notified
    // twice and a lab destroyed since has its record go.
    let t = TestApp::new();
    let w = Wire { code: Code::Other, message: "x".into() };
    record(&t, &[("a", &w), ("b", &w)]);
    assert_eq!(failed(&t), ["a", "b"]);
    record(&t, &[("b", &w)]);
    assert_eq!(failed(&t), ["b"]);
}

#[test]
fn a_lab_id_must_be_a_plain_name_to_be_a_record_file() {
    let t = TestApp::new();
    assert_eq!(last_failure(&t.dirs(), "../secrets"), None, "a path is never read as a record");
}

#[tokio::test]
async fn a_sweep_of_a_fresh_app_destroys_nothing_and_records_nothing() {
    // No launcher deployment to hand over, no cloud run past its auto-stop: a clean no-op. The
    // runner-backed destroy of an expired cloud lab is covered by the fake-CLI and integration tests.
    let t = TestApp::new();
    reap_expired_labs(&t.dirs(), &|_: &str, _: &str| {}).await;
    assert_eq!(failed(&t), [] as [String; 0]);
}

/// A cloud run the runner lists as expired, as it records one: `cyberctf-<lab>`, started on
/// account `host`, its stop time long past, its module in `module`.
fn expired_record(home: &std::path::Path, lab: &str, host: Option<&str>, module: &std::path::Path) {
    let mut record = serde_json::json!({
        "version": 0, "name": format!("cyberctf-{lab}"), "project": module,
        "format": "terraform", "cloud": "aws", "state": "ready", "expires_at": 1,
    });
    if let Some(host) = host {
        record["host_label"] = host.into();
    }
    crate::testing::runner::save_run(home, &record);
}

/// A runner home served on scripted tools: Terraform answers every call well and records the
/// destroys it is asked (with their environment), and each `(lab, host)` is an expired cloud
/// run of a module in the home's folder.
struct Expired {
    root: crate::testing::TempDir,
    home: std::path::PathBuf,
    fake: std::sync::Arc<isothrum_runner::fake::FakeProcesses>,
}

impl Expired {
    fn new(runs: &[(&str, Option<&str>)]) -> Expired {
        use isothrum_runner::fake::{FakeProcesses, Reply};
        let root = crate::testing::temp_dir("reap");
        let home = root.join("isothrum");
        let fake = std::sync::Arc::new(FakeProcesses::model(|call| match call.args.first().map(String::as_str) {
            Some("version") => Reply::ok(r#"{"terraform_version":"1.10.5"}"#),
            Some("show") => Reply::ok(r#"{"values":{"root_module":{"resources":[]}}}"#),
            Some("output") => Reply::ok("{}"),
            _ => Reply::ok(""),
        }));
        let mut config = isothrum_runner::RunnerConfig::new(&home);
        config.processes = std::sync::Arc::clone(&fake) as std::sync::Arc<dyn isothrum_runner::ProcessRunner>;
        for (lab, host) in runs {
            let module = root.join(lab);
            std::fs::create_dir_all(&module).unwrap();
            std::fs::write(module.join("main.tf"), "variable \"region\" {\n  default = \"eu-west-3\"\n}\n").unwrap();
            expired_record(&home, lab, *host, &module);
        }
        crate::testing::service::serve_home(config);
        Expired { root, home, fake }
    }

    /// The Terraform destroys run, each by where it ran and its line, with its environment.
    fn destroys(&self) -> Vec<(String, Vec<(String, String)>)> {
        self.fake
            .calls()
            .into_iter()
            .filter(|c| c.program == "terraform" && c.args.iter().any(|a| a == "destroy" || a == "-destroy"))
            .map(|c| (format!("{} {}", c.cwd.as_ref().map(|d| d.display().to_string()).unwrap_or_default(), c.line()), c.env.clone()))
            .collect()
    }
}

/// L-9's proof: the sweep lists the expired runs through the runner service and destroys each
/// with the account's credentials from the keychain (`account`), as secrets, never with what the
/// service could replay alone. A real expired cloud account is untested here.
#[tokio::test]
async fn an_expired_cloud_run_is_destroyed_through_the_service_with_its_accounts_credentials() {
    let x = Expired::new(&[("web", Some("aws1"))]);
    let account = |host: &str| (host == "aws1").then(|| vec![("AWS_ACCESS_KEY_ID".to_owned(), "AKIA-KEYCHAIN".to_owned())]);
    let failed = sweep(&x.home, account, |_| false).await.unwrap();
    assert!(failed.is_empty(), "{failed:?}");
    let destroys = x.destroys();
    assert!(!destroys.is_empty(), "terraform destroyed it: {:?}", x.fake.calls().iter().map(isothrum_runner::fake::Call::line).collect::<Vec<_>>());
    assert!(
        destroys.iter().all(|(_, env)| env.iter().any(|(k, v)| k == "AWS_ACCESS_KEY_ID" && v == "AKIA-KEYCHAIN")),
        "with the account's credentials: {destroys:?}"
    );
    let state = crate::testing::runner::run_state(&x.home, "cyberctf-web");
    assert_eq!(state, Some(isothrum_runner::RunState::Absent), "the runner records it gone");
    assert!(sweep(&x.home, |_: &str| None, |_| false).await.unwrap().is_empty(), "nothing left to sweep");
    drop(x.root);
}

/// An expired run whose account was removed since is left for a later sweep (the account may
/// come back), neither destroyed nor reported, and the sweep goes on with the others; a busy lab
/// is not even looked up; a run with no account recorded is reported with its code.
#[tokio::test]
async fn a_sweep_skips_a_gone_account_and_a_busy_lab_and_reports_a_run_with_no_account() {
    let x = Expired::new(&[("orphan", Some("gone1")), ("busy", Some("aws1")), ("nohost", None), ("web", Some("aws1"))]);
    let asked = std::sync::Mutex::new(Vec::new());
    let account = |host: &str| {
        asked.lock().unwrap().push(host.to_owned());
        (host == "aws1").then(|| vec![("AWS_ACCESS_KEY_ID".to_owned(), "AKIA".to_owned())])
    };
    let busy = |id: &str| id == "busy";
    let failed = sweep(&x.home, account, busy).await.unwrap();
    let codes: Vec<(&str, Code)> = failed.iter().map(|(name, e)| (name.as_str(), e.code())).collect();
    assert_eq!(codes, [("cyberctf-nohost", Code::ReapNoHost)]);
    assert_eq!(asked.into_inner().unwrap(), ["gone1", "aws1"], "every free run's account, never the busy one's");
    let destroyed: Vec<String> = x.destroys().into_iter().map(|(at, _)| at).collect();
    assert!(!destroyed.is_empty() && destroyed.iter().all(|d| d.contains("web") && !d.contains("orphan") && !d.contains("busy")), "{destroyed:?}");
    for kept in ["cyberctf-orphan", "cyberctf-busy", "cyberctf-nohost"] {
        assert!(crate::testing::runner::is_recorded(&x.home, kept), "{kept} keeps its record");
    }
    drop(x.root);
}
