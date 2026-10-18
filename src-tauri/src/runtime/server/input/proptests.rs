//! Property tests for the server setup form: what passes a field check can be interpolated into
//! an endpoint, a CLI argument or a Terraform variable without changing its meaning.

use proptest::prelude::*;

use super::*;
use crate::testing::proptest_support::config;

/// Anything a user could paste, including URLs, flags, shell and control characters.
fn pasted() -> impl Strategy<Value = String> {
    prop_oneof!["(?s).{0,40}", "(https?://)?[a-z0-9.-]{1,20}(:[0-9]{1,5})?(/[a-z]{0,5})?", "[ \t]{0,2}[a-zA-Z0-9.:-]{1,30}[ \t]{0,2}", "-{1,2}[a-z]{1,10}",]
}

/// A field-shape check.
type Check = fn(&str) -> bool;

proptest! {
    #![proptest_config(config())]

    #[test]
    fn a_valid_host_is_a_bare_name_or_address(host in pasted()) {
        if valid_host(&host) {
            prop_assert!(!host.is_empty() && host.len() <= 253);
            prop_assert!(!host.starts_with('-'), "never read as a flag");
            prop_assert!(!host.contains(['/', '@', '?', '#', ' ', '\'', '"', '\\', '$', ';', '`']), "not a URL or shell: {}", host);
        }
    }

    #[test]
    fn real_hosts_are_valid(host in prop_oneof![
        "[a-z0-9]([a-z0-9-]{0,20}[a-z0-9])?(\\.[a-z0-9]([a-z0-9-]{0,10}[a-z0-9])?){0,3}",
        any::<[u8; 4]>().prop_map(|o| std::net::Ipv4Addr::from(o).to_string()),
        any::<[u16; 8]>().prop_map(|s| std::net::Ipv6Addr::from(s).to_string()),
    ]) {
        prop_assert!(valid_host(&host), "{}", host);
    }

    #[test]
    fn clean_trims_and_bounds(value in pasted(), max in 1usize..64) {
        match clean(&value, "field", max) {
            Ok(v) => {
                prop_assert_eq!(v.as_str(), value.trim());
                prop_assert!(!v.is_empty() && v.len() <= max);
                prop_assert!(!v.chars().any(char::is_control));
            }
            Err(_) => {
                let t = value.trim();
                prop_assert!(t.is_empty() || t.len() > max || t.chars().any(char::is_control));
            }
        }
    }

    #[test]
    fn clean_opt_maps_blank_to_none(value in proptest::option::of(pasted())) {
        match clean_opt(value.clone(), "field") {
            Ok(None) => prop_assert!(value.as_deref().is_none_or(|v| v.trim().is_empty())),
            Ok(Some(v)) => prop_assert_eq!(Some(v.as_str()), value.as_deref().map(str::trim)),
            Err(_) => prop_assert!(value.is_some()),
        }
    }

    /// The provider ids are shapes, so a pasted value either is one or is refused; none of them
    /// lets through a separator, a quote or a flag.
    #[test]
    fn provider_ids_never_carry_separators(s in pasted()) {
        let checks: [(&str, Check); 13] = [
            ("id", valid_id),
            ("region", valid_region),
            ("azure_location", valid_azure_location),
            ("subscription", valid_subscription),
            ("gcp_region", valid_gcp_region),
            ("billing", valid_billing_account),
            ("org", valid_org_id),
            ("do_region", valid_do_region),
            ("do_token", valid_do_token),
            ("linode_region", valid_linode_region),
            ("oci_region", valid_oci_region),
            ("ocid", valid_ocid),
            ("access_key_id", valid_access_key_id),
        ];
        for (name, ok) in checks {
            if ok(&s) {
                prop_assert!(!s.starts_with('-'), "{} accepted a flag: {}", name, s);
                prop_assert!(!s.contains(|c: char| c.is_whitespace() || c.is_control() || "/:@?#'\"\\$;`=,".contains(c)), "{} accepted {:?}", name, s);
            }
        }
    }
}

/// What a setup field holds: pasted junk, or something shaped like the provider's ids so the
/// check gets past its first test.
fn field() -> impl Strategy<Value = String> {
    prop_oneof![
        2 => pasted(),
        1 => "[ \t]{0,1}[a-z]{2}(-[a-z0-9]{1,8}){1,3}[ \t&|^%<>\"!()-]{0,2}",
        1 => "[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}",
        1 => "[0-9A-Z]{6}-[0-9A-Z]{6}-[0-9A-Z]{6}",
        1 => "ocid1\\.[a-z]{4,12}\\.[a-z0-9.&|-]{10,40}",
        1 => "(AKIA|ASIA|-)[A-Z0-9&|^%]{12,20}",
        1 => "[a-zA-Z0-9_&|%-]{38,60}",
        1 => "[a-z0-9-]{1,20}@(pam|pve)",
    ]
}

fn remote() -> impl Strategy<Value = Provider> {
    proptest::sample::select(vec![
        Provider::VmwareEsxi,
        Provider::Proxmox,
        Provider::Aws,
        Provider::Azure,
        Provider::Gcp,
        Provider::DigitalOcean,
        Provider::Linode,
        Provider::Oci,
    ])
}

proptest! {
    #![proptest_config(config())]

    /// Whatever the form holds, what `check` lets through as a cloud's region, account or token
    /// can go to `az`/`gcloud`/`oci` (through cmd.exe on Windows) as an argument of its own.
    #[test]
    fn checked_cloud_fields_are_safe_cli_arguments(
        provider in remote(),
        host in field(),
        username in field(),
        password in proptest::option::of(field()),
        node in proptest::option::of(field()),
        use_cli in any::<bool>(),
    ) {
        let input: HostInput = serde_json::from_value(serde_json::json!({
            "name": "x", "provider": provider, "host": host, "username": username, "password": password,
            "node": node, "useCliCreds": use_cli,
        })).unwrap();
        let Ok(checked) = input.check() else { return Ok(()) };
        prop_assert!(valid_host(&checked.host) || provider.is_cloud(), "{:?}", checked.host);
        let mut args = vec![checked.host.as_str()];
        if provider.is_cloud() {
            args.push(checked.username.as_str());
            if provider == Provider::Gcp {
                args.extend(node.as_deref());
            }
            if matches!(provider, Provider::DigitalOcean | Provider::Linode) {
                args.extend(password.as_deref());
            }
        }
        for a in args.iter().filter(|a| !a.is_empty()) {
            prop_assert!(!a.starts_with('-'), "{:?} accepted a flag: {:?}", provider, a);
            prop_assert!(!a.chars().any(|c| c.is_control() || c.is_whitespace()), "{:?} accepted {:?}", provider, a);
            prop_assert!(crate::platform::exec::shim::check_args("az", &[a]).is_ok(), "{:?} accepted a cmd.exe operator: {:?}", provider, a);
            prop_assert!(!a.contains(['!', '(', ')']), "{:?} accepted {:?}", provider, a);
        }
        // The address of a server is never a URL or a `host:port`.
        if !provider.is_cloud() {
            prop_assert!(!checked.host.contains('/') && (checked.host.parse::<std::net::Ipv6Addr>().is_ok() || !checked.host.contains(':')));
        }
        prop_assert!(!checked.username.chars().any(char::is_control));
    }
}
