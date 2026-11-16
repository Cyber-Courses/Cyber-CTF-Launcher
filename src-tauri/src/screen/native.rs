//! A lab VM's screen in its hypervisor's own app, beside the built-in viewer ("Open in
//! VirtualBox"): the runner says which app the lab's provider has and what of it a computer needs
//! ([`isothrum_runner::NativeApp`]), and what opens a running VM in it
//! ([`isothrum_runner::NativeConsole`]: a program and its arguments, a macOS app and what it
//! opens, or an `https` URL). The launcher offers the app only where this computer has it, and
//! opens the VM the first way this computer can: never through a shell, each argument as the
//! runner gave it.

use std::path::{Path, PathBuf};
use std::time::Duration;

use isothrum_runner::{NativeApp, NativeNeed, NativeOpen};

use crate::error::{Error, Result};
use crate::platform::host::Os;

/// How this computer opens a VM in its app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Launch {
    /// Run `program` with `args`.
    Program {
        /// The program, where this computer has it.
        program: PathBuf,
        /// Its arguments, each as it is.
        args: Vec<String>,
    },
    /// Open `url` (`https`) in the default browser.
    Url(String),
}

/// Where a macOS app may be: `app` as the runner names it (`/Applications/X.app`), or the same
/// app in the user's own `~/Applications`.
fn mac_app_places(app: &Path, home: Option<&Path>) -> Vec<PathBuf> {
    let mut places = vec![app.to_path_buf()];
    if let (Some(home), Some(name)) = (home, app.file_name()) {
        places.push(home.join("Applications").join(name));
    }
    places
}

/// Where this computer has what `need` names: a program (on `PATH`, or by its full path), a macOS
/// app (`os` is a Mac's), the browser (always). `None` when it hasn't.
fn found(need: &NativeNeed, os: Os, home: Option<&Path>, exists: &impl Fn(&Path) -> bool, on_path: &impl Fn(&str) -> Option<PathBuf>) -> Option<PathBuf> {
    match need {
        NativeNeed::Program(program) => {
            let path = Path::new(program);
            if path.is_absolute() { exists(path).then(|| path.to_path_buf()) } else { on_path(program) }
        }
        NativeNeed::MacApp(app) if os == Os::Macos => mac_app_places(app, home).into_iter().find(|p| exists(p)),
        NativeNeed::Browser => Some(PathBuf::new()),
        _ => None,
    }
}

/// The app's name, when this computer has what one of its ways needs: what the lab page's
/// "Open in …" button says; `None` hides the button.
pub(crate) fn installed(
    app: Option<&NativeApp>,
    os: Os,
    home: Option<&Path>,
    exists: impl Fn(&Path) -> bool,
    on_path: impl Fn(&str) -> Option<PathBuf>,
) -> Option<String> {
    let app = app?;
    app.needs.iter().any(|n| found(n, os, home, &exists, &on_path).is_some()).then(|| app.name.clone())
}

/// [`installed`] on this computer: its OS, the user's home, its files and its `PATH`.
pub(crate) fn installed_here(app: Option<&NativeApp>) -> Option<String> {
    installed(app, Os::HERE, crate::platform::home_dir().as_deref(), Path::exists, on_path)
}

/// `name` where this computer's `PATH` has it (on Windows, `name.exe` too).
pub(crate) fn on_path(name: &str) -> Option<PathBuf> {
    let names: Vec<String> =
        if cfg!(windows) && Path::new(name).extension().is_none() { vec![name.to_owned(), format!("{name}.exe")] } else { vec![name.to_owned()] };
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).flat_map(|d| names.iter().map(move |n| d.join(n))).find(|p| p.is_file())
}

