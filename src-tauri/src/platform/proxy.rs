//! Corporate proxies, for the tools the app starts (docker, vagrant, terraform, git, the cloud
//! CLIs, apt): they read the proxy from the environment, in either case.
//!
//! - A GUI launch on macOS doesn't get the shell's proxy variables. When none is set, the
//!   system's proxy (System Settings > Network > Proxies, or an MDM profile, as `scutil --proxy`
//!   reports it) is put into this process's environment at startup, which every child inherits.
//!   A proxy auto-config (PAC) file can't be put into variables: the log says so.
//! - pkexec (Linux installs) clears the environment: [`assignments`] hands the variables to the
//!   root command explicitly.

/// The proxy variables, both cases: tools read one or the other (curl and apt the lowercase
/// ones, Go's and Docker's either).
#[cfg(any(target_os = "linux", target_os = "macos", test))]
pub const VARS: [&str; 6] = ["HTTP_PROXY", "HTTPS_PROXY", "NO_PROXY", "http_proxy", "https_proxy", "no_proxy"];

/// The proxy variables `var` reads that are set, as `NAME=value` (what `env` takes).
#[cfg(any(target_os = "linux", test))]
pub fn assignments(var: impl Fn(&str) -> Option<String>) -> Vec<String> {
    VARS.into_iter().filter_map(|name| var(name).filter(|v| !v.is_empty()).map(|v| format!("{name}={v}"))).collect()
}

/// macOS: when no proxy variable is set, sets them from the system's proxy. Call at the very
/// start of `run()`, before any thread is spawned. What happened, for the log (which isn't up yet).
#[cfg(target_os = "macos")]
pub fn inherit_system_proxy() -> Option<String> {
    if VARS.into_iter().any(|name| std::env::var_os(name).is_some()) {
        return None;
    }
    let out = match std::process::Command::new("/usr/sbin/scutil").arg("--proxy").output() {
        Ok(out) if out.status.success() => out,
        Ok(out) => return Some(format!("couldn't read the system proxy: scutil exited with {}", out.status)),
        Err(e) => return Some(format!("couldn't read the system proxy: {e}")),
    };
    let found = from_scutil(&String::from_utf8_lossy(&out.stdout));
    for (name, value) in &found.vars {
        crate::platform::sys::set_env_at_startup(name, std::ffi::OsStr::new(value));
    }
    found.note
}

/// What `scutil --proxy` says, as variables for the tools, and a note for the log.
#[cfg(any(target_os = "macos", test))]
#[derive(Debug, Default, PartialEq)]
struct SystemProxy {
    vars: Vec<(&'static str, String)>,
    note: Option<String>,
}

/// The proxy variables for the system proxy `scutil --proxy` printed (`text`): HTTP and HTTPS,
/// each when enabled, both cases; the bypass list as `NO_PROXY`, with this machine's own
/// addresses added (the app reaches its labs on 127.0.0.1).
#[cfg(any(target_os = "macos", test))]
fn from_scutil(text: &str) -> SystemProxy {
    let entries = scutil_entries(text);
    let get = |key: &str| entries.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str());
    let proxy = |scheme: &str| {
        let on = get(&format!("{scheme}Enable")) == Some("1");
        let host = get(&format!("{scheme}Proxy")).filter(|h| !h.is_empty())?;
        on.then(|| get(&format!("{scheme}Port")).map_or_else(|| format!("http://{host}"), |port| format!("http://{host}:{port}")))
    };
    let mut proxy_vars = Vec::new();
    for (scheme, upper, lower) in [("HTTP", "HTTP_PROXY", "http_proxy"), ("HTTPS", "HTTPS_PROXY", "https_proxy")] {
        if let Some(url) = proxy(scheme) {
            proxy_vars.push((upper, url.clone()));
            proxy_vars.push((lower, url));
        }
    }
    let pac = get("ProxyAutoConfigEnable") == Some("1");
    let note = match (proxy_vars.is_empty(), pac) {
        (true, true) => Some("the system proxy is a proxy auto-config (PAC) file, which tools can't read: set HTTP(S)_PROXY to use one".into()),
        (true, false) => None,
        (false, _) => Some(format!("tools use the system proxy: {}", proxy_vars.iter().map(|(n, v)| format!("{n}={v}")).collect::<Vec<_>>().join(" "))),
    };
    if !proxy_vars.is_empty() {
        let bypass = no_proxy(&exceptions(text));
        proxy_vars.push(("NO_PROXY", bypass.clone()));
        proxy_vars.push(("no_proxy", bypass));
    }
    SystemProxy { vars: proxy_vars, note }
}

/// The `Key : value` lines of `scutil --proxy` (its top-level dictionary).
#[cfg(any(target_os = "macos", test))]
fn scutil_entries(text: &str) -> Vec<(String, String)> {
    text.lines().filter_map(|line| line.split_once(" : ")).map(|(k, v)| (k.trim().to_string(), v.trim().to_string())).collect()
}

