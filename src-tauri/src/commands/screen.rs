//! A lab machine's screen window's commands: the main window opens one, the window asks for its
//! machine's console (VNC shown in it through its bridge; RDP opened in the system's client), the
//! RDP account's password, copied on request, and the attack box's sign-in where its screen asks.

use tauri::Manager;

use crate::app::window;
use crate::error::{Error, Result};
use crate::platform::AppDirs;
use crate::runtime::{Runtime, host, paths};
use crate::screen::{self, Protocol, Reached, Which, native, rdp};
use crate::{AppHandle, WebviewWindow};

/// Opens the screen window of lab `id`'s machine `machine`, titled after the lab's `title`.
#[tauri::command]
pub async fn screen_window(app: AppHandle, id: String, runtime: Runtime, machine: String, title: String) -> Result<()> {
    window::open_screen(&app, Which { id, runtime, machine }, &title).await
}

/// The screen of this window's machine, as the runner reaches it where the lab runs. Boxed: the
/// runner's futures are large.
#[tauri::command]
pub async fn screen_connect(window: WebviewWindow) -> Result<screen::Connection> {
    Box::pin(async move {
        let label = window.label().to_owned();
        let which = screen::which(&label)?;
        let dirs = AppDirs::from(window.app_handle());
        let dir = paths::lab_dir(&dirs, &which.id)?;
        let located = host::locate(&dirs, &dir, &which.id, which.runtime)?.ok_or_else(|| Error::Invalid("this lab doesn't run anywhere".into()))?;
        let reached = Reached::of(located.graphical_console(&dirs, &which.machine).await?)?;
        let client = (reached.protocol == Protocol::Rdp).then(client).flatten();
        screen::connect(&label, reached, client.as_ref())
    })
    .await
}

/// Opens lab `id`'s running VM `machine` in its hypervisor's own app ("Open in VirtualBox"): the
/// first way the runner gives that this computer has, run without a shell, or its web console in
/// the default browser. Boxed: the runner's futures are large.
#[tauri::command]
pub async fn screen_open_native(app: AppHandle, id: String, runtime: Runtime, machine: String) -> Result<()> {
    Box::pin(async move {
        let dirs = AppDirs::from(&app);
        let dir = paths::lab_dir(&dirs, &id)?;
        let located = host::locate(&dirs, &dir, &id, runtime)?.ok_or_else(|| Error::Invalid("this lab doesn't run anywhere".into()))?;
        let console = located.native_console(&dirs, &machine).await?;
        let home = crate::platform::home_dir();
        let launch = native::pick(&console.ways, std::env::consts::OS, home.as_deref(), std::path::Path::exists, native::on_path)
            .ok_or_else(|| Error::Invalid(format!("{} isn't installed on this computer", console.app)))?;
        match launch {
            native::Launch::Program { program, args } => native::run(&program, &args).await,
            native::Launch::Url(url) => {
                use tauri_plugin_opener::OpenerExt as _;
                app.opener().open_url(url, None::<&str>).map_err(|e| Error::Invalid(format!("couldn't open {}'s console: {e}", console.app)))
            }
        }
    })
    .await
}

/// Opens this window's Remote Desktop in the system's client.
#[tauri::command]
pub fn screen_open_rdp(window: WebviewWindow) -> Result<()> {
    let client = client().ok_or_else(|| Error::Invalid(missing_client(std::env::consts::OS).into()))?;
    let dirs = AppDirs::from(window.app_handle());
    screen::open_rdp(window.label(), &client, &dirs.data()?.join("screen"))
}

/// This window's Remote Desktop password, for the learner to paste in the client.
#[tauri::command]
pub fn screen_password(window: WebviewWindow) -> Result<String> {
    screen::password(window.label())
}

/// How the player signs in to this window's machine where its screen asks (the screen locker,
/// `LightDM`'s or the console's login), when it is the lab's attack box: its account and the
/// password the launcher keeps for it. None for any other machine.
#[tauri::command]
pub fn screen_sign_in(window: WebviewWindow) -> Result<Option<crate::runtime::attack_machine::SignIn>> {
    let which = screen::which(window.label())?;
    let dir = paths::lab_dir(&AppDirs::from(window.app_handle()), &which.id)?;
    crate::runtime::attack_machine::sign_in(&dir, Some(&which.machine))
}

