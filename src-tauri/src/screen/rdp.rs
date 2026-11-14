//! A machine's Remote Desktop, opened in the system's own client: a `.rdp` file naming the
//! console's loopback address (and the account, never its password), and the client this
//! computer has: Windows' `mstsc`, on a Mac Microsoft's Windows App (or the Remote Desktop app it
//! replaced), on Linux `FreeRDP`'s `xfreerdp` or Remmina. The password is the learner's to paste
//! (the screen window copies it on request); only `FreeRDP`, which reads it on its standard input
//! when asked to (`/from-stdin`), is given it, and never through a file or a command line.

use std::path::{Path, PathBuf};

use crate::platform::host::Os;

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

/// The Remote Desktop client of OS `os`, as `exists` and `on_path` find
/// them; `home` is the user's (a Mac app in `~/Applications`).
pub(crate) fn find(os: Os, home: Option<&Path>, exists: impl Fn(&Path) -> bool, on_path: impl Fn(&str) -> Option<PathBuf>) -> Option<Client> {
    match os {
        Os::Windows => {
            let system = std::env::var_os("SystemRoot").map(|r| PathBuf::from(r).join("System32").join("mstsc.exe"));
            system.filter(|p| exists(p)).or_else(|| on_path("mstsc.exe")).map(Client::Mstsc)
        }
        Os::Macos => {
            let dirs = std::iter::once(PathBuf::from("/Applications")).chain(home.map(|h| h.join("Applications")));
            let dirs: Vec<PathBuf> = dirs.collect();
            MAC_APPS.iter().flat_map(|app| dirs.iter().map(move |d| d.join(app))).find(|p| exists(p)).map(Client::MacApp)
        }
        Os::Linux => on_path("xfreerdp3").or_else(|| on_path("xfreerdp")).map(Client::FreeRdp).or_else(|| on_path("remmina").map(Client::Remmina)),
    }
}

/// What the client is told of the connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Connection {
    /// The console's loopback address (`127.0.0.1:3390`).
    pub(crate) address: String,
    /// The account, when the console signs in to the machine's own OS.
    pub(crate) user: Option<String>,
    /// Whether the learner shares this computer's clipboard with the machine, for this session
    /// only: off unless asked, as a lab machine is not trusted with what is copied here.
    pub(crate) clipboard: bool,
}

/// What a `.rdp` file says the client keeps on this computer: printers, ports, smart cards,
/// security keys, drives, other devices, the microphone and cameras are never handed to a lab
/// machine. The clipboard is the session's choice ([`Connection::clipboard`]).
const NOT_REDIRECTED: [&str; 8] = [
    "redirectprinters:i:0",
    "redirectcomports:i:0",
    "redirectsmartcards:i:0",
    "redirectwebauthn:i:0",
    "drivestoredirect:s:",
    "devicestoredirect:s:",
    "camerastoredirect:s:",
    "audiocapturemode:i:0",
];

/// The `.rdp` file of `c`: its address, its account, no password, and no device of this computer
/// redirected but the clipboard when the session shares it. A console with no account
/// (VirtualBox's remote display) takes none: no credential prompt, no Network Level
/// Authentication, which it doesn't speak. Its certificate is the machine's own, self-signed, on
/// a loopback or tunnelled connection: the client connects without asking about it.
pub(crate) fn file(c: &Connection) -> String {
    let mut lines = vec![format!("full address:s:{}", c.address), "screen mode id:i:1".to_owned(), "authentication level:i:0".to_owned()];
    lines.push(format!("redirectclipboard:i:{}", u8::from(c.clipboard)));
    lines.extend(NOT_REDIRECTED.iter().map(|l| (*l).to_owned()));
    if let Some(user) = &c.user {
        lines.push(format!("username:s:{}", user.replace(['\r', '\n'], "")));
        lines.push("prompt for credentials:i:1".to_owned());
    } else {
        lines.push("prompt for credentials:i:0".to_owned());
        lines.push("enablecredsspsupport:i:0".to_owned());
    }
    lines.join("\r\n") + "\r\n"
}

/// How a client is started for one connection.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Launch {
    /// The program run.
    pub(crate) program: PathBuf,
    /// Its arguments: the `.rdp` file, or `FreeRDP`'s own.
    pub(crate) args: Vec<String>,
    /// Whether it reads the password on its standard input (never on its command line).
    pub(crate) password_on_stdin: bool,
}

