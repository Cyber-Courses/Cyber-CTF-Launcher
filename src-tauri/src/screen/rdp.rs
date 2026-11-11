//! A machine's Remote Desktop, opened in the system's own client: a `.rdp` file naming the
//! console's loopback address (and the account, never its password), and the client this
//! computer has: Windows' `mstsc`, on a Mac Microsoft's Windows App (or the Remote Desktop app it
//! replaced), on Linux `FreeRDP`'s `xfreerdp` or Remmina. The password is the learner's to paste
//! (the screen window copies it on request); only `FreeRDP`, which reads it on its standard input
//! when asked to (`/from-stdin`), is given it, and never through a file or a command line.

use std::path::{Path, PathBuf};

/// A Remote Desktop client this computer has.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Client {
    /// Windows' Remote Desktop Connection.
    Mstsc(PathBuf),
    /// A Mac app that opens `.rdp` files (Windows App, Microsoft Remote Desktop), by its bundle.
    MacApp(PathBuf),
    /// `FreeRDP`'s command line (`xfreerdp3` or `xfreerdp`).
    FreeRdp(PathBuf),
    /// Remmina.
    Remmina(PathBuf),
}

impl Client {
    /// Its name, as the screen window shows it.
    pub(crate) fn name(&self) -> String {
        match self {
            Client::Mstsc(_) => "Remote Desktop Connection".to_owned(),
            Client::MacApp(app) => app.file_stem().map_or_else(|| "Windows App".to_owned(), |s| s.to_string_lossy().into_owned()),
            Client::FreeRdp(_) => "FreeRDP".to_owned(),
            Client::Remmina(_) => "Remmina".to_owned(),
        }
    }
}

/// The Mac apps that open a `.rdp` file, newest first.
const MAC_APPS: [&str; 2] = ["Windows App.app", "Microsoft Remote Desktop.app"];

/// The Remote Desktop client of OS `os` (`std::env::consts::OS`), as `exists` and `on_path` find
/// them; `home` is the user's (a Mac app in `~/Applications`).
pub(crate) fn find(os: &str, home: Option<&Path>, exists: impl Fn(&Path) -> bool, on_path: impl Fn(&str) -> Option<PathBuf>) -> Option<Client> {
    match os {
        "windows" => {
            let system = std::env::var_os("SystemRoot").map(|r| PathBuf::from(r).join("System32").join("mstsc.exe"));
            system.filter(|p| exists(p)).or_else(|| on_path("mstsc.exe")).map(Client::Mstsc)
        }
        "macos" => {
            let dirs = std::iter::once(PathBuf::from("/Applications")).chain(home.map(|h| h.join("Applications")));
            let dirs: Vec<PathBuf> = dirs.collect();
            MAC_APPS.iter().flat_map(|app| dirs.iter().map(move |d| d.join(app))).find(|p| exists(p)).map(Client::MacApp)
        }
        _ => on_path("xfreerdp3").or_else(|| on_path("xfreerdp")).map(Client::FreeRdp).or_else(|| on_path("remmina").map(Client::Remmina)),
    }
}

/// What the client is told of the connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Connection {
    /// The console's loopback address (`127.0.0.1:3390`).
    pub(crate) address: String,
    /// The account, when the console signs in to the machine's own OS.
    pub(crate) user: Option<String>,
}

/// The `.rdp` file of `c`: its address, its account, and no password. A console with no account
/// (VirtualBox's remote display) takes none: no credential prompt, no Network Level
/// Authentication, which it doesn't speak. Its certificate is the machine's own, self-signed, on
/// a loopback or tunnelled connection: the client connects without asking about it.
pub(crate) fn file(c: &Connection) -> String {
    let mut lines = vec![format!("full address:s:{}", c.address), "screen mode id:i:1".to_owned(), "authentication level:i:0".to_owned()];
    if let Some(user) = &c.user {
        lines.push(format!("username:s:{}", user.replace(['\r', '\n'], "")));
        lines.push("prompt for credentials:i:1".to_owned());
    } else {
        lines.push("prompt for credentials:i:0".to_owned());
        lines.push("enablecredsspsupport:i:0".to_owned());
    }
    lines.join("\r\n") + "\r\n"
}

