//! What `VBoxManage` prints, read: `list vms` / `list runningvms` and `showvminfo
//! --machinereadable`'s values. Its fixtures are in `fixtures/vboxmanage/`.

/// One VM of a `VBoxManage list vms` or `list runningvms` listing (`"<name>" {<uuid>}`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Vm {
    /// Its name, as VirtualBox shows it.
    pub name: String,
    /// Its UUID, without braces.
    pub uuid: String,
}

/// Each VM `out` lists; a line that isn't `"<name>" {<uuid>}` (a cut last line, a warning) names
/// none. A name may hold braces and quotes: the UUID is the last `{…}` of its line.
#[must_use]
pub fn vms(out: &str) -> Vec<Vm> {
    out.lines()
        .filter_map(|line| {
            let line = line.trim_end_matches('\r');
            let (head, tail) = line.rsplit_once('{')?;
            let uuid = tail.strip_suffix('}')?;
            let name = head.trim().strip_prefix('"')?.strip_suffix('"')?;
            Some(Vm { name: name.to_owned(), uuid: uuid.to_owned() })
        })
        .collect()
}

/// The value of `key` in `showvminfo --machinereadable`'s output (`key="value"` lines, or a bare
/// number), unescaped: VirtualBox 6.1 and later write a value's `\` as `\\` and its `"` as `\"`.
#[must_use]
pub fn value(info: &str, key: &str) -> Option<String> {
    let prefix = format!("{key}=");
    info.lines().find_map(|l| {
        let raw = l.trim_end_matches('\r').strip_prefix(&prefix)?;
        Some(match raw.strip_prefix('"').and_then(|r| r.strip_suffix('"')) {
            Some(quoted) => unescape(quoted),
            None => raw.to_owned(),
        })
    })
}

/// A quoted value's escapes undone (`\\` is `\`, `\"` is `"`).
fn unescape(quoted: &str) -> String {
    let mut out = String::with_capacity(quoted.len());
    let mut chars = quoted.chars();
    while let Some(c) = chars.next() {
        match (c, chars.clone().next()) {
            ('\\', Some(next @ ('\\' | '"'))) => {
                out.push(next);
                chars.next();
            }
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listings_read_as_vboxmanage_prints_them() {
        let names = |out: &str| vms(out).into_iter().map(|v| v.name).collect::<Vec<_>>();
        assert_eq!(names(include_str!("fixtures/vboxmanage/7.0.20-list-vms.txt")), ["lab · web", "lab · {odd} \"name\"", "<inaccessible>"]);
        assert_eq!(names(include_str!("fixtures/vboxmanage/7.1.4-windows-list-runningvms.txt")), ["lab · web"]);
        assert_eq!(names(include_str!("fixtures/vboxmanage/7.0.20-list-vms-cut.txt")), ["lab · web"], "a cut line names no VM");
        assert_eq!(vms("")[..], []);
        assert_eq!(vms(include_str!("fixtures/vboxmanage/7.0.20-list-vms.txt"))[0].uuid, "6f2c1d8e-4b3a-4c5d-9e8f-0a1b2c3d4e5f");
    }

    /// Regression (#369): a Windows path in a description is written with its `\` doubled.
    #[test]
    fn machine_readable_values_are_unescaped() {
        let info = include_str!("fixtures/vboxmanage/7.1.4-windows-showvminfo.txt");
        assert_eq!(value(info, "description").as_deref(), Some("cyberctf-selftest:C:\\Users\\ada\\AppData\\Local\\cyberctf\\selftest\\vm"));
        assert_eq!(value(info, "name").as_deref(), Some("cyberctf-selftest"));
        assert_eq!(value(info, "memory").as_deref(), Some("1024"), "a bare number");
        assert_eq!(value(info, "groups").as_deref(), Some("/"));
        assert_eq!(value(info, "missing"), None);
        assert_eq!(value("description=\"say \\\"hi\\\"\"", "description").as_deref(), Some("say \"hi\""));
    }
}
