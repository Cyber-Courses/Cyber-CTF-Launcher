//! A claimed capability must change the output: each capability a row of the host kinds table
//! ([`KINDS`]) claims, which the hosts' setup and the launch screens show for its provider, has a
//! witness here that the claim changes what is run or generated. The claims are read from every
//! row (a row added is checked as it is), and every field of a row is either a claim with a
//! witness or named as no claim: a field added to [`Kind`] fails to compile here until it is one
//! or the other.

use super::contract::{connection_env, terraform_env};
use super::kind::{KINDS, Kind, Secret};
use super::profile::tests::profile;
use crate::runtime::Runtime;
use crate::runtime::lab::{host_vars, terraform};
use crate::runtime::providers::Provider;

/// What a row claims its hosts do.
#[derive(Debug)]
enum Claim {
    /// Its labs run through this Terraform target (its Isoloom module).
    Terraform(&'static str),
    /// A host's instance size goes to this module variable.
    SizeVar(&'static str),
    /// The keychain keeps a secret for it, which reaches the launch.
    Secret,
    /// The account's sign-in is its cloud's CLI's, which isothrum-machine asks whether it holds.
    CliSignIn,
}

/// Every claim of row `k`. The destructuring names each field: a new one is a claim or not.
fn claims(k: &Kind) -> Vec<Claim> {
    let Kind {
        provider: _,
        terraform,
        // What the setup form fills in and checks (its own tests): no claim on what runs.
        default_port: _,
        host: _,
        region: _,
        account: _,
        account_shape: _,
        size_var,
        secret,
        // How the secret reaches Terraform: the `Secret` claim's witness reads its output.
        terraform_env: _,
        cli_sign_in,
    } = k;
    let mut out = Vec::new();
    out.extend(terraform.map(Claim::Terraform));
    out.extend(size_var.map(Claim::SizeVar));
    if *secret != Secret::None {
        out.push(Claim::Secret);
    }
    out.extend(cli_sign_in.as_ref().map(|_| Claim::CliSignIn));
    out
}

/// Why `claim` of `provider`'s row has no witness, or None when it changes the output.
fn unwitnessed(provider: Provider, claim: &Claim) -> Option<String> {
    let dir = std::path::Path::new("lab");
    match claim {
        // The module the lab runs from is the target's own, apart from every other target's.
        Claim::Terraform(t) => {
            let own = terraform(dir, Runtime::Docker, t).0;
            let mut others = KINDS.iter().filter_map(|k| k.terraform).filter(|o| o != t);
            others.any(|o| terraform(dir, Runtime::Docker, o).0 == own).then(|| format!("{t}'s module is another target's: {}", own.display()))
        }
        // The size set on the host reaches the module under that name; none set, none given.
        Claim::SizeVar(var) => {
            let sized = host_vars(&super::HostProfile { datastore: Some("witness-size".into()), ..profile(provider) }, "");
            let bare = host_vars(&super::HostProfile { datastore: None, ..profile(provider) }, "");
            let given = sized.iter().any(|(k, v)| k == var && v == "witness-size") && !bare.iter().any(|(k, _)| k == var);
            (!given).then(|| format!("the size doesn't reach `{var}`: {sized:?}"))
        }
        // The kept secret is in what the launch is given: the runner's environment, the module's
        // variables or Terraform's own environment.
        Claim::Secret => {
            let h = profile(provider);
            let given = [connection_env(&h, "witness-secret"), host_vars(&h, "witness-secret"), terraform_env(&h, "witness-secret")];
            (!given.iter().flatten().any(|(_, v)| v.contains("witness-secret"))).then(|| "the secret reaches nothing the launch is given".to_owned())
        }
        // isothrum-machine asks the cloud's CLI whether the sign-in holds.
        Claim::CliSignIn => {
            let cloud = isothrum_runner::from_wire_name::<isothrum_machine::cloud::Cloud>(provider.id());
            cloud.and_then(isothrum_machine::cloud::cli).is_none().then(|| "isothrum-machine has no CLI to ask its sign-in".to_owned())
        }
    }
}

#[test]
fn every_claim_of_the_host_kinds_table_changes_what_runs() {
    let mut missing = Vec::new();
    let mut witnessed = 0;
    for k in &KINDS {
        for claim in claims(k) {
            match unwitnessed(k.provider, &claim) {
                Some(why) => missing.push(format!("{:?} {claim:?}: {why}", k.provider)),
                None => witnessed += 1,
            }
        }
    }
    assert!(missing.is_empty(), "claims without a witness:\n{}", missing.join("\n"));
    // The table claims something of every kind of claim: a witness that can't fail isn't one.
    assert!(witnessed >= KINDS.len(), "only {witnessed} claims read from {} rows", KINDS.len());
}

/// The witnesses can fail: a claim the table doesn't keep has none.
#[test]
fn a_claim_the_code_does_not_keep_has_no_witness() {
    assert!(unwitnessed(Provider::Aws, &Claim::SizeVar("not_a_module_variable")).is_some());
    assert!(unwitnessed(Provider::Linode, &Claim::CliSignIn).is_some(), "a token cloud has no CLI sign-in");
    assert!(unwitnessed(Provider::Gcp, &Claim::Secret).is_some(), "GCP keeps no secret: nothing carries one");
}
