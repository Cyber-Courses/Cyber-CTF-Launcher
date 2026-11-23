use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::*;
use crate::error::Code;
use crate::runtime::paths::runner_home_in;
use crate::runtime::providers::testing::no_tools;
use crate::runtime::test_support::record_run;
use crate::testing::runner::is_recorded;
use crate::testing::test_app::TestApp;

const SPEC: &str =
    "version: 1\nname: invoice\nnetworks:\n  lab: { cidr: 10.30.0.0/24 }\nmachines:\n  web:\n    networks: { lab: 10 }\n    app: { image: nginx:1.27 }\n";

/// Every file under `root` (links as links, not followed) with its contents: what a test holds a
/// folder to, before and after.
fn tree(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut out = BTreeMap::new();
    let mut todo = vec![root.to_path_buf()];
    while let Some(dir) = todo.pop() {
        for e in std::fs::read_dir(&dir).unwrap().flatten() {
            let (path, kind) = (e.path(), e.file_type().unwrap());
            let rel = path.strip_prefix(root).unwrap().to_path_buf();
            if kind.is_symlink() {
                out.insert(rel, std::fs::read_link(&path).unwrap().to_string_lossy().into_owned().into_bytes());
            } else if kind.is_dir() {
                out.insert(rel, Vec::new());
                todo.push(path);
            } else {
                out.insert(rel, std::fs::read(&path).unwrap());
            }
        }
    }
    out
}

