//! Starting a lab on a server host: ESXi through Vagrant, Proxmox and the clouds through
//! Terraform, with the AWS budget check and this machine's public IP for the firewall rules.

use super::{
    AppDirs, CONTAINERS_NOT_ON_SERVERS, Error, Inputs, LabRef, PER_MACHINE_NOT_ON_SERVERS, Path, Provider, Result, Runtime, isothrum, lab, runner_home, server,
};

/// Starts `lab` on server `host` with the launch's `inputs`; `per_machine`: as one VM per machine
/// (a cloud account only). Only an ESXi host takes the default-ports choice: Terraform's labs
/// (Proxmox, a cloud account) publish their own.
pub(super) async fn start_on_server(
    dirs: &AppDirs,
    lab: LabRef<'_>,
    (host, per_machine): (&str, bool),
    inputs: Inputs<'_>,
    log: &mut impl FnMut(String),
) -> Result<()> {
    let LabRef { dir, id, runtime } = lab;
    // The per-machine form was checked with the destination (`destination`), before the download.
    let conn = server::connection(dirs, host)?;
    // So was a container lab's place, which is never a server (ESXi, Proxmox): checked again
    // before anything is marked, for a start that reaches here another way.
    if runtime == Runtime::Docker {
        containers_run_on(Some(conn.provider))?;
    }
    // GCP labs run in the account's labs project: make sure it exists (one free billing slot).
    if conn.provider == Provider::Gcp {
        log("Checking the Cyber CTF labs project on Google Cloud…".into());
        server::gcp::labs_project(dirs, host).await?;
    }
    // The run's record names the host from the start of its run, so a half-created lab can
    // still be destroyed on the same host.
    log(super::super::deploy_log::on_server_host(&conn.name, conn.provider.id()));
    // Every server lab runs through the Isothrum runner: a cloud account, an ESXi host (Vagrant),
    // or a Proxmox server (Terraform). The runner keeps the state, the slot and the connection.
    if conn.provider.is_cloud() {
        return start_cloud_run(dirs, CloudStart { dir, id, runtime, host, conn, env: inputs.values, per_machine }, log).await;
    }
    if conn.provider == Provider::VmwareEsxi {
        lab::prepare(dir, lab::Target::Vagrant)?;
        return esxi_run(dirs, dir, id, runtime, &conn)?.up(inputs, Some(Provider::VmwareEsxi.id()), log).await;
    }
    lab::prepare(dir, lab::terraform(dir, runtime, "proxmox").1)?;
    let place = server_place(dirs, host, &conn).await?;
    if place.jump.is_some() {
        log("The lab bridge is internal to the node: reaching the lab's VMs through the node.".into());
    }
    isothrum::Deployment::on_server(&runner_home(dirs)?, dir, id, runtime, place)?.up(inputs, None, log).await
}

/// One VM per machine on a server host is Isoloom's cloud-vm, which every cloud account runs; on a
/// server (ESXi, Proxmox) the lab runs as the
/// catalogue publishes it.
pub(super) fn per_machine_runs_on(provider: Option<Provider>) -> Result<()> {
    match provider {
        Some(p) if p.is_cloud() => Ok(()),
        _ => Err(Error::Refused(PER_MACHINE_NOT_ON_SERVERS.into())),
    }
}

/// Whether a container lab's containers run on `provider`: a cloud account's VM (Isoloom's
/// cloud-docker), not a server's (ESXi, Proxmox), which would hold them inside a helper VM.
pub(super) fn containers_run_on(provider: Option<Provider>) -> Result<()> {
    match provider {
        Some(p) if p.is_cloud() => Ok(()),
        _ => Err(Error::Refused(CONTAINERS_NOT_ON_SERVERS.into())),
    }
}

/// The runner's run of VM lab `id` (in `dir`) on the ESXi server of connection `c`: its
/// settings as params, its password as the run's connection.
pub(in crate::runtime) fn esxi_run(dirs: &AppDirs, dir: &Path, id: &str, runtime: Runtime, c: &server::Connection) -> Result<isothrum::Deployment> {
    isothrum::Deployment::on_esxi(&runner_home(dirs)?, dir, id, runtime, isothrum::EsxiPlace::from_env(&c.id, &c.name, &c.env))
}

/// Where a runner's run goes on server `host` (connection `conn`): its variables (with the
/// runner's key for the lab's VMs), the node when
/// its profile names none, and the node login SSH goes through first when the lab bridge is
/// internal (the runner's key, the launcher's own, let in on the node).
pub(in crate::runtime) async fn server_place(dirs: &AppDirs, host: &str, conn: &server::Connection) -> Result<isothrum::ServerPlace> {
    let home = runner_home(dirs)?;
    let public = isothrum::public_key(&home).await?;
    // The launcher's own key: SSH signs in with the node's password, and the runner's public key
    // is the one let in.
    let (key, _) = crate::runtime::ssh::ensure_key(dirs).await?;
    Ok(isothrum::ServerPlace {
        label: host.to_owned(),
        name: conn.name.clone(),
        vars: place_vars(conn, &key, isothrum::key_values(&home).await?),
        node: server::discovered_node(dirs, host).await?,
        jump: server::proxmox_jump_for_host(dirs, host, &key, &public).await?,
    })
}

