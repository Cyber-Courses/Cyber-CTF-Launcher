//! The machine-wide policy an administrator sets: a few switches that turn launcher features off
//! for everyone on this computer. Read once per process, from the OS's own managed location, and
//! never written by the app:
//!
//! - Windows: the values under `HKEY_LOCAL_MACHINE\SOFTWARE\Policies\CyberCTF\Launcher` (Group
//!   Policy, Intune), each a `REG_DWORD`, 1 on and 0 off;
//! - macOS: `/Library/Managed Preferences/org.cyberctf.desktop.plist`, which an MDM configuration
//!   profile for the `org.cyberctf.desktop` domain installs, each a boolean;
//! - Linux: `/etc/cyberctf/policy.json`, a JSON object of booleans.
//!
//! Linux takes JSON because the launcher already reads it (no other parser to ship), it is what
//! configuration management (Ansible, Puppet, Salt) writes and checks (`jq`) most easily, and it
//! holds the same flat name-to-boolean shape as the registry and the property list, with no
//! implicit typing (TOML's or YAML's) to turn a typo into another value.
//!
//! No policy, or a switch it doesn't name, leaves the feature on. A switch with a value that isn't
//! a boolean is on (the feature off): a policy that can't be read must not open what it closes. A
//! source that is there but can't be read at all turns every switch on. Both are logged, and so is
//! a name the launcher doesn't know (ignored).

#[cfg(test)]
use std::cell::Cell;
use std::sync::OnceLock;

use crate::error::{Code, Error, Result};
#[cfg(any(windows, test))]
use crate::platform::sys::RegistryValue;

/// A policy switch, by the name an administrator writes (matched regardless of case, as the
/// registry does): the UI knows it by that name too.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, ts_rs::TS)]
#[ts(rename = "PolicySetting")]
pub enum Key {
    /// No website agent: this computer doesn't register with Cyber CTF, show online, nor take
    /// launches from the website.
    DisableWebsiteAgent,
    /// No in-app updater: updates come through the organization's own deployment.
    DisableUpdater,
    /// No one-click installs (nor removals) of tools: Docker, hypervisors, Vagrant plugins, cloud
    /// CLIs.
    DisableToolInstalls,
    /// No lab starts on a cloud account (AWS, Azure, Google Cloud, Oracle Cloud, …).
    DisableCloudLaunches,
    /// No lab with a privileged machine (a container with kernel-level access) starts.
    RefusePrivilegedLabs,
}

impl Key {
    /// Every switch.
    pub const ALL: [Key; 5] = [Key::DisableWebsiteAgent, Key::DisableUpdater, Key::DisableToolInstalls, Key::DisableCloudLaunches, Key::RefusePrivilegedLabs];

    /// The switch's name in the policy.
    pub fn name(self) -> &'static str {
        match self {
            Key::DisableWebsiteAgent => "DisableWebsiteAgent",
            Key::DisableUpdater => "DisableUpdater",
            Key::DisableToolInstalls => "DisableToolInstalls",
            Key::DisableCloudLaunches => "DisableCloudLaunches",
            Key::RefusePrivilegedLabs => "RefusePrivilegedLabs",
        }
    }

    /// The switch named `name`, whatever its case.
    fn named(name: &str) -> Option<Key> {
        Key::ALL.into_iter().find(|k| k.name().eq_ignore_ascii_case(name))
    }
}

/// What the policy turns off on this computer: no switch is on unless it says so.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Policy {
    /// Each switch, by its place in [`Key::ALL`].
    on: [bool; Key::ALL.len()],
}

impl Policy {
    /// Whether `key` is on.
    pub fn is_on(self, key: Key) -> bool {
        self.on[key as usize]
    }

    /// The switches that are on.
    pub fn on(self) -> Vec<Key> {
        Key::ALL.into_iter().filter(|k| self.is_on(*k)).collect()
    }

    /// An error naming `key` when it is on: what an operation the policy turns off returns.
    pub fn check(self, key: Key) -> Result<()> {
        if self.is_on(key) { Err(Error::coded(Code::TurnedOffByPolicy { setting: key.name().into() })) } else { Ok(()) }
    }

    fn set(&mut self, key: Key, on: bool) {
        self.on[key as usize] = on;
    }

    /// Every switch on: a policy that is there but can't be read.
    fn all_on() -> Policy {
        Policy { on: [true; Key::ALL.len()] }
    }
}

/// A value as the policy holds it.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Bool(bool),
    /// Anything else, described for the log (`the string "yes"`, `a REG_SZ`).
    Other(String),
}

