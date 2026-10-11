//! Property test: a folder the setting accepts is absolute and mounts as exactly one Docker
//! `-v host:/shared` spec.

use proptest::prelude::*;

use super::*;
use crate::proptest_support::config;

proptest! {
    #![proptest_config(config())]

    #[test]
    fn an_accepted_folder_is_a_plain_absolute_path(path in prop_oneof!["(?s).{0,40}", "/[^\\x00]{0,40}", "[A-Z]:\\\\[^\\x00]{0,30}"]) {
        let p = Path::new(&path);
        if check(p).is_ok() {
            prop_assert!(p.is_absolute());
            prop_assert!(!path.contains(['"', '\n', '\r']));
            // Docker splits `-v` on `:`; the host side must come back whole.
            let spec = format!("{}:{MOUNT_POINT}", p.display());
            let host_parts = spec.split(':').count() - 1;
            // (On Windows, one more for the drive letter.)
            prop_assert!(host_parts == 1 || (cfg!(windows) && host_parts == 2), "{}", spec);
        }
    }

    #[test]
    fn plain_folders_are_accepted(name in "[A-Za-z0-9 ._-]{1,20}") {
        let home = if cfg!(windows) { r"C:\Users\alex" } else { "/Users/alex" };
        prop_assert!(check(&Path::new(home).join(&name)).is_ok());
    }
}
