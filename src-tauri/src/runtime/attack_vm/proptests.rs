//! Property test: the shared folder reaches the attack VM's Vagrantfile as a Ruby string that
//! reads back as the folder, whatever its name holds (the setting refuses `"` and newlines).

use std::path::Path;

use proptest::prelude::*;

use super::*;
use crate::proptest_support::config;

/// A Ruby double-quoted string's value (the subset of escapes `vagrantfile` writes); None when
/// it would interpolate (`#{`, `#@`, `#$`), i.e. run Ruby instead of naming a folder.
fn ruby_string(lit: &str) -> Option<String> {
    let mut out = String::new();
    let mut chars = lit.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => out.push(chars.next()?),
            '#' if matches!(chars.peek(), Some('{' | '@' | '$')) => return None,
            '"' => return None,
            _ => out.push(c),
        }
    }
    Some(out)
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn the_shared_folder_is_one_inert_ruby_string(path in "/[^\"\n\r\\x00]{0,40}") {
        let vf = vagrantfile("lab", "kalilinux/rolling", &[], Some(Path::new(&path)));
        let line = vf.lines().find(|l| l.trim_start().starts_with("m.vm.synced_folder") && !l.contains("disabled")).expect("the shared folder line");
        let lit = line.trim_start().strip_prefix("m.vm.synced_folder \"").and_then(|r| r.strip_suffix(&format!("\", \"{}\"", crate::shared_folder::MOUNT_POINT))).expect("the line's shape");
        prop_assert_eq!(ruby_string(lit), Some(path));
    }
}
