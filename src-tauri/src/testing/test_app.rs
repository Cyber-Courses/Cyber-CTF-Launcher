//! An app for unit tests: Tauri's mock runtime with an app data folder of its own (a unique
//! identifier, removed when the test ends), so code taking an app handle runs in tests. An
//! integration scenario's app ([`TestApp::scenario`]) keeps its folders in the scenario's temp
//! folder when the integration runner names one ([`SCENARIO_ROOT`]).

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use tauri::Manager;
use tauri::ipc::{Channel, InvokeResponseBody};

use crate::AppHandle;

/// The folder an integration scenario's app keeps its folders in: the scenario's own temp
/// folder, which the integration runner (`tests/integration/run.sh`) sets here. Read by
/// [`TestApp::scenario`] alone; a unit test's app ([`TestApp::new`]) keeps the system's place.
pub const SCENARIO_ROOT: &str = "CYBERCTF_TEST_APP_ROOT";

pub struct TestApp {
    app: tauri::App<crate::AppRuntime>,
    data: PathBuf,
}

impl TestApp {
    pub fn new() -> TestApp {
        let mut context = tauri::test::mock_context(tauri::test::noop_assets());
        context.config_mut().identifier = format!("com.cyberctf.unit-test.{}-{}", std::process::id(), rand::random::<u64>());
        let app = tauri::test::mock_builder().build(context).expect("mock app");
        let data = app.path().app_data_dir().expect("app data dir");
        std::fs::create_dir_all(&data).unwrap();
        TestApp { app, data }
    }

    /// An integration scenario's app: [`TestApp::new`]'s, with every folder Tauri resolves under
    /// the folder [`SCENARIO_ROOT`] names, when the runner sets it. A lab's runs live in the app
    /// data folder (`runs/<lab>`: Vagrant's `.vagrant`, a VM's disks), so they are then in the
    /// scenario's temp folder, which the runner removes when the scenario ends, first ending what
    /// still runs from it and, past its cap, the VirtualBox VMs its `.vagrant` names. In the
    /// system's place (`~/Library/Application Support/<identifier>` on macOS) the runner never
    /// reached them: a capped scenario's VMware VMs kept running and its data stayed (#308).
    pub fn scenario() -> TestApp {
        TestApp::in_root(root_of(std::env::var_os(SCENARIO_ROOT)))
    }

    /// An app whose folders are all in `<root>/<identifier>` (its data, config and local data
    /// folder; its cache in `caches/`, its logs in `logs/`), removed when it drops; without a
    /// root, [`TestApp::new`]'s.
    fn in_root(root: Option<PathBuf>) -> TestApp {
        let Some(root) = root else { return TestApp::new() };
        let mut context = tauri::test::mock_context(tauri::test::noop_assets());
        let identifier = format!("com.cyberctf.unit-test.{}-{}", std::process::id(), rand::random::<u64>());
        context.config_mut().app.app_directories_override = Some(tauri::utils::config::AppDirectoriesOverride::Root(root.join(&identifier)));
        context.config_mut().identifier = identifier;
        let app = tauri::test::mock_builder().build(context).expect("mock app");
        let data = app.path().app_data_dir().expect("app data dir");
        std::fs::create_dir_all(&data).unwrap();
        TestApp { app, data }
    }

    pub fn handle(&self) -> &AppHandle {
        self.app.handle()
    }

    /// The app data folder.
    pub fn data(&self) -> &Path {
        &self.data
    }

    /// The app's folders, as the app shell resolves them.
    pub fn dirs(&self) -> crate::platform::AppDirs {
        crate::platform::AppDirs::from(self.handle())
    }

    /// An installed lab's folder, created, with `files` in it.
    pub fn lab(&self, id: &str, files: &[(&str, &str)]) -> PathBuf {
        let dir = self.data.join("labs").join(id);
        std::fs::create_dir_all(&dir).unwrap();
        for (name, body) in files {
            let path = dir.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, body).unwrap();
        }
        dir
    }

    /// Saves a server host (`provider` as its id, e.g. `azure`), returning its id. Hosts that
    /// keep no secret only (CLI sign-in clouds, `use_cli_creds` AWS): no keychain in tests.
    pub fn host(&self, id: &str, provider: &str, extra: serde_json::Value) -> String {
        let path = self.data.join("server.json");
        let mut store: serde_json::Value =
            std::fs::read_to_string(&path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(serde_json::json!({ "hosts": [] }));
        let mut host =
            serde_json::json!({ "id": id, "name": format!("Host {id}"), "provider": provider, "host": "eu-west-1", "port": 443, "username": "acct" });
        if let (Some(h), serde_json::Value::Object(e)) = (host.as_object_mut(), extra) {
            h.extend(e);
        }
        store["hosts"].as_array_mut().unwrap().push(host);
        std::fs::write(path, store.to_string()).unwrap();
        id.to_string()
    }
}

