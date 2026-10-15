//! Google Cloud: every lab of an account runs in one project, "Cyber CTF labs", created once
//! (on the account's test, or its first launch) and kept in the host profile. A project per
//! lab would use one of the billing account's few project slots each time, and Google
//! refuses past that limit ("billing quota exceeded"); one project needs a single free slot,
//! checked here with a clear message instead of a failed launch.

use std::time::Duration;

use crate::AppHandle;

use super::profile::{HostProfile, random_hex};
use super::store::{find, load, update};
use crate::error::{Error, Result};
use crate::platform::exec::{CLI_TIMEOUT, run_env_timed};

/// How long creating the project, linking it to billing or enabling Compute Engine may take.
const PROJECT_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// What gcloud says of a saved labs project.
#[derive(Debug, PartialEq)]
enum Project {
    Active,
    /// Deleted (or being deleted): a new one is made.
    Gone,
}

/// The account's labs project, created (with Compute Engine enabled) when it doesn't exist yet
/// or was deleted. A project gcloud can't tell about (offline, signed out, signed in as someone
/// who can't see it, timed out) is an error, never "gone": a new project then would leave the
/// labs running in the old one (stopped against the new one, they'd keep billing) and use up a
/// billing slot each time.
pub async fn labs_project(app: &AppHandle, id: &str) -> Result<String> {
    // One at a time: two launches (or a test and a launch) at once each saw no project yet and
    // created one, and the last to save won. The second waits, then finds the first's project.
    static CREATING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let _one_at_a_time = CREATING.lock().await;
    // Read fresh under the lock, and only this host's field is written back (the rest of the
    // store may have been edited meanwhile).
    let host = find(&load(app)?, id)?;
    match host.gcp_project.as_deref() {
        Some(p) if project_state(p).await? == Project::Active => Ok(p.to_string()),
        _ => create_project(app, &host).await,
    }
}

/// A gcloud call given up after `limit`.
async fn gcloud(args: &[&str], limit: Duration) -> Result<String> {
    run_env_timed("gcloud", args, None, &[], limit).await
}

/// Whether the saved project `p` is active or gone. Gone only when gcloud says so plainly: it is
/// being deleted, or NOT_FOUND. "No permission (or it may not exist)" is also what another
/// signed-in Google account gets: an error that says to check the sign-in, never a new project.
async fn project_state(p: &str) -> Result<Project> {
    match gcloud(&["projects", "describe", p, "--format", "value(lifecycleState)"], CLI_TIMEOUT).await {
        Ok(state) => match state.trim() {
            "ACTIVE" => Ok(Project::Active),
            "DELETE_REQUESTED" | "DELETE_IN_PROGRESS" => Ok(Project::Gone),
            other => Err(Error::Invalid(format!("the Cyber CTF labs project {p} is {other}, so labs can't run in it. Check it in the Google Cloud console."))),
        },
        Err(Error::CommandFailed { stderr, .. }) if stderr.contains("NOT_FOUND") => Ok(Project::Gone),
        Err(e) => Err(Error::Invalid(format!(
            "couldn't check the Cyber CTF labs project {p}: {}. Check your connection and that gcloud is signed in as the account that owns it (gcloud auth login), then try again.",
            last_error_line(&e)
        ))),
    }
}

/// Creates a labs project linked to the account's billing account, with Compute Engine enabled,
/// and saves it as the account's only then. A project whose setup fails is deleted; when even
/// that fails, the error names it, to delete by hand (it is never saved half set up).
async fn create_project(app: &AppHandle, host: &HostProfile) -> Result<String> {
    let project = format!("cyberctf-labs-{}", random_hex(3));
    let org = host.gcp_org().map(|o| format!("--organization={o}"));
    gcloud(&create_args(&project, org.as_deref()), PROJECT_TIMEOUT)
        .await
        .map_err(|e| Error::Invalid(format!("couldn't create the Cyber CTF labs project: {}", last_error_line(&e))))?;
    if let Err(why) = set_up(&project, host.cloud_account().unwrap_or_default()).await {
        return Err(discard(&project, why).await);
    }
    save_project(app, &host.id, &project)?;
    Ok(project)
}

