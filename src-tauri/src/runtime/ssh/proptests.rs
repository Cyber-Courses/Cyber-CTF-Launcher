//! Property tests: whatever the paths, the command lines built here split back into exactly
//! the words meant, through every layer that parses them (the shell, ssh's `%` expansion, ssh's
//! option-value splitting). Nothing a path or host holds can add a word or escape its quotes.

use std::path::Path;

use proptest::prelude::*;

use super::*;

/// OpenSSH's `argv_split` (misc.c), how it splits an `-o Key=value` option value into words.
fn openssh_argv_split(s: &str) -> Vec<String> {
    let c: Vec<char> = s.chars().collect();
    let mut words = Vec::new();
    let mut i = 0;
    while i < c.len() {
        if c[i] == ' ' || c[i] == '\t' {
            i += 1;
            continue;
        }
        let mut word = String::new();
        let mut quote: Option<char> = None;
        while i < c.len() {
            let ch = c[i];
            if ch == '\\' {
                match c.get(i + 1) {
                    Some(&n) if n == '\'' || n == '"' || n == '\\' || (quote.is_none() && n == ' ') => {
                        word.push(n);
                        i += 1;
                    }
                    _ => word.push(ch),
                }
            } else if quote.is_none() && (ch == ' ' || ch == '\t') {
                break;
            } else if quote.is_none() && (ch == '"' || ch == '\'') {
                quote = Some(ch);
            } else if quote == Some(ch) {
                quote = None;
            } else {
                word.push(ch);
            }
            i += 1;
        }
        words.push(word);
    }
    words
}

/// ssh's `%` expansion: `%%` is a `%`, `%h`/`%p` the jump target. Any other token is a bug.
fn percent_expand(s: &str, host: &str, port: &str) -> Option<String> {
    let mut out = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        match chars.next()? {
            '%' => out.push('%'),
            'h' => out.push_str(host),
            'p' => out.push_str(port),
            _ => return None,
        }
    }
    Some(out)
}

/// The path ssh ends up with from a `UserKnownHostsFile=` option value.
fn known_hosts_as_ssh_reads_it(option: &str) -> String {
    let value = option.strip_prefix("UserKnownHostsFile=").expect("the known_hosts option");
    let words = openssh_argv_split(value);
    assert_eq!(words.len(), 1, "one file, not {words:?}");
    percent_expand(&words[0], "", "").expect("only %% in a path")
}

/// Checks `-o` words as ssh reads them: the fixed options, then the known_hosts file.
fn check_options(opts: &[String], batch: bool, known_hosts: &str) {
    let fixed: &[&str] = if batch { &["BatchMode=yes", "ConnectTimeout=10"] } else { &[] };
    assert_eq!(&opts[..fixed.len()], fixed);
    let rest = &opts[fixed.len()..];
    assert_eq!(rest.len(), 3, "{rest:?}");
    assert_eq!(rest[0], "StrictHostKeyChecking=accept-new");
    assert_eq!(known_hosts_as_ssh_reads_it(&rest[1]), known_hosts);
    assert_eq!(rest[2], "LogLevel=ERROR");
}

/// Splits `words` into its `-o` values (in order) and the rest.
fn take_options(words: &[String]) -> (Vec<String>, Vec<String>) {
    let (mut opts, mut rest) = (Vec::new(), Vec::new());
    let mut it = words.iter();
    while let Some(w) = it.next() {
        if w == "-o" {
            opts.push(it.next().expect("a value after -o").clone());
        } else {
            rest.push(w.clone());
        }
    }
    (opts, rest)
}

/// The ProxyCommand as ssh runs it: `%` tokens expanded, then split by the shell.
fn proxy_words(proxy: &str) -> Vec<String> {
    let cmd = proxy.strip_prefix("ProxyCommand=").expect("a ProxyCommand option");
    let expanded = percent_expand(cmd, "TARGET", "2222").expect("no stray % token in the ProxyCommand");
    shell_words::split(&expanded).expect("the ProxyCommand is valid shell")
}

fn check_proxy(proxy: &str, identity: &str, known_hosts: &str, jump: &str) {
    let words = proxy_words(proxy);
    let (opts, rest) = take_options(&words);
    assert_eq!(rest, ["ssh", "-i", identity, "-W", "TARGET:2222", jump]);
    check_options(&opts, true, known_hosts);
}

/// Any path a user's home or app data folder could have: spaces, quotes, `%`, `$`, backslashes,
/// newlines, non-ASCII. Never NUL (no OS path holds it) or a tab (see `ssh_config_path`).
fn any_path() -> impl Strategy<Value = String> {
    prop_oneof![
        "/[^\\x00\\t]{0,40}",
        "/Users/[a-z' %\"$`\\\\]{1,12}/Library/Application Support/org\\.cyberctf\\.desktop/ssh/[a-z_%]{1,12}",
        r"C:\\Users\\[A-Za-z '%]{1,12}\\AppData\\Roaming\\org\.cyberctf\.desktop\\ssh\\known_hosts",
    ]
}

/// What `safe_token` accepts.
fn token() -> impl Strategy<Value = String> {
    "[A-Za-z0-9_.:][A-Za-z0-9_.:-]{0,30}"
}

