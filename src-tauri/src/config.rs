//! Service endpoints. Defaults point at production; each can be overridden at
//! build time (option_env!) or at run time (env var of the same name), e.g. to
//! run the whole flow against a local cyber-auth and CyberBackend.

fn setting(name: &str, build_time: Option<&'static str>, default: &'static str) -> String {
    std::env::var(name).ok().filter(|v| !v.is_empty()).unwrap_or_else(|| build_time.unwrap_or(default).to_string())
}

/// Better Auth base, e.g. https://www.cyberauth.co/api/auth
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
/// http://127.0.0.1:<port>/callback (cyber-auth matches redirect URIs exactly).
pub const REDIRECT_PORTS: [u16; 3] = [47290, 47291, 47292];

/// Whether sign-in comes back as a `cyberctf://auth/callback` link instead of to a loopback
/// page (`CYBERCTF_REDIRECT=scheme`; `loopback` otherwise). The OAuth client in cyber-auth must
/// list that URI first: switch the default once it does.
pub fn scheme_redirect() -> bool {
    setting("CYBERCTF_REDIRECT", option_env!("CYBERCTF_REDIRECT"), "loopback") == "scheme"
}

// Used by the keychain-backed session store in release builds (dev stores a file).
#[cfg_attr(debug_assertions, allow(dead_code))]
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
    fn the_endpoints_have_values() {
        assert!(auth_base().starts_with("http"));
        assert!(api_url().starts_with("http"));
        assert!(!client_id().is_empty());
    }
}
