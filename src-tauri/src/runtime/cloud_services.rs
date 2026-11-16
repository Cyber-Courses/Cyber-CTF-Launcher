//! Cloud-services labs (runtime `CLOUD`): no machines, but cloud services (IAM users, functions,
//! buckets, instances...) from a Terraform module of the lab's own, hand-written (`cloud/*.tf`),
//! applied into the player's cloud account. Isoloom produces nothing for them: the launcher reads
//! what it needs of the lab from its own lab file beside the module ([`lab_file`]), and Isothrum
//! runs the module as any Terraform folder ([`isothrum::CloudRun::own`]): it applies it with the
//! values the launcher gives ([`values`]), keeps its state and expiry, reads its outputs, and
//! destroys it on a stop or the auto-stop.
//!
//! What stays the launcher's is the policy around a start:
//!
//! - The account: one of the lab's cloud (its credentials, region and auto-stop), the GCP labs
//!   project made sure of, the AWS budget guard.
//! - The module's values: the lab file's `vars`, the launch's values filled in, and this
//!   machine's public IP in its `player_ip_var` where the lab lets only this machine in; each
//!   checked against the variables the module declares before anything is created.
//! - What the launch dialog shows first ([`preview`]): the lab's cloud, its cost estimate, what
//!   it asks for and what it exposes. Stop destroys everything: nothing is parked.

pub(crate) mod lab_file;
pub(crate) mod values;

use crate::platform::AppDirs;

use super::launch::{check_aws_budget, public_ip};
use super::providers::Provider;
use super::{LabRef, Preview, isothrum, server};
use crate::error::{Error, Result};
use lab_file::{Cloud, LabFile};

/// "About $0.02 an hour, $0.08 until it auto-stops in 4 h", for the log and the launch dialog.
pub fn cost_line(hourly_usd: Option<f64>, auto_stop_hours: u32) -> String {
    match hourly_usd {
        Some(h) if auto_stop_hours > 0 => format!(
            "Estimated cost: about ${h:.2} an hour, so about ${:.2} if it runs until it auto-stops in {auto_stop_hours} h (the lab's own estimate; your cloud bills the real amount).",
            h * f64::from(auto_stop_hours)
        ),
        Some(h) => format!("Estimated cost: about ${h:.2} an hour until you stop it (the lab's own estimate; your cloud bills the real amount)."),
        None => "This lab gives no cost estimate; your cloud bills what it creates until you stop it.".into(),
    }
}

/// Starts the cloud-services lab `lab` on the player's cloud account `host`, its module run by
/// Isothrum with the values its lab file gives (the launch's values `env` filled in). The
/// launcher keeps the policy: the account is of the lab's cloud, the GCP labs project, the AWS
/// budget, this machine's public IP where the lab asks for it, every value the module needs
/// given before anything is created.
pub(super) async fn start(dirs: &AppDirs, lab: LabRef<'_>, host: Option<&str>, env: &[(String, String)], log: &mut impl FnMut(String)) -> Result<()> {
    // A cloud-services lab always runs as `Runtime::Cloud`: only its folder and id are read here.
    let LabRef { dir, id, .. } = lab;
    let host = host.ok_or_else(|| Error::coded(crate::error::Code::PickCloudAccount))?;
    let file = LabFile::read(dir)?;
    let declared = isothrum::module_variables(&lab_file::module_dir(dir))?;
    // Every value the module needs, checked before the account is touched (the IP comes later).
    values::module_values(&file, &declared, env, file.player_ip_var.as_ref().map(|_| "192.0.2.1"))?;
    let conn = server::connection(dirs, host)?;
    check_account(file.provider, &conn)?;
    let conn = ready_account(dirs, host, conn, log).await?;
    // The run's record names the account from the start of its run, so a half-created lab can
    // still be destroyed with the same account.
    log(super::deploy_log::in_cloud_account(conn.provider.label(), &conn.name, conn.region.as_deref().unwrap_or("default region")));
    if conn.provider == Provider::Aws {
        check_aws_budget(dirs, host, log).await?;
    }
    log(cost_line(file.hourly_usd, auto_stop_hours(&conn)));
    let ip = player_ip(&file, log).await?;
    let values = values::module_values(&file, &declared, env, ip.as_deref())?;
    log("Creating the lab's cloud services…".to_owned());
    let place = isothrum::CloudPlace::launch(&conn, None);
    isothrum::CloudRun::own(&super::paths::runner_home(dirs)?, dir, id)?.up_own(&place, &values, log).await
}

