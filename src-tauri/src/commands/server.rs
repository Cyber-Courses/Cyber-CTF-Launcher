//! The Server screen's Tauri commands: list, save, remove and test hosts, what each row shows
//! (running labs, capacity), an AWS account's month-to-date spend, and the host self-test. Each is
//! a thin adapter over the `crate::runtime::server` domain; the filesystem-only helpers that read
//! each lab's host marker (which need the app's data folder) live here with them.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::Manager;
use tauri::ipc::Channel;

use crate::AppHandle;
use crate::error::{Code, Error, Result};
use crate::machine::selftest::Event;
use crate::platform::AppDirs;
use crate::runtime::providers::Provider;
use crate::runtime::proxmox::placement;
use crate::runtime::proxmox::ssh_key;
use crate::runtime::server::SshKeySignIn;
use crate::runtime::server::budget::month_to_date_cost;
use crate::runtime::server::checks::{Check, TestResult};
use crate::runtime::server::gcp;
use crate::runtime::server::input::{HostInput, valid_id};
use crate::runtime::server::marked_host;
use crate::runtime::server::profile::{HostProfile, new_id};
use crate::runtime::server::reachability::test_host;
use crate::runtime::server::secrets::{delete_secret, get_secret, host_secret, set_secret};
use crate::runtime::server::store::{Store, find, load, load_host, update};

#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "ServerList")]
pub struct HostList {
    default: Option<String>,
    hosts: Vec<HostProfile>,
}

#[tauri::command]
pub fn server_list(app: AppHandle) -> Result<HostList> {
    let store = load(&AppDirs::from(&app))?;
    Ok(HostList { default: store.default, hosts: store.hosts })
}

/// Creates or updates a host. The first host saved becomes the default. A host labs are still
/// deployed on keeps its place (see `keeps_placement`). A password or token save keeps no SSH
/// key sign-in (see `server_save_ssh_key`), but a re-save of an SSH-key setup whose token is
/// already made, with the same key, keeps it.
#[tauri::command]
pub fn server_save(app: AppHandle, mut input: HostInput) -> Result<HostProfile> {
    let saved = input.id.as_deref().and_then(|id| load(&AppDirs::from(&app)).ok()?.hosts.into_iter().find(|h| h.id == id));
    if !saved.is_some_and(|h| input.same_ssh_key(&h)) {
        input.without_ssh_key();
    }
    save(&app, input)
}

/// A Proxmox server's placement choices (nodes, storages, bridges), read from its API with the
/// form's sign-in: its password or token secret, else the saved host's.
#[tauri::command]
pub async fn server_proxmox_placement(app: AppHandle, mut input: HostInput) -> Result<placement::Placement> {
    if input.provider != Provider::Proxmox {
        return Err(Error::Invalid("only Proxmox servers list their placement".into()));
    }
    input.without_ssh_key();
    let checked = input.check()?;
    let typed = input.password.take().filter(|p| !p.is_empty());
    let id = input.id.clone();
    let secret = match (typed, &id) {
        (Some(p), _) => p,
        (None, Some(id)) if load_host(&AppDirs::from(&app), id).is_ok() => get_secret(id)?,
        (None, _) => return Err(Error::Invalid("enter the password or token secret first".into())),
    };
    let profile = input.into_profile(id.unwrap_or_else(new_id), checked, None)?;
    let ip = tokio::net::lookup_host((profile.host.as_str(), profile.port)).await.ok().and_then(|mut a| a.next()).map(|a| a.ip());
    placement::read_host(&profile, &secret, ip).await.map_err(|e| Error::Invalid(format!("Couldn't read the server's nodes, storages and bridges: {e}")))
}

/// Saves a Proxmox host set up with the player's SSH key: signs in to the node with it, makes the
/// launcher's API token there and lets the launcher's key in (`proxmox::ssh_key`), then saves the
/// host as a token setup (the token's secret in the keychain) that remembers which key it used.
/// Nothing is saved when any of that fails.
#[tauri::command]
pub async fn server_save_ssh_key(app: AppHandle, mut input: HostInput) -> Result<HostProfile> {
    let dirs = AppDirs::from(&app);
    let key = input.ssh_key()?;
    // The address and the rest are checked before anything happens on the node.
    input.sign_in_as(format!("{}@pam", key.user), None);
    let checked = input.check()?;
    let (launcher, public) = crate::runtime::ssh::ensure_key(&dirs).await?;
    let known_hosts = crate::runtime::ssh::node_known_hosts(&launcher);
    let made = ssh_key::bootstrap(&checked.host, &key, &known_hosts, &public).await.map_err(|f| Error::Invalid(f.to_string()))?;
    input.sign_in_as(made.token_id.clone(), Some(made.secret));
    let profile = save(&app, input)?;
    let added = if made.key_added { "added to" } else { "already in" };
    let note = format!("Made the API token {} on the node with your key; the launcher's key was {added} {}'s authorized_keys.", made.token_id, key.user);
    MADE_WITH_KEY.lock().unwrap_or_else(std::sync::PoisonError::into_inner).insert(profile.id.clone(), note);
    Ok(profile)
}

/// What the last SSH-key setup of each host made, by host id, for its Test connection.
static MADE_WITH_KEY: std::sync::Mutex<BTreeMap<String, String>> = std::sync::Mutex::new(BTreeMap::new());

/// The SSH key the setup offers first: the first of `~/.ssh/id_ed25519`, `id_ecdsa`, `id_rsa`
/// that exists (its path only).
#[tauri::command]
pub fn server_default_ssh_key(app: AppHandle) -> Option<String> {
    let home = app.path().home_dir().ok()?;
    ssh_key::default_key(&home).map(|p| p.display().to_string())
}

