//! A cloud-services lab's run, from its own module: its status as its lab file says to show it
//! (what it exposes, by the lab's names, and its words with them filled in), and its checks
//! (pages that answer once it works).

use std::time::Duration;

use isothrum_runner::{OutputValue, RunState, Status};

use super::cloud_runs::CloudRun;
use crate::error::Result;
use crate::runtime::cloud_services::lab_file::{self, LabFile};
use crate::runtime::docker::{Check, CheckResult};
use crate::runtime::model::Output;
use crate::runtime::{LabStatus, Place};

/// How long a check's page may take to answer.
const CHECK_WAIT: Duration = Duration::from_secs(10);

impl CloudRun {
    /// The cloud-services lab as its page shows it: running once its run is ready, what it
    /// exposes and its words, from its lab file.
    pub(crate) async fn own_status(&self) -> Result<LabStatus> {
        let file = LabFile::read(&self.dir)?;
        // The values the lab gives its player, sensitive ones included, read back as they are.
        let status = self.read_with(&isothrum_runner::RunOptions::new().with_reveal()).await?;
        Ok(status_of(&file, &status))
    }

    /// The cloud-services lab's checks: each page its lab file names, at the address its outputs
    /// give, answering with a status below 400. Not available for a lab that names none.
    pub(crate) async fn own_check(&self) -> Result<Check> {
        let file = LabFile::read(&self.dir)?;
        if file.checks.is_empty() {
            return Ok(Check::default());
        }
        let status = self.read().await?;
        let client = crate::platform::http::probe_client(CHECK_WAIT, false)?;
        let mut results = Vec::new();
        for check in &file.checks {
            let reason = match lab_file::fill(&check.http, "outputs", |name| output(&file, &status, name).map(|o| o.value)) {
                Err(name) => format!("its output `{name}` has no value yet"),
                Ok(url) => match client.get(&url).send().await {
                    Ok(res) if res.status().as_u16() < 400 => String::new(),
                    Ok(res) => format!("{url} answered HTTP {}", res.status().as_u16()),
                    Err(e) => format!("{url} didn't answer: {e}"),
                },
            };
            results.push(CheckResult { name: check.http.clone(), from: "from this computer".into(), ok: reason.is_empty(), reason });
        }
        let output =
            results.iter().map(|r| if r.ok { format!("PASS {}", r.name) } else { format!("FAIL {}: {}", r.name, r.reason) }).collect::<Vec<_>>().join("\n");
        Ok(Check { available: true, ok: results.iter().all(|r| r.ok), output, results })
    }
}

/// The lab page's view of a cloud-services run `status`, as lab file `file` names what it shows.
fn status_of(file: &LabFile, status: &Status) -> LabStatus {
    let running = status.state == RunState::Ready;
    let outputs: Vec<Output> = file.outputs.keys().filter_map(|name| output(file, status, name)).collect();
    let message = file
        .message
        .as_deref()
        .filter(|_| running)
        .and_then(|m| lab_file::fill(m, "outputs", |name| outputs.iter().find(|o| o.name == name).map(|o| o.value.clone())).ok());
    LabStatus { running, place: Some(Place::Cloud), outputs, message, ..LabStatus::default() }
}

