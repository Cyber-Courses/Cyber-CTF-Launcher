//! The player's settings and what the launcher remembers of this machine for them: the attack
//! box's options, where labs run here (the hypervisor, the Docker engine, the host ports, a
//! dual-form lab's VM form), the last self-tests and server VM tests, and when each lab was last
//! started. One typed set ([`Settings`]), each setting with its default here and nowhere else,
//! kept in the launcher's store (`crate::store`, one key each), so the app, its command line and
//! its background work read the same values without a window. The webview reads them whole
//! (`settings_get`) and changes them one by one (`settings_change`); earlier versions kept them in
//! the webview's own storage, which the webview hands over once
//! ([`settings_import_webview`]).

use std::collections::BTreeMap;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::error::{Error, Result};
use crate::machine::system::docker::Engine;
use crate::platform::AppDirs;
use crate::runtime::providers::Provider;
use crate::store::{self, settings as kv};

/// How a container lab here gets its host ports.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum PortMode {
    /// Free ports, picked at each start: never a clash.
    Random,
    /// Each service on its own container port (`CTFd` on 8000): the start fails when one is taken.
    Default,
}

/// The form a dual-form lab (containers or VMs) takes here, saved by "don't ask again".
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum VmForm {
    /// One VM per machine of the lab.
    Vms,
}

/// How a test ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum TestResult {
    /// It passed.
    Ok,
    /// It failed.
    Fail,
}

/// A test's last run, as the Machine and Servers pages show it ("tested 2 days ago").
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TestRecord {
    /// How it ended.
    pub result: TestResult,
    /// When, in milliseconds since the Unix epoch (the webview's clock).
    pub at: u64,
    /// Why it failed: the failing step's detail; none for a test that passed.
    #[serde(default)]
    pub why: Option<String>,
}

/// Every setting, each at its value or its default.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    /// The attack box image the player chose; none follows the launcher's pinned default.
    pub attack_image: Option<String>,
    /// Start the attack box with each lab, wherever it runs (on by default).
    pub attack_auto: bool,
    /// A VM attack box gets Kali's desktop (off by default: a shell is enough).
    pub attack_desktop: bool,
    /// The hypervisor VM labs run on here; none picks one.
    pub vm_provider: Option<Provider>,
    /// The Docker engine a lab here runs on; none follows the current Docker context.
    pub docker_engine: Option<Engine>,
    /// How a container lab here gets its host ports; none asks at each start.
    pub port_mode: Option<PortMode>,
    /// A dual-form lab's form here; none asks.
    pub vm_form: Option<VmForm>,
    /// The last self-test of each kind (`docker`, `vm`) and of each engine or hypervisor
    /// (`docker.orbstack`, `vm.utm`).
    pub self_tests: BTreeMap<String, TestRecord>,
    /// The last VM test of each saved server, by its id.
    pub server_tests: BTreeMap<String, TestRecord>,
    /// When each lab was last started here, in milliseconds since the Unix epoch, by lab id.
    pub last_runs: BTreeMap<String, u64>,
}

impl Default for Settings {
    /// The settings of a player who chose nothing.
    fn default() -> Settings {
        Settings {
            attack_image: None,
            attack_auto: true,
            attack_desktop: false,
            vm_provider: None,
            docker_engine: None,
            port_mode: None,
            vm_form: None,
            self_tests: BTreeMap::new(),
            server_tests: BTreeMap::new(),
            last_runs: BTreeMap::new(),
        }
    }
}

/// One change to the settings. `null` (`None`) puts an optional setting back to its default.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "setting", content = "value", rename_all = "camelCase")]
pub enum Change {
    /// The attack box image; blank, the default or an earlier default follows the default.
    AttackImage(Option<String>),
    /// Start the attack box with each lab.
    AttackAuto(bool),
    /// A VM attack box gets Kali's desktop.
    AttackDesktop(bool),
    /// The hypervisor VM labs run on here: one of this machine's, never a server or a cloud.
    VmProvider(Option<Provider>),
    /// The Docker engine a lab here runs on.
    DockerEngine(Option<Engine>),
    /// How a container lab here gets its host ports.
    PortMode(Option<PortMode>),
    /// A dual-form lab's form here.
    VmForm(Option<VmForm>),
    /// A self-test's last run, under its key (`docker`, `vm.utm`).
    SelfTest {
        /// The kind, or the kind and the engine or hypervisor.
        key: String,
        /// How and when it ended.
        record: TestRecord,
    },
    /// A saved server's last VM test.
    ServerTest {
        /// The server's id.
        host: String,
        /// How and when it ended.
        record: TestRecord,
    },
    /// When a lab was last started here; none forgets it (the lab was removed).
    LastRun {
        /// The lab's id.
        lab: String,
        /// When, in milliseconds since the Unix epoch.
        at: Option<u64>,
    },
}