/// What the OS's policy location held.
#[derive(Debug, PartialEq)]
pub enum Source {
    /// No policy here.
    Absent,
    /// The policy's entries, by name.
    Entries(Vec<(String, Value)>),
    /// A policy is there but can't be read (why).
    Unreadable(String),
}

/// The policy `source` sets, and what is wrong with it (one line each, for the log).
pub fn from_source(source: Source) -> (Policy, Vec<String>) {
    match source {
        Source::Absent => (Policy::default(), Vec::new()),
        Source::Unreadable(why) => (Policy::all_on(), vec![format!("the policy can't be read ({why}): every switch in it is treated as on")]),
        Source::Entries(entries) => from_entries(entries),
    }
}

fn from_entries(entries: Vec<(String, Value)>) -> (Policy, Vec<String>) {
    let mut policy = Policy::default();
    let mut problems = Vec::new();
    for (name, value) in entries {
        let Some(key) = Key::named(&name) else {
            problems.push(format!("unknown policy setting {name:?}: ignored"));
            continue;
        };
        match value {
            Value::Bool(on) => policy.set(key, on),
            Value::Other(what) => {
                problems.push(format!("policy setting {} is {what}, not a boolean: treated as on", key.name()));
                policy.set(key, true);
            }
        }
    }
    (policy, problems)
}

/// The entries of a policy whose values `items` the OS holds in its own form, each read as a
/// boolean by `as_bool` or described as what else it is.
#[cfg(any(target_os = "linux", target_os = "macos", windows, test))]
fn entries_of<V>(items: impl IntoIterator<Item = (String, V)>, as_bool: impl Fn(V) -> std::result::Result<bool, String>) -> Source {
    Source::Entries(items.into_iter().map(|(name, value)| (name, as_bool(value).map_or_else(Value::Other, Value::Bool))).collect())
}

/// The policy in `text`, a JSON object (Linux's `policy.json`).
#[cfg(any(target_os = "linux", test))]
fn json_source(text: &str) -> Source {
    match serde_json::from_str::<serde_json::Value>(text) {
        Ok(serde_json::Value::Object(map)) => entries_of(map, |value| match value {
            serde_json::Value::Bool(on) => Ok(on),
            other => Err(format!("the JSON value {other}")),
        }),
        Ok(other) => Source::Unreadable(format!("not a JSON object: {other}")),
        Err(e) => Source::Unreadable(format!("not JSON: {e}")),
    }
}

/// The policy file at `path`, absent when there is none.
#[cfg(any(target_os = "linux", target_os = "macos", test))]
fn file_source(path: &std::path::Path, parse: impl FnOnce(&[u8]) -> Source) -> Source {
    match std::fs::read(path) {
        Ok(bytes) => parse(&bytes),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Source::Absent,
        Err(e) => Source::Unreadable(format!("{}: {e}", path.display())),
    }
}

/// Where the policy is on Linux.
#[cfg(target_os = "linux")]
pub const LINUX_POLICY: &str = "/etc/cyberctf/policy.json";

#[cfg(target_os = "linux")]
fn read_source() -> Source {
    file_source(std::path::Path::new(LINUX_POLICY), |bytes| match std::str::from_utf8(bytes) {
        Ok(text) => json_source(text),
        Err(e) => Source::Unreadable(format!("not UTF-8: {e}")),
    })
}

/// Where the policy is on macOS: the managed preferences of the app's domain.
#[cfg(target_os = "macos")]
pub const MACOS_POLICY: &str = "/Library/Managed Preferences/org.cyberctf.desktop.plist";

#[cfg(target_os = "macos")]
fn read_source() -> Source {
    file_source(std::path::Path::new(MACOS_POLICY), plist_source)
}

/// The policy in a property list (binary or XML), a dictionary.
#[cfg(target_os = "macos")]
fn plist_source(bytes: &[u8]) -> Source {
    match plist::Value::from_reader(std::io::Cursor::new(bytes)) {
        Ok(plist::Value::Dictionary(dict)) => entries_of(dict, |value| match value {
            plist::Value::Boolean(on) => Ok(on),
            other => Err(format!("the property list value {other:?}")),
        }),
        Ok(other) => Source::Unreadable(format!("not a dictionary: {other:?}")),
        Err(e) => Source::Unreadable(format!("not a property list: {e}")),
    }
}

/// Where the policy is on Windows, under `HKEY_LOCAL_MACHINE`.
#[cfg(any(windows, test))]
pub const WINDOWS_POLICY: &str = r"SOFTWARE\Policies\CyberCTF\Launcher";

