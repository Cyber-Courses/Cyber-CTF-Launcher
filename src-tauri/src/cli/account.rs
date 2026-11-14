//! The servers, cloud and account commands, through the functions the Servers and Cloud pages'
//! and the sign-in's commands call.

use serde_json::{Value, json};

use super::args::{Account, Cloud, CloudArg, Servers};
use super::{Answer, Context, Exit, Failure, Output, to_json};
use crate::cloud_cli;
use crate::commands::server::{host_list, public_key_in, set_default_in};
use crate::runtime::server::{connection_test, setup};

/// Runs a servers command.
pub(super) async fn servers(command: Servers, ctx: &Context, out: &mut Output<'_>) -> Exit {
    match command {
        Servers::List => {
            let answer = host_list(&ctx.dirs).map_err(Failure::from).and_then(|l| to_json(&l));
            out.finish(answer, servers_text)
        }
        Servers::Show { id } => {
            let answer = host_list(&ctx.dirs).map_err(Failure::from).and_then(|l| to_json(&l)).and_then(|l| one_host(&l, &id));
            out.finish(answer, |h| serde_json::to_string_pretty(h).unwrap_or_default())
        }
        Servers::Test { id } => {
            let answer = connection_test::test_connection(&ctx.dirs, &id).await.map_err(Failure::from).and_then(|r| to_json(&r));
            out.finish(answer, |r| format!("{}: {}", if r["ok"] == true { "ok" } else { "FAILED" }, r["message"].as_str().unwrap_or_default()))
        }
        Servers::Remove { id } => {
            let answer = ctx
                .confirm(out, &format!("Remove the saved server {id}"))
                .and_then(|()| setup::remove(&ctx.dirs, &id).map(|()| json!({ "removed": id })).map_err(Failure::from));
            out.finish(answer, |_| "Removed.".to_owned())
        }
        Servers::SetDefault { id, none } => {
            let answer = if id.is_none() && !none {
                Err(Failure::usage("name the server, or --none"))
            } else {
                set_default_in(&ctx.dirs, id.clone()).map(|()| json!({ "default": id })).map_err(Failure::from)
            };
            out.finish(answer, |_| "Done.".to_owned())
        }
        Servers::PublicKey => {
            let answer = public_key_in(&ctx.dirs).await.map(|k| json!({ "publicKey": k })).map_err(Failure::from);
            out.finish(answer, |k| k["publicKey"].as_str().unwrap_or_default().to_owned())
        }
    }
}

/// The saved host `id` of the list `l`.
fn one_host(l: &Value, id: &str) -> Answer {
    let host = l["hosts"].as_array().into_iter().flatten().find(|h| h["id"] == id).cloned();
    let mut host = host.ok_or_else(|| Failure::not_ready(format!("no saved server or cloud account `{id}`")))?;
    if let Some(h) = host.as_object_mut() {
        h.insert("default".into(), json!(l["default"] == id));
    }
    Ok(host)
}

/// The host list as text: one a line, the default marked.
fn servers_text(l: &Value) -> String {
    let rows: Vec<String> = l["hosts"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|h| {
            let mark = if l["default"] == h["id"] { "*" } else { " " };
            format!(
                "{mark} {:<24} {:<14} {}",
                h["id"].as_str().unwrap_or_default(),
                h["provider"].as_str().unwrap_or_default(),
                h["name"].as_str().unwrap_or_default()
            )
        })
        .collect();
    if rows.is_empty() { "No server or cloud account is saved.".into() } else { rows.join("\n") }
}

/// Runs a cloud command.
pub(super) async fn cloud(command: Cloud, out: &mut Output<'_>) -> Exit {
    match command {
        Cloud::Status => {
            let answer = cloud_status().await;
            out.finish(answer, cloud_text)
        }
        Cloud::Signin { cloud, profile } => {
            let mut on_line = |line: String| out.event(&json!({ "event": "line", "line": line }), Some(&line));
            let done = match cloud {
                CloudArg::Aws => cloud_cli::aws_login(profile, &mut on_line).await,
                CloudArg::Azure => cloud_cli::cloud_login(cloud_cli::Cloud::Azure, &mut on_line).await,
                CloudArg::Gcp => cloud_cli::cloud_login(cloud_cli::Cloud::Gcp, &mut on_line).await,
            };
            out.finish(done.map(|()| json!({ "signedIn": true })).map_err(Failure::from), |_| "Signed in.".to_owned())
        }
    }
}

/// What each cloud CLI is signed in as: the Cloud page's accounts.
async fn cloud_status() -> Answer {
    let aws = cloud_cli::aws_cli_identity(None).await;
    let profiles = cloud_cli::aws_profiles().await;
    let azure = cloud_cli::azure_subscriptions().await;
    let gcp = cloud_cli::gcp_account().await;
    let oci = cloud_cli::oci_config();
    Ok(json!({
        "aws": { "identity": aws, "profiles": profiles },
        "azure": { "subscriptions": to_json(&azure)? },
        "gcp": { "account": gcp },
        "oci": to_json(&oci)?,
    }))
}

/// The cloud status as text: one cloud a line.
fn cloud_text(c: &Value) -> String {
    let or_none = |v: &Value| v.as_str().unwrap_or("not signed in").to_owned();
    let azure = c["azure"]["subscriptions"].as_array().map_or(0, Vec::len);
    [
        format!("aws: {}", or_none(&c["aws"]["identity"])),
        format!("azure: {azure} subscriptions"),
        format!("gcp: {}", or_none(&c["gcp"]["account"])),
        format!("oci: {}", if c["oci"]["configured"] == true { "configured" } else { "not configured" }),
    ]
    .join("\n")
}

/// Runs an account command.
pub(super) async fn account(command: Account, out: &mut Output<'_>) -> Exit {
    match command {
        Account::Status => {
            let status = crate::account::auth::auth_status().await;
            out.finish(to_json(&status), account_text)
        }
        Account::Signin { browser } => {
            let status = if browser {
                crate::account::auth::auth_login(|url| {
                    out.event(&json!({ "event": "signInLink", "url": url.as_str() }), Some(&format!("Open this link to sign in: {url}")));
                })
                .await
            } else {
                crate::account::auth::auth_login_device(|code| {
                    let code = serde_json::to_value(code).unwrap_or(Value::Null);
                    let text = format!(
                        "Go to {} and enter the code {}",
                        code["verificationUri"].as_str().unwrap_or_default(),
                        code["userCode"].as_str().unwrap_or_default()
                    );
                    out.event(&json!({ "event": "deviceCode", "code": code }), Some(&text));
                })
                .await
            };
            out.finish(status.map_err(Failure::from).and_then(|s| to_json(&s)), account_text)
        }
        Account::Signout => {
            let done = crate::account::auth::auth_logout().await;
            out.finish(done.map(|()| json!({ "signedOut": true })).map_err(Failure::from), |_| "Signed out.".to_owned())
        }
    }
}

/// The account's status as text.
fn account_text(s: &Value) -> String {
    if s["loggedIn"] == true {
        format!("Signed in as {} <{}>", s["name"].as_str().unwrap_or_default(), s["email"].as_str().unwrap_or_default())
    } else {
        s["keychainError"].as_str().map_or_else(|| "Signed out.".to_owned(), |why| format!("Signed out ({why})"))
    }
}