/// How `client` opens the connection: its program, its arguments (the `.rdp` file at `rdp`, or
/// `FreeRDP`'s own), and whether it reads the password on its standard input.
pub(crate) fn command(client: &Client, rdp: &Path, c: &Connection, has_password: bool) -> (PathBuf, Vec<String>, bool) {
    let file = rdp.display().to_string();
    match client {
        Client::Mstsc(p) => (p.clone(), vec![file], false),
        Client::MacApp(app) => (PathBuf::from("open"), vec!["-a".into(), app.display().to_string(), file], false),
        Client::Remmina(p) => (p.clone(), vec!["-c".into(), file], false),
        Client::FreeRdp(p) => {
            let mut args = vec![format!("/v:{}", c.address), "/cert:ignore".to_owned(), "/dynamic-resolution".to_owned()];
            let stdin = has_password && c.user.is_some();
            if let Some(user) = &c.user {
                args.push(format!("/u:{user}"));
            }
            if stdin {
                args.push("/from-stdin:force".to_owned());
            }
            (p.clone(), args, stdin)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn none(_: &str) -> Option<PathBuf> {
        None
    }

    #[test]
    fn each_os_finds_its_own_client_or_none() {
        let mac = find("macos", Some(Path::new("/Users/u")), |p| p == Path::new("/Users/u/Applications/Microsoft Remote Desktop.app"), none);
        assert_eq!(mac, Some(Client::MacApp(PathBuf::from("/Users/u/Applications/Microsoft Remote Desktop.app"))));
        let both = find("macos", None, |p| p.starts_with("/Applications"), none);
        assert_eq!(both, Some(Client::MacApp(PathBuf::from("/Applications/Windows App.app"))), "the newer app first");
        assert_eq!(find("macos", None, |_| false, none), None);
        let linux = |names: &'static [&'static str]| move |n: &str| names.contains(&n).then(|| PathBuf::from(format!("/usr/bin/{n}")));
        assert_eq!(find("linux", None, |_| false, linux(&["xfreerdp", "remmina"])), Some(Client::FreeRdp(PathBuf::from("/usr/bin/xfreerdp"))));
        assert_eq!(find("linux", None, |_| false, linux(&["xfreerdp3", "xfreerdp"])), Some(Client::FreeRdp(PathBuf::from("/usr/bin/xfreerdp3"))));
        assert_eq!(find("linux", None, |_| false, linux(&["remmina"])), Some(Client::Remmina(PathBuf::from("/usr/bin/remmina"))));
        assert_eq!(find("linux", None, |_| false, none), None);
        let windows = find("windows", None, |_| false, |n: &str| (n == "mstsc.exe").then(|| PathBuf::from("C:/Windows/System32/mstsc.exe")));
        assert!(matches!(windows, Some(Client::Mstsc(_))));
    }

    #[test]
    fn the_rdp_file_names_the_loopback_and_the_account_but_no_password() {
        let with = file(&Connection { address: "127.0.0.1:24001".into(), user: Some("isoloom".into()) });
        assert_eq!(
            with,
            "full address:s:127.0.0.1:24001\r\nscreen mode id:i:1\r\nauthentication level:i:0\r\nusername:s:isoloom\r\nprompt for credentials:i:1\r\n"
        );
        assert!(!with.contains("password"));
        let vrde = file(&Connection { address: "127.0.0.1:3390".into(), user: None });
        assert!(vrde.contains("prompt for credentials:i:0\r\nenablecredsspsupport:i:0"), "{vrde}");
        let injected = file(&Connection { address: "127.0.0.1:1".into(), user: Some("a\r\nfull address:s:evil:3389".into()) });
        assert_eq!(injected.lines().filter(|l| l.starts_with("full address")).count(), 1, "one line each: {injected}");
    }

    #[test]
    fn each_client_opens_the_file_and_only_freerdp_reads_the_password_on_its_input() {
        let c = Connection { address: "127.0.0.1:24001".into(), user: Some("isoloom".into()) };
        let rdp = Path::new("/data/screen/x.rdp");
        let (p, args, stdin) = command(&Client::MacApp(PathBuf::from("/Applications/Windows App.app")), rdp, &c, true);
        assert_eq!(
            (p, args, stdin),
            (PathBuf::from("open"), vec!["-a".to_owned(), "/Applications/Windows App.app".into(), "/data/screen/x.rdp".into()], false)
        );
        let (_, args, stdin) = command(&Client::FreeRdp(PathBuf::from("/usr/bin/xfreerdp")), rdp, &c, true);
        assert_eq!(args, ["/v:127.0.0.1:24001", "/cert:ignore", "/dynamic-resolution", "/u:isoloom", "/from-stdin:force"]);
        assert!(stdin && !args.iter().any(|a| a.starts_with("/p:")), "never on the command line");
        let (_, args, _) = command(&Client::Mstsc(PathBuf::from("mstsc.exe")), rdp, &c, true);
        assert_eq!(args, ["/data/screen/x.rdp"]);
    }
}