/// How `client` opens the connection: its program, its arguments (the `.rdp` file at `rdp`, or
/// `FreeRDP`'s own), and whether it reads the password on its standard input.
pub(crate) fn command(client: &Client, rdp: &Path, c: &Connection, has_password: bool) -> Launch {
    let file = rdp.display().to_string();
    let file_only = |program: PathBuf, args: Vec<String>| Launch { program, args, password_on_stdin: false };
    match client {
        Client::Mstsc(p) => file_only(p.clone(), vec![file]),
        Client::MacApp(app) => file_only(PathBuf::from("open"), vec!["-a".into(), app.display().to_string(), file]),
        Client::Remmina(p) => file_only(p.clone(), vec!["-c".into(), file]),
        Client::FreeRdp(p) => {
            let mut args = vec![format!("/v:{}", c.address), "/cert:ignore".to_owned(), "/dynamic-resolution".to_owned()];
            // No drive, home folder or printer of this computer; the clipboard as the session chose.
            args.extend([if c.clipboard { "+clipboard" } else { "-clipboard" }, "-drives", "-home-drive"].map(str::to_owned));
            let stdin = has_password && c.user.is_some();
            if let Some(user) = &c.user {
                args.push(format!("/u:{user}"));
            }
            if stdin {
                args.push("/from-stdin:force".to_owned());
            }
            Launch { program: p.clone(), args, password_on_stdin: stdin }
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
        let mac = find(Os::Macos, Some(Path::new("/Users/u")), |p| p == Path::new("/Users/u/Applications/Microsoft Remote Desktop.app"), none);
        assert_eq!(mac, Some(Client::MacApp(PathBuf::from("/Users/u/Applications/Microsoft Remote Desktop.app"))));
        let both = find(Os::Macos, None, |p| p.starts_with("/Applications"), none);
        assert_eq!(both, Some(Client::MacApp(PathBuf::from("/Applications/Windows App.app"))), "the newer app first");
        assert_eq!(find(Os::Macos, None, |_| false, none), None);
        let linux = |names: &'static [&'static str]| move |n: &str| names.contains(&n).then(|| PathBuf::from(format!("/usr/bin/{n}")));
        assert_eq!(find(Os::Linux, None, |_| false, linux(&["xfreerdp", "remmina"])), Some(Client::FreeRdp(PathBuf::from("/usr/bin/xfreerdp"))));
        assert_eq!(find(Os::Linux, None, |_| false, linux(&["xfreerdp3", "xfreerdp"])), Some(Client::FreeRdp(PathBuf::from("/usr/bin/xfreerdp3"))));
        assert_eq!(find(Os::Linux, None, |_| false, linux(&["remmina"])), Some(Client::Remmina(PathBuf::from("/usr/bin/remmina"))));
        assert_eq!(find(Os::Linux, None, |_| false, none), None);
        let windows = find(Os::Windows, None, |_| false, |n: &str| (n == "mstsc.exe").then(|| PathBuf::from("C:/Windows/System32/mstsc.exe")));
        assert!(matches!(windows, Some(Client::Mstsc(_))));
    }

    #[test]
    fn the_rdp_file_names_the_loopback_and_the_account_but_no_password() {
        let with = file(&Connection { address: "127.0.0.1:24001".into(), user: Some("isoloom".into()), clipboard: false });
        assert!(with.starts_with("full address:s:127.0.0.1:24001\r\nscreen mode id:i:1\r\nauthentication level:i:0\r\n"), "{with}");
        assert!(with.ends_with("\r\nusername:s:isoloom\r\nprompt for credentials:i:1\r\n"), "{with}");
        assert!(!with.contains("password"));
        let vrde = file(&Connection { address: "127.0.0.1:3390".into(), user: None, clipboard: false });
        assert!(vrde.contains("prompt for credentials:i:0\r\nenablecredsspsupport:i:0"), "{vrde}");
        let injected = file(&Connection { address: "127.0.0.1:1".into(), user: Some("a\r\nfull address:s:evil:3389".into()), clipboard: false });
        assert_eq!(injected.lines().filter(|l| l.starts_with("full address")).count(), 1, "one line each: {injected}");
    }

    #[test]
    fn a_lab_machine_gets_no_device_of_this_computer_and_the_clipboard_only_when_shared() {
        let kept = file(&Connection { address: "127.0.0.1:24001".into(), user: None, clipboard: false });
        for line in [
            "redirectclipboard:i:0",
            "redirectprinters:i:0",
            "redirectcomports:i:0",
            "redirectsmartcards:i:0",
            "redirectwebauthn:i:0",
            "drivestoredirect:s:",
            "devicestoredirect:s:",
            "camerastoredirect:s:",
            "audiocapturemode:i:0",
        ] {
            assert!(kept.lines().any(|l| l == line), "{line} in {kept}");
        }
        let shared = file(&Connection { address: "127.0.0.1:24001".into(), user: None, clipboard: true });
        assert!(shared.lines().any(|l| l == "redirectclipboard:i:1") && shared.lines().any(|l| l == "redirectprinters:i:0"), "{shared}");
        let freerdp = Client::FreeRdp(PathBuf::from("/usr/bin/xfreerdp"));
        let rdp = Path::new("/x.rdp");
        let kept = command(&freerdp, rdp, &Connection { address: "127.0.0.1:1".into(), user: None, clipboard: false }, false).args;
        assert!(kept.iter().any(|a| a == "-clipboard") && kept.iter().any(|a| a == "-drives") && kept.iter().any(|a| a == "-home-drive"), "{kept:?}");
        let shared = command(&freerdp, rdp, &Connection { address: "127.0.0.1:1".into(), user: None, clipboard: true }, false).args;
        assert!(shared.iter().any(|a| a == "+clipboard") && !shared.iter().any(|a| a == "-clipboard"), "{shared:?}");
    }

    #[test]
    fn each_client_opens_the_file_and_only_freerdp_reads_the_password_on_its_input() {
        let c = Connection { address: "127.0.0.1:24001".into(), user: Some("isoloom".into()), clipboard: false };
        let rdp = Path::new("/data/screen/x.rdp");
        assert_eq!(
            command(&Client::MacApp(PathBuf::from("/Applications/Windows App.app")), rdp, &c, true),
            Launch {
                program: PathBuf::from("open"),
                args: vec!["-a".to_owned(), "/Applications/Windows App.app".into(), "/data/screen/x.rdp".into()],
                password_on_stdin: false,
            }
        );
        let free = command(&Client::FreeRdp(PathBuf::from("/usr/bin/xfreerdp")), rdp, &c, true);
        assert_eq!(
            free.args,
            ["/v:127.0.0.1:24001", "/cert:ignore", "/dynamic-resolution", "-clipboard", "-drives", "-home-drive", "/u:isoloom", "/from-stdin:force"]
        );
        assert!(free.password_on_stdin && !free.args.iter().any(|a| a.starts_with("/p:")), "never on the command line");
        assert_eq!(command(&Client::Mstsc(PathBuf::from("mstsc.exe")), rdp, &c, true).args, ["/data/screen/x.rdp"]);
    }
}
