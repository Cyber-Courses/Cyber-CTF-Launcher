//! An old deployment's run record (`run.json` beside its Terraform state, written by the
//! launcher's own Terraform path, `terraform::state`), read as what the runner's adoption takes:
//! the params it was applied with, by the run manifest's names, and when it stops itself.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

/// The record's object holding the provider environment it kept.
const ENV: &str = "env";

/// When the deployment stops itself, in Unix seconds.
const EXPIRES_AT: &str = "expires_at";

/// The provider environment an old record kept, as the module variable each stands for: taken
/// only when the record kept no variable of that name, the first of a name winning (the
/// launcher's AWS path set `AWS_REGION`, its cloud-services path the GCP project and region).
const ENV_AS_PARAM: [(&str, &str); 5] = [
    ("ARM_SUBSCRIPTION_ID", "subscription_id"),
    ("AWS_REGION", "region"),
    ("AWS_DEFAULT_REGION", "region"),
    ("GOOGLE_PROJECT", "project"),
    ("GOOGLE_REGION", "region"),
];

/// What an old run record says, parsed once: a missing or malformed one says nothing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct OldRecord {
    /// The module variables it kept (strings only).
    vars: BTreeMap<String, String>,
    /// The provider environment it kept.
    env: BTreeMap<String, String>,
    /// When the deployment stops itself.
    expires_at: Option<u64>,
}

impl OldRecord {
    /// The record in `text` (the file's contents; empty when there is none).
    pub(crate) fn parse(text: &str) -> OldRecord {
        let Ok(Value::Object(mut run)) = serde_json::from_str::<Value>(text) else { return OldRecord::default() };
        let env = match run.remove(ENV) {
            Some(Value::Object(env)) => strings(env),
            _ => BTreeMap::new(),
        };
        let expires_at = run.get(EXPIRES_AT).and_then(Value::as_u64);
        OldRecord { vars: strings(run), env, expires_at }
    }

    /// When the deployment stops itself, in Unix seconds.
    pub(crate) fn expires_at(&self) -> Option<u64> {
        self.expires_at
    }

    /// The params it was applied with that a manifest takes (`takes`: the names of the params
    /// the embedder sets): each kept variable by its own name, then the kept environment by the
    /// variable it stands for; a name the manifest has no param for is left out.
    pub(crate) fn params(&self, takes: &BTreeSet<String>) -> BTreeMap<String, String> {
        let mut out: BTreeMap<String, String> = self.vars.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        for (var, param) in ENV_AS_PARAM {
            if let Some(value) = self.env.get(var).filter(|v| !v.is_empty()) {
                out.entry(param.to_owned()).or_insert_with(|| value.clone());
            }
        }
        out.retain(|name, _| takes.contains(name));
        out
    }
}

/// The string members of a JSON object.
fn strings(object: serde_json::Map<String, Value>) -> BTreeMap<String, String> {
    object.into_iter().filter_map(|(k, v)| v.as_str().map(|s| (k, s.to_owned()))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn takes(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|n| (*n).to_owned()).collect()
    }

    fn pairs(p: &BTreeMap<String, String>) -> Vec<(&str, &str)> {
        p.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect()
    }

    #[test]
    fn kept_variables_become_the_params_the_manifest_takes_and_the_rest_is_dropped() {
        let r = OldRecord::parse(
            r#"{"region":"eu-west-3","allowed_cidr":"203.0.113.7/32","ssh_public_key":"ssh-ed25519 AAAA","ssh_private_key_file":"/k/id","expires_at":4102444800}"#,
        );
        assert_eq!(pairs(&r.params(&takes(&["region", "allowed_cidr", "AWS_PROFILE"]))), [("allowed_cidr", "203.0.113.7/32"), ("region", "eu-west-3")]);
        assert_eq!(r.expires_at(), Some(4_102_444_800));
    }

    #[test]
    fn the_subscription_comes_from_the_kept_environment() {
        let r = OldRecord::parse(r#"{"region":"westeurope","env":{"ARM_SUBSCRIPTION_ID":"0000-1111"}}"#);
        assert_eq!(pairs(&r.params(&takes(&["region", "subscription_id"]))), [("region", "westeurope"), ("subscription_id", "0000-1111")]);
    }

    #[test]
    fn the_aws_region_fills_region_only_when_no_variable_was_kept() {
        let env_only = OldRecord::parse(r#"{"env":{"AWS_REGION":"us-east-1","AWS_DEFAULT_REGION":"us-west-2"}}"#);
        assert_eq!(pairs(&env_only.params(&takes(&["region"]))), [("region", "us-east-1")]);
        let both = OldRecord::parse(r#"{"region":"eu-west-3","env":{"AWS_REGION":"us-east-1"}}"#);
        assert_eq!(pairs(&both.params(&takes(&["region"]))), [("region", "eu-west-3")]);
    }

    #[test]
    fn a_gcp_projects_kept_variable_wins_over_its_environment() {
        let r = OldRecord::parse(r#"{"project":"labs-1","region":"europe-west1","env":{"GOOGLE_PROJECT":"other","GOOGLE_REGION":"us-central1"}}"#);
        assert_eq!(pairs(&r.params(&takes(&["project", "region"]))), [("project", "labs-1"), ("region", "europe-west1")]);
        let services = OldRecord::parse(r#"{"cloud_services":true,"env":{"GOOGLE_PROJECT":"labs-1","GOOGLE_REGION":"us-central1"}}"#);
        assert_eq!(pairs(&services.params(&takes(&["project", "region", "cloud_services"]))), [("project", "labs-1"), ("region", "us-central1")]);
    }

    #[test]
    fn a_missing_or_odd_record_says_nothing() {
        for raw in ["", "garbage", "[]", r#"{"expires_at":"1","env":"x","region":3}"#, r#"{"expires_at":-1}"#] {
            let r = OldRecord::parse(raw);
            assert_eq!((r.params(&takes(&["region"])).len(), r.expires_at()), (0, None), "{raw}");
        }
        let blank = OldRecord::parse(r#"{"env":{"AWS_REGION":""}}"#);
        assert!(blank.params(&takes(&["region"])).is_empty(), "an empty value is no region");
    }
}