/// A server run's variables: the profile's, the node key a token setup's snippet upload signs in
/// with when the profile names none (`launcher_key`, the launcher's own), and `keys`, the key the
/// lab's VMs let in.
fn place_vars(conn: &server::Connection, launcher_key: &Path, keys: Vec<(String, String)>) -> Vec<(String, String)> {
    let mut vars = conn.tf_vars.clone();
    let token = vars.iter().any(|(k, _)| k == "proxmox_api_token");
    if token && !vars.iter().any(|(k, _)| k == lab::NODE_KEY) {
        vars.push((lab::NODE_KEY.to_owned(), launcher_key.to_string_lossy().into_owned()));
    }
    vars.extend(keys);
    vars
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
    let cloud = conn.terraform()?;
    // The form the player picked, resolved to the runtime that drives the cloud module (cloud-vm
    // for one VM per machine, else cloud-docker): the run's record keeps its module's folder, which
    // every later cloud dispatch (status, stop, check, the attack box) reads it from.
    let runtime = cloud_form(runtime, per_machine);
    // Generate the lab's module for this cloud (a cloud a lab can't run on has no module, and the
    // run surfaces that when the directory is missing).
    let (_, target) = lab::terraform(dir, runtime, cloud);
    lab::prepare(dir, target)?;
    if conn.provider == Provider::Aws {
        check_aws_budget(dirs, host, log).await?;
    }
    log(format!("This lab runs in your {} account and is billed there until you stop it.", conn.provider.label()));
    // The firewall opens the lab's SSH and published ports to this machine's public IP only.
    let allowed = public_ip().await?;
    let place = isothrum::CloudPlace::launch(&conn, Some(&allowed));
    let inputs = isothrum::Inputs { values: env, ..isothrum::Inputs::default() };
    isothrum::Deployment::in_cloud(&runner_home(dirs)?, dir, id, (runtime, cloud), place)?.up(inputs, None, log).await
}

/// The runtime that drives a `runtime` lab's Terraform module on a cloud account: for a container
/// lab, "one VM per machine" (`per_machine`) runs cloud-vm (its VM recipes, one instance per
/// machine), anything else cloud-docker (its containers on one cloud VM). A VM lab is cloud-vm
/// whatever the pick says.
pub(super) fn cloud_form(runtime: Runtime, per_machine: bool) -> Runtime {
    if per_machine && runtime == Runtime::Docker { Runtime::Vm } else { runtime }
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
    if cfg!(test) && std::env::var_os("CYBERCTF_TEST_CLOUD_REAL_IP").is_none() {
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
    let client = crate::platform::http::probe_client(std::time::Duration::from_secs(8), false)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn conn(vars: &[(&str, &str)]) -> server::Connection {
        server::Connection {
            id: "pve".into(),
            provider: Provider::Proxmox,
            name: "pve".into(),
            env: Vec::new(),
            tf_vars: vars.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect(),
            tf_env: Vec::new(),
            region: None,
            auto_stop_hours: None,
        }
    }

    /// A token setup's snippet upload signs in to the node with a key (Isoloom's module asks
    /// for a password otherwise, which a token setup has none of): the player's own when the
    /// profile names it, else the launcher's. A password setup signs in with its password.
    #[test]
    fn a_token_setup_signs_in_to_the_node_with_a_key() {
        let keys = vec![("ssh_public_key".to_owned(), "ssh-ed25519 AAAA runner".to_owned())];
        let node_key = |vars: &[(String, String)]| vars.iter().filter(|(k, _)| k == lab::NODE_KEY).map(|(_, v)| v.clone()).collect::<Vec<_>>();
        let launcher = Path::new("/app/ssh/id_ed25519");
        let token = place_vars(&conn(&[("proxmox_api_token", "root@pam!t=s")]), launcher, keys.clone());
        assert_eq!(node_key(&token), ["/app/ssh/id_ed25519"]);
        assert!(token.contains(&keys[0]), "the runner's key for the lab's VMs: {token:?}");
        let own = place_vars(&conn(&[("proxmox_api_token", "root@pam!t=s"), (lab::NODE_KEY, "/home/me/.ssh/pve")]), launcher, keys.clone());
        assert_eq!(node_key(&own), ["/home/me/.ssh/pve"], "the player's own key, once");
        assert_eq!(node_key(&place_vars(&conn(&[("proxmox_password", "pw")]), launcher, keys)), [] as [String; 0], "a password setup");
    }
}
