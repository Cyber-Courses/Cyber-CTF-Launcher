//! Property tests for reading tool versions: any output reads without a panic, and the version
//! is the first real line (or `az version`'s `azure-cli` entry).

use proptest::prelude::*;
use serde_json::json;

use super::*;
use crate::testing::proptest_support::{config, json_ish_text};

proptest! {
    #![proptest_config(config())]

    #[test]
    fn first_line_is_the_first_non_blank_line_trimmed(out in "(?s).{0,120}") {
        let want = out.lines().map(str::trim).find(|l| !l.is_empty());
        let got = first_line(&out);
        prop_assert_eq!(got.as_deref(), want);
        if let Some(l) = got {
            prop_assert!(!l.is_empty() && !l.contains('\n') && l == l.trim());
        }
    }

    #[test]
    fn a_banner_of_blank_lines_is_skipped(blank in proptest::collection::vec("[ \t\r]{0,3}", 0..4), version in "[!-~][ -~]{0,20}[!-~]|[!-~]", rest in "(?s).{0,40}") {
        let out = format!("{}{version}\n{rest}", blank.iter().flat_map(|b| [b.as_str(), "\n"]).collect::<String>());
        prop_assert_eq!(first_line(&out), Some(version));
    }

    #[test]
    fn azure_version_reads_only_the_azure_cli_string(out in json_ish_text(), v in "\\PC{0,12}", extra in proptest::collection::btree_map("[a-z-]{1,12}", "\\PC{0,8}", 0..4), pad in "[ \n\r\t]{0,3}") {
        let _ = azure_version(&out);
        let mut doc = json!(extra);
        doc["azure-cli"] = json!(v);
        let text = format!("{pad}{}{pad}", serde_json::to_string_pretty(&doc).unwrap());
        prop_assert_eq!(azure_version(&text), Some(v));
        // Anything but a string is no version.
        doc["azure-cli"] = json!(1);
        prop_assert_eq!(azure_version(&doc.to_string()), None);
    }
}
