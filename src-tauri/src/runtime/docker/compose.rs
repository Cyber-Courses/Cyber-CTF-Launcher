//! A lab's Compose project name (its run's too), and the names an older version's attack box
//! carried beside it.

/// What every lab's Compose project, and its run with the runner, is named after: `cyberctf-<id>`.
const PROJECT_PREFIX: &str = "cyberctf-";

/// Lab `id`'s Compose project (and run name): one per lab, so labs never collide and can be
/// cleaned up by name.
pub fn project(id: &str) -> String {
    format!("{PROJECT_PREFIX}{id}")
}

/// The lab whose project (or run, or a name built on it) `name` is: what follows `cyberctf-`, if
/// it starts so. The inverse of [`project`]; whether the rest is a valid lab id is the caller's.
pub fn lab_of_project(name: &str) -> Option<&str> {
    name.strip_prefix(PROJECT_PREFIX)
}

/// Upgrade only: the suffixes of the attack box containers older versions of the launcher ran
/// beside a lab's project (`<project>-isoloom-access`, `-attacker`, `-exegol`). Those were bare
/// `docker run`s with no Compose project, so a teardown by the project label never removes them.
/// Today's box is a service of the lab's own project; nothing makes these any more.
pub const LEGACY_ATTACK_BOX_SUFFIXES: [&str; 3] = ["-isoloom-access", "-attacker", "-exegol"];

/// Upgrade only: the names a box an older version left beside `project` can carry
/// ([`LEGACY_ATTACK_BOX_SUFFIXES`]), for a teardown to remove it by name.
pub fn legacy_attack_box_names(project: &str) -> [String; 3] {
    LEGACY_ATTACK_BOX_SUFFIXES.map(|s| format!("{project}{s}"))
}

#[cfg(test)]
mod tests {
    use super::{lab_of_project, legacy_attack_box_names, project};

    #[test]
    fn project_name_is_namespaced_per_lab() {
        assert_eq!(project("sqli"), "cyberctf-sqli");
        assert_ne!(project("a"), project("b"));
    }

    #[test]
    fn project_names_keep_the_id_verbatim() {
        // Ids are validated before they get here (64 chars, [A-Za-z0-9_-]); the longest is kept whole.
        let long = "a".repeat(64);
        assert_eq!(project(&long), format!("cyberctf-{long}"));
        assert_eq!(project("Up_Per"), "cyberctf-Up_Per");
        // And back: the lab a project, a run or a name built on one is.
        assert_eq!(lab_of_project(&project(&long)), Some(long.as_str()));
        assert_eq!(lab_of_project("cyberctf-web-isoloom-access"), Some("web-isoloom-access"));
        assert_eq!(lab_of_project("other-web"), None);
    }

    #[test]
    fn legacy_attack_box_names_cover_every_older_name() {
        // Tearing a lab down removes these by name: an older version's box is a bare container
        // with no project label, so a project-label removal never catches it.
        assert_eq!(
            legacy_attack_box_names("cyberctf-sqli"),
            ["cyberctf-sqli-isoloom-access", "cyberctf-sqli-attacker", "cyberctf-sqli-exegol"].map(String::from)
        );
    }
}