/// The system's Remote Desktop client, when this computer has one.
fn client() -> Option<rdp::Client> {
    let home = crate::platform::home_dir();
    rdp::find(std::env::consts::OS, home.as_deref(), std::path::Path::exists, |name| {
        std::env::var_os("PATH").and_then(|path| std::env::split_paths(&path).map(|d| d.join(name)).find(|p| p.is_file()))
    })
}

/// What to install for a Remote Desktop on OS `os`.
fn missing_client(os: &str) -> &'static str {
    match os {
        "macos" => "No Remote Desktop app is installed: install Windows App from the App Store, then open the screen again.",
        "windows" => "Windows' Remote Desktop Connection (mstsc) isn't there.",
        _ => "No Remote Desktop client is installed: install FreeRDP (xfreerdp) or Remmina, then open the screen again.",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::test_app::TestApp;

    #[test]
    fn a_missing_client_says_what_to_install() {
        assert!(missing_client("macos").contains("Windows App"));
        assert!(missing_client("linux").contains("FreeRDP"));
    }

    #[tokio::test]
    async fn a_window_with_no_session_reaches_no_screen() {
        let t = TestApp::new();
        let w = window::builder(t.handle(), "screen-unopened:web", "").unwrap().build().unwrap();
        assert_eq!(screen_connect(w.clone()).await.unwrap_err().to_string(), "this screen window has no session");
        assert!(screen_password(w.clone()).is_err());
        // No Remote Desktop to open: nothing runs (a client this machine lacks says what to install).
        let tools = crate::platform::exec::fake::Tools::rules(&[]);
        assert!(screen_open_rdp(w).is_err());
        assert_eq!(tools.calls(), Vec::<String>::new());
    }

    #[tokio::test]
    async fn the_attack_box_screen_alone_gets_its_sign_in() {
        use crate::runtime::attack_machine::{self, Cpu, Kind, NAME, PLAYER};
        let t = TestApp::new();
        let spec = "version: 1\nname: t\nnetworks: { lan: { cidr: 10.70.0.0/24 } }\nmachines:\n  web: { os: debian@12, networks: { lan: 10 } }\n";
        let dir = t.lab("signin", &[("isoloom.yml", spec)]);
        attack_machine::add(&dir, Kind::Vm { desktop: true }, Cpu::Lab, None, None).unwrap();
        let app = t.handle();
        let sign_in = |machine: &str| {
            let label = format!("screen-signin:{machine}");
            let which = crate::screen::Which { id: "signin".into(), runtime: Runtime::Vm, machine: machine.into() };
            async move {
                window::open_screen(app, which, "Sign in").await.unwrap();
                let got = screen_sign_in(app.get_webview_window(&label).unwrap());
                crate::screen::close(&label);
                got
            }
        };
        let box_screen = sign_in(NAME).await.unwrap().expect("the box's sign-in");
        assert_eq!(box_screen.user, PLAYER);
        assert_eq!(Some(box_screen), attack_machine::sign_in(&dir, None).unwrap(), "the password the launcher keeps");
        assert_eq!(sign_in("web").await.unwrap(), None, "a target's screen gets none");
        attack_machine::remove(&dir).unwrap();
    }

    #[tokio::test]
    async fn a_lab_that_is_not_installed_opens_in_no_app() {
        let t = TestApp::new();
        let tools = crate::platform::exec::fake::Tools::rules(&[]);
        let err = screen_open_native(t.handle().clone(), "gone".into(), Runtime::Vm, "web".into()).await.unwrap_err();
        assert_eq!(err.to_string(), "lab `gone` is not installed");
        assert!(tools.calls().is_empty(), "nothing ran: {:?}", tools.calls());
    }

    #[tokio::test]
    async fn a_screen_for_a_lab_that_is_not_installed_reaches_nothing() {
        let t = TestApp::new();
        screen_window(t.handle().clone(), "gone".into(), Runtime::Vm, "web".into(), "Gone".into()).await.unwrap();
        let w = t.handle().get_webview_window("screen-gone:web").unwrap();
        let tools = crate::platform::exec::fake::Tools::rules(&[]);
        assert!(screen_connect(w).await.is_err());
        assert!(tools.calls().is_empty(), "nothing ran");
        crate::screen::close("screen-gone:web");
    }
}
