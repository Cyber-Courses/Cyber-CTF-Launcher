//! Property tests for the server setup form: what passes a field check can be interpolated into
//! an endpoint, a CLI argument or a Terraform variable without changing its meaning.

use proptest::prelude::*;

use super::*;
use crate::proptest_support::config;

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