#[cfg(windows)]
fn read_source() -> Source {
    use crate::platform::sys::{Hive, registry_values};
    registry_source(registry_values(Hive::LocalMachine, WINDOWS_POLICY))
}

/// The policy in the registry key's values, as `read` (`registry_values`) found them: absent
/// without the key, each `REG_DWORD` 0 or 1 a boolean.
#[cfg(any(windows, test))]
fn registry_source(read: std::io::Result<Option<Vec<(String, RegistryValue)>>>) -> Source {
    match read {
        Ok(None) => Source::Absent,
        Ok(Some(values)) => entries_of(values, |value| match value {
            RegistryValue::Dword(0) => Ok(false),
            RegistryValue::Dword(1) => Ok(true),
            RegistryValue::Dword(n) => Err(format!("the REG_DWORD {n}")),
            RegistryValue::Other(kind) => Err(format!("a registry value of type {kind}, not a REG_DWORD")),
        }),
        Err(e) => Source::Unreadable(format!(r"HKLM\{WINDOWS_POLICY}: {e}")),
    }
}

/// Reads the OS's policy and logs what is set and what is wrong with it.
fn load() -> Policy {
    let (policy, problems) = from_source(read_source());
    for problem in problems {
        log::warn!("{problem}");
    }
    let on: Vec<&str> = policy.on().into_iter().map(Key::name).collect();
    if !on.is_empty() {
        log::info!("managed by policy: {}", on.join(", "));
    }
    policy
}

#[cfg(test)]
thread_local! {
    /// A test's own policy, in place of this machine's (see [`Override`]).
    static OVERRIDE: Cell<Option<Policy>> = const { Cell::new(None) };
}

/// This computer's policy, read the first time it is asked for.
pub fn current() -> Policy {
    static POLICY: OnceLock<Policy> = OnceLock::new();
    #[cfg(test)]
    if let Some(policy) = OVERRIDE.with(Cell::get) {
        return policy;
    }
    *POLICY.get_or_init(load)
}

/// Tests: `policy` in place of this machine's, on this thread, until dropped. A `#[tokio::test]`
/// runs its tasks on its own thread, so what it awaits sees the override too.
#[cfg(test)]
pub struct Override;

#[cfg(test)]
impl Override {
    pub fn with(keys: &[Key]) -> Override {
        let mut policy = Policy::default();
        for key in keys {
            policy.set(*key, true);
        }
        OVERRIDE.with(|o| o.set(Some(policy)));
        Override
    }
}