/// The first of `ways` (a [`isothrum_runner::NativeConsole`]'s, best first) this computer can
/// take, as it takes it.
pub(crate) fn pick(
    ways: &[NativeOpen],
    os: Os,
    home: Option<&Path>,
    exists: impl Fn(&Path) -> bool,
    on_path: impl Fn(&str) -> Option<PathBuf>,
) -> Option<Launch> {
    ways.iter().find_map(|way| {
        let at = found(&way.need(), os, home, &exists, &on_path)?;
        match way {
            NativeOpen::Program { args, .. } => Some(Launch::Program { program: at, args: args.clone() }),
            NativeOpen::MacApp { args, .. } => {
                let mut open = vec!["-a".to_owned(), at.to_string_lossy().into_owned()];
                open.extend(args.iter().cloned());
                Some(Launch::Program { program: PathBuf::from("/usr/bin/open"), args: open })
            }
            NativeOpen::Url { url } => url.starts_with("https://").then(|| Launch::Url(url.clone())),
            _ => None,
        }
    })
}

/// How long an app's program gets to fail before it is taken as opened: `VBoxManage startvm`
/// says at once that a VM can't take a window; an app that stays open (a viewer) is left running.
const FAILS_WITHIN: Duration = Duration::from_secs(5);

/// Runs `program` with `args` (never through a shell): fails with what it said when it exits
/// unsuccessfully within [`FAILS_WITHIN`], else leaves it running (its exit reaped in the
/// background).
pub(crate) async fn run(program: &Path, args: &[String]) -> Result<()> {
    #[cfg(test)]
    if let Some(scripted) = crate::platform::exec::fake::answer_step(&program.to_string_lossy(), args, &mut |_| {}) {
        return scripted;
    }
    run_within(program, args, FAILS_WITHIN).await
}