proptest! {
    #![proptest_config(crate::proptest_support::config())]

    #[test]
    fn sh_quote_splits_back_to_the_input(s in "[^\\x00]{0,64}") {
        prop_assert_eq!(shell_words::split(&sh_quote(&s)).unwrap(), vec![s.clone()]);
        // Two quoted words next to each other stay two words.
        let two = format!("{} {}", sh_quote(&s), sh_quote("x"));
        prop_assert_eq!(shell_words::split(&two).unwrap(), vec![s, "x".to_string()]);
    }

    #[test]
    fn known_hosts_reaches_ssh_as_one_verbatim_path(known_hosts in any_path(), batch: bool) {
        check_options(&options(batch, &known_hosts), batch, &known_hosts);
        // In a shell command line too: the shell, then ssh.
        let words = shell_words::split(&shell_options(batch, Path::new(&known_hosts))).unwrap();
        let (opts, rest) = take_options(&words);
        prop_assert!(rest.is_empty(), "{:?}", rest);
        check_options(&opts, batch, &known_hosts);
    }

    #[test]
    fn attack_shell_command_splits_into_exactly_its_words(
        identity in any_path(),
        known_hosts in any_path(),
        host in token(),
        user in token(),
        port: u16,
        jump in proptest::option::of((token(), token())),
    ) {
        let jump = jump.map(|(u, h)| format!("{u}@{h}"));
        let t = Target { host: host.clone(), port, user: user.clone(), identity: identity.clone().into(), jump: jump.clone() };
        let cmd = t.attack_shell_command(Path::new(&known_hosts)).unwrap();
        let words = shell_words::split(&cmd).unwrap();
        let (opts, rest) = take_options(&words);
        let login = format!("{user}@{host}");
        let port = port.to_string();
        prop_assert_eq!(rest, ["ssh", "-t", "-i", identity.as_str(), "-p", port.as_str(), login.as_str(), "sudo", "docker", "exec", "-it", "attacker", "bash"]);
        check_options(&opts[..3], false, &known_hosts);
        match &jump {
            Some(jump) => {
                prop_assert_eq!(opts.len(), 4);
                check_proxy(&opts[3], &identity, &known_hosts, jump);
            }
            None => prop_assert_eq!(opts.len(), 3),
        }
    }

    #[test]
    fn exec_args_hold_the_paths_verbatim(
        identity in any_path(),
        known_hosts in any_path(),
        host in token(),
        user in token(),
        jump in proptest::option::of((token(), token())),
        command in "[^\\x00]{0,40}",
    ) {
        let jump = jump.map(|(u, h)| format!("{u}@{h}"));
        let t = Target { host: host.clone(), port: 22, user: user.clone(), identity: identity.clone().into(), jump: jump.clone() };
        let args = t.exec_args(Path::new(&known_hosts), &command).unwrap();
        let (opts, rest) = take_options(&args);
        let login = format!("{user}@{host}");
        prop_assert_eq!(rest, ["-i", identity.as_str(), "-p", "22", login.as_str(), command.as_str()]);
        check_options(&opts[..5], true, &known_hosts);
        if let Some(jump) = &jump {
            check_proxy(&opts[5], &identity, &known_hosts, jump);
        }
    }

    #[test]
    fn odd_hosts_users_and_jumps_are_refused(bad in "[^A-Za-z0-9_.:-]", at in 0usize..8) {
        let mut host = "10.0.0.5".to_string();
        host.insert_str(at.min(host.len()), &bad);
        let t = Target::direct(host.clone(), "debian", "/k".into());
        prop_assert!(t.attack_shell_command(Path::new("/kh")).is_err());
        let t = Target::direct("10.0.0.5", host.clone(), "/k".into());
        prop_assert!(t.exec_args(Path::new("/kh"), "true").is_err());
        let t = Target { jump: Some(format!("root@{host}")), ..Target::direct("10.0.0.5", "debian", "/k".into()) };
        prop_assert!(t.attack_shell_command(Path::new("/kh")).is_err());
    }

    #[test]
    fn a_leading_dash_never_reaches_ssh_as_a_flag(rest in "[A-Za-z0-9]{0,10}") {
        let t = Target::direct(format!("-{rest}"), "debian", "/k".into());
        prop_assert!(t.attack_shell_command(Path::new("/kh")).is_err());
        let t = Target::direct("h", format!("-{rest}"), "/k".into());
        prop_assert!(t.attack_shell_command(Path::new("/kh")).is_err());
    }

    #[test]
    fn parse_ssh_config_never_panics(out in "(?s).{0,400}") {
        let _ = parse_ssh_config(&out);
    }

    #[test]
    fn parse_ssh_config_reads_back_what_vagrant_prints(
        host in "[A-Za-z0-9.-]{1,30}",
        user in "[A-Za-z0-9_-]{1,16}",
        port: u16,
        identity in "/[^\\x00\\n\\r\"]{0,40}",
        quote_identity: bool,
    ) {
        let identity = identity.trim().to_string();
        let shown = if quote_identity { format!("\"{identity}\"") } else { identity.clone() };
        let out = format!("Host default\n  HostName {host}\n  User {user}\n  Port {port}\n  UserKnownHostsFile /dev/null\n  IdentityFile {shown}\n  IdentityFile /second\n");
        let t = parse_ssh_config(&out).unwrap();
        prop_assert_eq!(t.host, host);
        prop_assert_eq!(t.user, user);
        prop_assert_eq!(t.port, port);
        prop_assert_eq!(t.identity, std::path::PathBuf::from(identity));
        prop_assert_eq!(t.jump, None);
    }
}
