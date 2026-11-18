//! Quitting while a lab operation runs in this process. An interrupted cloud apply can leave
//! billable resources, so a quit is held and the frontend asks first; the player can also let
//! the app finish in the background with its windows hidden. What the runner service runs never
//! holds the app: the service finishes it on its own.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::AppHandle;
use tauri::Manager;

/// Where the app stands on quitting: one state for the close/exit handlers, the background
/// finish and the prompt, read and changed under one lock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// Running as usual: a quit while a lab operation runs is held.
    Open,
    /// Finishing in the background with its windows hidden (`linger_quit`); a relaunch takes it
    /// back to `Open`, so the app stays open instead of exiting once the operations are done.
    Lingering,
    /// The user confirmed quitting while a lab operation is in progress: the close/exit handlers
    /// stop intercepting and let the app go.
    Forced,
}

/// The quit state: its phase, and when the last quit prompt was shown, so a second quit right
/// after it means "quit anyway" (there must always be a way out that doesn't go through Activity
/// Monitor or Task Manager).
struct QuitState {
    phase: Phase,
    prompted_at: Option<Instant>,
}

static QUIT: Mutex<QuitState> = Mutex::new(QuitState { phase: Phase::Open, prompted_at: None });

/// The quit state, locked (a panic elsewhere doesn't stop a quit).
fn state() -> std::sync::MutexGuard<'static, QuitState> {
    QUIT.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The quit state's phase.
fn phase() -> Phase {
    state().phase
}

const QUIT_AGAIN_WINDOW: Duration = Duration::from_secs(20);

/// How long a lingering app waits for its in-app operations before exiting anyway.
const LINGER_CAP: Duration = Duration::from_secs(60 * 60);

/// Quit by finishing in the background: the windows go away now; the process exits once the
/// lab operations of this process ([`busy`]: a stop, a park, a start) are done.
pub fn linger_quit(app: AppHandle) {
    for (_, w) in app.webview_windows() {
        let _ = w.hide();
    }
    {
        let mut state = state();
        if state.phase == Phase::Open {
            state.phase = Phase::Lingering;
        }
    }
    tauri::async_runtime::spawn(async move {
        // Bounded: a wedged operation (a hung VBoxManage) must not keep an invisible app alive
        // for good. Past the cap it exits anyway; the runner service's operations go on without it.
        let deadline = Instant::now() + LINGER_CAP;
        while busy(&app) > 0 && Instant::now() < deadline && phase() == Phase::Lingering {
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
        // Relaunched meanwhile (the windows are back): stay open. A quit forced meanwhile, or
        // before, still exits: the windows are hidden, and nothing would end the app otherwise.
        if phase() == Phase::Open {
            return;
        }
        exit_now(&app);
    });
}

/// The user chose to quit anyway from the "a lab operation is still running" prompt: stop intercepting
/// and exit.
pub fn force_quit(app: &AppHandle) {
    exit_now(app);
}

fn exit_now(app: &AppHandle) {
    // The lock is let go before exiting: the exit handlers read the state.
    state().phase = Phase::Forced;
    app.exit(0);
}

/// The app was opened again (a second launch, a `cyberctf://` link): it stays open instead of
/// exiting once a background finish is done.
pub fn stop_lingering() {
    let mut state = state();
    if state.phase == Phase::Lingering {
        state.phase = Phase::Open;
    }
}

/// Whether a quit gets the prompt: a window can show it, and it wasn't just shown (a second quit
/// right after it is the user insisting).
fn should_prompt(visible: bool, prompted_at: Option<Instant>) -> bool {
    let insisting = prompted_at.is_some_and(|t| t.elapsed() < QUIT_AGAIN_WINDOW);
    visible && !insisting
}

/// The labs with an operation of this process under way (its run service's: a stop as much as a
/// start), counted.
fn busy(app: &AppHandle) -> usize {
    match crate::run_service::LocalRunService::of(&crate::platform::AppDirs::from(app)) {
        Ok(service) => service.busy_labs().len(),
        Err(e) => {
            log::warn!("the operations under way can't be told ({e}): the quit goes on");
            0
        }
    }
}

/// If a lab operation runs in this process ([`busy`], a stop as much as a start) and
/// the user hasn't already confirmed, keep the app open and ask the frontend to confirm. Returns true when the quit was intercepted. Nothing is
/// intercepted when no window could show the prompt (the app already lingers hidden), nor on a
/// second quit within a few seconds of the prompt: that is the user insisting.
pub fn intercept(app: &AppHandle) -> bool {
    let busy = busy(app);
    if phase() == Phase::Forced || busy == 0 {
        return false;
    }
    let visible = app.webview_windows().values().any(|w| w.is_visible().unwrap_or(false));
    {
        let mut state = state();
        if !should_prompt(visible, state.prompted_at) {
            state.phase = Phase::Forced;
            return false;
        }
        state.prompted_at = Some(Instant::now());
    }
    crate::app::events::emit(app, crate::app::events::QUIT_BLOCKED, busy);
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

    /// One test for both: they share the process-wide quit state.
    #[test]
    fn a_background_finish_stays_open_once_relaunched_and_a_forced_quit_exits_at_once() {
        let t = crate::testing::test_app::TestApp::new();
        // A deploy in flight holds the background finish (it looks again every 2 s).
        let service = crate::run_service::LocalRunService::of(&t.dirs()).unwrap();
        let deploy = service.hold("quit-test-linger", crate::runtime::Operation::Launch);
        linger_quit(t.handle().clone());
        assert_eq!(phase(), Phase::Lingering);
        // Opened again meanwhile: once the deploy is done, the app stays open.
        stop_lingering();
        assert_eq!(phase(), Phase::Open);
        drop(deploy);
        std::thread::sleep(Duration::from_millis(2500));
        assert_ne!(phase(), Phase::Forced, "a relaunched app doesn't exit");

        // Quit anyway: the quit guard lets go and the runtime is asked to exit (Tauri's mock
        // runtime has no exit to request: it panics there).
        let exit = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| force_quit(t.handle())));
        assert!(exit.is_err(), "the runtime was asked to exit");
        assert_eq!(phase(), Phase::Forced);
        let deploy = service.hold("quit-test-forced", crate::runtime::Operation::Launch);
        assert!(!intercept(t.handle()), "a forced quit is never held");
        drop(deploy);
    }
}
