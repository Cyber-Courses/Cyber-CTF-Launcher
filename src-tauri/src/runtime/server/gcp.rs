//! Google Cloud: every lab of an account runs in one project, "Cyber CTF labs", created once
//! (on the account's test, or its first launch) and kept in the host profile. A project per
//! lab would use one of the billing account's few project slots each time, and Google
//! refuses past that limit ("billing quota exceeded"); one project needs a single free slot,
//! checked here with a clear message instead of a failed launch.

use isothrum_machine::cloud::gcp::{self, ProjectState};
use isothrum_machine::{Failure, Refusal};

use crate::platform::AppDirs;

use super::profile::HostProfile;
use super::store::{find, load, update};
use crate::cloud_cli::{machine, tool_error};
use crate::error::{Error, Result};
use crate::platform::random_hex;

/// What the labs project is called in the Google Cloud console.
const PROJECT_NAME: &str = "Cyber CTF labs";

/// The account's labs project, created (with Compute Engine enabled) when it doesn't exist yet
/// or was deleted. A project gcloud can't tell about (offline, signed out, signed in as someone
/// who can't see it, timed out) is an error, never "gone": a new project then would leave the
/// labs running in the old one (stopped against the new one, they'd keep billing) and use up a
/// billing slot each time.
pub async fn labs_project(dirs: &AppDirs, id: &str) -> Result<String> {
    // One at a time: two launches (or a test and a launch) at once each saw no project yet and
    // created one, and the last to save won. The second waits, then finds the first's project.
    // In this process first, then across processes: the app and the command line are two
    // processes, and each may launch a lab on the account or test it.
    static CREATING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let one_at_a_time = CREATING.lock().await;
    let one_process_at_a_time = crate::runtime::inflight::lock::acquire(&creating_lock(dirs)?).await.map_err(crate::runtime::inflight::lock::unavailable)?;
    // Read fresh under the lock, and only this host's field is written back (the rest of the
    // store may have been edited meanwhile).
    let host = find(&load(dirs)?, id)?;
    if let Some(p) = host.gcp_project.as_deref()
        && project_state(p).await? == ProjectState::Active
    {
        return Ok(p.to_string());
    }
    // Made, set up and saved (or deleted) on a task of its own, the locks with it: a caller
    // dropped meanwhile (the account's test, its window reloaded) left a project made but never
    // saved nor deleted, holding one of the billing account's few slots.
    let dirs = dirs.clone();
    let creating = tokio::spawn(async move {
        let _held = (one_at_a_time, one_process_at_a_time);
        create_project(&dirs, &host).await
    });
    creating.await.map_err(|e| Error::Invalid(format!("creating the Cyber CTF labs project: {e}")))?
}

/// The OS lock held across processes while the labs project is checked and, when needed,
/// created and saved (see [`labs_project`]).
fn creating_lock(dirs: &AppDirs) -> Result<std::path::PathBuf> {
    let dir = dirs.data()?;
    std::fs::create_dir_all(dir)?;
    Ok(dir.join("gcp-project.lock"))
}

/// Whether the saved project `p` is active or gone ([`gcp::project_state`]). Gone only when
/// Google says so plainly: it is being deleted, or `NOT_FOUND`. "No permission (or it may not
/// exist)" is also what another signed-in Google account gets: an error that says to check the
/// sign-in, never a new project.
async fn project_state(p: &str) -> Result<ProjectState> {
    match gcp::project_state(&machine(), p).await {
        Ok(ProjectState::Other(other)) => {
            Err(Error::Invalid(format!("the Cyber CTF labs project {p} is {other}, so labs can't run in it. Check it in the Google Cloud console.")))
        }
        Ok(state) => Ok(state),
        Err(e) => Err(Error::Invalid(format!(
            "couldn't check the Cyber CTF labs project {p}: {}. Check your connection and that gcloud is signed in as the account that owns it (gcloud auth login), then try again.",
            last_error_line(&e)
        ))),
    }
}