/// Links a new project to `billing` and enables Compute Engine on it; why it failed.
async fn set_up(project: &str, billing: &str) -> std::result::Result<(), String> {
    if let Err(e) = gcloud(&["billing", "projects", "link", project, "--billing-account", billing], PROJECT_TIMEOUT).await {
        return Err(link_failure(billing, &last_error_line(&e)));
    }
    gcloud(&["services", "enable", "compute.googleapis.com", "--project", project], PROJECT_TIMEOUT)
        .await
        .map(|_| ())
        .map_err(|e| format!("couldn't enable Compute Engine on the labs project {project}: {}", last_error_line(&e)))
}

/// Deletes a project whose setup failed (`why`), freeing its billing slot; the error to give.
async fn discard(project: &str, why: String) -> Error {
    match gcloud(&["projects", "delete", project, "--quiet"], PROJECT_TIMEOUT).await {
        Ok(_) => Error::Invalid(why),
        Err(_) => {
            Error::Invalid(format!("{why} The project {project} couldn't be deleted either: delete it in the Google Cloud console to free its billing slot."))
        }
    }
}

/// Saves `project` as host `id`'s labs project (only that field: the store may have changed).
fn save_project(app: &AppHandle, id: &str, project: &str) -> Result<()> {
    update(app, |store| {
        if let Some(h) = store.hosts.iter_mut().find(|h| h.id == id) {
            h.gcp_project = Some(project.to_string());
        }
        Ok(())
    })
}

/// `gcloud projects create` for the labs project, under the organization flag when one is set.
fn create_args<'a>(project: &'a str, org: Option<&'a str>) -> Vec<&'a str> {
    let mut create = vec!["projects", "create", project, "--name", "Cyber CTF labs"];
    if let Some(o) = org {
        create.push(o);
    }
    create
}

/// Why the labs project couldn't be linked to `billing`: no free project slot (said plainly,
/// with the way out), or gcloud's own reason.
fn link_failure(billing: &str, why: &str) -> String {
    if why.contains("quota") || why.contains("Precondition") {
        format!(
            "No free project slot on billing account {billing}: Google allows only a few projects per billing account. Unlink one you don't use (gcloud billing projects unlink <project>) or request an increase at https://support.google.com/code/contact/billing_quota_increase"
        )
    } else {
        format!("couldn't link the labs project to billing account {billing}: {why}")
    }
}

