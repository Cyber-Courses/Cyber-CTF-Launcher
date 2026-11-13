//! What Vagrant prints, read: `vagrant plugin list`.

/// Each plugin of `vagrant plugin list` with its version, from lines like `vagrant-libvirt
/// (0.12.2, global)`; `None` for a line that names no version whole (a wrapped or cut line).
#[must_use]
pub fn plugins(out: &str) -> Vec<(String, Option<String>)> {
    out.lines()
        .filter_map(|l| {
            let mut words = l.split_whitespace();
            let name = words.next().filter(|name| name.starts_with("vagrant-") || name.starts_with("vagrant_"))?;
            // `(0.12.2,` or `(0.12.2)`: a version word cut before its end (a cut last line) is
            // no version, not a shorter one.
            let version = words
                .next()
                .filter(|w| w.starts_with('(') && (w.ends_with(',') || w.ends_with(')')))
                .map(|w| w.trim_matches(|c: char| matches!(c, '(' | ')' | ',')).to_string())
                .filter(|v| v.starts_with(|c: char| c.is_ascii_digit()));
            Some((name.to_string(), version))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugins_are_read_as_vagrant_lists_them() {
        let listed = plugins(include_str!("fixtures/vagrant/2.4.3-plugin-list.txt"));
        assert_eq!(
            listed,
            [
                ("vagrant-libvirt".to_owned(), Some("0.12.2".to_owned())),
                ("vagrant-vmware-desktop".to_owned(), Some("3.0.4".to_owned())),
                ("vagrant_utm".to_owned(), None),
            ]
        );
        assert_eq!(plugins("No plugins installed."), Vec::<(String, Option<String>)>::new());
    }
}