/// Creates a labs project linked to the account's billing account, with Compute Engine enabled,
/// and saves it as the account's only then. A project whose setup fails is deleted; when even
/// that fails, the error names it, to delete by hand (it is never saved half set up).
async fn create_project(dirs: &AppDirs, host: &HostProfile) -> Result<String> {
    let project = format!("cyberctf-labs-{}", random_hex(3));
    gcp::create_project(&machine(), &project, PROJECT_NAME, host.gcp_org())
        .await
        .map_err(|e| Error::Invalid(format!("couldn't create the Cyber CTF labs project: {}", last_error_line(&e))))?;
    if let Err(why) = set_up(&project, host.cloud_account().unwrap_or_default()).await {
        return Err(discard(&project, why).await);
    }
    save_project(dirs, &host.id, &project)?;
    Ok(project)
}

/// Links a new project to `billing` and enables Compute Engine on it; why it failed.
async fn set_up(project: &str, billing: &str) -> std::result::Result<(), String> {
    let cx = machine();
    if let Err(e) = gcp::link_billing(&cx, project, billing).await {
        return Err(link_failure(billing, &e));
    }
    gcp::enable_service(&cx, project, "compute.googleapis.com")
        .await
        .map_err(|e| format!("couldn't enable Compute Engine on the labs project {project}: {}", last_error_line(&e)))
}

/// Deletes a project whose setup failed (`why`), freeing its billing slot; the error to give.
async fn discard(project: &str, why: String) -> Error {
    match gcp::delete_project(&machine(), project).await {
        Ok(()) => Error::Invalid(why),
        Err(_) => {
            Error::Invalid(format!("{why} The project {project} couldn't be deleted either: delete it in the Google Cloud console to free its billing slot."))
        }
    }
}

/// Saves `project` as host `id`'s labs project (only that field: the store may have changed).
fn save_project(dirs: &AppDirs, id: &str, project: &str) -> Result<()> {
    update(dirs, |store| {
        if let Some(h) = store.hosts.iter_mut().find(|h| h.id == id) {
            h.gcp_project = Some(project.to_string());
        }
        Ok(())
    })
}

/// Why the labs project couldn't be linked to `billing`: no free project slot (said plainly,
/// with the way out), or gcloud's own reason.
fn link_failure(billing: &str, failure: &Failure) -> String {
    if failure.refusal() == Some(Refusal::QuotaExceeded) {
        format!(
            "No free project slot on billing account {billing}: Google allows only a few projects per billing account. Unlink one you don't use (gcloud billing projects unlink <project>) or request an increase at https://support.google.com/code/contact/billing_quota_increase"
        )
    } else {
        format!("couldn't link the labs project to billing account {billing}: {}", last_error_line(failure))
    }
}

