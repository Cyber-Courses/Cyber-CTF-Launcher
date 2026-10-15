//! Starting a lab on a server host: ESXi through Vagrant, Proxmox and the clouds through
//! Terraform, with the AWS budget check and this machine's public IP for the firewall rules.

use super::*;

pub(super) async fn start_on_server(
    app: &AppHandle,
    dir: &Path,
    id: &str,
    runtime: Runtime,
    host: &str,
    env: &[(String, String)],
    log: &mut impl FnMut(String),
) -> Result<()> {
    // GCP labs run in the account's labs project: make sure it exists (one free billing slot).
    if server::connection(app, host)?.provider == Provider::Gcp {
        log("Checking the Cyber CTF labs project on Google Cloud…".into());
        server::gcp::labs_project(app, host).await?;
    }
    let conn = server::connection(app, host)?;
    // Mark first, so a half-created lab can still be destroyed on the same host.
    server::mark_lab(dir, Some(host))?;
    log(format!("Running on server host {} ({})", conn.name, conn.provider.id()));
    match server::terraform_target(conn.provider) {
        Some(tf) => start_terraform(app, dir, id, runtime, host, tf, conn, env, log).await,
        // ESXi: the same Vagrantfiles as on this machine, with the vmware_esxi provider.
        None => {
            let spec = lab::prepare(dir, lab::vagrant_target(runtime))?;
            let env: Vec<(String, String)> = env.iter().cloned().chain(conn.env).collect();
            let vagrant = lab::vagrant_dir(dir, runtime);
            // Switched here from a local (or other) provider: clear the stale state first, so the
            // ESXi start doesn't refuse with "an active machine was found with a different provider".
            vm::reconcile_provider(&vagrant, conn.provider, &env, log).await;
            vm::start(&vagrant, conn.provider, &env, &mut *log).await?;
            if runtime == Runtime::Docker {
                attack_box::in_lab_vm_or_note(&vagrant, &spec, &env, log).await;
            }
            finish_vagrant(dir, &spec, runtime, log);
            Ok(())
        }
    }
}

/// Proxmox and the clouds: Isoloom's Terraform for the lab's target, applied with the host's
/// variables, the launcher's SSH key and (on a cloud) a firewall locked to this machine.
#[allow(clippy::too_many_arguments)]
pub(super) async fn start_terraform(
    app: &AppHandle,
    dir: &Path,
    id: &str,
    runtime: Runtime,
    host: &str,
    tf: &'static str,
    conn: server::Connection,
    env: &[(String, String)],
    log: &mut impl FnMut(String),
) -> Result<()> {
    let provider = conn.provider;
    let (module, target) = lab::terraform(dir, runtime, tf)?;
    let spec = lab::prepare(dir, target)?;
    // The launcher's key: Terraform copies the lab over SSH with it, and "Open shell"
    // reaches the attack box on the lab host.
    let (key, public) = ssh::ensure_key(app).await?;
    // A Proxmox lab bridge this machine can't route to: Terraform and the launcher reach
    // the VM through the node.
    let jump = server::proxmox_jump(app, dir, &key, &public).await?;
    if jump.is_some() {
        log("The lab bridge is internal to the node: reaching the lab VM through the node.".into());
    }
    // Every cloud firewall opens SSH and the published ports to this machine's public IP only.
    let allowed_ip = if provider.is_cloud() { Some(public_ip().await?) } else { None };
    let vars = lab::run_vars(&conn.tf_vars, lab::inputs_json(&spec, env), jump.is_some(), public, &key, allowed_ip.as_deref());
    if provider == Provider::Aws {
        check_aws_budget(app, host, log).await?;
    }
    if provider.is_cloud() {
        log(format!("This lab runs in your {} account and is billed there until you stop it.", provider.id().to_uppercase()));
    }
    let state = state_dir(app, id, tf)?;
    std::fs::create_dir_all(&state)?;
    // Which output ran (containers or one VM per machine), and on which host, for the auto-stop
    // reaper (which also destroys a state left behind once the lab runs elsewhere).
    record_runtime(&state, runtime)?;
    record_host(&state, host)?;
    match &jump {
        Some(login) => std::fs::write(state.join(terraform::JUMP_FILE), login)?,
        None => {
            let _ = std::fs::remove_file(state.join(terraform::JUMP_FILE));
        }
    }
    terraform::apply(&module, &state, &vars, &conn.tf_env, &mut *log).await?;
    if runtime == Runtime::Docker {
        // The lab is up at this point: a failing attack box is noted, not a failed deploy (the
        // lab still gets recorded and its welcome shown).
        attack_box::on_terraform_host_or_note(app, id, &spec, env, log).await;
    }
    welcome(dir, &spec, lab::vagrant_target(runtime), log);
    Ok(())
}

/// Stops before spending if this AWS account is over its monthly budget, or if a budget is set
/// and the spend is over it; if the spend can't be read, warns and lets the launch go on.
pub(in crate::runtime) async fn check_aws_budget(app: &AppHandle, host: &str, log: &mut impl FnMut(String)) -> Result<()> {
    budget_verdict(server::check_budget(app, host).await, log)
}