/// A lab folder to import, outside the app's data folder.
fn source() -> PathBuf {
    let src = std::env::temp_dir().join(format!("cyberctf-remove-src-{}", rand::random::<u32>()));
    std::fs::create_dir_all(src.join("web")).unwrap();
    std::fs::write(src.join("isoloom.yml"), SPEC).unwrap();
    std::fs::write(src.join("web").join("index.html"), "<h1>mine</h1>").unwrap();
    std::fs::create_dir_all(src.join(".ctf")).unwrap();
    std::fs::write(src.join(".ctf").join("metadata.json"), r#"{ "title": "Supplier portal" }"#).unwrap();
    src
}

// Each test that removes through `remove` names its lab its own way: a removal holds the lab in
// its data folder's run service, by id, so two tests on one id would find each other's lab held.

/// Lab `id`'s records under the app data folder `base`, beside its copy: the runner's run (taken
/// down: `absent`), a kept offline source and leftover copies.
fn records(base: &Path, id: &str) {
    record_run(base, id, None, "docker", "absent");
    std::fs::create_dir_all(base.join("offline")).unwrap();
    std::fs::write(base.join("offline").join(format!("{id}.json")), "{}").unwrap();
    for leftover in [format!(".{id}.staging"), format!(".{id}.old")] {
        std::fs::create_dir_all(base.join("labs").join(leftover).join("x")).unwrap();
    }
}

/// Lab `id`'s leftover copies under `base` (`records`).
fn leftovers(base: &Path, id: &str) -> [PathBuf; 2] {
    [format!(".{id}.staging"), format!(".{id}.old")].map(|name| base.join("labs").join(name))
}

#[tokio::test]
async fn a_removal_deletes_the_apps_copy_and_records_and_never_the_folder_it_was_imported_from() {
    let t = TestApp::new();
    let _tools = no_tools();
    crate::testing::service::serve_silent(&crate::runtime::paths::runner_home(&t.dirs()).unwrap());
    let src = source();
    let dir = super::super::for_tests::reimport(&t.dirs(), "local-copy", &src).await.unwrap();
    // A link in the copy pointing at the source (never one the import makes: planted, the worst
    // case): the removal deletes the link, not what it points at.
    #[cfg(unix)]
    std::os::unix::fs::symlink(&src, dir.join("to-source")).unwrap();
    let before = tree(&src);
    records(t.data(), "local-copy");
    // Another lab, and its records: untouched.
    let other = t.lab("local-other", &[("isoloom.yml", SPEC)]);
    records(t.data(), "local-other");

    let removal = remove(&t.dirs(), "local-copy").await.unwrap();

    assert_eq!(removal, Removal::default(), "every trace went");
    let labs = t.data().join("labs");
    for gone in [
        dir.clone(),
        labs.join(".local-copy.removing"),
        labs.join(".local-copy.staging"),
        labs.join(".local-copy.old"),
        t.data().join("offline").join("local-copy.json"),
    ] {
        assert!(std::fs::symlink_metadata(&gone).is_err(), "{} is still there", gone.display());
    }
    assert_eq!(tree(&src), before, "the folder it was imported from is as it was");
    assert!(!is_recorded(&runner_home_in(t.data()), "cyberctf-local-copy"), "the runner forgot its run");
    assert!(other.join("isoloom.yml").is_file() && is_recorded(&runner_home_in(t.data()), "cyberctf-local-other"));
    assert!(leftovers(t.data(), "local-other").iter().all(|d| d.is_dir()) && t.data().join("offline").join("local-other.json").is_file());
    assert!(crate::labs::imported::list(&t.dirs()).unwrap().iter().all(|l| l.id != "local-copy"), "no longer listed");
    // Removed already: nothing left to remove.
    let again = remove(&t.dirs(), "local-copy").await.unwrap_err();
    assert_eq!(again.to_string(), "lab `local-copy` is not installed");
    let _ = std::fs::remove_dir_all(&src);
}

#[tokio::test]
async fn a_catalogue_lab_is_removed_the_same_way() {
    let t = TestApp::new();
    let id = "66d71cbf-9b4e-4a78-b3cf-8208227a96b2";
    let dir = t.lab(id, &[("isoloom.yml", SPEC), (runtime::lab::COMMIT_MARKER, "0123456789abcdef0123456789abcdef01234567")]);
    records(t.data(), id);
    assert_eq!(remove(&t.dirs(), id).await.unwrap(), Removal::default());
    assert!(!dir.exists() && !t.data().join("offline").join(format!("{id}.json")).exists() && leftovers(t.data(), id).iter().all(|d| !d.exists()));
}

#[tokio::test]
async fn a_removal_is_refused_while_the_lab_is_up_anywhere_parked_or_busy() {
    let t = TestApp::new();
    let dir = t.lab("local-refused", &[("isoloom.yml", SPEC)]);
    let refused = async |state: &str, host: Option<&str>| {
        record_run(t.data(), "local-refused", host, if host.is_some() { "vagrant" } else { "docker" }, state);
        remove(&t.dirs(), "local-refused").await.unwrap_err()
    };
    // Up here, being created, failed with its machines left, or parked: what the runner records.
    for state in ["ready", "creating", "failed", "parked"] {
        assert_eq!(refused(state, None).await.code(), Code::LabRunningHere, "{state}");
    }
    // Up on a server: named.
    assert_eq!(refused("ready", Some("esxi1")).await.code(), Code::LabRunningOn { host: "esxi1".into() });
    record_run(t.data(), "local-refused", None, "docker", "absent");
    // Another operation holds it.
    let held = crate::run_service::LocalRunService::of(&t.dirs()).unwrap().claim("local-refused").unwrap();
    assert_eq!(remove(&t.dirs(), "local-refused").await.unwrap_err().code(), Code::AlreadyInProgress);
    drop(held);
    // Through every refusal, the lab and its run's record stayed.
    assert!(dir.join("isoloom.yml").is_file() && is_recorded(&runner_home_in(t.data()), "cyberctf-local-refused"));
    // A bad id is refused before anything is read.
    assert_eq!(remove(&t.dirs(), "../x").await.unwrap_err().to_string(), "invalid lab id `../x`");
}

#[test]
fn what_couldnt_be_deleted_is_reported_and_the_rest_goes() {
    let t = TestApp::new();
    let dir = t.lab("local-invoice", &[("isoloom.yml", SPEC)]);
    records(t.data(), "local-invoice");
    let run = t.data().join("offline").join("local-invoice.json");
    let held = |path: &Path| if path == run { Err(std::io::Error::other("held by another program")) } else { delete(path) };

    let removal = remove_files(t.data(), "local-invoice", |a, b| std::fs::rename(a, b), held).unwrap();

    assert_eq!(removal.left, [Leftover { path: run.display().to_string(), error: "held by another program".into() }]);
    assert!(run.is_file(), "what failed is still there");
    assert!(!dir.exists() && !t.data().join("labs").join(".local-invoice.removing").exists(), "the copy went");
    assert!(leftovers(t.data(), "local-invoice").iter().all(|d| !d.exists()), "the rest went");
}

#[test]
fn a_copy_that_wont_move_aside_refuses_the_removal_with_nothing_deleted() {
    let t = TestApp::new();
    let dir = t.lab("local-invoice", &[("isoloom.yml", SPEC)]);
    records(t.data(), "local-invoice");
    let denied = |_: &Path, _: &Path| Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied));

    let err = remove_files(t.data(), "local-invoice", denied, delete).unwrap_err();

    assert!(err.is_refusal() && err.to_string().contains("is open in another program") && err.to_string().contains("nothing was deleted"), "{err}");
    assert!(dir.join("isoloom.yml").is_file() && leftovers(t.data(), "local-invoice").iter().all(|d| d.is_dir()));
    assert!(t.data().join("offline").join("local-invoice.json").is_file());
}