/// Saves a checked form: under its id, its secret in the keychain.
fn save(app: &AppHandle, mut input: HostInput) -> Result<HostProfile> {
    let checked = input.check()?;
    let deployed = input.id.as_deref().map_or(0, |id| labs_on(app, id));
    update(&AppDirs::from(app), |store| {
        let id = save_id(store, input.id.take())?;
        let gcp_project = kept_gcp_project(store, &id, input.provider, &checked.username);
        let password = input.password.take();
        let profile = input.into_profile(id.clone(), checked, gcp_project)?;
        if let Some(saved) = store.hosts.iter().find(|h| h.id == id) {
            keeps_placement(saved, &profile, deployed)?;
        }
        match new_password(password)? {
            Some(p) => set_secret(&id, &p)?,
            None if profile.keeps_secret() && get_secret(&id).is_err() => {
                return Err(Error::Invalid("enter the host's password".into()));
            }
            None => {}
        }
        put_host(store, profile.clone());
        Ok(profile)
    })
}

/// "Test connection" of a host set up with an SSH key: each part of the setup, checked again
/// (SSH with the player's key, the launcher's key in `authorized_keys` — added back if it went),
/// before the API's own answer with the token.
async fn ssh_key_checks(dirs: &AppDirs, host: &HostProfile, key: &SshKeySignIn) -> Vec<Check> {
    let mut checks = Vec::new();
    let login = format!("{}@{}:{}", key.user, host.host, key.port);
    let authorized = match crate::runtime::ssh::ensure_key(dirs).await {
        Ok((launcher, public)) => ssh_key::authorize(&host.host, key, &crate::runtime::ssh::node_known_hosts(&launcher), &public).await,
        Err(e) => Err(ssh_key::Failure { part: ssh_key::Part::Key, detail: format!("the launcher's own key isn't ready: {e}") }),
    };
    match &authorized {
        Err(f) if f.part == ssh_key::Part::Ssh => checks.push(Check::fail("SSH with your key", f.to_string())),
        _ => checks.push(Check::ok("SSH with your key", format!("Signed in to {login} with {}.", key.path))),
    }
    let made = MADE_WITH_KEY.lock().unwrap_or_else(std::sync::PoisonError::into_inner).get(&host.id).cloned();
    checks.push(Check::ok("API token", made.unwrap_or_else(|| format!("Signs in with the token {}, made on the node with your key.", host.username))));
    checks.push(match authorized {
        Ok(true) => Check::ok("Launcher key", format!("Added the launcher's key to {}'s authorized_keys.", key.user)),
        Ok(false) => Check::ok("Launcher key", format!("The launcher's key is in {}'s authorized_keys.", key.user)),
        Err(f) if f.part == ssh_key::Part::Ssh => Check::fail("Launcher key", "Not checked: SSH with your key failed."),
        Err(f) => Check::fail("Launcher key", f.to_string()),
    });
    checks
}

/// Refuses to move a host (another provider, region or address, port, node, or the account labs
/// are created in) while `deployed` labs still have something on it: a stop would then look for
/// them where they aren't, find nothing, and leave them running (and, on a cloud, billing).
/// Everything else (its name, credentials, size, auto-stop, budget) can change.
fn keeps_placement(saved: &HostProfile, edited: &HostProfile, deployed: usize) -> Result<()> {
    let (before, after) = (saved.placement(), edited.placement());
    let mut changed: Vec<&str> = Vec::new();
    for (name, _) in before.iter().filter(|f| !after.contains(f)).chain(after.iter().filter(|f| !before.contains(f))) {
        if !changed.contains(name) {
            changed.push(name);
        }
    }
    if deployed == 0 || changed.is_empty() {
        return Ok(());
    }
    let (count, fields) = (deployed as u64, changed.join(", "));
    Err(Error::coded(if saved.provider.is_cloud() { Code::PlacementLockedAccount { count, fields } } else { Code::PlacementLockedHost { count, fields } }))
}

/// The id a save writes under: an existing host's (edit), or a new one.
fn save_id(store: &Store, id: Option<String>) -> Result<String> {
    match id {
        Some(id) if valid_id(&id) && store.hosts.iter().any(|h| h.id == id) => Ok(id),
        Some(_) => Err(Error::Invalid("unknown server host".into())),
        None => Ok(new_id()),
    }
}

/// GCP keeps its labs project across edits, unless the billing account changed.
fn kept_gcp_project(store: &Store, id: &str, provider: Provider, username: &str) -> Option<String> {
    store
        .hosts
        .iter()
        .find(|h| h.id == id && h.provider == Provider::Gcp && provider == Provider::Gcp && h.username == username)
        .and_then(|h| h.gcp_project.clone())
}

/// A password entered in the form, if one was (blank means "keep the stored one").
fn new_password(password: Option<String>) -> Result<Option<String>> {
    match password.filter(|p| !p.is_empty()) {
        Some(p) if p.len() <= 1024 && !p.contains('\0') => Ok(Some(p)),
        Some(_) => Err(Error::Invalid("invalid password".into())),
        None => Ok(None),
    }
}

/// Adds or replaces a host; the first server saved becomes the default.
fn put_host(store: &mut Store, profile: HostProfile) {
    let id = profile.id.clone();
    let server = !profile.provider.is_cloud();
    match store.hosts.iter_mut().find(|h| h.id == id) {
        Some(existing) => *existing = profile,
        None => store.hosts.push(profile),
    }
    if store.default.is_none() && server {
        store.default = Some(id);
    }
}

/// The host id of each installed lab's marker (trimmed; empty when the marker is). A lab leaves
/// a `.cyberctf-host` marker in its directory while it runs on a host (written on start,
/// cleared on stop), so this is filesystem-only: no host calls, no credentials.
fn lab_markers(app: &AppHandle) -> Vec<String> {
    let Ok(labs) = app.path().app_data_dir().map(|d| d.join("labs")) else { return Vec::new() };
    markers_in(&labs)
}

/// `lab_markers` for the labs folder `labs`.
fn markers_in(labs: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(labs) else { return Vec::new() };
    entries.flatten().filter_map(|e| marked_host(&e.path())).collect()
}

/// Terraform deployments still holding resources on host `id` (under the app data folder
/// `base`) of labs no longer marked as running there: their state can only be destroyed with
/// that host's credentials.
fn leftovers_on(base: &Path, id: &str) -> usize {
    let paths = crate::runtime::paths::deployed_ids(base);
    paths
        .iter()
        .filter(|lab| marked_host(&base.join("labs").join(lab)).as_deref() != Some(id))
        .flat_map(|lab| crate::runtime::paths::deployment_states(base, lab))
        .filter(|(_, state)| crate::runtime::paths::recorded_host(state).as_deref() == Some(id) && crate::runtime::terraform::deployed(state))
        .count()
}