/// [`run`], the program given `within` to fail.
async fn run_within(program: &Path, args: &[String], within: Duration) -> Result<()> {
    use tokio::io::AsyncReadExt as _;
    let name = program.file_name().map_or_else(|| program.display().to_string(), |n| n.to_string_lossy().into_owned());
    #[allow(clippy::disallowed_methods, reason = "the hypervisor's own console client, waited for a moment then left to the player")]
    let mut cmd = tokio::process::Command::new(program);
    cmd.args(args).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| Error::Invalid(format!("couldn't open {name}: {e}")))?;
    // Read as it comes, for as long as the program runs: a pipe nobody reads fills up and blocks
    // it, and one whose reading end is closed kills it (SIGPIPE) at its next warning.
    let mut stderr = child.stderr.take();
    let said = tokio::spawn(async move {
        let mut bytes = Vec::new();
        if let Some(err) = stderr.as_mut() {
            let _ = err.read_to_end(&mut bytes).await;
        }
        String::from_utf8_lossy(&bytes).into_owned()
    });
    match tokio::time::timeout(within, child.wait()).await {
        Ok(Ok(status)) if !status.success() => {
            let said = said.await.unwrap_or_default();
            let said = said.lines().map(str::trim).rfind(|l| !l.is_empty()).unwrap_or("it failed").to_owned();
            Err(Error::Invalid(format!("{name} couldn't open the VM: {said}")))
        }
        Ok(Ok(_)) => Ok(()),
        Ok(Err(e)) => Err(Error::Invalid(format!("couldn't open {name}: {e}"))),
        Err(_) => {
            // Still open (a viewer's window): reaped once it ends, its error output read until then.
            tokio::spawn(async move {
                let _ = child.wait().await;
                let _ = said.await;
            });
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn program(p: &str, args: &[&str]) -> NativeOpen {
        NativeOpen::Program { program: p.into(), args: args.iter().map(|a| (*a).to_owned()).collect() }
    }

    fn app(name: &str, needs: &[NativeNeed]) -> NativeApp {
        serde_json::from_value(serde_json::json!({ "name": name, "needs": needs })).unwrap()
    }

    /// `PATH` with these programs on it, in `/bin`.
    fn path_with(programs: &'static [&'static str]) -> impl Fn(&str) -> Option<PathBuf> {
        move |name| programs.contains(&name).then(|| PathBuf::from("/bin").join(name))
    }

    fn exists_of(paths: &'static [&'static str]) -> impl Fn(&Path) -> bool {
        move |p| paths.iter().any(|q| Path::new(q) == p)
    }

    #[test]
    fn the_app_is_offered_only_where_this_computer_has_it() {
        let vbox = app("VirtualBox", &[NativeNeed::Program("VBoxManage".into())]);
        assert_eq!(installed(Some(&vbox), Os::Linux, None, exists_of(&[]), path_with(&["VBoxManage"])), Some("VirtualBox".into()));
        assert_eq!(installed(Some(&vbox), Os::Linux, None, exists_of(&[]), path_with(&[])), None, "not installed");
        let fusion = app("VMware Fusion", &[NativeNeed::MacApp("/Applications/VMware Fusion.app".into())]);
        let home = Path::new("/Users/u");
        assert_eq!(
            installed(Some(&fusion), Os::Macos, Some(home), exists_of(&["/Users/u/Applications/VMware Fusion.app"]), path_with(&[])),
            Some("VMware Fusion".into()),
            "in the user's own Applications"
        );
        assert_eq!(installed(Some(&fusion), Os::Macos, Some(home), exists_of(&[]), path_with(&[])), None);
        assert_eq!(installed(Some(&fusion), Os::Linux, None, exists_of(&["/Applications/VMware Fusion.app"]), path_with(&[])), None, "a Mac app off a Mac");
        let workstation = app("VMware Workstation", &[NativeNeed::Program(r"C:\Program Files (x86)\VMware\VMware Workstation\vmware.exe".into())]);
        assert_eq!(installed(Some(&workstation), Os::Windows, None, exists_of(&[]), path_with(&[])), None);
        let esxi = app("ESXi", &[NativeNeed::Browser]);
        assert_eq!(installed(Some(&esxi), Os::Linux, None, exists_of(&[]), path_with(&[])), Some("ESXi".into()), "a web console needs only the browser");
        assert_eq!(installed(None, Os::Macos, None, exists_of(&[]), path_with(&[])), None, "vagrant-qemu has no app");
    }

    #[test]
    fn the_first_way_this_computer_has_is_taken() {
        let libvirt = [program("virt-viewer", &["--connect", "qemu:///system", "lab_web"]), program("remote-viewer", &["vnc://127.0.0.1:2"])];
        assert_eq!(
            pick(&libvirt, Os::Linux, None, exists_of(&[]), path_with(&["remote-viewer"])),
            Some(Launch::Program { program: "/bin/remote-viewer".into(), args: vec!["vnc://127.0.0.1:2".into()] }),
            "virt-viewer isn't installed"
        );
        assert_eq!(
            pick(&libvirt, Os::Linux, None, exists_of(&[]), path_with(&["virt-viewer", "remote-viewer"])),
            Some(Launch::Program { program: "/bin/virt-viewer".into(), args: vec!["--connect".into(), "qemu:///system".into(), "lab_web".into()] })
        );
        assert_eq!(pick(&libvirt, Os::Linux, None, exists_of(&[]), path_with(&[])), None);
        let parallels = [NativeOpen::MacApp { app: "/Applications/Parallels Desktop.app".into(), args: vec!["/Users/u/Parallels/web.pvm".into()] }];
        assert_eq!(
            pick(&parallels, Os::Macos, None, exists_of(&["/Applications/Parallels Desktop.app"]), path_with(&[])),
            Some(Launch::Program {
                program: "/usr/bin/open".into(),
                args: vec!["-a".into(), "/Applications/Parallels Desktop.app".into(), "/Users/u/Parallels/web.pvm".into()]
            })
        );
        let proxmox = [NativeOpen::Url { url: "https://pve:8006/?console=kvm&novnc=1&vmid=104&node=pve".into() }];
        assert_eq!(
            pick(&proxmox, Os::Linux, None, exists_of(&[]), path_with(&[])),
            Some(Launch::Url("https://pve:8006/?console=kvm&novnc=1&vmid=104&node=pve".into()))
        );
        let plain = [NativeOpen::Url { url: "http://esxi/ui".into() }];
        assert_eq!(pick(&plain, Os::Linux, None, exists_of(&[]), path_with(&[])), None, "only https is opened");
    }

    #[tokio::test]
    async fn a_program_that_fails_at_once_says_why_and_one_that_stays_open_is_left_running() {
        let tools = crate::platform::exec::fake::Tools::strict_rules(&[(
            "startvm",
            crate::platform::exec::fake::fail("VBoxManage: error: The machine is already locked for a session"),
        )]);
        let err = run(Path::new("/bin/VBoxManage"), &["startvm".into(), "web".into(), "--type".into(), "separate".into()]).await.unwrap_err();
        assert!(err.to_string().contains("VBoxManage"), "{err}");
        assert_eq!(tools.calls(), ["/bin/VBoxManage startvm web --type separate"]);
        drop(tools);
        let _ok = crate::platform::exec::fake::Tools::strict_rules(&[("virt-viewer lab_web", crate::platform::exec::fake::ok(""))]);
        run(Path::new("/bin/virt-viewer"), &["lab_web".into()]).await.unwrap();
    }
    /// Regression: a viewer left running had its error output's pipe closed under it, so the
    /// first warning it printed after the launcher stopped waiting killed it (SIGPIPE): the VM's
    /// window closed by itself.
    ///
    /// Told by the test when to warn (`go`, once the launcher stopped waiting), the viewer either
    /// says it lived (`alive`) or is gone without saying so: no clock decides, where a 3 s wait
    /// for a viewer that slept 0.5 s ran out on a loaded machine.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_viewer_left_running_survives_printing_warnings() {
        let dir = crate::testing::temp_dir("native-viewer");
        let (pid, go, alive) = (dir.join("pid"), dir.join("go"), dir.join("alive"));
        let script = format!(
            "echo $$ > '{}'; while [ ! -e '{}' ]; do sleep 0.05; done; echo 'Gtk-WARNING: a late warning' >&2; echo yes > '{}'",
            pid.display(),
            go.display(),
            alive.display()
        );
        run_within(Path::new("/bin/sh"), &["-c".into(), script], Duration::from_millis(100)).await.unwrap();
        // A bound only on a test that would otherwise hang, never reached by one that passes or fails.
        let hung = tokio::time::Instant::now() + Duration::from_secs(120);
        let pid = loop {
            if let Some(pid) = std::fs::read_to_string(&pid).ok().and_then(|s| s.trim().parse::<u32>().ok()) {
                break pid;
            }
            assert!(tokio::time::Instant::now() < hung, "the viewer never started");
            tokio::time::sleep(Duration::from_millis(20)).await;
        };
        std::fs::write(&go, "").unwrap();
        loop {
            // Gone, then not alive: a viewer that ended after saying so lived.
            let gone = !running(pid);
            if std::fs::read_to_string(&alive).is_ok_and(|s| s.trim() == "yes") {
                break;
            }
            assert!(!gone, "the viewer was killed by its first warning");
            assert!(tokio::time::Instant::now() < hung, "the viewer neither warned nor ended");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    /// Whether process `pid` still runs (a zombie, not yet reaped, has ended).
    #[cfg(unix)]
    fn running(pid: u32) -> bool {
        use sysinfo::{Pid, ProcessRefreshKind, ProcessStatus, ProcessesToUpdate, System};
        let mut sys = System::new();
        sys.refresh_processes_specifics(ProcessesToUpdate::Some(&[Pid::from_u32(pid)]), true, ProcessRefreshKind::nothing());
        sys.process(Pid::from_u32(pid)).is_some_and(|p| !matches!(p.status(), ProcessStatus::Zombie | ProcessStatus::Dead))
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_program_that_fails_in_time_says_its_last_line() {
        let err =
            run_within(Path::new("/bin/sh"), &["-c".into(), "echo first >&2; echo 'no such VM' >&2; exit 1".into()], Duration::from_secs(5)).await.unwrap_err();
        assert_eq!(err.to_string(), "sh couldn't open the VM: no such VM");
    }
}