/// What the webview's import took: the keys of its storage now in the store (the webview removes
/// them), and the settings after it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
pub struct Imported {
    /// The webview storage keys taken, each one to remove there.
    pub taken: Vec<String>,
    /// The settings with what was taken.
    pub settings: Settings,
}

/// Each setting's key in the store's `settings` table.
mod key {
    pub(super) const ATTACK_IMAGE: &str = "attack_box.image";
    pub(super) const ATTACK_AUTO: &str = "attack_box.auto";
    pub(super) const ATTACK_DESKTOP: &str = "attack_box.desktop";
    pub(super) const VM_PROVIDER: &str = "labs.vm_provider";
    pub(super) const DOCKER_ENGINE: &str = "labs.docker_engine";
    pub(super) const PORT_MODE: &str = "labs.port_mode";
    pub(super) const VM_FORM: &str = "labs.vm_form";
    pub(super) const SELF_TESTS: &str = "self_tests";
    pub(super) const SERVER_TESTS: &str = "server_tests";
    pub(super) const LAST_RUNS: &str = "last_runs";
}

/// The settings kept in the store of `dirs`.
///
/// # Errors
///
/// The store can't be opened or read.
pub(crate) fn settings_get(dirs: &AppDirs) -> Result<Settings> {
    store::open(dirs)?.read(load)
}

/// Applies `changes` in order, all or none, and answers the settings after them.
///
/// # Errors
///
/// A change that isn't valid (a hypervisor that isn't this machine's), or the store can't be
/// written: nothing changed.
pub(crate) fn settings_change(dirs: &AppDirs, changes: &[Change]) -> Result<Settings> {
    store::open(dirs)?.write(|tx| {
        for change in changes {
            apply(tx, change)?;
        }
        load(tx)
    })
}

/// The settings `conn`'s store keeps. A value that isn't one (a newer build's, a hand edit) reads
/// as the default, logged, as the webview read its own storage.
pub(crate) fn load(conn: &Connection) -> Result<Settings> {
    let d = Settings::default();
    Ok(Settings {
        attack_image: lenient::<String>(conn, key::ATTACK_IMAGE)?.filter(|i| !follows_default(i)),
        attack_auto: lenient(conn, key::ATTACK_AUTO)?.unwrap_or(d.attack_auto),
        attack_desktop: lenient(conn, key::ATTACK_DESKTOP)?.unwrap_or(d.attack_desktop),
        vm_provider: lenient::<Provider>(conn, key::VM_PROVIDER)?.filter(|p| !p.is_remote()),
        docker_engine: lenient(conn, key::DOCKER_ENGINE)?,
        port_mode: lenient(conn, key::PORT_MODE)?,
        vm_form: lenient(conn, key::VM_FORM)?,
        self_tests: lenient(conn, key::SELF_TESTS)?.unwrap_or_default(),
        server_tests: lenient(conn, key::SERVER_TESTS)?.unwrap_or_default(),
        last_runs: lenient(conn, key::LAST_RUNS)?.unwrap_or_default(),
    })
}

/// Setting `key` as a `T`; none when unset or when what is kept isn't a `T` (logged).
fn lenient<T: serde::de::DeserializeOwned>(conn: &Connection, key: &str) -> Result<Option<T>> {
    let Some(value) = kv::get::<serde_json::Value>(conn, key)? else { return Ok(None) };
    match serde_json::from_value(value) {
        Ok(v) => Ok(Some(v)),
        Err(e) => {
            log::warn!("the setting {key} isn't one this version reads, so it reads as its default: {e}");
            Ok(None)
        }
    }
}

/// Applies one change in `conn`'s transaction.
fn apply(conn: &Connection, change: &Change) -> Result<()> {
    match change {
        Change::AttackImage(image) => {
            let image = image.as_deref().map(str::trim).filter(|i| !i.is_empty() && !follows_default(i));
            kv::set(conn, key::ATTACK_IMAGE, image.as_ref())
        }
        Change::AttackAuto(on) => kv::set(conn, key::ATTACK_AUTO, Some(on)),
        Change::AttackDesktop(on) => kv::set(conn, key::ATTACK_DESKTOP, Some(on)),
        Change::VmProvider(provider) => {
            if let Some(p) = provider.filter(|p| p.is_remote()) {
                return Err(Error::Invalid(format!("{} isn't a hypervisor of this machine", p.label())));
            }
            kv::set(conn, key::VM_PROVIDER, provider.as_ref())
        }
        Change::DockerEngine(engine) => kv::set(conn, key::DOCKER_ENGINE, engine.as_ref()),
        Change::PortMode(mode) => kv::set(conn, key::PORT_MODE, mode.as_ref()),
        Change::VmForm(form) => kv::set(conn, key::VM_FORM, form.as_ref()),
        Change::SelfTest { key: test, record } => update_map(conn, key::SELF_TESTS, test, Some(record.clone())),
        Change::ServerTest { host, record } => update_map(conn, key::SERVER_TESTS, host, Some(record.clone())),
        Change::LastRun { lab, at } => update_map(conn, key::LAST_RUNS, lab, *at),
    }
}

