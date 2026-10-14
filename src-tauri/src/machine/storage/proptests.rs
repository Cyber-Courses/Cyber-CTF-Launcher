//! Property tests for reading the boxes a Vagrantfile uses and measuring them: any line reads
//! without a panic, assignments read back, and a measured box is one folder of Vagrant's own.

use std::path::Path;

use proptest::prelude::*;

use super::*;
use crate::proptest_support::config;

/// A box name as Vagrant Cloud spells it (`owner/name`) or a plain name.
fn box_name() -> impl Strategy<Value = String> {
    "[a-z0-9][a-z0-9._-]{0,15}(/[a-z0-9][a-z0-9._-]{0,15})?"
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn any_line_reads_without_a_panic(line in "(?s).{0,120}") {
        if let Some(b) = box_assignment(&line) {
            // What comes back was in the line, between the same quotes.
            prop_assert!(line.contains(&b));
            prop_assert!(!b.contains('\n'));
        }
        let _ = parse_image_line(&line);
    }

    #[test]
    fn box_assignments_read_back(
        var in "[a-z][a-z0-9_]{0,8}",
        name in box_name(),
        quote in prop_oneof![Just('"'), Just('\'')],
        indent in "[ \t]{0,6}",
        ws in ("[ ]{0,2}", "[ ]{0,2}"),
        tail in prop_oneof![Just(String::new()), Just(" # was \"old/box\"".to_string()), Just(" if ENV[\"X\"]".to_string()), Just("\r".to_string())],
    ) {
        let line = format!("{indent}{var}.vm.box{}={}{quote}{name}{quote}{tail}", ws.0, ws.1);
        prop_assert_eq!(box_assignment(&line), Some(name.clone()));
        // Commented out, or another setting with the same prefix: not a box.
        prop_assert_eq!(box_assignment(&format!("{indent}# {}", line.trim_start())), None);
        prop_assert_eq!(box_assignment(&format!("{indent}{var}.vm.box_version = {quote}{name}{quote}")), None);
    }

    /// The storage page measures each box's own folder under `boxes/`, never the folder of all
    /// boxes or Vagrant's home.
    #[test]
    fn a_box_folder_is_one_folder_under_boxes(name in box_name()) {
        let home = Path::new("/h/.vagrant.d");
        let dir = box_dir(home, &name);
        let boxes = home.join("boxes");
        prop_assert_eq!(dir.parent(), Some(boxes.as_path()));
        prop_assert!(matches!(dir.components().next_back(), Some(std::path::Component::Normal(_))));
    }

    #[test]
    fn image_lines_read_back(id in "sha256:[a-f0-9]{1,64}", size in any::<u64>(), crlf in any::<bool>()) {
        let end = if crlf { "\r\n" } else { "\n" };
        prop_assert_eq!(parse_image_line(&format!("{id}\t{size}{end}")), Some((id, size)));
    }
}

/// Found by checking `box_dir` against what `box_assignment` can return (shrunk from any line).
#[test]
fn an_empty_or_dot_dot_box_name_is_not_a_box() {
    // They pointed at Vagrant's boxes folder itself (or its home): measured as every box, and
    // `vagrant box remove "" --all` at Cleanup. No such name is read as a box now.
    for line in ["config.vm.box = \"\"", "config.vm.box = \"..\"", "config.vm.box = \".\"", "config.vm.box = \"a/../b\"", "config.vm.box = \"-x\""] {
        assert_eq!(box_assignment(line), None, "{line}");
    }
    // A real box still maps to its one folder under `boxes/`.
    let home = Path::new("/h/.vagrant.d");
    let dir = box_dir(home, &box_assignment("config.vm.box = \"bento/debian-12\"").unwrap());
    assert_eq!(dir.parent(), Some(home.join("boxes").as_path()));
}