/// Lab counts per host id (markers with no id aren't counted).
fn count_by_host(markers: &[String]) -> HashMap<String, u32> {
    let mut counts: HashMap<String, u32> = HashMap::new();
    for id in markers.iter().filter(|id| !id.is_empty()) {
        *counts.entry(id.clone()).or_default() += 1;
    }
    counts
}

/// How many labs still have something on host `id`: marked as running there, or a Terraform
/// state left there that holds resources.
fn labs_on(app: &AppHandle, id: &str) -> usize {
    let left = app.path().app_data_dir().map_or(0, |base| leftovers_on(&base, id));
    lab_markers(app).iter().filter(|m| *m == id).count() + left
}

#[tauri::command]
pub fn server_remove(app: AppHandle, id: &str) -> Result<()> {
    // Don't strand running labs: this host holds the credentials and state needed to stop and
    // destroy its labs. Removing it mid-run would leave them unstoppable from the app, and a
    // cloud lab would keep billing with no way left to tear it down.
    no_labs_running(labs_on(&app, id))?;
    update(&AppDirs::from(&app), |store| {
        remove_host(store, id);
        Ok(())
    })?;
    delete_secret(id);
    Ok(())
}

/// Refuses to remove a host `running` labs still run on.
fn no_labs_running(running: usize) -> Result<()> {
    if running > 0 {
        return Err(Error::coded(Code::HostHasLabs { count: running as u64 }));
    }
    Ok(())
}

/// Drops a host; when it was the default, another server (never a cloud account) takes over.
fn remove_host(store: &mut Store, id: &str) {
    store.hosts.retain(|h| h.id != id);
    if store.default.as_deref() == Some(id) {
        // The default is a *server* to run VM labs on; a cloud account must never become it.
        store.default = store.hosts.iter().find(|h| !h.provider.is_cloud()).map(|h| h.id.clone());
    }
}

/// Sets (or clears, with None) the host VM labs run on by default.
#[tauri::command]
pub fn server_set_default(app: AppHandle, id: Option<String>) -> Result<()> {
    update(&AppDirs::from(&app), |store| set_default_host(store, id))
}

/// `server_set_default` on a loaded store.
fn set_default_host(store: &mut Store, id: Option<String>) -> Result<()> {
    if let Some(id) = &id
        && find(store, id)?.provider.is_cloud()
    {
        return Err(Error::Invalid("a cloud account can't be the default host".into()));
    }
    store.default = id;
    Ok(())
}

static STARTED: OnceLock<Instant> = OnceLock::new();

/// Notes the app's start, for `launched_recently`.
pub fn mark_started() {
    STARTED.get_or_init(Instant::now);
}

/// Whether the app started less than a minute ago.
fn launched_recently() -> bool {
    STARTED.get_or_init(Instant::now).elapsed() < Duration::from_secs(60)
}

#[tauri::command]
pub async fn server_test(app: AppHandle, id: String) -> Result<TestResult> {
    let dirs = AppDirs::from(&app);
    let host = load_host(&dirs, &id)?;
    // CLI-credential hosts keep no secret; the CLI resolves its own credentials.
    let password = host_secret(&host)?;
    // Proxmox is checked over SSH too (the launcher's key on token hosts, the password next to
    // it otherwise): make sure the key exists first.
    if host.provider == Provider::Proxmox {
        crate::runtime::ssh::ensure_key(&dirs).await?;
    }
    let mut result = test_host(&host, &password).await;
    // macOS answers the app's first connection to a LAN host after a launch with "no route to
    // host" while it checks the Local Network permission; the next one goes through. One retry
    // keeps every freshly opened app from listing reachable servers as Unreachable.
    // Right after an update (a new binary) that takes longer, so for the app's first minute it
    // keeps trying for about 10 s; later, an unreachable host is reported after one retry.
    if !result.reachable && cfg!(target_os = "macos") {
        let delays: &[u64] = if launched_recently() { &[2, 3, 5] } else { &[2] };
        for secs in delays {
            tokio::time::sleep(Duration::from_secs(*secs)).await;
            result = test_host(&host, &password).await;
            if result.reachable {
                break;
            }
        }
    }
    // Set up with an SSH key: each part of that setup, then the API's answer with the token.
    if let Some(key) = host.ssh_key.clone().filter(|_| host.provider == Provider::Proxmox) {
        result = result.with_sign_in_checks(ssh_key_checks(&dirs, &host, &key).await);
    }
    // GCP: the labs project (created on the first test), which needs a free billing slot.
    if host.provider == Provider::Gcp && result.ok {
        match gcp::labs_project(&dirs, &id).await {
            Ok(p) => result.checks.push(Check::ok("Labs project", format!("Labs run in the project {p}."))),
            Err(e) => {
                result.checks.push(Check::fail("Labs project", e.to_string()));
                result.ok = false;
            }
        }
    }
    Ok(result)
}

/// The launcher's SSH public key (generated on first use). Token-auth Proxmox hosts must
/// authorize it for the token's SSH user, so the launcher can upload the cloud-init snippet
/// over SSH; the setup form shows it for those hosts.
#[tauri::command]
pub async fn server_public_key(app: AppHandle) -> Result<String> {
    Ok(crate::runtime::ssh::ensure_key(&AppDirs::from(&app)).await?.1)
}

/// How many installed labs are currently running on each host, keyed by host id (see
/// `lab_markers`).
#[tauri::command]
pub fn server_running_labs(app: AppHandle) -> HashMap<String, u32> {
    count_by_host(&lab_markers(&app))
}

/// A host's hardware headroom, shown on its row so you can see what it can run.
#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct HostCapacity {
    pub cores: u64,
    pub mem_total: u64,
    pub mem_free: u64,
}