/// The last non-blank line of a failed gcloud's stderr (its error), else the error itself.
fn last_error_line(e: &Error) -> String {
    match e {
        Error::CommandFailed { stderr, .. } => stderr.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or_default().trim().to_string(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::exec::fake::{Tools, fail, ok};
    use crate::testing::test_app::TestApp;

    /// A GCP account `gcp1` whose labs project is `project`, if any.
    fn account(t: &TestApp, project: Option<&str>) -> String {
        t.host("gcp1", "gcp", serde_json::json!({ "username": "0X0X0X-0X0X0X-0X0X0X", "gcpProject": project }))
    }

    fn saved_project(t: &TestApp) -> Option<String> {
        find(&load(t.handle()).unwrap(), "gcp1").unwrap().gcp_project
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
            let tools = Tools::rules(&[("projects describe", fail(stderr))]);
            let err = labs_project(t.handle(), &id).await.unwrap_err().to_string();
            assert!(err.starts_with("couldn't check the Cyber CTF labs project cyberctf-labs-aaaaaa:"), "{err}");
            assert!(!tools.called("projects create"), "{:?}", tools.calls());
            assert_eq!(saved_project(&t).as_deref(), Some("cyberctf-labs-aaaaaa"));
        }
    }

    #[tokio::test]
    async fn an_active_project_is_kept_and_a_deleted_one_replaced() {
        let t = TestApp::new();
        let id = account(&t, Some("cyberctf-labs-aaaaaa"));
        let tools = Tools::rules(&[("projects describe", ok("ACTIVE\n"))]);
        assert_eq!(labs_project(t.handle(), &id).await.unwrap(), "cyberctf-labs-aaaaaa");
        assert!(!tools.called("projects create") && !tools.called("services enable"));
        drop(tools);
        let tools = Tools::rules(&[("projects describe", fail("ERROR: NOT_FOUND: project not found"))]);
        let made = labs_project(t.handle(), &id).await.unwrap();
        assert!(made.starts_with("cyberctf-labs-") && made != "cyberctf-labs-aaaaaa", "{made}");
        assert!(tools.called("billing projects link") && tools.called("--billing-account 0X0X0X-0X0X0X-0X0X0X"));
        assert!(tools.called(&format!("services enable compute.googleapis.com --project {made}")));
        assert_eq!(saved_project(&t), Some(made));
    }

    #[tokio::test]
    async fn a_project_being_deleted_is_replaced_one_that_isnt_active_is_said() {
        assert_eq!(state_of(ok("DELETE_REQUESTED")).await.unwrap(), Project::Gone);
        assert_eq!(state_of(ok("ACTIVE")).await.unwrap(), Project::Active);
        let err = state_of(ok("SUSPENDED")).await.unwrap_err().to_string();
        assert!(err.contains("is SUSPENDED"), "{err}");
    }

    /// What `project_state` makes of a `describe` answering `describe`.
    async fn state_of(describe: Option<crate::platform::exec::fake::Reply>) -> Result<Project> {
        let _tools = Tools::rules(&[("projects describe", describe)]);
        project_state("p").await
    }

    #[tokio::test]
    async fn a_project_whose_setup_failed_is_deleted_never_saved_half_set_up() {
        // Regression: when `services enable` failed after billing was linked, the project was
        // neither saved nor deleted: a billing slot leaked on every try.
        let t = TestApp::new();
        let id = account(&t, None);
        for (step, stderr) in [("services enable", "ERROR: quota exceeded"), ("billing projects link", "Cloud billing quota exceeded")] {
            let tools = Tools::rules(&[(step, fail(stderr))]);
            assert!(labs_project(t.handle(), &id).await.is_err());
            assert!(tools.called("projects delete"), "{step}: {:?}", tools.calls());
            assert_eq!(saved_project(&t), None);
        }
        // Not even deletable: the error names it, to delete by hand.
        let _tools = Tools::rules(&[("services enable", fail("quota")), ("projects delete", fail("permission denied"))]);
        let err = labs_project(t.handle(), &id).await.unwrap_err().to_string();
        assert!(err.contains("couldn't be deleted either: delete it in the Google Cloud console"), "{err}");
        assert_eq!(saved_project(&t), None);
    }

    #[test]
    fn the_labs_project_is_created_under_the_organization_when_set() {
        assert_eq!(create_args("cyberctf-labs-abc", None), ["projects", "create", "cyberctf-labs-abc", "--name", "Cyber CTF labs"]);
        assert_eq!(create_args("p", Some("--organization=42")).last(), Some(&"--organization=42"));
    }

    #[test]
    fn a_full_billing_account_is_explained() {
        for why in ["ERROR: Cloud billing quota exceeded", "FAILED_PRECONDITION: Precondition check failed."] {
            let msg = link_failure("0X0X0X-0X0X0X-0X0X0X", why);
            assert!(msg.starts_with("No free project slot on billing account 0X0X0X-0X0X0X-0X0X0X"), "{msg}");
        }
        assert_eq!(link_failure("B", "permission denied"), "couldn't link the labs project to billing account B: permission denied");
    }

    #[test]
    fn last_error_line_skips_trailing_blank_lines() {
        let e = Error::CommandFailed { command: "gcloud".into(), stderr: "WARNING: x\nERROR: quota exceeded\n\n  \n".into() };
        assert_eq!(last_error_line(&e), "ERROR: quota exceeded");
        let e = Error::CommandFailed { command: "gcloud".into(), stderr: String::new() };
        assert_eq!(last_error_line(&e), "");
        assert_eq!(last_error_line(&Error::Invalid("boom".into())), "boom");
    }
}