/// Sets (or, for `None`, removes) `entry` in the map kept under `key`.
fn update_map<T: Serialize + serde::de::DeserializeOwned>(conn: &Connection, key: &str, entry: &str, value: Option<T>) -> Result<()> {
    let mut map: BTreeMap<String, T> = lenient(conn, key)?.unwrap_or_default();
    match value {
        Some(v) => map.insert(entry.to_owned(), v),
        None => map.remove(entry),
    };
    kv::set(conn, key, (!map.is_empty()).then_some(&map))
}

/// Whether attack box image `image` is the launcher's default, now or once (its repository, its
/// floating tag, an earlier pinned digest): saved, it would hold the player on an old image.
fn follows_default(image: &str) -> bool {
    image == crate::runtime::attack_box::default_image() || crate::runtime::attack_box::earlier_default_images().iter().any(|i| i == image)
}

/// The keys earlier versions kept the settings under in the webview's storage, and how each one's
/// text reads as a change (none: a value that read as the default there too).
mod webview {
    use super::{BTreeMap, Change, Engine, PortMode, Provider, TestRecord, VmForm};

    /// The prefix of every key the webview's storage kept.
    const PREFIX: &str = "cyberctf.";

    /// The changes stored item `key` = `value` stands for: none for a key that isn't a setting
    /// (the webview's own preferences stay there), an empty list for a setting at its default or
    /// a value that read as none.
    pub(super) fn changes(key: &str, value: &str) -> Option<Vec<Change>> {
        let name = key.strip_prefix(PREFIX)?;
        let one = |c: Option<Change>| Some(c.into_iter().collect());
        match name {
            "attackbox.image" => one(Some(Change::AttackImage(Some(value.to_owned())))),
            "attackbox.auto" => one(Some(Change::AttackAuto(value != "false"))),
            "attackbox.desktop" => one(Some(Change::AttackDesktop(value == "true"))),
            "vm.provider" => one(Provider::from_id(value).filter(|p| !p.is_remote()).map(|p| Change::VmProvider(Some(p)))),
            "docker.engine" => one(serde_json::from_value::<Engine>(value.into()).ok().map(|e| Change::DockerEngine(Some(e)))),
            "labs.ports" => one(match value {
                "random" => Some(Change::PortMode(Some(PortMode::Random))),
                "default" => Some(Change::PortMode(Some(PortMode::Default))),
                _ => None,
            }),
            "labs.vmForm" => one((value == "vms").then_some(Change::VmForm(Some(VmForm::Vms)))),
            "server.vmtest" => Some(server_tests(value)),
            _ => {
                if let Some(test) = name.strip_prefix("selftest.") {
                    let record = serde_json::from_str::<TestRecord>(value).ok();
                    return one(record.map(|record| Change::SelfTest { key: test.to_owned(), record }));
                }
                let lab = name.strip_prefix("lastrun.")?;
                one(value.trim().parse::<u64>().ok().map(|ms| Change::LastRun { lab: lab.to_owned(), at: Some(ms) }))
            }
        }
    }

    /// The servers' VM tests, kept as one JSON object by server id; the entries that aren't a
    /// test are left out, as the webview left them out.
    fn server_tests(value: &str) -> Vec<Change> {
        let all: BTreeMap<String, serde_json::Value> = serde_json::from_str(value).unwrap_or_default();
        all.into_iter().filter_map(|(host, v)| serde_json::from_value::<TestRecord>(v).ok().map(|record| Change::ServerTest { host, record })).collect()
    }
}

/// Takes the settings earlier versions kept in the webview's storage, `items` (its keys and their
/// text): each setting among them goes into the store, over what it holds, in one transaction,
/// and its key is answered for the webview to remove; anything else is left alone. Taking the same
/// items again gives the same settings, so a webview that couldn't remove them (it closed
/// first) hands them over again harmlessly at its next start.
///
/// # Errors
///
/// The store can't be written: nothing was taken.
pub(crate) fn settings_import_webview(dirs: &AppDirs, items: &BTreeMap<String, String>) -> Result<Imported> {
    let mut taken = Vec::new();
    let mut changes = Vec::new();
    for (key, value) in items {
        if let Some(found) = webview::changes(key, value) {
            taken.push(key.clone());
            changes.extend(found);
        }
    }
    let settings = if changes.is_empty() { settings_get(dirs)? } else { settings_change(dirs, &changes)? };
    if !taken.is_empty() {
        log::info!("the settings the webview kept were moved into the launcher's store: {}", taken.join(", "));
    }
    Ok(Imported { taken, settings })
}

#[cfg(test)]
mod tests;