/// The hosts in `ExceptionsList : <array> { 0 : *.local ... }`.
#[cfg(any(target_os = "macos", test))]
fn exceptions(text: &str) -> Vec<String> {
    let mut lines = text.lines().skip_while(|l| !l.trim_start().starts_with("ExceptionsList"));
    lines.next();
    lines.take_while(|l| l.trim() != "}").filter_map(|l| l.split_once(" : ")).map(|(_, host)| host.trim().to_string()).collect()
}

/// A `NO_PROXY` value: the system's exceptions, a `*.domain` written `.domain` as the tools read
/// it, and this machine's own addresses.
#[cfg(any(target_os = "macos", test))]
fn no_proxy(exceptions: &[String]) -> String {
    with_loopback(exceptions.iter().map(|host| host.strip_prefix('*').unwrap_or(host)))
}

/// A `no_proxy` list of `hosts` (trimmed, each once, empty ones dropped) with this machine's own
/// addresses added where missing: what reaches a loopback address never goes through a proxy.
pub fn with_loopback<'a>(hosts: impl IntoIterator<Item = &'a str>) -> String {
    let mut list: Vec<&str> = Vec::new();
    for host in hosts.into_iter().map(str::trim).chain(["localhost", "127.0.0.1", "::1"]) {
        if !host.is_empty() && !list.contains(&host) {
            list.push(host);
        }
    }
    list.join(",")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCUTIL: &str = "<dictionary> {
  ExceptionsList : <array> {
    0 : *.local
    1 : 169.254/16
    2 : intranet.corp
  }
  FTPPassive : 1
  HTTPEnable : 1
  HTTPPort : 3128
  HTTPProxy : proxy.corp
  HTTPSEnable : 1
  HTTPSPort : 3129
  HTTPSProxy : proxy.corp
}
";

    #[test]
    fn root_commands_get_the_proxy_variables_that_are_set() {
        // Regression: pkexec cleared the environment, so apt and curl ran as root without the
        // corporate proxy and the install failed behind it.
        let env = |name: &str| match name {
            "https_proxy" => Some("http://proxy.corp:3128".to_string()),
            "NO_PROXY" => Some(".corp,localhost".to_string()),
            "HTTP_PROXY" => Some(String::new()),
            _ => None,
        };
        assert_eq!(assignments(env), ["NO_PROXY=.corp,localhost", "https_proxy=http://proxy.corp:3128"]);
        assert_eq!(assignments(|_| None), [] as [String; 0]);
    }

    #[test]
    fn a_no_proxy_list_keeps_its_entries_once_and_gains_this_machines_addresses() {
        // Regression: written twice, the two lists differed in order and one lacked ::1.
        assert_eq!(with_loopback([".corp", " intranet.lan", ""]), ".corp,intranet.lan,localhost,127.0.0.1,::1");
        assert_eq!(with_loopback(["localhost", ".corp", ".corp"]), "localhost,.corp,127.0.0.1,::1");
        assert_eq!(with_loopback([]), "localhost,127.0.0.1,::1");
    }

    #[test]
    fn the_system_proxy_becomes_the_tools_variables() {
        let found = from_scutil(SCUTIL);
        let get = |name: &str| found.vars.iter().find(|(n, _)| *n == name).map(|(_, v)| v.as_str());
        assert_eq!(get("HTTP_PROXY"), Some("http://proxy.corp:3128"));
        assert_eq!(get("http_proxy"), Some("http://proxy.corp:3128"));
        assert_eq!(get("HTTPS_PROXY"), Some("http://proxy.corp:3129"));
        assert_eq!(get("https_proxy"), Some("http://proxy.corp:3129"));
        assert_eq!(get("NO_PROXY"), Some(".local,169.254/16,intranet.corp,localhost,127.0.0.1,::1"));
        assert_eq!(get("no_proxy"), get("NO_PROXY"));
        assert!(found.note.is_some_and(|n| n.contains("HTTPS_PROXY=http://proxy.corp:3129")));
    }

    #[test]
    fn a_disabled_proxy_or_a_pac_file_sets_nothing() {
        assert_eq!(from_scutil(&SCUTIL.replace("HTTPEnable : 1", "HTTPEnable : 0").replace("HTTPSEnable : 1", "HTTPSEnable : 0")), SystemProxy::default());
        assert_eq!(from_scutil("<dictionary> {\n  HTTPEnable : 0\n}\n"), SystemProxy::default());
        let pac = from_scutil("<dictionary> {\n  ProxyAutoConfigEnable : 1\n  ProxyAutoConfigURLString : http://wpad/wpad.dat\n}\n");
        assert!(pac.vars.is_empty() && pac.note.is_some_and(|n| n.contains("PAC")));
        // Only HTTPS: HTTP stays unset.
        let https = from_scutil(&SCUTIL.replace("HTTPEnable : 1", "HTTPEnable : 0"));
        assert!(https.vars.iter().all(|(n, _)| !n.eq_ignore_ascii_case("http_proxy")));
        assert!(https.vars.iter().any(|(n, _)| *n == "HTTPS_PROXY"));
    }
}