/// What the lab exposes as `name` (its lab file's name for one of the module's outputs), as the
/// run gives it: its value (a sensitive one in full where the runner read it here; masked when it
/// came through the service, which never sends one), none before it has one.
fn output(file: &LabFile, status: &Status, name: &str) -> Option<Output> {
    let module_name = file.outputs.get(name)?;
    let found: &OutputValue = status.outputs.iter().find(|o| &o.name == module_name)?;
    let value = found.revealed().or(found.value.as_deref())?;
    Some(Output { name: name.to_owned(), value: value.to_owned(), sensitive: found.sensitive })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(state: &str, outputs: &serde_json::Value) -> Status {
        serde_json::from_value(serde_json::json!({ "state": state, "live": true, "provider": null, "machines": [], "outputs": outputs })).unwrap()
    }

    const FILE: &str =
        "provider: aws\noutputs: { app: app_url, key: secret_key, later: not_yet }\nmessage: |\n  Open {{ outputs.app }} with {{ outputs.key }}.\n";

    #[test]
    fn a_ready_run_shows_its_outputs_by_the_labs_names_and_its_words_filled_in() {
        let file = LabFile::parse(FILE).unwrap();
        let outputs = serde_json::json!([
            { "name": "app_url", "sensitive": false, "value": "https://app.example" },
            { "name": "secret_key", "sensitive": true, "value": "***" },
            { "name": "unshown", "sensitive": false, "value": "x" },
        ]);
        let shown = status_of(&file, &status("ready", &outputs));
        assert!(shown.running);
        assert_eq!(
            shown.outputs,
            [
                Output { name: "app".into(), value: "https://app.example".into(), sensitive: false },
                Output { name: "key".into(), value: "***".into(), sensitive: true },
            ],
            "by the lab's names, an output not given yet left out, one the lab doesn't name never shown"
        );
        assert_eq!(shown.message.as_deref(), Some("Open https://app.example with ***.\n"));
        let down = status_of(&file, &status("absent", &serde_json::json!([])));
        assert_eq!((down.running, down.outputs.len(), down.message), (false, 0, None), "nothing up: nothing to show");
        let failed = status_of(&file, &status("failed", &outputs));
        assert!(!failed.running && failed.message.is_none(), "a failed run says no words of a working lab");
    }

    /// The cloud-bucket lab, run by the runner on scripted Terraform: what the start hands the
    /// module (`TF_VAR_<name>`, the sensitive one as a secret), and what its outputs read back as.
    #[tokio::test]
    async fn a_cloud_services_lab_runs_its_module_with_its_values_and_reads_its_outputs_back() {
        use isothrum_runner::fake::{FakeProcesses, Reply};
        use std::sync::Arc;
        let root = crate::testing::temp_dir("cloud-bucket");
        let dir = root.join("labs").join("bucket");
        let module = lab_file::module_dir(&dir);
        std::fs::create_dir_all(&module).unwrap();
        std::fs::write(module.join(lab_file::LAB_FILE), "provider: aws\nvars: { launch_token: \"{{ inputs.CTF_LAUNCH_TOKEN }}\" }\nplayer_ip_var: allowed_cidr\noutputs: { site: website_url, key: access_key }\nmessage: Open {{ outputs.site }}.\n").unwrap();
        std::fs::write(
            module.join("main.tf"),
            "variable \"launch_token\" {\n  type      = string\n  sensitive = true\n}\nvariable \"allowed_cidr\" { type = string }\n",
        )
        .unwrap();
        let fake = Arc::new(FakeProcesses::model(|call| match call.args.first().map(String::as_str) {
            Some("version") => Reply::ok(r#"{"terraform_version":"1.10.5"}"#),
            Some("show") => Reply::ok(
                r#"{"values":{"root_module":{"resources":[{"address":"aws_s3_bucket.site","mode":"managed","type":"aws_s3_bucket","values":{"id":"bucket-1"}}]}}}"#,
            ),
            Some("output") => Reply::ok(
                r#"{"website_url":{"value":"http://bucket.example","type":"string","sensitive":false},"access_key":{"value":"AKIA","type":"string","sensitive":true}}"#,
            ),
            Some("apply") => crate::runtime::isothrum::scripted_apply(call),
            _ => Reply::ok(""),
        }));
        let mut config = isothrum_runner::RunnerConfig::new(crate::runtime::paths::runner_home_in(&root));
        config.processes = Arc::clone(&fake) as Arc<dyn isothrum_runner::ProcessRunner>;
        let run = CloudRun::own_with(config, &dir, "bucket").unwrap();
        let declared = crate::runtime::isothrum::module_variables(&module).unwrap();
        let file = LabFile::read(&dir).unwrap();
        let launch = [("CTF_LAUNCH_TOKEN".to_owned(), "t0k".to_owned())];
        let values = crate::runtime::cloud_services::values::module_values(&file, &declared, &launch, Some("203.0.113.7")).unwrap();
        let place = super::super::CloudPlace { label: "acct".into(), ..super::super::CloudPlace::default() };
        let mut log = Vec::new();
        run.up_own(&place, &values, &mut |l| log.push(l)).await.unwrap();
        let apply = fake.calls().into_iter().find(|c| c.program == "terraform" && c.args.first().is_some_and(|a| a == "apply")).expect("an apply");
        assert_eq!(apply.env("TF_VAR_allowed_cidr"), Some("203.0.113.7/32"), "this machine's IP in the lab's allow-list variable");
        assert_eq!(apply.env("TF_VAR_launch_token"), Some("t0k"), "the launch's value in its variable");
        let shown = run.own_status().await.unwrap();
        assert!(shown.running, "{shown:?}");
        assert_eq!(
            shown.outputs.iter().map(|o| (o.name.as_str(), o.value.as_str(), o.sensitive)).collect::<Vec<_>>(),
            [("key", "AKIA", true), ("site", "http://bucket.example", false)]
        );
        assert_eq!(shown.message.as_deref(), Some("Open http://bucket.example."));
        assert_eq!(log.last().map(String::as_str), Some("Open http://bucket.example."), "the lab's words end the start's log");
        let _ = std::fs::remove_dir_all(root);
    }
}