/// Refuses an account of another cloud than the lab's.
fn check_account(cloud: Cloud, conn: &server::Connection) -> Result<()> {
    if conn.provider == cloud.provider() {
        return Ok(());
    }
    let wanted = cloud.provider().label();
    Err(Error::Invalid(format!("This lab runs on {wanted}, and {} is a {} account. Pick a {wanted} account.", conn.name, conn.provider.label())))
}

/// The account ready to run labs in: on GCP, its labs project made sure of (one free billing
/// slot), and the connection read again with it.
async fn ready_account(dirs: &AppDirs, host: &str, conn: server::Connection, log: &mut impl FnMut(String)) -> Result<server::Connection> {
    if conn.provider != Provider::Gcp {
        return Ok(conn);
    }
    log("Checking the Cyber CTF labs project on Google Cloud…".into());
    server::gcp::labs_project(dirs, host).await?;
    server::connection(dirs, host)
}

/// The account's auto-stop, in hours (0: none).
fn auto_stop_hours(conn: &server::Connection) -> u32 {
    conn.auto_stop_hours.unwrap_or(server::DEFAULT_AUTO_STOP_HOURS)
}

/// This machine's public IP when the lab lets only it in somewhere (its `player_ip_var`).
async fn player_ip(file: &LabFile, log: &mut impl FnMut(String)) -> Result<Option<String>> {
    if file.player_ip_var.is_none() {
        return Ok(None);
    }
    let ip = public_ip().await?;
    log(format!("Only your public IP ({ip}) is let in where the lab asks for an allow-list."));
    Ok(Some(ip))
}

/// What the cloud-services lab of lab file `file` runs and takes before it starts.
pub fn preview(file: &LabFile) -> Preview {
    Preview {
        provider: file.provider.provider().id().to_owned(),
        hourly_usd: file.hourly_usd,
        inputs: file.inputs(),
        allow_list: file.player_ip_var.is_some(),
        outputs: file.outputs.keys().cloned().collect(),
    }
}

/// [`Preview`] of a lab at its `source`'s commit: its lab file in the installed copy when it is
/// that commit, else fetched from GitHub; none for a lab that has no lab file (no cloud services).
pub async fn preview_at(dirs: &AppDirs, lab_id: &str, source: super::lab::Source<'_>) -> Result<Option<Preview>> {
    let text = lab_file_at(dirs, lab_id, source, fetch_lab_file).await?;
    text.map(|t| LabFile::parse(&t).map(|f| preview(&f))).transpose()
}

/// The one way a cloud-services lab at its `source`'s commit starts: in an account of its lab
/// file's cloud.
pub async fn run_options_at(dirs: &AppDirs, lab_id: &str, source: super::lab::Source<'_>) -> Result<super::lab::RunOptions> {
    let text = lab_file_at(dirs, lab_id, source, fetch_lab_file).await?;
    let text = text.ok_or_else(|| Error::Invalid(format!("this lab has no {}/{}: it runs no cloud services", lab_file::MODULE_DIR, lab_file::LAB_FILE)))?;
    let cloud = LabFile::parse(&text)?.provider.provider();
    let path = super::lab::RunPath { provider: Some(cloud), target: CLOUD_SERVICES.to_owned(), variant: Some(cloud.id().to_owned()) };
    Ok(super::lab::RunOptions { cloud_services: vec![path], ..super::lab::RunOptions::default() })
}

/// The launch path of a cloud-services lab, as the picker names it.
const CLOUD_SERVICES: &str = "cloud-services";

