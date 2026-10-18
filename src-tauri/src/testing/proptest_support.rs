//! Shared strategies for the property tests (`proptests.rs` next to each parser).

use proptest::prelude::*;
use serde_json::Value;

/// How many cases each property runs: enough to find edge cases, small enough that the whole
/// suite stays well under a few seconds.
///
/// Two settings keep this deterministic in CI, where a random seed once failed a different
/// property each run while all passed locally:
///
/// - A fixed RNG seed, so every run (local and CI) explores the same cases. A property that
///   holds therefore holds every run; it cannot pass on one seed and fail on the next. Set
///   `PROPTEST_RNG_SEED=<u64>` to explore elsewhere (a fuzzing run, chasing a new edge) — the
///   seed is only the default.
/// - Failure persistence on (the proptest default; the suite used to turn it `None`, throwing
///   every counterexample away). A found case is written to a `.proptest-regressions` file beside
///   the test and replayed first on every later run. Committed, it pins the case forever, so a
///   finding is reproducible rather than a seed that surfaced a bug in one run and not the next.
pub fn config() -> ProptestConfig {
    let mut cfg = ProptestConfig {
        cases: 256,
        failure_persistence: Some(Box::new(proptest::test_runner::FileFailurePersistence::SourceParallel("proptest-regressions"))),
        ..ProptestConfig::default()
    };
    // `ProptestConfig::default()` already read `PROPTEST_RNG_SEED`; honour it when set, otherwise
    // pin a fixed seed so the suite is reproducible by default.
    if std::env::var_os("PROPTEST_RNG_SEED").is_none() {
        cfg.rng_seed = proptest::test_runner::RngSeed::Fixed(0x0C7F_5EED);
    }
    cfg
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
