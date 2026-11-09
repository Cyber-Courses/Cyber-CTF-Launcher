//! Starting a lab on a server host: ESXi through Vagrant, Proxmox and the clouds through
//! Terraform, with the AWS budget check and this machine's public IP for the firewall rules.

use super::super::paths::local_runtime;
use super::{
    AppDirs, Error, LabRef, PER_MACHINE_HERE_ONLY, Path, Provider, Result, Runtime, isothrum, lab, mark_parked, mark_per_machine_vm, runner_home, server,
};

/// Starts `lab` on server `host`; `per_machine`: as one VM per machine (a cloud account only).
pub(super) async fn start_on_server(
    dirs: &AppDirs,
    lab: LabRef<'_>,
    (host, per_machine): (&str, bool),
    env: &[(String, String)],
    log: &mut impl FnMut(String),
) -> Result<()> {
    let LabRef { dir, id, runtime } = lab;
    // One VM per machine is cloud-vm on a cloud account; on a server (ESXi, Proxmox) the lab runs
    // as the catalogue publishes it. Refuse before any side effect (the secret, GCP, the marker).
    if per_machine {
        match server::host_provider(dirs, host) {
            Some(p) if p.runs_cloud_vm() => {}
            Some(p) if p.is_cloud() => {
                return Err(Error::Invalid(format!("One VM per machine doesn't run on {} yet: run the lab as one VM there.", p.label())));
            }
            _ => return Err(Error::Invalid(PER_MACHINE_HERE_ONLY.into())),
        }
    }
    let conn = server::connection(dirs, host)?;
    // GCP labs run in the account's labs project: make sure it exists (one free billing slot).
    if conn.provider == Provider::Gcp {
        log("Checking the Cyber CTF labs project on Google Cloud…".into());
        server::gcp::labs_project(dirs, host).await?;
    }
    // Mark first, so a half-created lab can still be destroyed on the same host.
    server::mark_lab(dir, Some(host))?;
    log(format!("Running on server host {} ({})", conn.name, conn.provider.id()));
    // Every server lab runs through the Isothrum runner: a cloud account, an ESXi host (Vagrant),
    // or a Proxmox server (Terraform). The runner keeps the state, the slot and the connection.
    if conn.provider.is_cloud() {
        return start_cloud_run(dirs, CloudStart { dir, id, runtime, host, conn, env, per_machine }, log).await;
    }
    if conn.provider == Provider::VmwareEsxi {
        lab::prepare(dir, lab::vagrant_target(runtime))?;
        esxi_run(dirs, dir, id, runtime, &conn)?.up(env, Some(Provider::VmwareEsxi.id()), log).await?;
        return mark_parked(dir, None);
    }
    lab::prepare(dir, lab::terraform(dir, runtime, "proxmox").1)?;
    let run = isothrum::ServerRun::new(&runner_home(dirs)?, dir, id, runtime)?;
    let place = server_place(dirs, host, &conn, &run).await?;
    if place.jump.is_some() {
        log("The lab bridge is internal to the node: reaching the lab's VMs through the node.".into());
    }
    run.up(&place, env, log).await
}

/// Whether the Isothrum runner runs labs of `runtime` on `provider` servers: VM labs on Proxmox
/// (Terraform) and on ESXi (Vagrant).
pub(in crate::runtime) fn runner_runs_on(provider: Provider, runtime: Runtime) -> bool {
    matches!(provider, Provider::Proxmox | Provider::VmwareEsxi) && runtime == Runtime::Vm
}

/// The runner's run of VM lab `id` (in `dir`) on the ESXi server it is marked for, with that
/// server's connection `c`: its settings as params, its password as the run's connection.
pub(crate) fn esxi_run(dirs: &AppDirs, dir: &Path, id: &str, runtime: Runtime, c: &server::Connection) -> Result<isothrum::LabRun> {
    let host = server::marked_host(dir).unwrap_or_default();
    isothrum::LabRun::on_esxi(&runner_home(dirs)?, dir, id, runtime, isothrum::EsxiPlace::from_env(&host, &c.env))
}

/// Where a runner's run goes on server `host` (connection `conn`): its variables, the node when
/// its profile names none, and the node login SSH goes through first when the lab bridge is
/// internal (the runner's key, the launcher's own, let in on the node).
pub(in crate::runtime) async fn server_place(
    dirs: &AppDirs,
    host: &str,
    conn: &server::Connection,
    run: &isothrum::ServerRun,
) -> Result<isothrum::ServerPlace> {
    let public = run.public_key().await?;
    let key = runner_home(dirs)?.join("ssh").join("id_ed25519");
    Ok(isothrum::ServerPlace {
        label: host.to_owned(),
        name: conn.name.clone(),
        vars: conn.tf_vars.clone(),
        node: server::discovered_node(dirs, host).await?,
        jump: server::proxmox_jump_for_host(dirs, host, &key, &public).await?,
    })
}

