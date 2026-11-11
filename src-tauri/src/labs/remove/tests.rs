use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::*;
use crate::platform::exec::fake::Tools;
use crate::runtime::test_support::record_run;
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

// Each test that removes through `remove` names its lab its own way: the lab's lock is this
// process's, by id, so two tests on one id running at once would find each other's lab held.

/// Lab `id`'s records under the app data folder `base`, beside its copy: the runner's run (taken
/// down: `absent`), an emptied Terraform deployment, a kept offline source, leftover copies, and
/// its deploy worker's files. Returns those worker files.
fn records(base: &Path, id: &str) -> Vec<PathBuf> {
    record_run(base, id, None, "docker", "absent");
    std::fs::write(base.join("runs").join(format!("cyberctf-{id}")).join("events.jsonl"), "{}\n").unwrap();
    std::fs::create_dir_all(base.join("deployments").join(id).join("aws")).unwrap();
    std::fs::create_dir_all(base.join("offline")).unwrap();
    std::fs::write(base.join("offline").join(format!("{id}.json")), "{}").unwrap();
    for leftover in [format!(".{id}.staging"), format!(".{id}.old")] {
        std::fs::create_dir_all(base.join("labs").join(leftover).join("x")).unwrap();
    }
    let deploys = base.join("deploys");
    std::fs::create_dir_all(&deploys).unwrap();
    let files: Vec<PathBuf> = ["json", "log", "previous.log", "status", "launch-status"].iter().map(|ext| deploys.join(format!("{id}.{ext}"))).collect();
    for f in &files {
        std::fs::write(f, "x").unwrap();
    }
    files
}

#[tokio::test]
async fn a_removal_deletes_the_apps_copy_and_records_and_never_the_folder_it_was_imported_from() {
    let t = TestApp::new();
    let _tools = Tools::rules(&[]);
    let src = source();
    let dir = super::super::for_tests::reimport(&t.dirs(), "local-copy", &src).await.unwrap();
    // A link in the copy pointing at the source (never one the import makes: planted, the worst
    // case): the removal deletes the link, not what it points at.
    #[cfg(unix)]
    std::os::unix::fs::symlink(&src, dir.join("to-source")).unwrap();
    let before = tree(&src);
    let worker = records(t.data(), "local-copy");
    // Another lab, and its records: untouched.
    let other = t.lab("local-other", &[("isoloom.yml", SPEC)]);
    let other_worker = records(t.data(), "local-other");

    let removal = remove(&t.dirs(), "local-copy", || Ok(worker.clone())).await.unwrap();

    assert_eq!(removal, Removal::default(), "every trace went");
    let labs = t.data().join("labs");
    for gone in [
        dir.clone(),
        labs.join(".local-copy.removing"),
        labs.join(".local-copy.staging"),
        labs.join(".local-copy.old"),
        t.data().join("runs").join("cyberctf-local-copy"),
        t.data().join("deployments").join("local-copy"),
        t.data().join("offline").join("local-copy.json"),
    ]
    .iter()
    .chain(&worker)
    {
        assert!(std::fs::symlink_metadata(gone).is_err(), "{} is still there", gone.display());
    }
    assert_eq!(tree(&src), before, "the folder it was imported from is as it was");
    assert!(other.join("isoloom.yml").is_file() && t.data().join("runs").join("cyberctf-local-other").is_dir());
    assert!(other_worker.iter().all(|f| f.is_file()) && t.data().join("offline").join("local-other.json").is_file());
    assert!(crate::labs::imported::list(&t.dirs()).unwrap().iter().all(|l| l.id != "local-copy"), "no longer listed");
    // Removed already: nothing left to remove.
    let again = remove(&t.dirs(), "local-copy", || Ok(Vec::new())).await.unwrap_err();
    assert_eq!(again.to_string(), "lab `local-copy` is not installed");
    let _ = std::fs::remove_dir_all(&src);
}

#[tokio::test]
async fn a_catalogue_lab_is_removed_the_same_way() {
    let t = TestApp::new();
    let id = "66d71cbf-9b4e-4a78-b3cf-8208227a96b2";
    let dir = t.lab(id, &[("isoloom.yml", SPEC), (runtime::lab::COMMIT_MARKER, "0123456789abcdef0123456789abcdef01234567")]);
    let worker = records(t.data(), id);
    assert_eq!(remove(&t.dirs(), id, || Ok(worker.clone())).await.unwrap(), Removal::default());
    assert!(!dir.exists() && !t.data().join("offline").join(format!("{id}.json")).exists() && worker.iter().all(|f| !f.exists()));
}

#[tokio::test]
async fn a_removal_is_refused_while_the_lab_is_up_anywhere_parked_or_busy() {
    let t = TestApp::new();
    let dir = t.lab("local-refused", &[("isoloom.yml", SPEC)]);
    let no_worker = || panic!("the worker isn't asked once the removal is refused");
    let refused = async |state: &str, host: Option<&str>| {
        record_run(t.data(), "local-refused", host, if host.is_some() { "vagrant" } else { "docker" }, state);
        remove(&t.dirs(), "local-refused", no_worker).await.unwrap_err()
    };
    // Up here, starting, failed with its machines left, or parked: what the runner records.
    for state in ["ready", "starting", "failed", "parked"] {
        assert_eq!(refused(state, None).await.code(), Code::LabRunningHere, "{state}");
    }
    // Up on a server: named.
    assert_eq!(refused("ready", Some("esxi1")).await.code(), Code::LabRunningOn { host: "esxi1".into() });
    // Taken down, but marked parked by the launcher: its machines are kept.
    record_run(t.data(), "local-refused", None, "docker", "absent");
    paths::mark_parked(&dir, Some(runtime::Park::Shutdown)).unwrap();
    assert_eq!(remove(&t.dirs(), "local-refused", no_worker).await.unwrap_err().code(), Code::LabRunningHere);
    paths::mark_parked(&dir, None).unwrap();
    // Another operation holds it.
    let held = runtime::lock_lab(&t.dirs(), "local-refused").await.unwrap();
    assert_eq!(remove(&t.dirs(), "local-refused", no_worker).await.unwrap_err().code(), Code::AlreadyInProgress);
    drop(held);
    // A deploy worker runs on it.
    let err = remove(&t.dirs(), "local-refused", || Err(Error::coded(Code::AlreadyInProgress))).await.unwrap_err();
    assert_eq!(err.code(), Code::AlreadyInProgress);
    // Through every refusal, the lab and its run's record stayed.
    assert!(dir.join("isoloom.yml").is_file() && t.data().join("runs").join("cyberctf-local-refused").join("record.json").is_file());
    // A bad id is refused before anything is read.
    assert_eq!(remove(&t.dirs(), "../x", no_worker).await.unwrap_err().to_string(), "invalid lab id `../x`");
}