/// Queries a host for its capacity (Proxmox only; cheap API calls). Returns `None` for other
/// providers or if anything fails, so the UI just omits it rather than showing an error.
#[tauri::command]
pub async fn server_capacity(app: AppHandle, id: String) -> Option<HostCapacity> {
    let host = load_host(&AppDirs::from(&app), &id).ok()?;
    if host.provider != Provider::Proxmox {
        return None;
    }
    let password = get_secret(&id).ok()?;
    proxmox_capacity(&host, &password).await
}

async fn proxmox_capacity(h: &HostProfile, password: &str) -> Option<HostCapacity> {
    let session = crate::runtime::proxmox::sign_in(h, password).await.ok()?;
    let node = session.node(h).await.ok()?;
    let d = session.call(&format!("/nodes/{node}/status")).await.ok()?;
    capacity_from(&d)
}

/// A Proxmox node's `/status` as the row's capacity (None when a figure is missing).
fn capacity_from(d: &serde_json::Value) -> Option<HostCapacity> {
    Some(HostCapacity { cores: d["cpuinfo"]["cpus"].as_u64()?, mem_total: d["memory"]["total"].as_u64()?, mem_free: d["memory"]["free"].as_u64()? })
}

/// This month's spend so far (USD) on the saved AWS account `id`, with its own credentials (its
/// stored keys, or its CLI profile): the figure the Cloud screen shows is the one the launch
/// budget gate checks, never another account's (the CLI's default profile's).
#[tauri::command]
pub async fn server_month_to_date_cost(app: AppHandle, id: String) -> Result<f64> {
    let host = load_host(&AppDirs::from(&app), &id)?;
    if host.provider != Provider::Aws {
        return Err(Error::Invalid("only AWS accounts report their spend".into()));
    }
    month_to_date_cost(&host, &host_secret(&host)?).await
}