#[cfg(test)]
impl Drop for Override {
    fn drop(&mut self) {
        OVERRIDE.with(|o| o.set(None));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_registry_keys_values_are_read_as_the_policy_on_every_os() {
        // Regression: this mapping sat in the Windows-only reader, never tested.
        assert_eq!(registry_source(Ok(None)), Source::Absent);
        let read = registry_source(Ok(Some(vec![
            ("DisableUpdater".into(), RegistryValue::Dword(1)),
            ("DisableToolInstalls".into(), RegistryValue::Dword(0)),
            ("DisableCloudLaunches".into(), RegistryValue::Dword(2)),
            ("RefusePrivilegedLabs".into(), RegistryValue::Other(1)),
        ])));
        let (policy, problems) = from_source(read);
        assert!(policy.is_on(Key::DisableUpdater) && !policy.is_on(Key::DisableToolInstalls));
        assert!(policy.is_on(Key::DisableCloudLaunches) && policy.is_on(Key::RefusePrivilegedLabs), "not a boolean: on");
        assert_eq!(
            problems,
            [
                "policy setting DisableCloudLaunches is the REG_DWORD 2, not a boolean: treated as on",
                "policy setting RefusePrivilegedLabs is a registry value of type 1, not a REG_DWORD, not a boolean: treated as on"
            ]
        );
        let denied = registry_source(Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied)));
        assert!(matches!(&denied, Source::Unreadable(why) if why.starts_with(r"HKLM\SOFTWARE\Policies\CyberCTF\Launcher: ")), "{denied:?}");
    }

    fn entries(list: &[(&str, Value)]) -> Source {
        Source::Entries(list.iter().map(|(n, v)| ((*n).to_string(), v.clone())).collect())
    }

    #[test]
    fn each_switch_has_its_own_place() {
        for (i, key) in Key::ALL.into_iter().enumerate() {
            assert_eq!(key as usize, i, "{key:?}");
        }
    }

    #[test]
    fn no_policy_turns_nothing_off() {
        let (policy, problems) = from_source(Source::Absent);
        assert_eq!(policy, Policy::default());
        assert!(problems.is_empty());
        assert!(Key::ALL.into_iter().all(|k| !policy.is_on(k) && policy.check(k).is_ok()));
    }

    #[test]
    fn each_switch_is_read_by_its_name_whatever_its_case() {
        for key in Key::ALL {
            let (policy, problems) = from_source(entries(&[(&key.name().to_lowercase(), Value::Bool(true))]));
            assert!(problems.is_empty(), "{problems:?}");
            assert_eq!(policy.on(), [key]);
        }
        let (policy, _) = from_source(entries(&[("DisableUpdater", Value::Bool(false))]));
        assert!(!policy.is_on(Key::DisableUpdater));
    }

    #[test]
    fn an_unknown_name_is_logged_and_ignored() {
        let (policy, problems) = from_source(entries(&[("DisableEverything", Value::Bool(true)), ("DisableUpdater", Value::Bool(true))]));
        assert!(policy.is_on(Key::DisableUpdater) && !policy.is_on(Key::DisableToolInstalls));
        assert_eq!(problems, [r#"unknown policy setting "DisableEverything": ignored"#]);
    }

    #[test]
    fn a_value_that_is_not_a_boolean_fails_closed_for_its_switch_only() {
        let (policy, problems) =
            from_source(entries(&[("DisableToolInstalls", Value::Other(r#"the JSON value "no""#.into())), ("DisableUpdater", Value::Bool(false))]));
        assert!(policy.is_on(Key::DisableToolInstalls));
        assert!(!policy.is_on(Key::DisableUpdater) && !policy.is_on(Key::DisableCloudLaunches));
        assert_eq!(problems, [r#"policy setting DisableToolInstalls is the JSON value "no", not a boolean: treated as on"#]);
    }

    #[test]
    fn a_policy_that_cannot_be_read_turns_every_switch_on() {
        let (policy, problems) = from_source(Source::Unreadable("permission denied".into()));
        assert!(Key::ALL.into_iter().all(|k| policy.is_on(k)));
        assert_eq!(problems, ["the policy can't be read (permission denied): every switch in it is treated as on"]);
    }

    #[test]
    fn a_switch_that_is_on_refuses_with_its_name() {
        let (policy, _) = from_source(entries(&[("DisableCloudLaunches", Value::Bool(true))]));
        let err = policy.check(Key::DisableCloudLaunches).unwrap_err();
        assert_eq!(err.code(), Code::TurnedOffByPolicy { setting: "DisableCloudLaunches".into() });
        assert!(err.is_refusal(), "nothing was changed: {err}");
        assert!(policy.check(Key::DisableUpdater).is_ok());
    }

    #[test]
    fn linux_reads_a_json_object_of_booleans() {
        assert_eq!(
            json_source(r#"{ "DisableUpdater": true, "DisableToolInstalls": "yes" }"#),
            Source::Entries(vec![("DisableUpdater".into(), Value::Bool(true)), ("DisableToolInstalls".into(), Value::Other(r#"the JSON value "yes""#.into()))])
        );
        assert!(matches!(json_source("[true]"), Source::Unreadable(why) if why.starts_with("not a JSON object")));
        assert!(matches!(json_source("{ DisableUpdater: true }"), Source::Unreadable(why) if why.starts_with("not JSON")));
    }

    #[test]
    fn a_missing_file_is_no_policy_and_an_unreadable_one_is_said() {
        let dir = std::env::temp_dir().join(format!("cyberctf-policy-{}", rand::random::<u64>()));
        assert_eq!(file_source(&dir.join("policy.json"), |_| unreachable!()), Source::Absent);
        std::fs::create_dir_all(dir.join("policy.json")).unwrap();
        // A folder where the file should be: there, but not readable as one.
        assert!(matches!(file_source(&dir.join("policy.json"), |_| Source::Absent), Source::Unreadable(_)));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_reads_the_managed_preferences_dictionary() {
        let xml = br#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict><key>DisableWebsiteAgent</key><true/><key>RefusePrivilegedLabs</key><integer>1</integer></dict></plist>"#;
        let Source::Entries(found) = plist_source(xml) else { panic!("not read") };
        assert_eq!(found[0], ("DisableWebsiteAgent".into(), Value::Bool(true)));
        assert!(matches!(&found[1], (name, Value::Other(_)) if name == "RefusePrivilegedLabs"));
        assert!(matches!(plist_source(b"not a plist"), Source::Unreadable(_)));
    }

    #[test]
    fn a_test_override_stands_in_for_the_machines_policy_until_dropped() {
        let before = current();
        {
            let _policy = Override::with(&[Key::DisableUpdater]);
            assert!(current().is_on(Key::DisableUpdater));
        }
        assert_eq!(current(), before);
    }
}
