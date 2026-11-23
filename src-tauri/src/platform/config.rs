//! Service endpoints. Defaults point at production; each can be overridden at build time
//! (`option_env!`), and in a debug build also at run time (env var of the same name), e.g. to run
//! the whole flow against a local cyber-auth and CyberBackend.
//!
//! A shipped (release) build never reads them from its environment: whoever or whatever starts
//! the app (a login script, a shortcut, another program) could otherwise send its sign-in and its
//! session token to a server of their choosing.

/// Whether endpoints may come from the environment at run time: debug builds (and tests) only.
const RUNTIME_OVERRIDES: bool = cfg!(debug_assertions);

fn setting(name: &str, build_time: Option<&'static str>, default: &'static str) -> String {
    setting_in(RUNTIME_OVERRIDES, name, build_time, default)
}

/// The value of setting `name`: the environment's when `runtime_overrides` allows it and it is
/// set, else the build's, else the default.
fn setting_in(runtime_overrides: bool, name: &str, build_time: Option<&'static str>, default: &'static str) -> String {
    let from_env = if runtime_overrides { std::env::var(name).ok() } else { None };
    from_env.filter(|v| !v.is_empty()).unwrap_or_else(|| build_time.unwrap_or(default).to_string())
}

/// Better Auth base, e.g. <https://www.cyberauth.co/api/auth>
pub fn auth_base() -> String {
    setting("CYBERCTF_AUTH_BASE", option_env!("CYBERCTF_AUTH_BASE"), "https://www.cyberauth.co/api/auth")
}

/// CyberBackend origin; also the OAuth `resource` (token audience).
pub fn api_url() -> String {
    setting("CYBERBACKEND_URL", option_env!("CYBERBACKEND_URL"), "https://cyberbackend.com")
}

/// Public OAuth client registered in cyber-auth for the launcher (PKCE, no secret).
pub fn client_id() -> String {
    setting("CYBERCTF_CLIENT_ID", option_env!("CYBERCTF_CLIENT_ID"), "MORSNcedpYSLupYIwbaKyHIXdGgVhanC")
}

/// Loopback redirect ports, all registered on the OAuth client as
/// `http://127.0.0.1:<port>/callback` (cyber-auth matches redirect URIs exactly).
pub const REDIRECT_PORTS: [u16; 3] = [47290, 47291, 47292];

/// Whether sign-in comes back as a `cyberctf://auth/callback` link instead of to a loopback
/// page (`CYBERCTF_REDIRECT=scheme`; `loopback` otherwise). The OAuth client in cyber-auth must
/// list that URI first: switch the default once it does.
pub fn scheme_redirect() -> bool {
    setting("CYBERCTF_REDIRECT", option_env!("CYBERCTF_REDIRECT"), "loopback") == "scheme"
}

/// The keychain service the session and the server secrets are kept under.
pub const KEYCHAIN_SERVICE: &str = "org.cyberctf.desktop";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unset_setting_is_the_build_value_else_the_default() {
        // A name nothing sets (tests don't touch the environment: other threads read it).
        let name = "CYBERCTF_CONFIG_TEST_ONLY_SETTING";
        assert_eq!(setting(name, None, "https://default"), "https://default");
        assert_eq!(setting(name, Some("https://built"), "https://default"), "https://built");
    }

    #[test]
    fn only_a_debug_build_reads_endpoints_from_its_environment() {
        // A release build pointed at another server by whoever starts it would hand that server
        // the sign-in and the session token. PATH stands for a variable that is set.
        assert_eq!(RUNTIME_OVERRIDES, cfg!(debug_assertions));
        let path = std::env::var("PATH").unwrap();
        assert_eq!(setting_in(true, "PATH", Some("https://built"), "https://default"), path);
        assert_eq!(setting_in(false, "PATH", Some("https://built"), "https://default"), "https://built");
        assert_eq!(setting_in(false, "PATH", None, "https://default"), "https://default");
    }

    #[test]
    fn the_endpoints_have_values() {
        assert!(auth_base().starts_with("http"));
        assert!(api_url().starts_with("http"));
        assert_ne!(client_id(), "");
    }
}