/// Runs a real-VM self-test against host `id`, streaming step events (see
/// [`crate::runtime::server_selftest::run`]).
#[tauri::command]
pub async fn server_selftest(app: AppHandle, id: String, events: Channel<Event>) -> Result<()> {
    crate::runtime::server_selftest::run(&AppDirs::from(&app), &id, move |e| {
        let _ = events.send(e);
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::server::HOST_MARKER;
    use crate::runtime::server::profile::tests::profile;

    fn host(id: &str, provider: Provider) -> HostProfile {
        HostProfile { id: id.into(), ..profile(provider) }
    }

    fn store(hosts: Vec<HostProfile>, default: Option<&str>) -> Store {
        Store { default: default.map(str::to_string), hosts }
    }

    #[tokio::test]
    async fn hosts_are_saved_listed_tested_and_removed_through_the_commands() {
        use crate::platform::exec::fake::{Tools, ok};
        let t = crate::testing::test_app::TestApp::new();
        let app = || t.handle().clone();
        let input = |id: Option<&str>| -> HostInput {
            serde_json::from_value(serde_json::json!({
                "id": id, "name": "My Azure", "provider": "azure", "host": "westeurope", "username": "00000000-0000-0000-0000-000000000000"
            }))
            .unwrap()
        };
        let saved = server_save(app(), input(None)).unwrap();
        assert_eq!(saved.provider, Provider::Azure);
        let edited = server_save(app(), input(Some(&saved.id))).unwrap();
        assert_eq!(edited.id, saved.id);
        let list = server_list(app()).unwrap();
        assert_eq!(list.hosts.len(), 1);
        assert_eq!(list.default, None, "a cloud account never becomes the default");
        assert_eq!(server_set_default(app(), Some(saved.id.clone())).unwrap_err().to_string(), "a cloud account can't be the default host");
        server_set_default(app(), None).unwrap();
        assert!(server_save(app(), input(Some("ffffffffffffffff"))).is_err());
        // Testing it asks the Azure CLI (no network here).
        let tools = Tools::rules(&[("account show", ok("Pay-As-You-Go\n"))]);
        let result = server_test(app(), saved.id.clone()).await.unwrap();
        assert!(tools.called("az account show --subscription 00000000-0000-0000-0000-000000000000"));
        assert!(!result.checks.is_empty());
        assert!(server_test(app(), "nope".into()).await.is_err());
        assert!(server_capacity(app(), saved.id.clone()).await.is_none(), "only Proxmox reports capacity");
        // A lab running on it keeps it.
        let lab = t.lab("web", &[]);
        std::fs::write(lab.join(HOST_MARKER), &saved.id).unwrap();
        assert_eq!(server_running_labs(app()).get(&saved.id), Some(&1));
        assert!(server_remove(app(), &saved.id).unwrap_err().to_string().starts_with("1 lab still running"));
        std::fs::remove_file(lab.join(HOST_MARKER)).unwrap();
        server_remove(app(), &saved.id).unwrap();
        assert_eq!(server_list(app()).unwrap().hosts, Vec::<HostProfile>::new());
        // The launcher's key, made once.
        let ssh = t.data().join("ssh");
        std::fs::create_dir_all(&ssh).unwrap();
        std::fs::write(ssh.join("id_ed25519"), "k").unwrap();
        std::fs::write(ssh.join("id_ed25519.pub"), "ssh-ed25519 AAAA cyberctf-launcher\n").unwrap();
        assert_eq!(server_public_key(app()).await.unwrap(), "ssh-ed25519 AAAA cyberctf-launcher");
    }

    /// A Proxmox form signing in with the SSH key at `key`.
    fn ssh_key_input(key: &Path) -> HostInput {
        serde_json::from_value(serde_json::json!({
            "id": null, "name": "Garage", "provider": "proxmox", "host": "10.0.0.50", "port": 8006, "username": "",
            "datastore": null, "network": null, "node": null, "insecureTls": true, "autoStopHours": null, "password": null,
            "sshKey": { "path": key.display().to_string(), "user": "root", "port": 22 }
        }))
        .unwrap()
    }

    #[tokio::test]
    async fn placement_choices_need_a_proxmox_server_and_its_secret() {
        let t = crate::testing::test_app::TestApp::new();
        let form = |provider: &str, password: Option<&str>| -> HostInput {
            serde_json::from_value(serde_json::json!({
                "id": null, "name": "P", "provider": provider, "host": "127.0.0.1", "port": 1, "username": "root",
                "datastore": null, "network": null, "node": null, "insecureTls": true, "autoStopHours": null, "password": password
            }))
            .unwrap()
        };
        let err = |r: Result<placement::Placement>| r.unwrap_err().to_string();
        assert_eq!(err(server_proxmox_placement(t.handle().clone(), form("vmware_esxi", Some("pw"))).await), "only Proxmox servers list their placement");
        assert_eq!(err(server_proxmox_placement(t.handle().clone(), form("proxmox", None)).await), "enter the password or token secret first");
        // Nothing listens on port 1: the read fails and says so (the setup then offers manual entry).
        assert!(
            err(server_proxmox_placement(t.handle().clone(), form("proxmox", Some("pw"))).await)
                .starts_with("Couldn't read the server's nodes, storages and bridges")
        );
    }

    #[cfg(debug_assertions)]
    #[tokio::test]
    async fn an_ssh_key_setup_saves_the_token_it_made_like_a_token_setup() {
        use crate::platform::exec::fake::{Tools, fail, ok};
        let t = crate::testing::test_app::TestApp::new();
        let app = || t.handle().clone();
        crate::runtime::server::secrets::use_test_file(t.data().join("secrets.json"));
        // The launcher's key, already made; the player's own key (never read, only its path used).
        let ssh = t.data().join("ssh");
        std::fs::create_dir_all(&ssh).unwrap();
        std::fs::write(ssh.join("id_ed25519"), "k").unwrap();
        std::fs::write(ssh.join("id_ed25519.pub"), "ssh-ed25519 AAAA cyberctf-launcher\n").unwrap();
        let mine = t.data().join("my_key");
        std::fs::write(&mine, "PRIVATE KEY CONTENTS").unwrap();

        // The node refuses the key: nothing is saved, and the message says which part failed.
        let tools = Tools::rules(&[("authorized_keys", fail("root@10.0.0.50: Permission denied (publickey)."))]);
        let err = server_save_ssh_key(app(), ssh_key_input(&mine)).await.unwrap_err().to_string();
        assert!(err.starts_with("Couldn't sign in to the node over SSH with your key"), "{err}");
        assert!(!tools.called("pveum"), "no token is made when SSH fails");
        assert!(server_list(app()).unwrap().hosts.is_empty());
        drop(tools);

        let name = ssh_key::token_name("ssh-ed25519 AAAA cyberctf-launcher");
        let answer = format!(r#"{{"full-tokenid":"root@pam!{name}","info":{{"privsep":"0"}},"value":"6f1d2c3b-aaaa-bbbb-cccc-0123456789ab"}}"#);
        let tools = Tools::rules(&[("authorized_keys", ok("added")), ("pveum user token", ok(answer))]);
        let saved = server_save_ssh_key(app(), ssh_key_input(&mine)).await.unwrap();
        assert_eq!(saved.username, format!("root@pam!{name}"), "a token setup from here on");
        assert!(crate::runtime::proxmox::is_token(&saved.username));
        assert_eq!(saved.ssh_key.as_ref().map(|k| k.path.clone()), Some(mine.display().to_string()));
        assert_eq!(get_secret(&saved.id).unwrap(), "6f1d2c3b-aaaa-bbbb-cccc-0123456789ab", "the token's secret is in the keychain");
        assert_eq!(tools.calls().len(), 2);
        assert!(tools.calls().iter().all(|c| !c.contains("PRIVATE KEY CONTENTS")), "the key's contents are never passed on");
        drop(tools);

        // Its test checks each part again (the key already there now), and only those commands.
        let tools = Tools::rules(&[("authorized_keys", ok("present"))]);
        let host = load_host(&t.dirs(), &saved.id).unwrap();
        let checks = ssh_key_checks(&t.dirs(), &host, host.ssh_key.as_ref().unwrap()).await;
        let names: Vec<&str> = checks.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["SSH with your key", "API token", "Launcher key"]);
        assert!(checks.iter().all(|c| c.state == crate::runtime::server::checks::CheckState::Ok), "{checks:?}");
        assert!(checks[1].detail.contains(&name) && checks[2].detail.contains("is in root's authorized_keys"), "{checks:?}");
        assert_eq!(tools.calls().len(), 1);
        drop(tools);

        // Saving it again (its placement, say) with the same key and token keeps the key.
        let again: HostInput = serde_json::from_value(serde_json::json!({
            "id": saved.id, "name": "Garage", "provider": "proxmox", "host": "10.0.0.50", "port": 8006, "username": saved.username,
            "datastore": "local-lvm", "network": "vmbr1", "node": "pve", "insecureTls": true, "autoStopHours": null, "password": null,
            "sshKey": { "path": mine.display().to_string(), "user": "root", "port": 22 }, "imageStorage": "local", "snippetStorage": "local"
        }))
        .unwrap();
        let kept = server_save(app(), again).unwrap();
        assert_eq!(kept.ssh_key, saved.ssh_key);
        assert_eq!((kept.network.as_deref(), kept.image_storage.as_deref()), (Some("vmbr1"), Some("local")));
        assert_eq!(get_secret(&saved.id).unwrap(), "6f1d2c3b-aaaa-bbbb-cccc-0123456789ab", "the token's secret is kept");
        // A password save of the same host drops the SSH key sign-in.
        let password: HostInput = serde_json::from_value(serde_json::json!({
            "id": saved.id, "name": "Garage", "provider": "proxmox", "host": "10.0.0.50", "port": 8006, "username": "root@pam",
            "datastore": null, "network": null, "node": null, "insecureTls": true, "autoStopHours": null, "password": "pw",
            "sshKey": { "path": mine.display().to_string(), "user": "root", "port": 22 }
        }))
        .unwrap();
        let resaved = server_save(app(), password).unwrap();
        assert_eq!((resaved.ssh_key, resaved.username.as_str()), (None, "root@pam"));
        assert_eq!(get_secret(&saved.id).unwrap(), "pw");
        // The default key offered is one of the usual ones, when the player has one.
        if let Some(path) = server_default_ssh_key(app()) {
            assert!(["id_ed25519", "id_ecdsa", "id_rsa"].iter().any(|n| path.ends_with(n)), "{path}");
        }
    }

    #[test]
    fn an_account_with_labs_on_it_keeps_its_place_but_not_its_credentials() {
        // Regression: the player moved an AWS account with a lab running in eu-west-3 to
        // us-east-1 for their next lab; Stop then destroyed in us-east-1, found nothing, and the
        // instance kept billing in eu-west-3.
        let t = crate::testing::test_app::TestApp::new();
        let app = || t.handle().clone();
        let azure = |id: Option<&str>, region: &str, subscription: &str, name: &str| -> HostInput {
            serde_json::from_value(
                serde_json::json!({ "id": id, "name": name, "provider": "azure", "host": region, "username": subscription, "datastore": "Standard_B2s" }),
            )
            .unwrap()
        };
        let sub = "00000000-0000-0000-0000-000000000000";
        let saved = server_save(app(), azure(None, "westeurope", sub, "Azure")).unwrap();
        let lab = t.lab("placed", &[]);
        std::fs::write(lab.join(HOST_MARKER), &saved.id).unwrap();
        let err = server_save(app(), azure(Some(&saved.id), "eastus", sub, "Azure")).unwrap_err().to_string();
        assert_eq!(err, "1 lab is still on this account: stop it before changing its address or region.");
        let err = server_save(app(), azure(Some(&saved.id), "westeurope", "11111111-0000-0000-0000-000000000000", "Azure")).unwrap_err().to_string();
        assert!(err.ends_with("changing its account."), "{err}");
        assert_eq!(server_list(app()).unwrap().hosts[0].host, "westeurope", "nothing saved");
        // Its name (and credentials, size, auto-stop) can change.
        assert_eq!(server_save(app(), azure(Some(&saved.id), "westeurope", sub, "Renamed")).unwrap().name, "Renamed");
        // Stopped: it can move.
        std::fs::remove_file(lab.join(HOST_MARKER)).unwrap();
        assert_eq!(server_save(app(), azure(Some(&saved.id), "eastus", sub, "Renamed")).unwrap().host, "eastus");
    }

    #[test]
    fn a_host_moves_only_with_nothing_on_it_and_renewed_keys_never_move_it() {
        let aws = |region: &str, key: &str| HostProfile { host: region.into(), username: key.into(), ..host("a1", Provider::Aws) };
        let (now, keys) = (aws("eu-west-3", "AKIAOLD"), aws("eu-west-3", "AKIANEW"));
        // A new access key (the old one deleted or expired) is how to sign in, not where.
        keeps_placement(&now, &keys, 2).unwrap();
        let err = keeps_placement(&now, &aws("us-east-1", "AKIAOLD"), 2).unwrap_err().to_string();
        assert_eq!(err, "2 labs are still on this account: stop them before changing its address or region.");
        keeps_placement(&now, &aws("us-east-1", "AKIAOLD"), 0).unwrap();
        // A server pointed at another machine or node, or turned into another provider.
        let pve = host("p1", Provider::Proxmox);
        let err = keeps_placement(&pve, &HostProfile { node: Some("pve2".into()), host: "10.0.0.9".into(), ..pve.clone() }, 1).unwrap_err().to_string();
        assert_eq!(err, "1 lab is still on this host: stop it before changing its address or region, node or organization.");
        assert!(keeps_placement(&pve, &HostProfile { provider: Provider::VmwareEsxi, ..pve.clone() }, 1).unwrap_err().to_string().contains("provider"));
        keeps_placement(&pve, &HostProfile { username: "root@pam!token".into(), datastore: Some("other".into()), ..pve.clone() }, 1).unwrap();
        // An AWS CLI account switched to another profile (likely another AWS account), or from
        // stored keys to the CLI.
        let cli = HostProfile { use_cli_creds: true, aws_profile: Some("labs".into()), ..host("a2", Provider::Aws) };
        let err = keeps_placement(&cli, &HostProfile { aws_profile: Some("personal".into()), ..cli.clone() }, 1).unwrap_err().to_string();
        assert!(err.ends_with("changing its AWS CLI profile."), "{err}");
        assert!(keeps_placement(&HostProfile { use_cli_creds: false, aws_profile: None, ..cli.clone() }, &cli, 1).is_err());
    }

    #[test]
    fn saves_edit_known_hosts_or_make_new_ones() {
        let s = store(vec![host("ab12", Provider::Proxmox)], None);
        assert_eq!(save_id(&s, Some("ab12".into())).unwrap(), "ab12");
        assert_eq!(save_id(&s, Some("cd34".into())).unwrap_err().to_string(), "unknown server host");
        assert_eq!(save_id(&s, Some("../x".into())).unwrap_err().to_string(), "unknown server host");
        let fresh = save_id(&s, None).unwrap();
        assert_eq!(fresh.len(), 16);
        assert_ne!(fresh, "ab12");
    }

    #[test]
    fn gcp_keeps_its_project_unless_the_billing_account_changed() {
        let gcp = HostProfile { username: "AAAAAA-BBBBBB-CCCCCC".into(), gcp_project: Some("cyberctf-labs-abc123".into()), ..host("ab12", Provider::Gcp) };
        let s = store(vec![gcp], None);
        assert_eq!(kept_gcp_project(&s, "ab12", Provider::Gcp, "AAAAAA-BBBBBB-CCCCCC").as_deref(), Some("cyberctf-labs-abc123"));
        assert_eq!(kept_gcp_project(&s, "ab12", Provider::Gcp, "DDDDDD-BBBBBB-CCCCCC"), None);
        assert_eq!(kept_gcp_project(&s, "ab12", Provider::Aws, "AAAAAA-BBBBBB-CCCCCC"), None);
        assert_eq!(kept_gcp_project(&s, "zz", Provider::Gcp, "AAAAAA-BBBBBB-CCCCCC"), None);
    }

    #[test]
    fn passwords_are_kept_when_blank_and_bounded_when_given() {
        assert_eq!(new_password(None).unwrap(), None);
        assert_eq!(new_password(Some(String::new())).unwrap(), None);
        assert_eq!(new_password(Some("hunter2".into())).unwrap().as_deref(), Some("hunter2"));
        assert!(new_password(Some("a\0b".into())).is_err());
        assert!(new_password(Some("x".repeat(1025))).is_err());
        assert!(new_password(Some("x".repeat(1024))).is_ok());
    }

    #[test]
    fn the_first_server_saved_becomes_the_default_but_never_a_cloud() {
        let mut s = store(Vec::new(), None);
        put_host(&mut s, host("aws1", Provider::Aws));
        assert_eq!(s.default, None);
        put_host(&mut s, host("pve1", Provider::Proxmox));
        assert_eq!(s.default.as_deref(), Some("pve1"));
        put_host(&mut s, host("esx1", Provider::VmwareEsxi));
        assert_eq!(s.default.as_deref(), Some("pve1"));
        // Saving again replaces the entry instead of adding one.
        put_host(&mut s, HostProfile { name: "Renamed".into(), ..host("pve1", Provider::Proxmox) });
        assert_eq!(s.hosts.len(), 3);
        assert_eq!(s.hosts.iter().find(|h| h.id == "pve1").unwrap().name, "Renamed");
    }

    #[test]
    fn removing_the_default_hands_it_to_another_server() {
        let mut s = store(vec![host("aws1", Provider::Aws), host("pve1", Provider::Proxmox), host("esx1", Provider::VmwareEsxi)], Some("pve1"));
        remove_host(&mut s, "pve1");
        assert_eq!(s.default.as_deref(), Some("esx1"));
        assert_eq!(s.hosts.len(), 2);
        // Removing a non-default leaves the default alone.
        remove_host(&mut s, "aws1");
        assert_eq!(s.default.as_deref(), Some("esx1"));
        // No server left: no default (the cloud account never takes it).
        let mut s = store(vec![host("aws1", Provider::Aws), host("esx1", Provider::VmwareEsxi)], Some("esx1"));
        remove_host(&mut s, "esx1");
        assert_eq!(s.default, None);
    }

    #[test]
    fn hosts_with_running_labs_are_not_removed() {
        assert!(no_labs_running(0).is_ok());
        assert_eq!(no_labs_running(1).unwrap_err().to_string(), "1 lab still running on this host. Stop it before removing the host.");
        assert_eq!(no_labs_running(3).unwrap_err().to_string(), "3 labs still running on this host. Stop them before removing the host.");
    }

    #[test]
    fn only_a_saved_server_can_be_the_default() {
        let mut s = store(vec![host("aws1", Provider::Aws), host("pve1", Provider::Proxmox)], None);
        set_default_host(&mut s, Some("pve1".into())).unwrap();
        assert_eq!(s.default.as_deref(), Some("pve1"));
        assert_eq!(set_default_host(&mut s, Some("aws1".into())).unwrap_err().to_string(), "a cloud account can't be the default host");
        assert!(set_default_host(&mut s, Some("nope".into())).is_err());
        assert_eq!(s.default.as_deref(), Some("pve1"));
        set_default_host(&mut s, None).unwrap();
        assert_eq!(s.default, None);
    }

    #[test]
    fn a_deployment_left_on_a_host_keeps_it_from_being_removed() {
        let base = std::env::temp_dir().join(format!("cyberctf-leftovers-{}", rand::random::<u32>()));
        let deploy = |lab: &str, host: &str, marker: Option<&str>| {
            let state = base.join("deployments").join(lab).join("aws");
            std::fs::create_dir_all(&state).unwrap();
            std::fs::write(state.join("terraform.tfstate"), r#"{"outputs":{"ip":{"value":"203.0.113.7"}}}"#).unwrap();
            std::fs::write(state.join("host"), host).unwrap();
            std::fs::create_dir_all(base.join("labs").join(lab)).unwrap();
            if let Some(m) = marker {
                std::fs::write(base.join("labs").join(lab).join(HOST_MARKER), m).unwrap();
            }
        };
        assert_eq!(leftovers_on(&base, "aws1"), 0);
        // Still marked as running there: counted by its marker already.
        deploy("web-1", "aws1", Some("aws1"));
        assert_eq!(leftovers_on(&base, "aws1"), 0);
        // Moved elsewhere, its AWS state left behind.
        deploy("db-1", "aws1", Some("pve1"));
        deploy("api-1", "aws2", None);
        assert_eq!(leftovers_on(&base, "aws1"), 1);
        assert_eq!(leftovers_on(&base, "aws2"), 1);
        std::fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn a_deployment_left_by_an_uninstalled_lab_keeps_its_host() {
        // The lab's folder is gone (uninstalled) but its Terraform state still holds resources
        // on the account: only this host's credentials can destroy them, so it stays.
        let t = crate::testing::test_app::TestApp::new();
        let app = || t.handle().clone();
        let host = t.host("aws1", "aws", serde_json::json!({ "useCliCreds": true }));
        let state = t.data().join("deployments").join("web").join("aws");
        std::fs::create_dir_all(&state).unwrap();
        std::fs::write(state.join("terraform.tfstate"), r#"{"outputs":{},"resources":[{"type":"aws_instance"}]}"#).unwrap();
        std::fs::write(state.join("host"), "aws1\n").unwrap();
        let err = server_remove(app(), &host).unwrap_err().to_string();
        assert_eq!(err, "1 lab still running on this host. Stop it before removing the host.");
        assert_eq!(server_list(app()).unwrap().hosts.len(), 1);
        assert!(server_running_labs(app()).is_empty(), "no installed lab is marked as running there");
        // Destroyed (its state gone): free to remove.
        std::fs::remove_file(state.join("terraform.tfstate")).unwrap();
        server_remove(app(), &host).unwrap();
        assert_eq!(server_list(app()).unwrap().hosts, Vec::<HostProfile>::new());
    }

    #[test]
    fn removing_a_host_that_is_not_saved_changes_nothing() {
        let t = crate::testing::test_app::TestApp::new();
        let app = || t.handle().clone();
        t.host("az1", "azure", serde_json::json!({}));
        server_remove(app(), "nope").unwrap();
        let list = server_list(app()).unwrap();
        assert_eq!(list.hosts.iter().map(|h| h.id.as_str()).collect::<Vec<_>>(), ["az1"]);
        // A marker of a lab on a host that was never saved blocks nothing else.
        let lab = t.lab("web", &[]);
        std::fs::write(lab.join(HOST_MARKER), "nope").unwrap();
        assert_eq!(server_running_labs(app()).get("nope"), Some(&1));
        server_remove(app(), "az1").unwrap();
        assert_eq!(server_list(app()).unwrap().hosts, Vec::<HostProfile>::new());
    }

    #[test]
    fn markers_are_read_from_each_lab_folder() {
        let labs = std::env::temp_dir().join(format!("cyberctf-markers-{}", rand::random::<u32>()));
        assert_eq!(markers_in(&labs), Vec::<String>::new());
        for (lab, marker) in [("a", Some("pve1\n")), ("b", Some("pve1")), ("c", None), ("d", Some(""))] {
            std::fs::create_dir_all(labs.join(lab)).unwrap();
            if let Some(m) = marker {
                std::fs::write(labs.join(lab).join(HOST_MARKER), m).unwrap();
            }
        }
        let mut markers = markers_in(&labs);
        markers.sort();
        assert_eq!(markers, ["", "pve1", "pve1"]);
        assert_eq!(count_by_host(&markers).get("pve1"), Some(&2));
        std::fs::remove_dir_all(labs).unwrap();
    }

    #[test]
    fn capacity_needs_every_figure() {
        let d = serde_json::json!({"cpuinfo": {"cpus": 8}, "memory": {"total": 64, "free": 20}});
        let c = capacity_from(&d).unwrap();
        assert_eq!((c.cores, c.mem_total, c.mem_free), (8, 64, 20));
        assert!(capacity_from(&serde_json::json!({"cpuinfo": {"cpus": 8}})).is_none());
    }

    #[test]
    fn the_start_is_recent_right_after_marking_it() {
        mark_started();
        assert!(launched_recently());
    }

    #[test]
    fn running_labs_count_per_host_and_skip_blank_markers() {
        let markers: Vec<String> = ["ab", "cd", "ab", ""].iter().map(ToString::to_string).collect();
        let counts = count_by_host(&markers);
        assert_eq!(counts.get("ab"), Some(&2));
        assert_eq!(counts.get("cd"), Some(&1));
        assert_eq!(counts.len(), 2);
    }

    #[tokio::test]
    async fn an_accounts_spend_is_read_with_its_own_keys_not_the_clis_default() {
        // Regression: the Cloud screen read the spend of an account added with stored keys through
        // the host CLI's default profile: another account's spend, or nothing, while the launch
        // budget gate read the stored keys.
        use crate::platform::exec::fake::{Tools, ok};
        let t = crate::testing::test_app::TestApp::new();
        let cli = t.host("aws-cli", "aws", serde_json::json!({ "useCliCreds": true, "awsProfile": "labs" }));
        let tools = Tools::rules(&[("ce get-cost-and-usage", ok("7.25\n"))]);
        crate::testing::assert_close(server_month_to_date_cost(t.handle().clone(), cli).await.unwrap(), 7.25);
        assert!(tools.called("aws ce get-cost-and-usage"), "{:?}", tools.calls());
        assert_eq!(tools.env_of("ce get-cost-and-usage", "AWS_PROFILE").as_deref(), Some("labs"));
        // Not an AWS account, or not saved: no figure.
        let az = t.host("az1", "azure", serde_json::json!({}));
        assert_eq!(server_month_to_date_cost(t.handle().clone(), az).await.unwrap_err().to_string(), "only AWS accounts report their spend");
        assert!(server_month_to_date_cost(t.handle().clone(), "nope".into()).await.is_err());
        // The setup form's figure, for a profile not saved yet.
        crate::testing::assert_close(crate::runtime::server::cli_month_to_date_cost(None).await.unwrap(), 7.25);
    }

    /// Each self-test event's step and state, as the UI gets them.
    fn steps(events: &crate::testing::test_app::Sink) -> Vec<(String, String)> {
        events.messages().iter().map(|e| (e["step"].as_str().unwrap().to_string(), e["state"].as_str().unwrap().to_string())).collect()
    }

    #[tokio::test]
    async fn the_vm_test_runs_on_proxmox_and_esxi_hosts_only() {
        let t = crate::testing::test_app::TestApp::new();
        let tools = crate::platform::exec::fake::Tools::rules(&[]);
        let app = || t.handle().clone();
        let pair = |a: &str, b: &str| (a.to_string(), b.to_string());

        // No such host: the first step fails and nothing else runs.
        let events = crate::testing::test_app::Sink::default();
        assert!(server_selftest(app(), "nohost".into(), events.channel()).await.is_err());
        assert_eq!(steps(&events), [pair("connect", "running"), pair("connect", "fail")]);

        // A cloud account: reached, then refused.
        t.host("az1", "azure", serde_json::json!({}));
        let events = crate::testing::test_app::Sink::default();
        let err = server_selftest(app(), "az1".into(), events.channel()).await.unwrap_err();
        assert_eq!(err.to_string(), "the VM test runs only on a Proxmox or ESXi host");
        assert_eq!(steps(&events), [pair("connect", "running"), pair("connect", "ok")]);
        assert_eq!(events.messages()[1]["detail"], "Host az1");

        // AWS is billed: never a test. A folder a previous run left is cleared first.
        t.host("aws1", "aws", serde_json::json!({ "useCliCreds": true }));
        let work = t.data().join("selftest").join("server").join("aws1");
        std::fs::create_dir_all(&work).unwrap();
        let events = crate::testing::test_app::Sink::default();
        let err = server_selftest(app(), "aws1".into(), events.channel()).await.unwrap_err();
        assert!(err.to_string().contains("AWS labs are billed"), "{err}");
        assert!(steps(&events).contains(&pair("prepare", "ok")), "{:?}", steps(&events));
        assert!(!work.exists());
        assert!(tools.calls().is_empty(), "{:?}", tools.calls());
    }
}