/// What a cloud VM lab starts with: the lab (its folder, id and runtime), the account it runs on
/// and the launch's values for its input slots.
pub(crate) struct CloudStart<'a> {
    pub(crate) dir: &'a Path,
    pub(crate) id: &'a str,
    pub(crate) runtime: Runtime,
    pub(crate) host: &'a str,
    pub(crate) conn: server::Connection,
    pub(crate) env: &'a [(String, String)],
    /// The player picked "one VM per machine" (cloud-vm): a container lab's machines as cloud
    /// instances, not its containers on one cloud VM (cloud-docker). Only for a lab with VM
    /// recipes; ignored for a VM lab, which is cloud-vm already.
    pub(crate) per_machine: bool,
}

/// A cloud VM lab (`cloud-vm`, `cloud-docker`) in the player's cloud account through the Isothrum
/// runner. The launcher keeps the policy — the AWS budget guard, this machine's public IP for the
/// firewall, the GCP labs project (done in [`start_on_server`]), the account's credentials — and
/// the runner does the work: it delivers the lab over SSH, applies the egress seal, reads the
/// module's `ssh_user` output and keeps the Terraform state and the run's expiry.
pub(crate) async fn start_cloud_run(dirs: &AppDirs, start: CloudStart<'_>, log: &mut impl FnMut(String)) -> Result<()> {
    let CloudStart { dir, id, runtime, host, conn, env, per_machine } = start;
    let cloud = server::terraform_target(conn.provider).ok_or_else(|| Error::Invalid("this account's cloud runs no Terraform labs".into()))?;
    // The form the player picked, recorded before anything runs so every later cloud dispatch
    // (status, stop, check, the attack box) reads it, and resolved to the runtime that drives the
    // cloud module (cloud-vm for one VM per machine, else cloud-docker).
    let runtime = record_cloud_form(dir, runtime, per_machine)?;
    // Generate the lab's module for this cloud (a cloud a lab can't run on has no module, and the
    // run surfaces that when the directory is missing).
    let (_, target) = lab::terraform(dir, runtime, cloud);
    lab::prepare(dir, target)?;
    if conn.provider == Provider::Aws {
        check_aws_budget(dirs, host, log).await?;
    }
    log(format!("This lab runs in your {} account and is billed there until you stop it.", conn.provider.id().to_uppercase()));
    // The firewall opens the lab's SSH and published ports to this machine's public IP only.
    let allowed = public_ip().await?;
    let place = cloud_place(host, &conn, &allowed);
    let run = isothrum::CloudRun::new(&runner_home(dirs)?, dir, id, runtime, cloud)?;
    run.up(&place, env, log).await?;
    mark_parked(dir, None)
}

/// Records the form a container lab is launched in on a cloud account and resolves the runtime
/// that drives its Terraform module: "one VM per machine" runs cloud-vm (its VM recipes, one
/// instance per machine), anything else cloud-docker (its containers on one cloud VM). The mark is
/// the same one a lab on this machine uses, so `local_runtime` reads it everywhere (status, stop,
/// check, the attack box). A VM lab is cloud-vm whatever the pick says: the mark is container-only.
pub(super) fn record_cloud_form(dir: &Path, runtime: Runtime, per_machine: bool) -> Result<Runtime> {
    mark_per_machine_vm(dir, per_machine && runtime == Runtime::Docker)?;
    Ok(local_runtime(dir, runtime))
}

/// Where a cloud VM lab runs, from the account's connection `conn` and this machine's public IP:
/// the account's region and credentials, the firewall CIDR, and the account's auto-stop as the
/// run's expiry.
pub(in crate::runtime) fn cloud_place(host: &str, conn: &server::Connection, allowed_ip: &str) -> isothrum::CloudPlace {
    let hours = conn.auto_stop_hours.unwrap_or(0);
    isothrum::CloudPlace {
        label: host.to_owned(),
        region: conn.region.clone(),
        allowed_cidr: Some(format!("{allowed_ip}/32")),
        tf_env: conn.tf_env.clone(),
        lifetime: (hours > 0).then(|| std::time::Duration::from_secs(u64::from(hours) * 3600)),
    }
}

/// Stops before spending if this AWS account is over its monthly budget, or if a budget is set
/// and the spend is over it; if the spend can't be read, warns and lets the launch go on.
pub(in crate::runtime) async fn check_aws_budget(dirs: &AppDirs, host: &str, log: &mut impl FnMut(String)) -> Result<()> {
    budget_verdict(server::check_budget(dirs, host).await, log)
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
    // Unit tests never reach the internet: a documentation address stands in. An integration test
    // (a lab in a real account, its firewall opened to this machine) asks for the real one.
    if cfg!(test) && std::env::var_os("CYBERCTF_TEST_REAL_PUBLIC_IP").is_none() {
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
    let resp = client.get(url).send().await.and_then(reqwest::Response::error_for_status).map_err(|e| e.to_string())?;
    // An address is a few bytes: a page (a captive portal) is no answer.
    let body = crate::platform::http::body_limited(resp, 256).await.map_err(|e| e.to_string())?;
    let ip = parse_ipv4(&String::from_utf8_lossy(&body)).ok_or("an unexpected answer")?;
    let ip: std::net::Ipv4Addr = ip.parse().map_err(|_| "an unexpected answer")?;
    if is_public_ipv4(ip) { Ok(ip) } else { Err(format!("{ip}, not a public address")) }
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
