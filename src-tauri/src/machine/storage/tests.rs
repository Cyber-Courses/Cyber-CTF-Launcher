//! Storage over the runner's record: what earlier launchers recorded taken in once, the attack
//! box's images by name, one row per image, and a clean-up that removes only the record's.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex, PoisonError};

use isothrum_runner::fake::{Call, FakeProcesses, Reply};

use super::*;
use crate::testing::test_app::TestApp;

/// Docker and Vagrant on this machine: `nginx:1.27` (1000 bytes) as 1.9 recorded it, `redis:7`
/// pulled again since (another id than recorded: the player's now), the attack box's image (3000
/// bytes, also tagged as a digest) and the player's own Kali; Vagrant holds `generic/alpine319`.
/// What `image rm` untags is gone after.
fn machine(removed: &Mutex<BTreeSet<String>>, call: &Call) -> Reply {
    let args: Vec<&str> = call.args.iter().map(String::as_str).collect();
    let gone = |r: &str| removed.lock().unwrap_or_else(PoisonError::into_inner).contains(r);
    match args.as_slice() {
        [.., "image", "inspect", "--format", "{{.Id}}", reference] if !gone(reference) => match *reference {
            "nginx:1.27" => Reply::ok("sha256:nginx"),
            "redis:7" => Reply::ok("sha256:new"),
            "cyberctf/attack-box:latest" | "cyberctf/attack-box@sha256:d" => Reply::ok("sha256:box"),
            "kalilinux/kali-rolling" => Reply::ok("sha256:kali"),
            _ => Reply::fail(1, &format!("Error: No such object: {reference}")),
        },
        [.., "image", "inspect", "--format", "{{.Size}}", id] => match *id {
            "sha256:nginx" if !gone("nginx:1.27") => Reply::ok("1000"),
            "sha256:box" if !gone("cyberctf/attack-box:latest") || !gone("cyberctf/attack-box@sha256:d") => Reply::ok("3000"),
            _ => Reply::fail(1, &format!("Error: No such object: {id}")),
        },
        [.., "image", "rm", reference] => {
            removed.lock().unwrap_or_else(PoisonError::into_inner).insert((*reference).to_owned());
            Reply::ok("")
        }
        ["box", "list", ..] => Reply::ok("1,,box-name,generic/alpine319\n1,,box-provider,virtualbox\n1,,box-version,4\n1,,box-architecture,amd64\n"),
        _ => Reply::ok(""),
    }
}

/// An app whose runner runs on [`machine`], and its runner's scripted tools.
fn app() -> (TestApp, Arc<FakeProcesses>) {
    let t = TestApp::new();
    let removed = Mutex::new(BTreeSet::new());
    let fake = Arc::new(FakeProcesses::model(move |call| machine(&removed, call)));
    let mut config = isothrum_runner::RunnerConfig::new(crate::runtime::paths::runner_home(&t.dirs()).unwrap());
    config.processes = Arc::clone(&fake) as Arc<dyn isothrum_runner::ProcessRunner>;
    crate::testing::service::serve_home(config);
    (t, fake)
}

/// The attack-box images the UI knows about, and the player's Kali.
fn extra() -> Vec<String> {
    ["cyberctf/attack-box:latest", "cyberctf/attack-box@sha256:d", "kalilinux/kali-rolling"].map(str::to_owned).to_vec()
}

#[tokio::test]
async fn storage_lists_and_cleans_only_the_apps_downloads_taken_in_from_what_earlier_launchers_recorded() {
    let (t, fake) = app();
    let earlier = r#"{"images": [{"reference": "nginx:1.27", "id": "sha256:nginx"}, {"reference": "redis:7", "id": "sha256:old"}],
        "boxes": [{"name": "generic/alpine319", "provider": "virtualbox", "version": "4"}]}"#;
    std::fs::write(t.data().join("storage.json"), earlier).unwrap();
    std::fs::write(t.data().join("storage.json.lock"), "").unwrap();

    let listed = machine_storage(t.dirs(), extra()).await.unwrap();
    let names = |items: &[StoredItem]| items.iter().map(|i| (i.name.clone(), i.bytes)).collect::<Vec<_>>();
    // Two references to the attack box's image are one download; redis:7 names another image now.
    assert_eq!(names(&listed.images), [("cyberctf/attack-box:latest".to_owned(), 3000), ("nginx:1.27".to_owned(), 1000)]);
    assert!(!t.data().join("storage.json").exists() && !t.data().join("storage.json.lock").exists(), "taken in once");

    assert_eq!(machine_storage_clean(t.dirs(), extra()).await.unwrap(), 4000);
    let removals: Vec<String> = fake.made("docker").iter().filter(|c| c.args.iter().any(|a| a == "rm")).filter_map(|c| c.args.last().cloned()).collect();
    assert!(removals.iter().all(|r| r.starts_with("cyberctf/attack-box") || r == "nginx:1.27"), "only the app's: {removals:?}");
    assert!(fake.made("vagrant").iter().any(|c| c.args.starts_with(&["box".to_owned(), "remove".to_owned()])), "the box it took in too");
    let after = machine_storage(t.dirs(), Vec::new()).await.unwrap();
    assert!(after.images.is_empty() && after.boxes.is_empty());
}

#[test]
fn only_the_attack_boxs_own_repository_is_the_apps_by_name() {
    let extra = ["cyberctf/attack-box@sha256:abc".to_string(), "docker.io/cyberctf/attack-box:latest".into(), "kalilinux/kali-rolling".into(), "-bad".into()];
    let names: Vec<String> = attack_box_images(&extra)
        .into_iter()
        .filter_map(|a| match a {
            DownloadToAdopt::Image { reference, image_id: None } => Some(reference),
            _ => None,
        })
        .collect();
    assert_eq!(names, ["cyberctf/attack-box@sha256:abc", "docker.io/cyberctf/attack-box:latest"]);
}
