//! What Windows' `CreateProcessW` is given for a tool, worked out without calling it, so it is
//! tested on every OS: the program found on `PATH`, the command line quoted as the C runtime (and
//! Rust's std) parse it back into arguments, and the environment block's variables.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

/// The program `program` as `CreateProcessW` is given it: a path as it is, else the first
/// `<dir>\<program>` on `path` (the tool's own `PATH`), `.exe` added when it has no extension, as
/// std finds a program. `exists` says whether a file is there. None when nothing is.
pub(super) fn resolve(program: &str, path: Option<&OsStr>, exists: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    let with_exe = |p: PathBuf| if p.extension().is_some() { p } else { p.with_extension("exe") };
    if program.contains(['/', '\\']) || Path::new(program).is_absolute() {
        return Some(with_exe(PathBuf::from(program))).filter(|p| exists(p));
    }
    std::env::split_paths(path?).filter(|dir| !dir.as_os_str().is_empty()).map(|dir| with_exe(dir.join(program))).find(|p| exists(p))
}

/// The command line for `program` and `args`: the program quoted, each argument quoted when it is
/// empty or holds a space or a tab, a `"` escaped with a backslash and the backslashes before it
/// (or before a closing quote) doubled, so the tool reads back exactly `args`.
pub(super) fn command_line(program: &str, args: &[String]) -> String {
    let mut line = String::new();
    append(&mut line, program, true);
    for arg in args {
        line.push(' ');
        append(&mut line, arg, false);
    }
    line
}

/// Appends `arg` to `line`, quoted when `always` or when it needs it.
fn append(line: &mut String, arg: &str, always: bool) {
    let quoted = always || arg.is_empty() || arg.contains([' ', '\t']);
    if quoted {
        line.push('"');
    }
    let mut backslashes = 0;
    for c in arg.chars() {
        match c {
            '\\' => backslashes += 1,
            '"' => {
                // The backslashes before it are doubled, and one more escapes the quote.
                line.extend(std::iter::repeat_n('\\', backslashes + 1));
                backslashes = 0;
            }
            _ => backslashes = 0,
        }
        line.push(c);
    }
    if quoted {
        line.extend(std::iter::repeat_n('\\', backslashes));
        line.push('"');
    }
}

/// The tool's environment: `inherited` (this process's) with `changes` applied in order (a value
/// sets the variable, none removes it), names compared as Windows does (case aside), sorted by
/// name as the block must be.
pub(super) fn environment(inherited: impl IntoIterator<Item = (OsString, OsString)>, changes: &[(OsString, Option<OsString>)]) -> Vec<(OsString, OsString)> {
    let key = |name: &OsStr| name.to_string_lossy().to_uppercase();
    let mut vars: Vec<(String, OsString, OsString)> = inherited.into_iter().map(|(name, value)| (key(&name), name, value)).collect();
    for (name, value) in changes {
        let k = key(name);
        vars.retain(|(other, _, _)| *other != k);
        if let Some(value) = value {
            vars.push((k, name.clone(), value.clone()));
        }
    }
    vars.sort_by(|a, b| a.0.cmp(&b.0));
    vars.into_iter().map(|(_, name, value)| (name, value)).collect()
}

/// The tool's `PATH` in `vars` (an [`environment`]), to find its program on.
pub(super) fn path_of(vars: &[(OsString, OsString)]) -> Option<&OsStr> {
    vars.iter().find(|(name, _)| name.eq_ignore_ascii_case("PATH")).map(|(_, value)| value.as_os_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// How the C runtime splits a command line into arguments (the rules `command_line` writes
    /// for): what a tool reads back.
    fn parsed(line: &str) -> Vec<String> {
        let mut args = Vec::new();
        let mut chars = line.chars().peekable();
        loop {
            while chars.next_if(|c| *c == ' ' || *c == '\t').is_some() {}
            if chars.peek().is_none() {
                return args;
            }
            let (mut arg, mut quoted) = (String::new(), false);
            while let Some(c) = chars.next() {
                match c {
                    '\\' => {
                        let mut n = 1;
                        while chars.next_if_eq(&'\\').is_some() {
                            n += 1;
                        }
                        if chars.peek() == Some(&'"') {
                            arg.extend(std::iter::repeat_n('\\', n / 2));
                            if n % 2 == 1 {
                                arg.push('"');
                                chars.next();
                            }
                        } else {
                            arg.extend(std::iter::repeat_n('\\', n));
                        }
                    }
                    '"' => quoted = !quoted,
                    ' ' | '\t' if !quoted => break,
                    c => arg.push(c),
                }
            }
            args.push(arg);
        }
    }

    #[test]
    fn every_argument_reads_back_as_it_was_given() {
        let args: Vec<String> =
            ["up", "", "two words", "tab\there", r#"say "hi""#, r"C:\Program Files\", r"trailing\\", r#"back\"quote"#, r"a\\\b", "--name=lab one"]
                .map(String::from)
                .to_vec();
        let line = command_line(r"C:\Program Files\Vagrant\bin\vagrant.exe", &args);
        let mut expected = vec![r"C:\Program Files\Vagrant\bin\vagrant.exe".to_owned()];
        expected.extend(args);
        assert_eq!(parsed(&line), expected, "{line}");
        assert_eq!(command_line("docker", &["ps".into(), "-q".into()]), r#""docker" ps -q"#);
    }

    #[test]
    fn a_program_is_found_on_the_tools_path_with_exe_added() {
        let dirs = std::env::join_paths([Path::new("/first"), Path::new(""), Path::new("/second")]).unwrap();
        let there = |p: &Path| p == Path::new("/second/docker.exe") || p == Path::new("/first/tool.cmd") || p == Path::new("/abs/bin.exe");
        assert_eq!(resolve("docker", Some(&dirs), there), Some(PathBuf::from("/second/docker.exe")));
        assert_eq!(resolve("tool.cmd", Some(&dirs), there), Some(PathBuf::from("/first/tool.cmd")));
        assert_eq!(resolve("/abs/bin", Some(&dirs), there), Some(PathBuf::from("/abs/bin.exe")));
        assert_eq!(resolve("vagrant", Some(&dirs), there), None);
        assert_eq!(resolve("docker", None, there), None, "no PATH, no program");
    }

    #[test]
    fn the_environment_is_the_inherited_one_changed_in_order_names_case_aside() {
        let os = |s: &str| OsString::from(s);
        let inherited = [("Path", r"C:\Windows"), ("DOCKER_HOST", "tcp://x"), ("b", "1"), ("A", "2")].map(|(k, v)| (os(k), os(v)));
        let changes = [(os("docker_host"), None), (os("NEW"), Some(os("v"))), (os("new"), Some(os("w"))), (os("PATH"), Some(os(r"C:\tools")))];
        let vars = environment(inherited, &changes);
        let names: Vec<&OsStr> = vars.iter().map(|(n, _)| n.as_os_str()).collect();
        assert_eq!(names, ["A", "b", "new", "PATH"]);
        assert_eq!(vars.iter().find(|(n, _)| n == "new").map(|(_, v)| v.as_os_str()), Some(OsStr::new("w")), "the last change wins");
        assert_eq!(path_of(&vars), Some(OsStr::new(r"C:\tools")));
        assert_eq!(path_of(&[]), None);
    }
}