/// What a budget check means for the launch: refused over budget, a note when unverifiable.
pub(super) fn budget_verdict(check: server::BudgetCheck, log: &mut impl FnMut(String)) -> Result<()> {
    match check {
        server::BudgetCheck::Over(spent, limit) => Err(Error::Invalid(format!(
            "Monthly budget reached for this account: ${spent:.2} of ${limit:.2} spent this month. Raise the budget in the account settings, or wait until next month."
        ))),
        // Best effort: if the spend can't be read (Cost Explorer off, expired session,
        // Docker/CLI missing), note it and launch anyway. The lab still auto-stops, so
        // cost is bounded; blocking every launch over this would be too aggressive.
        server::BudgetCheck::Unverifiable(why) => {
            log(format!("Monthly budget not checked: {why} Launching anyway; the lab still auto-stops."));
            Ok(())
        }
        server::BudgetCheck::Ok => Ok(()),
    }
}

/// This machine's public IPv4, for cloud firewall rules.
pub(in crate::runtime) async fn public_ip() -> Result<String> {
    // Independent services, run by different companies: two of them must see the same address
    // (see `public_ip_from`), so one that is down or blocked doesn't fail the launch, and one that
    // answers wrong (compromised, or a captive portal's page) doesn't open the lab to someone else.
    const RESOLVERS: [&str; 4] = ["https://checkip.amazonaws.com", "https://api.ipify.org", "https://ifconfig.me/ip", "https://icanhazip.com"];
    // Unit tests never reach the internet: a documentation address stands in.
    if cfg!(test) {
        return Ok("203.0.113.7".into());
    }
    public_ip_from(&RESOLVERS).await
}

/// The public IPv4 `resolvers` agree on, all asked at once (each with its own short timeout, so
/// a hanging one doesn't stall the launch). At least two must give it, and none another one:
/// the lab's firewall opens to this address, so a single service's word isn't enough, and when
/// they disagree (a VPN or a second internet connection, or a service lying) the launch stops
/// rather than guess.
pub(super) async fn public_ip_from(resolvers: &[&str]) -> Result<String> {
    let client = reqwest::Client::builder().timeout(std::time::Duration::from_secs(8)).build().map_err(|e| Error::Invalid(e.to_string()))?;
    let mut asked = tokio::task::JoinSet::new();
    for (i, url) in resolvers.iter().enumerate() {
        let (client, url) = (client.clone(), url.to_string());
        asked.spawn(async move { (i, ask_ip(&client, &url).await) });
    }
    let mut answers: Vec<(usize, std::result::Result<std::net::Ipv4Addr, String>)> = asked.join_all().await;
    answers.sort_by_key(|(i, _)| *i);
    agreed_ip(resolvers, &answers.into_iter().map(|(_, a)| a).collect::<Vec<_>>())
}

/// One resolver's answer: a public IPv4 address, or what went wrong.
async fn ask_ip(client: &reqwest::Client, url: &str) -> std::result::Result<std::net::Ipv4Addr, String> {
    let resp = client.get(url).send().await.and_then(|r| r.error_for_status()).map_err(|e| e.to_string())?;
    // An address is a few bytes: a page (a captive portal) is no answer.
    let body = crate::platform::http::body_limited(resp, 256).await.map_err(|e| e.to_string())?;
    let ip = parse_ipv4(&String::from_utf8_lossy(&body)).ok_or("an unexpected answer")?;
    let ip: std::net::Ipv4Addr = ip.parse().map_err(|_| "an unexpected answer")?;
    if !is_public_ipv4(ip) { Err(format!("{ip}, not a public address")) } else { Ok(ip) }
}

/// The verdict on the resolvers' answers (in the order of `resolvers`).
pub(super) fn agreed_ip(resolvers: &[&str], answers: &[std::result::Result<std::net::Ipv4Addr, String>]) -> Result<String> {
    let seen: Vec<(&str, std::net::Ipv4Addr)> = resolvers.iter().zip(answers).filter_map(|(url, a)| a.as_ref().ok().map(|ip| (*url, *ip))).collect();
    if let Some(&(_, first)) = seen.first()
        && seen.iter().any(|&(_, ip)| ip != first)
    {
        let said = seen.iter().map(|(url, ip)| format!("{url}: {ip}")).collect::<Vec<_>>().join(", ");
        return Err(Error::Invalid(format!(
            "The services that tell Cyber CTF your public IP disagree ({said}), so it won't open the lab's firewall to an address it isn't sure of. \
             This happens behind a VPN that changes address or with two internet connections: disconnect the VPN or use one connection, then try again."
        )));
    }
    if seen.len() >= 2 {
        return Ok(seen[0].1.to_string());
    }
    let failed = resolvers.iter().zip(answers).filter_map(|(url, a)| a.as_ref().err().map(|e| format!("{url}: {e}"))).collect::<Vec<_>>().join("; ");
    Err(Error::Invalid(format!(
        "Couldn't confirm your public IP, needed to allow only your machine through the lab's firewall: fewer than two services answered with it. Check your internet connection and try again. ({failed})"
    )))
}

/// A resolver's answer as an IPv4 address, surrounding whitespace ignored.
pub(super) fn parse_ipv4(body: &str) -> Option<String> {
    let ip = body.trim();
    ip.parse::<std::net::Ipv4Addr>().is_ok().then(|| ip.to_string())
}

/// Whether `ip` is an address on the internet: not private, loopback, link-local, shared
/// (carrier-grade NAT), documentation, benchmarking, multicast, reserved or broadcast. Anything
/// else can't be where this machine's traffic comes from, as a cloud firewall sees it.
pub(super) fn is_public_ipv4(ip: std::net::Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    let special = ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_documentation()
        || ip.is_multicast()
        || a == 0
        || (a == 100 && (64..128).contains(&b))
        || (a == 192 && b == 0 && c == 0)
        || (a == 198 && (b == 18 || b == 19))
        || a >= 240;
    !special
}