#[test]
fn an_earlier_removals_leftover_copy_goes_with_this_one() {
    let t = TestApp::new();
    let dir = t.lab("local-invoice", &[("isoloom.yml", SPEC)]);
    let earlier = t.data().join("labs").join(".local-invoice.removing");
    std::fs::create_dir_all(earlier.join("left")).unwrap();
    assert_eq!(remove_files(t.data(), "local-invoice", |a, b| std::fs::rename(a, b), delete).unwrap(), Removal::default());
    assert!(!dir.exists() && !earlier.exists());
}

/// This test's own secrets file (never the player's), where the attack box passwords are kept,
/// in a folder removed when the returned guard drops.
fn own_secrets() -> crate::testing::TempDir {
    let dir = crate::testing::temp_dir("remove-secrets");
    crate::runtime::server::secrets::use_test_file(dir.join("secrets.json"));
    dir
}

#[tokio::test]
async fn the_attack_boxs_password_goes_with_the_lab_and_stays_while_the_removal_is_refused() {
    use crate::runtime::server::secrets::{get_named, set_named};
    let t = TestApp::new();
    let _file = own_secrets();
    let dir = t.lab("local-password", &[("isoloom.yml", SPEC)]);
    t.lab("local-other", &[("isoloom.yml", SPEC)]);
    set_named("attack-box:local-password", "k7mpq-zrt4w-hn8xc-ls2vd").unwrap();
    set_named("attack-box:local-other", "abcde-fghij-kmnpq-rstuv").unwrap();
    // Refused (still up): the password stays with the lab.
    record_run(t.data(), "local-password", None, "docker", "ready");
    assert_eq!(remove(&t.dirs(), "local-password").await.unwrap_err().code(), Code::LabRunningHere);
    assert!(get_named("attack-box:local-password").unwrap().is_some());
    // Removed: its password went too, another lab's stayed.
    record_run(t.data(), "local-password", None, "docker", "absent");
    assert_eq!(remove(&t.dirs(), "local-password").await.unwrap(), Removal::default());
    assert!(!dir.exists());
    assert_eq!(get_named("attack-box:local-password").unwrap(), None);
    assert!(get_named("attack-box:local-other").unwrap().is_some());
    // A lab whose box never had a password is removed all the same.
    let bare = t.lab("local-bare", &[("isoloom.yml", SPEC)]);
    assert_eq!(remove(&t.dirs(), "local-bare").await.unwrap(), Removal::default());
    assert!(!bare.exists());
}

#[tokio::test]
async fn the_values_its_run_was_launched_with_go_with_the_lab() {
    use crate::runtime::server::secrets::get_named;
    let t = TestApp::new();
    let _file = own_secrets();
    t.lab("local-values", &[("isoloom.yml", SPEC)]);
    t.lab("local-kept", &[("isoloom.yml", SPEC)]);
    let token = [("CTF_LAUNCH_TOKEN".to_owned(), "tok".to_owned())];
    crate::runtime::launch_values::keep("cyberctf-local-values", &token);
    crate::runtime::launch_values::keep("cyberctf-local-kept", &token);
    record_run(t.data(), "local-values", None, "docker", "absent");
    assert_eq!(remove(&t.dirs(), "local-values").await.unwrap(), Removal::default());
    assert_eq!(get_named("launch-values:cyberctf-local-values").unwrap(), None);
    assert!(get_named("launch-values:cyberctf-local-kept").unwrap().is_some(), "another lab's stay");
}

#[tokio::test]
async fn a_password_store_that_refuses_leaves_the_password_listed_and_the_lab_removed() {
    let t = TestApp::new();
    let secrets = own_secrets();
    let file = secrets.join("secrets.json");
    // A secrets file that doesn't parse: the store can't delete from it (as a locked keychain can't).
    std::fs::write(&file, "{").unwrap();
    let dir = t.lab("local-store", &[("isoloom.yml", SPEC)]);
    records(t.data(), "local-store");
    let removal = remove(&t.dirs(), "local-store").await.unwrap();
    // The lab and its records went all the same.
    assert!(!dir.exists() && leftovers(t.data(), "local-store").iter().all(|d| !d.exists()));
    assert!(!is_recorded(&runner_home_in(t.data()), "cyberctf-local-store"));
    // What the store keeps for it alone stayed, said with why, for the player to delete.
    let [left, values] = removal.left.as_slice() else { panic!("{removal:?}") };
    assert_eq!(left.path, "the attack box's password is still in the keychain (attack-box:local-store)");
    assert_eq!(values.path, "the values its run was launched with are still in the keychain (launch-values:cyberctf-local-store)");
    assert!(left.error.contains("isn't readable") && values.error.contains("isn't readable"), "{removal:?}");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "{", "the store as it was");
}
