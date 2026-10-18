//! Shared strategies for the property tests (`proptests.rs` next to each parser).

use proptest::prelude::*;
use serde_json::Value;

/// How many cases each property runs: enough to find edge cases, small enough that the whole
/// suite stays well under a few seconds.
pub fn config() -> ProptestConfig {
    ProptestConfig { cases: 256, failure_persistence: None, ..ProptestConfig::default() }
}

/// Keys the parsers look for, so arbitrary JSON reaches past the first `get`.
const KEYS: &[&str] = &[
    "services",
    "ports",
    "published",
    "target",
    "host_ip",
    "protocol",
    "outputs",
    "value",
    "ip",
    "ssh_user",
    "resources",
    "instances",
    "status",
    "expires_at",
    "Service",
    "State",
    "Name",
    "Publishers",
    "PublishedPort",
    "TargetPort",
    "Protocol",
    "Labels",
    "Health",
    "Image",
];

/// Any JSON document, biased toward the shapes the launcher reads (compose config, compose ps,
/// Terraform state) so a property explores the parsers' branches, not just their first check.
pub fn json() -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::from),
        any::<i64>().prop_map(Value::from),
        any::<u64>().prop_map(Value::from),
        (0u32..70000).prop_map(Value::from),
        any::<f64>().prop_filter("finite", |f| f.is_finite()).prop_map(Value::from),
        prop_oneof![Just(""), Just("0"), Just("80"), Just("65536"), Just("-1"), Just("tainted"), Just("running")].prop_map(Value::from),
        ".{0,12}".prop_map(Value::from),
    ];
    leaf.prop_recursive(5, 96, 6, |inner| {
        let key = prop_oneof![3 => proptest::sample::select(KEYS).prop_map(str::to_string), 1 => ".{0,8}"];
        prop_oneof![
            proptest::collection::vec(inner.clone(), 0..5).prop_map(Value::Array),
            proptest::collection::btree_map(key, inner, 0..5).prop_map(|m| Value::Object(m.into_iter().collect())),
        ]
    })
}

/// Arbitrary text that is sometimes JSON: what a tool may print when it changes format, fails
/// halfway or is not the tool we expected.
pub fn json_ish_text() -> impl Strategy<Value = String> {
    prop_oneof![
        json().prop_map(|v| v.to_string()),
        proptest::collection::vec(json(), 0..4).prop_map(|vs| vs.iter().map(Value::to_string).collect::<Vec<_>>().join("\n")),
        "(?s).{0,300}",
    ]
}