/// A failed gcloud's error: the last non-blank line it printed, else the launcher's error.
fn last_error_line(failure: &Failure) -> String {
    match failure {
        Failure::Refused { said, .. } => said.clone(),
        other => match tool_error(other) {
            Error::CommandFailed { stderr, .. } => stderr.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or_default().trim().to_string(),
            e => e.to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::testing::test_app::TestApp;
    use isothrum_runner::fake::{FakeProcesses, Reply, Script};

    use crate::runtime::providers::testing::fake;

    /// A GCP account `gcp1` whose labs project is `project`, if any.
    fn account(t: &TestApp, project: Option<&str>) -> String {
        t.host("gcp1", "gcp", serde_json::json!({ "username": "0X0X0X-0X0X0X-0X0X0X", "gcpProject": project }))
    }

    /// `script`, then a new labs project made: created, linked to billing, Compute enabled.
    fn project_made(script: Script) -> FakeProcesses {
        script
            .expect("gcloud projects create", Reply::ok(""))
            .expect("gcloud billing projects link", Reply::ok(""))
            .expect("gcloud services enable", Reply::ok(""))
            .build()
    }

    fn saved_project(t: &TestApp) -> Option<String> {
        find(&load(&t.dirs()).unwrap(), "gcp1").unwrap().gcp_project
    }

    #[tokio::test]
    async fn a_project_gcloud_cant_check_is_never_replaced() {
        // Regression: any failed `describe` (a network blip, an expired gcloud sign-in, a timeout,
        // another Google account signed in) made a new project and saved it: the labs still
        // running in the old one were then stopped against the new one and kept billing, and each
        // time used a billing slot.
        let t = TestApp::new();
        let id = account(&t, Some("cyberctf-labs-aaaaaa"));
        for stderr in [
            "ERROR: There was a problem refreshing your current auth tokens: Reauthentication required.",
            "ERROR: (gcloud.projects.describe) User [other@example.com] does not have permission to access projects instance [cyberctf-labs-aaaaaa] (or it may not exist)",
            "timed out after 60s (the tool or its backend may be stuck)",
        ] {
            let tools = fake(FakeProcesses::script().expect("gcloud projects describe", Reply::fail(1, stderr)).build());
            let err = labs_project(&t.dirs(), &id).await.unwrap_err().to_string();
            assert!(err.starts_with("couldn't check the Cyber CTF labs project cyberctf-labs-aaaaaa:"), "{err}");
            assert!(tools.made("gcloud projects create").is_empty(), "{:?}", tools.lines());
            assert_eq!(saved_project(&t).as_deref(), Some("cyberctf-labs-aaaaaa"));
        }
    }

    #[tokio::test]
    async fn an_active_project_is_kept_and_a_deleted_one_replaced() {
        let t = TestApp::new();
        let id = account(&t, Some("cyberctf-labs-aaaaaa"));
        let tools = fake(FakeProcesses::script().expect("gcloud projects describe", Reply::ok("ACTIVE\n")).build());
        assert_eq!(labs_project(&t.dirs(), &id).await.unwrap(), "cyberctf-labs-aaaaaa");
        assert!(tools.made("gcloud projects create").is_empty() && tools.made("gcloud services enable").is_empty());
        drop(tools);
        let tools = fake(project_made(FakeProcesses::script().expect("gcloud projects describe", Reply::fail(1, "ERROR: NOT_FOUND: project not found"))));
        let made = labs_project(&t.dirs(), &id).await.unwrap();
        assert!(made.starts_with("cyberctf-labs-") && made != "cyberctf-labs-aaaaaa", "{made}");
        assert_eq!(tools.made("gcloud billing projects link").len(), 1);
        assert!(!tools.made(&format!("gcloud billing projects link {made} --billing-account 0X0X0X-0X0X0X-0X0X0X")).is_empty(), "{:?}", tools.lines());
        assert!(!tools.made(&format!("gcloud services enable compute.googleapis.com --project {made}")).is_empty(), "{:?}", tools.lines());
        assert_eq!(saved_project(&t), Some(made));
    }

    /// A real race (across processes): the lock was this process's own, so two labs launched
    /// together on an account without a project (one from the app, one from the command line)
    /// each made one; the last to save won and the other lab ran on, billing, in a
    /// project the launcher no longer knew. Another process holding the lock (a handle of its
    /// own here) is waited for, and the project it saved is the one used.
    #[tokio::test]
    async fn a_project_being_made_by_another_process_is_waited_for_then_used() {
        let t = TestApp::new();
        let id = account(&t, None);
        let tools = fake(FakeProcesses::script().expect("gcloud projects describe", Reply::ok("ACTIVE\n")).build());
        let other = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(creating_lock(&t.dirs()).unwrap()).unwrap();
        other.lock().unwrap();
        let waited = tokio::time::timeout(Duration::from_millis(600), labs_project(&t.dirs(), &id)).await;
        assert!(waited.is_err(), "waits while another process makes the project");
        assert!(tools.made("gcloud projects create").is_empty(), "{:?}", tools.lines());
        // The other process saved its project and let go.
        save_project(&t.dirs(), &id, "cyberctf-labs-bbbbbb").unwrap();
        drop(other);
        assert_eq!(labs_project(&t.dirs(), &id).await.unwrap(), "cyberctf-labs-bbbbbb");
        assert!(tools.made("gcloud projects create").is_empty(), "{:?}", tools.lines());
    }

    #[tokio::test]
    async fn a_project_being_deleted_is_replaced_one_that_isnt_active_is_said() {
        assert_eq!(state_of(Reply::ok("DELETE_REQUESTED")).await.unwrap(), ProjectState::Gone);
        assert_eq!(state_of(Reply::ok("ACTIVE")).await.unwrap(), ProjectState::Active);
        let err = state_of(Reply::ok("SUSPENDED")).await.unwrap_err().to_string();
        assert!(err.contains("is SUSPENDED"), "{err}");
    }

    /// What `project_state` makes of a `describe` answering `describe`.
    async fn state_of(describe: Reply) -> Result<ProjectState> {
        let _tools = fake(FakeProcesses::script().expect("gcloud projects describe", describe).build());
        project_state("p").await
    }

    #[tokio::test]
    async fn a_project_whose_setup_failed_is_deleted_never_saved_half_set_up() {
        // Regression: when `services enable` failed after billing was linked, the project was
        // neither saved nor deleted: a billing slot leaked on every try.
        let t = TestApp::new();
        let id = account(&t, None);
        for (step, stderr) in [("gcloud services enable", "ERROR: quota exceeded"), ("gcloud billing projects link", "Cloud billing quota exceeded")] {
            // The steps before the failing one succeed (none runs after it but the deletion).
            let answer = |call: &str| if call == step { Reply::fail(1, stderr) } else { Reply::ok("") };
            let tools = fake(
                FakeProcesses::script()
                    .expect("gcloud projects create", Reply::ok(""))
                    .expect("gcloud billing projects link", answer("gcloud billing projects link"))
                    .allow("gcloud services enable", answer("gcloud services enable"))
                    .expect("gcloud projects delete", Reply::ok(""))
                    .build(),
            );
            assert!(labs_project(&t.dirs(), &id).await.is_err());
            assert!(!tools.made("gcloud projects delete").is_empty(), "{step}: {:?}", tools.lines());
            assert_eq!(saved_project(&t), None);
        }
        // Not even deletable: the error names it, to delete by hand.
        let _tools = fake(
            FakeProcesses::script()
                .expect("gcloud projects create", Reply::ok(""))
                .expect("gcloud billing projects link", Reply::ok(""))
                .expect("gcloud services enable", Reply::fail(1, "quota"))
                .expect("gcloud projects delete", Reply::fail(1, "permission denied"))
                .build(),
        );
        let err = labs_project(&t.dirs(), &id).await.unwrap_err().to_string();
        assert!(err.contains("couldn't be deleted either: delete it in the Google Cloud console"), "{err}");
        assert_eq!(saved_project(&t), None);
    }

    #[tokio::test]
    async fn the_labs_project_is_created_under_the_organization_when_set() {
        let t = TestApp::new();
        let id = t.host("gcp1", "gcp", serde_json::json!({ "username": "0X0X0X-0X0X0X-0X0X0X", "node": "42" }));
        let tools = fake(project_made(FakeProcesses::script()));
        let made = labs_project(&t.dirs(), &id).await.unwrap();
        let created = tools.made("gcloud projects create");
        assert_eq!(
            created.iter().map(|c| c.args.clone()).collect::<Vec<_>>(),
            [["projects", "create", made.as_str(), "--name", "Cyber CTF labs", "--organization=42"]],
            "{:?}",
            tools.lines()
        );
    }

    /// gcloud refusing, saying `said`, read as isothrum-machine reads it.
    fn refused(said: &str) -> Failure {
        let last = said.lines().rev().map(str::trim).find(|l| !l.is_empty()).unwrap_or_default();
        Failure::Refused { program: "gcloud".into(), refusal: gcp::refusal(said), said: last.into() }
    }

    #[test]
    fn a_full_billing_account_is_explained() {
        for why in ["ERROR: Cloud billing quota exceeded", "FAILED_PRECONDITION: Precondition check failed."] {
            let msg = link_failure("0X0X0X-0X0X0X-0X0X0X", &refused(why));
            assert!(msg.starts_with("No free project slot on billing account 0X0X0X-0X0X0X-0X0X0X"), "{msg}");
        }
        assert_eq!(link_failure("B", &refused("ERROR: permission denied")), "couldn't link the labs project to billing account B: ERROR: permission denied");
    }

    #[test]
    fn last_error_line_is_what_gcloud_said_last_else_the_launchers_error() {
        assert_eq!(last_error_line(&refused("WARNING: x\nERROR: quota exceeded\n\n  \n")), "ERROR: quota exceeded");
        let stuck = Failure::TimedOut { program: "gcloud".into(), limit: std::time::Duration::from_secs(300) };
        assert!(last_error_line(&stuck).starts_with("timed out after 300s"), "{}", last_error_line(&stuck));
        assert_ne!(last_error_line(&Failure::Missing { program: "gcloud".into() }), "");
    }
}