impl Drop for TestApp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.data);
        for dir in [self.app.path().app_cache_dir(), self.app.path().app_config_dir(), self.app.path().app_local_data_dir()].into_iter().flatten() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

/// The folder `value` names, only an absolute one: a relative path would follow the folder the
/// test binary runs in (cargo's, the crate's).
fn root_of(value: Option<OsString>) -> Option<PathBuf> {
    value.map(PathBuf::from).filter(|p| p.is_absolute())
}

/// What a command sent the UI over its channels, each message as the JSON the webview gets.
#[derive(Clone, Default)]
pub struct Sink(Arc<Mutex<Vec<serde_json::Value>>>);

impl Sink {
    /// A channel whose messages land here.
    pub fn channel<T>(&self) -> Channel<T> {
        let sink = self.0.clone();
        Channel::new(move |body| {
            if let InvokeResponseBody::Json(json) = body {
                sink.lock().unwrap().push(serde_json::from_str(&json).unwrap());
            }
            Ok(())
        })
    }

    /// A line callback whose lines land here, as a log channel's would.
    pub fn on_line(&self) -> impl FnMut(String) + Send + 'static {
        let sink = self.0.clone();
        move |line| sink.lock().unwrap().push(serde_json::Value::String(line))
    }

    /// Every message so far.
    pub fn messages(&self) -> Vec<serde_json::Value> {
        self.0.lock().unwrap().clone()
    }

    /// The text messages so far (a log channel's lines).
    pub fn lines(&self) -> Vec<String> {
        self.messages().iter().filter_map(|m| m.as_str().map(str::to_string)).collect()
    }
}

mod tests {
    use super::*;

    #[test]
    fn a_scenarios_app_keeps_every_folder_in_the_runners_root_and_they_go_when_it_drops() {
        // Regression (#308): the integration tests' app kept a lab's runs, its VMs' disks with
        // them, in ~/Library/Application Support, where a capped scenario's VMs kept running.
        let root = crate::testing::temp_dir("test-app-root");
        let t = TestApp::in_root(Some(root.to_path_buf()));
        let own = t.data().to_path_buf();
        assert_eq!(own.parent(), Some(&*root), "{}", own.display());
        assert_eq!(own.file_name().unwrap().to_string_lossy(), t.handle().config().identifier, "a folder of its own, named after the app");
        let paths = t.handle().path();
        assert_eq!((paths.app_config_dir().unwrap(), paths.app_local_data_dir().unwrap()), (own.clone(), own.clone()));
        assert_eq!((paths.app_cache_dir().unwrap(), paths.app_log_dir().unwrap()), (own.join("caches"), own.join("logs")));
        assert_eq!(t.dirs().data().unwrap(), own);
        let lab = t.lab("pm-1", &[("isoloom.yml", "version: 1\n")]);
        assert!(lab.starts_with(&*root), "a lab installed in the scenario's folder: {}", lab.display());
        let other = TestApp::in_root(Some(root.to_path_buf()));
        assert_ne!(other.data(), own, "each app its own folder");
        drop(t);
        assert!(!own.exists(), "removed when it drops");
        assert!(other.data().is_dir());
        // No root: a unit test's app, in the system's place.
        let unit = TestApp::in_root(None);
        assert!(!unit.data().starts_with(&*root), "{}", unit.data().display());
    }

    #[test]
    fn the_runners_root_is_taken_only_as_an_absolute_path() {
        let abs = std::env::temp_dir().join("cyberctf-it.AbC123");
        assert_eq!(root_of(Some(abs.clone().into_os_string())), Some(abs));
        assert_eq!(root_of(Some("cyberctf-it.AbC123".into())), None);
        assert_eq!(root_of(None), None);
    }
}
