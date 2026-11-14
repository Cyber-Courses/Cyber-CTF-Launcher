//! A cloud-services lab's own file, `cloud/lab.yml`, beside its hand-written Terraform module
//! (`cloud/*.tf`): what the launcher needs to know of the lab that the module doesn't say. Isoloom
//! produces nothing for such a lab, and Isothrum runs the module as any Terraform folder.
//!
//! ```yaml
//! provider: aws                 # the cloud whose account runs it: aws, azure or gcp
//! hourly_usd: 0.02              # what it costs an hour, the lab's own estimate
//! vars:                         # values for the module's variables
//!   region: us-east-1
//!   launch_token: "{{ inputs.CTF_LAUNCH_TOKEN }}"
//! player_ip_var: allowed_cidr   # the variable given this machine's public IP, as a /32 (or /128)
//! outputs:                      # what the lab page shows, by the lab's name, from the module's outputs
//!   app: app_url
//!   key: access_key_id
//! checks:                       # pages that answer once the lab works
//!   - http: "{{ outputs.app }}"
//! message: |
//!   Sign in with {{ outputs.key }} at {{ outputs.app }}.
//! ```

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::{Error, Result};
use crate::runtime::providers::Provider;

/// The folder of a cloud-services lab's module and lab file, in the lab's folder.
pub(crate) const MODULE_DIR: &str = "cloud";

/// The lab file's name, in [`MODULE_DIR`].
pub(crate) const LAB_FILE: &str = "lab.yml";

/// A cloud-services lab's file ([`LAB_FILE`]).
#[derive(Deserialize, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct LabFile {
    /// The cloud whose account runs it.
    pub(crate) provider: Cloud,
    /// What it costs an hour, in US dollars (the lab's own estimate).
    #[serde(default)]
    pub(crate) hourly_usd: Option<f64>,
    /// Values for the module's variables, by variable: text, where `{{ inputs.NAME }}` takes the
    /// launch's value `NAME`.
    #[serde(default)]
    pub(crate) vars: BTreeMap<String, String>,
    /// The module's variable given this machine's public IP as a one-address block, when the lab
    /// lets only this machine in somewhere.
    #[serde(default)]
    pub(crate) player_ip_var: Option<String>,
    /// What the lab page shows once it is up: the lab's name for each, and the module's output.
    #[serde(default)]
    pub(crate) outputs: BTreeMap<String, String>,
    /// What answers once the lab works.
    #[serde(default)]
    pub(crate) checks: Vec<Check>,
    /// The lab's words once it is up, where `{{ outputs.NAME }}` takes output `NAME`'s value.
    #[serde(default)]
    pub(crate) message: Option<String>,
}

/// A cloud a cloud-services lab runs in.
#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Cloud {
    /// Amazon Web Services.
    Aws,
    /// Microsoft Azure.
    Azure,
    /// Google Cloud.
    Gcp,
}

impl Cloud {
    /// The launcher's provider for it (whose accounts can run the lab).
    pub(crate) fn provider(self) -> Provider {
        match self {
            Cloud::Aws => Provider::Aws,
            Cloud::Azure => Provider::Azure,
            Cloud::Gcp => Provider::Gcp,
        }
    }
}

/// One check of a cloud-services lab: a page that answers (any status below 400) once it works.
#[derive(Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Check {
    /// Its address, where `{{ outputs.NAME }}` takes output `NAME`'s value.
    pub(crate) http: String,
}

/// The lab's module folder, in its folder `dir`.
pub(crate) fn module_dir(dir: &Path) -> PathBuf {
    dir.join(MODULE_DIR)
}

impl LabFile {
    /// The lab file of the lab in `dir`.
    ///
    /// # Errors
    ///
    /// When it is missing or can't be read, saying which file and why.
    pub(crate) fn read(dir: &Path) -> Result<LabFile> {
        let path = module_dir(dir).join(LAB_FILE);
        let text = std::fs::read_to_string(&path).map_err(|e| Error::Invalid(format!("this lab's {MODULE_DIR}/{LAB_FILE} can't be read: {e}")))?;
        LabFile::parse(&text)
    }

    /// The lab file `text`.
    ///
    /// # Errors
    ///
    /// When it isn't one, saying what is wrong.
    pub(crate) fn parse(text: &str) -> Result<LabFile> {
        let file: LabFile = serde_yaml_ng::from_str(text).map_err(|e| Error::Invalid(format!("this lab's {MODULE_DIR}/{LAB_FILE}: {e}")))?;
        if file.hourly_usd.is_some_and(|h| !h.is_finite() || h < 0.0) {
            return Err(Error::Invalid(format!("this lab's {MODULE_DIR}/{LAB_FILE}: hourly_usd must be a positive number of US dollars")));
        }
        Ok(file)
    }

    /// The launch's values its variables take (`{{ inputs.NAME }}`), each once, in their order.
    pub(crate) fn inputs(&self) -> Vec<String> {
        self.vars.values().flat_map(|v| placeholders(v, "inputs")).fold(Vec::new(), |mut all, i| {
            if !all.contains(&i) {
                all.push(i);
            }
            all
        })
    }
}