#[test]
fn what_couldnt_be_deleted_is_reported_and_the_rest_goes() {
    let t = TestApp::new();
    let dir = t.lab("local-invoice", &[("isoloom.yml", SPEC)]);
    let worker = records(t.data(), "local-invoice");
    let run = t.data().join("runs").join("cyberctf-local-invoice");
    let held = |path: &Path| if path == run { Err(std::io::Error::other("held by another program")) } else { delete(path) };

    let removal = remove_files(t.data(), "local-invoice", &worker, |a, b| std::fs::rename(a, b), held).unwrap();

    assert_eq!(removal.left, [Leftover { path: run.display().to_string(), error: "held by another program".into() }]);
    assert!(run.join("record.json").is_file(), "what failed is still there");
    assert!(!dir.exists() && !t.data().join("labs").join(".local-invoice.removing").exists(), "the copy went");
    assert!(worker.iter().all(|f| !f.exists()) && !t.data().join("offline").join("local-invoice.json").exists(), "the rest went");
}

#[test]
fn a_copy_that_wont_move_aside_refuses_the_removal_with_nothing_deleted() {
    let t = TestApp::new();
    let dir = t.lab("local-invoice", &[("isoloom.yml", SPEC)]);
    let worker = records(t.data(), "local-invoice");
    let denied = |_: &Path, _: &Path| Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied));

    let err = remove_files(t.data(), "local-invoice", &worker, denied, delete).unwrap_err();

    assert!(err.is_refusal() && err.to_string().contains("is open in another program") && err.to_string().contains("nothing was deleted"), "{err}");
    assert!(dir.join("isoloom.yml").is_file() && worker.iter().all(|f| f.is_file()));
    assert!(t.data().join("runs").join("cyberctf-local-invoice").is_dir() && t.data().join("offline").join("local-invoice.json").is_file());
}

#[test]
fn an_earlier_removals_leftover_copy_goes_with_this_one() {
    let t = TestApp::new();
    let dir = t.lab("local-invoice", &[("isoloom.yml", SPEC)]);
    let earlier = t.data().join("labs").join(".local-invoice.removing");
    std::fs::create_dir_all(earlier.join("left")).unwrap();
    assert_eq!(remove_files(t.data(), "local-invoice", &[], |a, b| std::fs::rename(a, b), delete).unwrap(), Removal::default());
    assert!(!dir.exists() && !earlier.exists());
}

/// This test's own secrets file (never the player's): where the attack box passwords are kept.
fn own_secrets() -> PathBuf {
    let file = std::env::temp_dir().join(format!("cyberctf-remove-secrets-{}", rand::random::<u32>())).join("secrets.json");
    crate::runtime::server::secrets::use_test_file(file.clone());
    file
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
    assert_eq!(remove(&t.dirs(), "local-password", || Ok(Vec::new())).await.unwrap_err().code(), Code::LabRunningHere);
    assert!(get_named("attack-box:local-password").unwrap().is_some());
    // Removed: its password went too, another lab's stayed.
    record_run(t.data(), "local-password", None, "docker", "absent");
    assert_eq!(remove(&t.dirs(), "local-password", || Ok(Vec::new())).await.unwrap(), Removal::default());
    assert!(!dir.exists());
    assert_eq!(get_named("attack-box:local-password").unwrap(), None);
    assert!(get_named("attack-box:local-other").unwrap().is_some());
    // A lab whose box never had a password is removed all the same.
    let bare = t.lab("local-bare", &[("isoloom.yml", SPEC)]);
    assert_eq!(remove(&t.dirs(), "local-bare", || Ok(Vec::new())).await.unwrap(), Removal::default());
    assert!(!bare.exists());
}

#[tokio::test]
async fn a_password_store_that_refuses_leaves_the_password_listed_and_the_lab_removed() {
    let t = TestApp::new();
    let file = own_secrets();
    // A secrets file that doesn't parse: the store can't delete from it (as a locked keychain can't).
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, "{").unwrap();
    let dir = t.lab("local-store", &[("isoloom.yml", SPEC)]);
    let worker = records(t.data(), "local-store");
    let removal = remove(&t.dirs(), "local-store", || Ok(worker.clone())).await.unwrap();
    // The lab and its records went all the same.
    assert!(!dir.exists() && worker.iter().all(|f| !f.exists()));
    assert!(!t.data().join("runs").join("cyberctf-local-store").exists());
    // The password alone stayed, said with why, for the player to delete.
    let [left] = removal.left.as_slice() else { panic!("{removal:?}") };
    assert_eq!(left.path, "the attack box's password is still in the keychain (attack-box:local-store)");
    assert!(left.error.contains("isn't readable"), "{}", left.error);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "{", "the store as it was");
}
