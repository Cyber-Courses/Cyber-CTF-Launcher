//! Property tests for reading winget's output: any line is read without a panic, and the
//! installer's exit code comes back however winget prints it.

use proptest::prelude::*;

use super::*;
use crate::testing::proptest_support::config;

const PREFIX: &str = "Installer failed with exit code:";

proptest! {
    #![proptest_config(config())]

    #[test]
    fn any_line_reads_without_a_panic(line in "(?s).{0,80}", rest in "[0-9a-fA-FxX+ -]{0,14}") {
        let _ = installer_exit_code(&line);
        let _ = installer_exit_code(&format!("{PREFIX}{rest}"));
        let _ = installer_exit_code(&format!("{PREFIX} {line}"));
    }

    #[test]
    fn the_exit_code_reads_back_in_every_form_winget_prints(code in any::<u32>(), ws in "[ \t]{0,3}", upper in any::<bool>(), crlf in any::<bool>()) {
        let end = if crlf { "\r" } else { "" };
        let hex = if upper { format!("0X{code:X}") } else { format!("0x{code:x}") };
        for v in [code.to_string(), hex, format!("0x{code:08X}")] {
            prop_assert_eq!(installer_exit_code(&format!("{ws}{PREFIX}{ws}{v}{ws}{end}")), Some(code), "{}", v);
        }
        // A negative HRESULT as a signed number is the same code.
        prop_assert_eq!(installer_exit_code(&format!("{PREFIX} {}", code as i32)), Some(code));
    }

    #[test]
    fn a_code_out_of_range_or_another_line_is_none(big in (u32::MAX as u64 + 1)..u64::MAX, word in "[a-zA-Z ]{1,20}") {
        prop_assert_eq!(installer_exit_code(&format!("{PREFIX} {big}")), None);
        prop_assert_eq!(installer_exit_code(&format!("{PREFIX} 0x{big:x}")), None);
        prop_assume!(!word.trim().is_empty());
        prop_assert_eq!(installer_exit_code(&format!("{PREFIX} {word}")), None);
        prop_assert_eq!(installer_exit_code(&format!("Installer exited with code: {big}")), None);
    }

    /// A summary is given only for codes known to mean something, and a cancelled installer is
    /// said as a cancel whatever winget's own code around it.
    #[test]
    fn summaries_are_only_for_known_codes(code in prop_oneof![any::<i32>(), Just(WINGET_INSTALLER_FAILED as i32), Just(0x8A15_010C_u32 as i32)], installer in proptest::option::of(prop_oneof![any::<u32>(), Just(1223u32), Just(1602u32)]), removing in any::<bool>()) {
        let got = winget_failure(Some(code), installer, removing);
        let cancelled = installer.is_some_and(installer_cancelled) && code as u32 == WINGET_INSTALLER_FAILED;
        prop_assert_eq!(got.is_some(), cancelled || winget_summary(code as u32).is_some());
        if cancelled {
            prop_assert!(got.unwrap().contains("cancelled"));
        }
        prop_assert_eq!(winget_failure(None, installer, removing), None);
        prop_assert_eq!(installer_failure(Some(code), removing).is_some(), installer_cancelled(code as u32));
    }
}
