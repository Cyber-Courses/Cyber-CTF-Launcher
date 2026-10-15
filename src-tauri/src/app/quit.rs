//! Quitting while a lab operation runs in this process. An interrupted cloud apply can leave
//! billable resources, so a quit is held and the frontend asks first; the player can also let
//! the app finish in the background with its windows hidden. Detached deploy workers never hold
//! the app: they finish on their own.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::AppHandle;
use tauri::{Emitter, Manager};

use crate::runtime;

/// Set once the user confirms quitting while a deploy is in progress, so the close/exit handlers
/// stop intercepting and let the app go.
static FORCE_QUIT: AtomicBool = AtomicBool::new(false);
/// Set while the app finishes in the background with its windows hidden (`linger_quit`); a
/// relaunch clears it, so the app stays open instead of exiting once the deploys are done.
static LINGERING: AtomicBool = AtomicBool::new(false);

/// When the last quit prompt was shown, so a second quit right after it means "quit anyway":
/// there must always be a way out that doesn't go through Activity Monitor or Task Manager.
static QUIT_PROMPTED_AT: Mutex<Option<Instant>> = Mutex::new(None);
const QUIT_AGAIN_WINDOW: Duration = Duration::from_secs(20);

/// How long a lingering app waits for its in-app deploys before exiting anyway.
const LINGER_CAP: Duration = Duration::from_secs(60 * 60);

/// Quit by finishing in the background: the windows go away now; the process exits once the
/// in-app deploys (the fallback when no worker could be started) are done.
#[tauri::command]
pub fn linger_quit(app: AppHandle) {
    for (_, w) in app.webview_windows() {
        let _ = w.hide();
    }
    LINGERING.store(true, Ordering::SeqCst);
    tauri::async_runtime::spawn(async move {
        // Bounded: a wedged operation (a hung VBoxManage) must not keep an invisible app alive
        // for good. Past the cap it exits anyway; a detached worker would have been unaffected.
        let deadline = Instant::now() + LINGER_CAP;
        while runtime::active_deploys() > 0 && Instant::now() < deadline && LINGERING.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
        // Relaunched meanwhile (the windows are back): stay open.
        if !LINGERING.swap(false, Ordering::SeqCst) {
            return;
        }
        exit_now(&app);
    });
}

/// The user chose to quit anyway from the "a lab is still deploying" prompt: stop intercepting
/// and exit.
#[tauri::command]
pub fn force_quit(app: AppHandle) {
    exit_now(&app);
}

fn exit_now(app: &AppHandle) {
    FORCE_QUIT.store(true, Ordering::SeqCst);
    app.exit(0);
}

/// The app was opened again (a second launch, a `cyberctf://` link): it stays open instead of
/// exiting once a background finish is done.
pub fn stop_lingering() {
    LINGERING.store(false, Ordering::SeqCst);
}

/// Whether a quit gets the prompt: a window can show it, and it wasn't just shown (a second quit
/// right after it is the user insisting).
fn should_prompt(visible: bool, prompted_at: Option<Instant>) -> bool {
    let insisting = prompted_at.is_some_and(|t| t.elapsed() < QUIT_AGAIN_WINDOW);
    visible && !insisting
}

/// If an in-app operation is in progress (and the user hasn't already confirmed), keep the app
/// open and ask the frontend to confirm. Returns true when the quit was intercepted. Nothing is
/// intercepted when no window could show the prompt (the app already lingers hidden), nor on a
/// second quit within a few seconds of the prompt: that is the user insisting.
pub fn intercept(app: &AppHandle) -> bool {
    if FORCE_QUIT.load(Ordering::SeqCst) || runtime::active_deploys() == 0 {
        return false;
    }
    let visible = app.webview_windows().values().any(|w| w.is_visible().unwrap_or(false));
    let mut prompted = QUIT_PROMPTED_AT.lock().unwrap_or_else(|e| e.into_inner());
    if !should_prompt(visible, *prompted) {
        FORCE_QUIT.store(true, Ordering::SeqCst);
        return false;
    }
    *prompted = Some(Instant::now());
    let _ = app.emit("quit-blocked", runtime::active_deploys());
    if let Some(w) = app.get_webview_window(crate::app::window::MAIN) {
        let _ = w.set_focus();
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_quit_is_held_once_and_only_with_a_window_to_ask_in() {
        assert!(should_prompt(true, None));
        assert!(!should_prompt(false, None));
        assert!(!should_prompt(true, Some(Instant::now())));
        if let Some(long_ago) = Instant::now().checked_sub(QUIT_AGAIN_WINDOW + Duration::from_secs(1)) {
            assert!(should_prompt(true, Some(long_ago)));
        }
    }

    /// One test for both: they share the process-wide flags.
    #[test]
    fn a_background_finish_stays_open_once_relaunched_and_a_forced_quit_exits_at_once() {
        let t = crate::testing::test_app::TestApp::new();
        // A deploy in flight holds the background finish (it looks again every 2 s).
        let deploy = runtime::DeployGuard::new("quit-test-linger", runtime::Action::Start);
        linger_quit(t.handle().clone());
        assert!(LINGERING.load(Ordering::SeqCst));
        // Opened again meanwhile: once the deploy is done, the app stays open.
        stop_lingering();
        assert!(!LINGERING.load(Ordering::SeqCst));
        drop(deploy);
        std::thread::sleep(Duration::from_millis(2500));
        assert!(!FORCE_QUIT.load(Ordering::SeqCst), "a relaunched app doesn't exit");

        // Quit anyway: the quit guard lets go and the runtime is asked to exit (Tauri's mock
        // runtime has no exit to request: it panics there).
        let exit = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| force_quit(t.handle().clone())));
        assert!(exit.is_err(), "the runtime was asked to exit");
        assert!(FORCE_QUIT.load(Ordering::SeqCst));
        let deploy = runtime::DeployGuard::new("quit-test-forced", runtime::Action::Start);
        assert!(!intercept(t.handle()), "a forced quit is never held");
        drop(deploy);
    }
}
