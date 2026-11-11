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
fn found(need: &NativeNeed, os: &str, home: Option<&Path>, exists: &impl Fn(&Path) -> bool, on_path: &impl Fn(&str) -> Option<PathBuf>) -> Option<PathBuf> {
    match need {
        NativeNeed::Program(program) => {
            let path = Path::new(program);
            if path.is_absolute() { exists(path).then(|| path.to_path_buf()) } else { on_path(program) }
        }
        NativeNeed::MacApp(app) if os == "macos" => mac_app_places(app, home).into_iter().find(|p| exists(p)),
        NativeNeed::Browser => Some(PathBuf::new()),
        _ => None,
    }
}

/// The app's name, when this computer has what one of its ways needs: what the lab page's
/// "Open in …" button says; `None` hides the button.
pub(crate) fn installed(
    app: Option<&NativeApp>,
    os: &str,
    home: Option<&Path>,
    exists: impl Fn(&Path) -> bool,
    on_path: impl Fn(&str) -> Option<PathBuf>,
) -> Option<String> {
    let app = app?;
    app.needs.iter().any(|n| found(n, os, home, &exists, &on_path).is_some()).then(|| app.name.clone())
}

/// [`installed`] on this computer: its OS, the user's home, its files and its `PATH`.
pub(crate) fn installed_here(app: Option<&NativeApp>) -> Option<String> {
    installed(app, std::env::consts::OS, crate::platform::home_dir().as_deref(), Path::exists, on_path)
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
    os: &str,
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
    let name = program.file_name().map_or_else(|| program.display().to_string(), |n| n.to_string_lossy().into_owned());
    let mut cmd = tokio::process::Command::new(program);
    cmd.args(args).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| Error::Invalid(format!("couldn't open {name}: {e}")))?;
    let stderr = child.stderr.take();
    match tokio::time::timeout(FAILS_WITHIN, child.wait()).await {
        Ok(Ok(status)) if !status.success() => {
            let mut said = String::new();
            if let Some(mut err) = stderr {
                use tokio::io::AsyncReadExt as _;
                let _ = err.read_to_string(&mut said).await;
            }
            let said = said.lines().map(str::trim).rfind(|l| !l.is_empty()).unwrap_or("it failed").to_owned();
            Err(Error::Invalid(format!("{name} couldn't open the VM: {said}")))
        }
        Ok(Ok(_)) => Ok(()),
        Ok(Err(e)) => Err(Error::Invalid(format!("couldn't open {name}: {e}"))),
        Err(_) => {
            // Still open (a viewer's window): reaped once it ends.
            tokio::spawn(async move {
                let _ = child.wait().await;
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
        assert_eq!(installed(Some(&vbox), "linux", None, exists_of(&[]), path_with(&["VBoxManage"])), Some("VirtualBox".into()));
        assert_eq!(installed(Some(&vbox), "linux", None, exists_of(&[]), path_with(&[])), None, "not installed");
        let fusion = app("VMware Fusion", &[NativeNeed::MacApp("/Applications/VMware Fusion.app".into())]);
        let home = Path::new("/Users/u");
        assert_eq!(
            installed(Some(&fusion), "macos", Some(home), exists_of(&["/Users/u/Applications/VMware Fusion.app"]), path_with(&[])),
            Some("VMware Fusion".into()),
            "in the user's own Applications"
        );
        assert_eq!(installed(Some(&fusion), "macos", Some(home), exists_of(&[]), path_with(&[])), None);
        assert_eq!(installed(Some(&fusion), "linux", None, exists_of(&["/Applications/VMware Fusion.app"]), path_with(&[])), None, "a Mac app off a Mac");
        let workstation = app("VMware Workstation", &[NativeNeed::Program(r"C:\Program Files (x86)\VMware\VMware Workstation\vmware.exe".into())]);
        assert_eq!(installed(Some(&workstation), "windows", None, exists_of(&[]), path_with(&[])), None);
        let esxi = app("ESXi", &[NativeNeed::Browser]);
        assert_eq!(installed(Some(&esxi), "linux", None, exists_of(&[]), path_with(&[])), Some("ESXi".into()), "a web console needs only the browser");
        assert_eq!(installed(None, "macos", None, exists_of(&[]), path_with(&[])), None, "vagrant-qemu has no app");
    }

    #[test]
    fn the_first_way_this_computer_has_is_taken() {
        let libvirt = [program("virt-viewer", &["--connect", "qemu:///system", "lab_web"]), program("remote-viewer", &["vnc://127.0.0.1:2"])];
        assert_eq!(
            pick(&libvirt, "linux", None, exists_of(&[]), path_with(&["remote-viewer"])),
            Some(Launch::Program { program: "/bin/remote-viewer".into(), args: vec!["vnc://127.0.0.1:2".into()] }),
            "virt-viewer isn't installed"
        );
        assert_eq!(
            pick(&libvirt, "linux", None, exists_of(&[]), path_with(&["virt-viewer", "remote-viewer"])),
            Some(Launch::Program { program: "/bin/virt-viewer".into(), args: vec!["--connect".into(), "qemu:///system".into(), "lab_web".into()] })
        );
        assert_eq!(pick(&libvirt, "linux", None, exists_of(&[]), path_with(&[])), None);
        let parallels = [NativeOpen::MacApp { app: "/Applications/Parallels Desktop.app".into(), args: vec!["/Users/u/Parallels/web.pvm".into()] }];
        assert_eq!(
            pick(&parallels, "macos", None, exists_of(&["/Applications/Parallels Desktop.app"]), path_with(&[])),
            Some(Launch::Program {
                program: "/usr/bin/open".into(),
                args: vec!["-a".into(), "/Applications/Parallels Desktop.app".into(), "/Users/u/Parallels/web.pvm".into()]
            })
        );
        let proxmox = [NativeOpen::Url { url: "https://pve:8006/?console=kvm&novnc=1&vmid=104&node=pve".into() }];
        assert_eq!(
            pick(&proxmox, "linux", None, exists_of(&[]), path_with(&[])),
            Some(Launch::Url("https://pve:8006/?console=kvm&novnc=1&vmid=104&node=pve".into()))
        );
        let plain = [NativeOpen::Url { url: "http://esxi/ui".into() }];
        assert_eq!(pick(&plain, "linux", None, exists_of(&[]), path_with(&[])), None, "only https is opened");
    }

    #[tokio::test]
    async fn a_program_that_fails_at_once_says_why_and_one_that_stays_open_is_left_running() {
        let tools = crate::platform::exec::fake::Tools::rules(&[(
            "startvm",
            crate::platform::exec::fake::fail("VBoxManage: error: The machine is already locked for a session"),
        )]);
        let err = run(Path::new("/bin/VBoxManage"), &["startvm".into(), "web".into(), "--type".into(), "separate".into()]).await.unwrap_err();
        assert!(err.to_string().contains("VBoxManage"), "{err}");
        assert_eq!(tools.calls(), ["/bin/VBoxManage startvm web --type separate"]);
        drop(tools);
        let _ok = crate::platform::exec::fake::Tools::rules(&[]);
        run(Path::new("/bin/virt-viewer"), &["lab_web".into()]).await.unwrap();
    }
}
