//! A lab's Compose project name (its run's too).

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

#[cfg(test)]
mod tests {
    use super::{lab_of_project, project};

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
}
