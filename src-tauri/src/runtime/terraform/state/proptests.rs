//! Property tests for reading Terraform state and the run record: any file content is read
//! without a panic, and the run record round-trips what a destroy needs.

use proptest::prelude::*;
use serde_json::{Value, json};

use super::*;
use crate::proptest_support::{config, json as any_json, json_ish_text};

const KEPT: [&str; 3] = ["ssh_public_key", "ssh_private_key_file", "allowed_cidr"];

fn var() -> impl Strategy<Value = (String, String)> {
    let key = prop_oneof![
        proptest::sample::select(&KEPT[..]).prop_map(str::to_string),
        Just("auto_stop_hours".to_string()),
        Just("aws_secret_access_key".to_string()),
        "[a-z_]{1,12}",
    ];
    (key, prop_oneof!["\\PC{0,30}", any::<u64>().prop_map(|n| n.to_string())])
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn readers_never_panic_on_any_state(v in any_json()) {
        let _ = output_of(&v, "ip");
        let _ = has_tainted(&v);
        let _ = has_instance(&v["outputs"]);
        let _ = has_instance(&v);
    }

    #[test]
    fn saved_vars_never_panics_and_keeps_only_strings(text in json_ish_text()) {
        let vars = saved_vars(&text);
        if let Ok(Value::Object(m)) = serde_json::from_str::<Value>(&text) {
            prop_assert_eq!(vars.len(), m.values().filter(|v| v.is_string()).count());
        } else {
            prop_assert!(vars.is_empty());
        }
    }

    /// What a destroy gets back is exactly the connection variables (the last value of each),
    /// never a secret, and the expiry only when the host stops itself.
    #[test]
    fn the_run_record_round_trips_connection_vars(vars in proptest::collection::vec(var(), 0..8), now in 0u64..=u64::MAX) {
        let record = run_record(&vars, now);
        let mut back = saved_vars(&Value::Object(record.clone()).to_string());
        back.sort();
        let mut want: Vec<(String, String)> = Vec::new();
        for (k, v) in vars.iter().filter(|(k, _)| KEPT.contains(&k.as_str())) {
            want.retain(|(wk, _)| wk != k);
            want.push((k.clone(), v.clone()));
        }
        want.sort();
        prop_assert_eq!(back, want);
        let hours = vars.iter().find(|(k, _)| k == "auto_stop_hours").and_then(|(_, v)| v.parse::<u64>().ok()).filter(|h| *h > 0);
        match hours {
            Some(h) => prop_assert_eq!(record[EXPIRES_AT].as_u64(), Some(now.saturating_add(h.saturating_mul(3600)))),
            None => prop_assert!(!record.contains_key(EXPIRES_AT)),
        }
    }

    #[test]
    fn outputs_are_nonempty_strings_only(value in any_json(), s in "\\PC{0,12}") {
        let state = json!({ "outputs": { "ip": { "value": value.clone() }, "other": { "value": s.clone() } } });
        prop_assert_eq!(output_of(&state, "ip"), value.as_str().filter(|s| !s.is_empty()));
        prop_assert_eq!(has_instance(&state["outputs"]), value.as_str().is_some_and(|s| !s.is_empty()));
        prop_assert_eq!(output_of(&state, "other").is_some(), !s.is_empty());
        prop_assert_eq!(output_of(&state, "missing"), None);
    }

    #[test]
    fn tainted_means_some_instance_says_so(statuses in proptest::collection::vec(proptest::collection::vec(prop_oneof![
        Just(Value::Null), Just(json!("tainted")), Just(json!("ok")), Just(json!(1))
    ], 0..3), 0..4)) {
        let state = json!({ "resources": statuses.iter().map(|is| json!({ "instances": is.iter().map(|s| json!({ "status": s })).collect::<Vec<_>>() })).collect::<Vec<_>>() });
        prop_assert_eq!(has_tainted(&state), statuses.iter().flatten().any(|s| s == "tainted"));
    }
}