/// The names of the `{{ <scope>.NAME }}` placeholders in `text`, in order.
pub(crate) fn placeholders(text: &str, scope: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("{{") {
        let Some(len) = rest[start..].find("}}") else { break };
        let inner = rest[start + 2..start + len].trim();
        if let Some(name) = inner.strip_prefix(scope).and_then(|n| n.strip_prefix('.')) {
            found.push(name.to_owned());
        }
        rest = &rest[start + len + 2..];
    }
    found
}

/// `text` with each `{{ <scope>.NAME }}` replaced by `value(NAME)`.
///
/// # Errors
///
/// The first name `value` has none for (`Err(NAME)`).
pub(crate) fn fill(text: &str, scope: &str, value: impl Fn(&str) -> Option<String>) -> std::result::Result<String, String> {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find("{{") {
        let Some(len) = rest[start..].find("}}") else { break };
        let inner = rest[start + 2..start + len].trim();
        out.push_str(&rest[..start]);
        match inner.strip_prefix(scope).and_then(|n| n.strip_prefix('.')) {
            Some(name) => out.push_str(&value(name).ok_or_else(|| name.to_owned())?),
            None => out.push_str(&rest[start..start + len + 2]),
        }
        rest = &rest[start + len + 2..];
    }
    out.push_str(rest);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = "provider: aws\nhourly_usd: 0.02\nvars:\n  region: us-east-1\n  launch_token: \"{{ inputs.CTF_LAUNCH_TOKEN }}\"\n  tag: \"lab-{{ inputs.CTF_LAUNCH_TOKEN }}-{{ inputs.CTF_API_URL }}\"\nplayer_ip_var: allowed_cidr\noutputs: { app: app_url, key: access_key_id }\nchecks:\n  - http: \"{{ outputs.app }}\"\nmessage: |\n  Sign in with {{ outputs.key }} at {{ outputs.app }}.\n";

    #[test]
    fn a_lab_file_reads_with_every_field_and_its_inputs_each_once() {
        let file = LabFile::parse(FILE).unwrap();
        assert_eq!(file.provider, Cloud::Aws);
        assert_eq!(file.provider.provider(), Provider::Aws);
        assert_eq!(file.hourly_usd, Some(0.02));
        assert_eq!(file.player_ip_var.as_deref(), Some("allowed_cidr"));
        assert_eq!(file.outputs, BTreeMap::from([("app".into(), "app_url".into()), ("key".into(), "access_key_id".into())]));
        assert_eq!(file.checks, [Check { http: "{{ outputs.app }}".into() }]);
        assert_eq!(file.message.as_deref(), Some("Sign in with {{ outputs.key }} at {{ outputs.app }}.\n"));
        assert_eq!(file.inputs(), ["CTF_LAUNCH_TOKEN", "CTF_API_URL"]);
    }

    #[test]
    fn a_lab_file_with_nothing_but_its_cloud_reads_and_one_that_isnt_says_why() {
        let bare = LabFile::parse("provider: gcp\n").unwrap();
        assert_eq!((bare.provider, bare.vars.len(), bare.outputs.len(), bare.checks.len()), (Cloud::Gcp, 0, 0, 0));
        for (text, says) in [
            ("hourly_usd: 1\n", "missing field `provider`"),
            ("provider: digitalocean\n", "unknown variant `digitalocean`"),
            ("provider: aws\nbefore: [keys.sh]\n", "unknown field `before`"),
            ("provider: aws\nhourly_usd: -1\n", "hourly_usd must be a positive number"),
            ("provider: aws\nchecks: [{ tcp: 22 }]\n", "unknown field `tcp`"),
        ] {
            let err = LabFile::parse(text).unwrap_err().to_string();
            assert!(err.starts_with("this lab's cloud/lab.yml") && err.contains(says), "{text}: {err}");
        }
    }

    #[test]
    fn a_lab_file_is_read_from_the_labs_cloud_folder() {
        let dir = crate::testing::temp_dir("cloud-lab-file");
        assert!(LabFile::read(&dir).unwrap_err().to_string().starts_with("this lab's cloud/lab.yml can't be read: "));
        std::fs::create_dir_all(module_dir(&dir)).unwrap();
        std::fs::write(module_dir(&dir).join(LAB_FILE), FILE).unwrap();
        assert_eq!(LabFile::read(&dir).unwrap(), LabFile::parse(FILE).unwrap());
    }

    #[test]
    fn placeholders_are_filled_by_scope_and_a_missing_value_is_named() {
        assert_eq!(placeholders("a {{inputs.X}} {{ outputs.Y }} {{ inputs.Z }}", "inputs"), ["X", "Z"]);
        let value = |n: &str| (n == "X").then(|| "1".to_owned());
        assert_eq!(fill("a{{ inputs.X }}b {{ outputs.Y }} {{ unclosed", "inputs", value).unwrap(), "a1b {{ outputs.Y }} {{ unclosed");
        assert_eq!(fill("{{ inputs.Z }}", "inputs", value), Err("Z".to_owned()));
    }
}