/// The lab file of lab `lab_id` at its `source`'s commit ([`preview_at`]), fetched by `fetch`
/// when the installed copy isn't that commit; none when the lab has none.
async fn lab_file_at<F>(dirs: &AppDirs, lab_id: &str, source: super::lab::Source<'_>, fetch: impl FnOnce(String) -> F) -> Result<Option<String>>
where
    F: Future<Output = Result<Option<String>>>,
{
    let super::lab::Source { repository, commit, check } = source;
    super::paths::validate_id(lab_id)?;
    check(repository, commit)?;
    let path = format!("{}/{}", lab_file::MODULE_DIR, lab_file::LAB_FILE);
    if let Ok(dir) = super::paths::lab_dir(dirs, lab_id)
        && super::lab::installed_at(&dir, commit)
    {
        return Ok(std::fs::read_to_string(dir.join(&path)).ok());
    }
    fetch(format!("https://raw.githubusercontent.com/{repository}/{commit}/{path}")).await
}

/// The text at `url`; none when there is nothing there (404).
async fn fetch_lab_file(url: String) -> Result<Option<String>> {
    use crate::platform::http::{MAX_LAB_SPEC, api_client, body_limited};
    let unread = |e: &dyn std::fmt::Display| Error::Invalid(format!("couldn't read the lab's cloud/lab.yml: {e}"));
    let res = api_client()?.get(&url).send().await.map_err(|e| unread(&e))?;
    if res.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !res.status().is_success() {
        return Err(unread(&format!("HTTP {}", res.status())));
    }
    let body = body_limited(res, MAX_LAB_SPEC).await.map_err(|e| unread(&e))?;
    String::from_utf8(body).map(Some).map_err(|e| unread(&e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::Runtime;

    /// The cloud-bucket lab's file and module (a public S3 website), as its author writes them.
    const LAB_FILE: &str = "provider: aws\nhourly_usd: 0.02\nvars:\n  launch_token: \"{{ inputs.CTF_LAUNCH_TOKEN }}\"\nplayer_ip_var: allowed_cidr\noutputs: { site: website_url }\nmessage: |\n  Open {{ outputs.site }}.\n";
    const MODULE: &str = "variable \"launch_token\" {\n  type = string\n  sensitive = true\n}\nvariable \"allowed_cidr\" { type = string }\nvariable \"region\" {\n  type = string\n  default = \"us-east-1\"\n}\noutput \"website_url\" { value = \"http://example\" }\n";

    #[test]
    fn the_catalogues_runtimes_read_as_launcher_runtimes() {
        for (json, rt) in [("\"DOCKER\"", Runtime::Docker), ("\"VM\"", Runtime::Vm), ("\"CLOUD\"", Runtime::Cloud)] {
            assert_eq!(serde_json::from_str::<Runtime>(json).unwrap(), rt);
            assert_eq!(serde_json::to_string(&rt).unwrap(), json);
        }
        assert!(serde_json::from_str::<Runtime>("\"HOSTED\"").is_err());
    }

    #[test]
    fn cost_lines() {
        assert!(cost_line(Some(0.02), 4).contains("$0.02 an hour, so about $0.08"));
        assert!(cost_line(Some(0.5), 0).contains("until you stop it"));
        assert!(cost_line(None, 4).contains("no cost estimate"));
    }

    #[test]
    fn a_preview_says_the_labs_cloud_cost_inputs_allow_list_and_outputs() {
        let preview = preview(&LabFile::parse(LAB_FILE).unwrap());
        assert_eq!(
            preview,
            Preview { provider: "aws".into(), hourly_usd: Some(0.02), inputs: vec!["CTF_LAUNCH_TOKEN".into()], allow_list: true, outputs: vec!["site".into()] }
        );
    }

    #[test]
    fn the_module_is_read_for_its_variables_and_the_lab_files_values_checked_against_them() {
        let t = crate::testing::test_app::TestApp::new();
        let dir = t.lab("bucket", &[("cloud/lab.yml", LAB_FILE), ("cloud/main.tf", MODULE)]);
        let declared = isothrum::module_variables(&lab_file::module_dir(&dir)).unwrap();
        let launch = [("CTF_LAUNCH_TOKEN".to_owned(), "t0k".to_owned())];
        let values = values::module_values(&LabFile::read(&dir).unwrap(), &declared, &launch, Some("203.0.113.7")).unwrap();
        assert_eq!(values.plain, [("allowed_cidr".to_owned(), "203.0.113.7/32".to_owned())], "the region keeps its default");
        assert_eq!(values.secret, [("launch_token".to_owned(), "t0k".to_owned())]);
    }

    #[tokio::test]
    async fn a_start_missing_a_value_or_on_another_clouds_account_is_refused_before_anything_runs() {
        use crate::platform::exec::fake::Tools;
        let t = crate::testing::test_app::TestApp::new();
        let dir = t.lab("bucket", &[("cloud/lab.yml", LAB_FILE), ("cloud/main.tf", MODULE)]);
        let tools = Tools::strict_rules(&[]);
        let start = |host: &'static str, env: Vec<(String, String)>| {
            let (dirs, dir) = (t.dirs(), dir.clone());
            async move {
                let lab = LabRef { dir: &dir, id: "bucket", runtime: Runtime::Cloud };
                start(&dirs, lab, Some(host), &env, &mut |_| {}).await.unwrap_err().to_string()
            }
        };
        // No launch token: said before the account (no such account here) is read.
        assert_eq!(start("no-such-account", Vec::new()).await, "this lab's `launch_token` takes the launch's `CTF_LAUNCH_TOKEN`, which the launch didn't give");
        let azure = t.host("az", "azure", serde_json::json!({ "useCliCreds": true }));
        let azure: &'static str = Box::leak(azure.into_boxed_str());
        let err = start(azure, vec![("CTF_LAUNCH_TOKEN".into(), "t0k".into())]).await;
        assert!(err.starts_with("This lab runs on AWS, and Host az is a Azure account. Pick a AWS account."), "{err}");
        assert!(tools.calls().is_empty(), "{:?}", tools.calls());
        assert!(crate::runtime::recorded::Recorded::of(&t.dirs(), "bucket").await.unwrap().record().is_none(), "not recorded: nothing was created");
    }

    #[tokio::test]
    async fn a_preview_and_the_run_options_read_the_installed_lab_file_at_that_commit_once_its_source_is_allowed() {
        let t = crate::testing::test_app::TestApp::new();
        let commit = "0123456789abcdef0123456789abcdef01234567";
        t.lab("bucket", &[("cloud/lab.yml", LAB_FILE), ("cloud/main.tf", MODULE), (crate::runtime::lab::COMMIT_MARKER, commit)]);
        let source = |check| super::super::lab::Source { repository: "Org/bucket", commit, check };
        let got = preview_at(&t.dirs(), "bucket", source(|_, _| Ok(()))).await.unwrap();
        assert_eq!(got, Some(preview(&LabFile::parse(LAB_FILE).unwrap())));
        let options = run_options_at(&t.dirs(), "bucket", source(|_, _| Ok(()))).await.unwrap();
        assert_eq!(options.cloud_services.iter().map(|p| (p.provider, p.target.as_str())).collect::<Vec<_>>(), [(Some(Provider::Aws), "cloud-services")]);
        // A lab without a lab file runs no cloud services.
        let docker =
            "version: 1\nname: web\nnetworks:\n  lab: { cidr: 10.30.0.0/24 }\nmachines:\n  web:\n    networks: { lab: 10 }\n    app: { image: nginx:1.27 }\n";
        t.lab("web", &[("isoloom.yml", docker), (crate::runtime::lab::COMMIT_MARKER, commit)]);
        assert_eq!(preview_at(&t.dirs(), "web", source(|_, _| Ok(()))).await.unwrap(), None);
        assert!(run_options_at(&t.dirs(), "web", source(|_, _| Ok(()))).await.unwrap_err().to_string().ends_with("it runs no cloud services"));
        // The caller's rule for the source is asked first, and a bad id refused, before anything is read.
        let refused = preview_at(&t.dirs(), "bucket", source(|_, _| Err(Error::Invalid("not this source".into())))).await;
        assert_eq!(refused.unwrap_err().to_string(), "not this source");
        assert_eq!(preview_at(&t.dirs(), "../x", source(|_, _| Ok(()))).await.unwrap_err().to_string(), "invalid lab id `../x`");
    }

    #[tokio::test]
    async fn a_lab_file_not_installed_at_that_commit_is_fetched_and_a_missing_one_is_none() {
        let t = crate::testing::test_app::TestApp::new();
        let source = super::super::lab::Source { repository: "Org/bucket", commit: "c0ffee", check: |_, _| Ok(()) };
        let asked = std::sync::Mutex::new(Vec::new());
        let fetched = lab_file_at(&t.dirs(), "bucket", source, |url| {
            asked.lock().unwrap().push(url);
            async { Ok(Some(LAB_FILE.to_owned())) }
        })
        .await
        .unwrap();
        assert_eq!(fetched.as_deref(), Some(LAB_FILE));
        assert_eq!(*asked.lock().unwrap(), ["https://raw.githubusercontent.com/Org/bucket/c0ffee/cloud/lab.yml"]);
        assert_eq!(lab_file_at(&t.dirs(), "bucket", source, |_| async { Ok(None) }).await.unwrap(), None);
    }
}
