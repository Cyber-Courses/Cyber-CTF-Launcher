//! An app for unit tests: Tauri's mock runtime with an app data folder of its own (a unique
//! identifier, removed when the test ends), so code taking an app handle runs in tests.

use std::path::{Path, PathBuf};

use tauri::Manager;

use crate::AppHandle;

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

    pub fn handle(&self) -> &AppHandle {
        self.app.handle()
    }

    /// The app data folder.
    pub fn data(&self) -> &Path {
        &self.data
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
        if let (Some(h), Some(e)) = (host.as_object_mut(), extra.as_object()) {
            h.extend(e.clone());
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
