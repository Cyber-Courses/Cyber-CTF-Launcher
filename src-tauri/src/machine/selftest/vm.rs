//! The VM self-test, run by isothrum-machine (`selftest::hypervisor`): a real throwaway VM on a
//! local hypervisor, through a runner of its own. The hypervisor it runs on and the test box's
//! download are decided here; each step is said here, in the app's words.

use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use isothrum_machine::selftest::Progress;
use isothrum_machine::selftest::hypervisor::{self, ADDRESSES, Choices, Failure, Found, Step, StepProgress, TestVm};
use isothrum_runner::BoxFuture;

use super::Reporter;
use crate::error::{Error, Result};
use crate::platform::AppDirs;
use crate::platform::sync::Busy;
use crate::runtime::providers::{self, Provider};
use crate::runtime::selftest_report::StepState;

/// How the test VM shows up in the hypervisor's own app while it runs.
pub(crate) const VM_NAME: &str = "CyberCTF test VM";
/// The test VM's host name inside.
const HOSTNAME: &str = "cyberctf-selftest";

/// A test box download in flight (setup's, ahead of the test, or the test's): a second waits for it.
static VM_BOX_DOWNLOADING: AtomicBool = AtomicBool::new(false);
/// Last line of the box download, so a test waiting on it can show progress.
static VM_BOX_PROGRESS: Mutex<String> = Mutex::new(String::new());

/// The local provider the VM test uses ([`providers::pick`] of the hypervisors ready here, Vagrant
/// included): the preferred one (Settings) when it's ready, else the first ready one.
pub(super) async fn test_provider(preferred: Option<Provider>) -> Option<Provider> {
    providers::pick(preferred, &providers::ready_here().await)
}

/// Why no local hypervisor can run the test VM, of `statuses` ([`providers::detect`]'s): what keeps
/// the preferred one (else the first one installed) from running VMs, in the words the machine
/// check uses (`/dev/kvm` this account can't open, a group that applies from the next login…).
fn none_ready(preferred: Option<Provider>, statuses: &[providers::ProviderStatus]) -> String {
    let installed = |s: &&providers::ProviderStatus| !s.remote && s.hypervisor == Some(true);
    let blocked = statuses
        .iter()
        .filter(installed)
        .find(|s| preferred.is_some_and(|p| s.provider == p))
        .or_else(|| statuses.iter().find(installed))
        .and_then(|s| s.reason.as_ref().map(|why| (Provider::from(s.provider), why)));
    match blocked {
        Some((p, why)) => format!("{} can't run VMs here: {}", p.label(), providers::said(why)),
        None => "no local hypervisor with its Vagrant plugin is installed".to_owned(),
    }
}

/// Makes sure box `bx` is here for `p`: added once (the app's runner records it, for Storage),
/// or waited for while another download of it runs, its progress said to `on_line` either way.
/// Whether it was there without being added by this call.
pub(super) async fn ensure_box(dirs: &AppDirs, p: isothrum_runner::Provider, bx: &'static str, on_line: &(dyn Fn(String) + Send + Sync)) -> Result<bool> {
    let cx = providers::machine_selftest();
    let mut waited = false;
    loop {
        if hypervisor::box_for(&cx, p).await == (bx, true) {
            return Ok(true);
        }
        if let Some(_busy) = Busy::try_take(&VM_BOX_DOWNLOADING) {
            on_line(format!("Downloading {bx} for {}, once…", p.id()));
            let mut line = |l: &str| {
                if let Ok(mut last) = VM_BOX_PROGRESS.lock() {
                    l.clone_into(&mut last);
                }
                on_line(l.to_owned());
            };
            let added = hypervisor::add_box(&cx, p, bx, &mut line).await.map_err(Error::Invalid)?;
            super::keep(dirs, added).await;
            return Ok(waited);
        }
        waited = true;
        let line = VM_BOX_PROGRESS.lock().map(|l| l.clone()).unwrap_or_default();
        on_line(if line.trim().is_empty() { format!("Finishing the background download of {bx}…") } else { line });
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

/// What the test asks of the app: the hypervisor (Settings' preference, else the first ready),
/// and the box (fetched once, for Storage to clean up).
struct Picks<'a> {
    dirs: &'a AppDirs,
    preferred: Option<Provider>,
}

impl Choices for Picks<'_> {
    fn provider(&self) -> BoxFuture<'_, std::result::Result<isothrum_runner::Provider, String>> {
        Box::pin(async move {
            match test_provider(self.preferred).await.and_then(Provider::vagrant) {
                Some(p) => Ok(p),
                None => Err(none_ready(self.preferred, &providers::detect(true).await)),
            }
        })
    }

    /// UTM, asked as a lab's start asks it ([`check_utm`](crate::runtime::launch::check_utm)):
    /// opened hidden when it isn't running, where `utmctl list` would bring its window up.
    fn probe(&self, p: isothrum_runner::Provider) -> BoxFuture<'_, std::result::Result<bool, String>> {
        Box::pin(async move {
            if p != isothrum_runner::Provider::Utm {
                return Ok(false);
            }
            crate::runtime::launch::check_utm(&mut |_| {}).await.map(|()| true).map_err(|e| e.to_string())
        })
    }

    fn fetch_box<'a>(
        &'a self,
        p: isothrum_runner::Provider,
        name: &'static str,
        line: &'a (dyn Fn(String) + Send + Sync),
    ) -> BoxFuture<'a, std::result::Result<bool, String>> {
        Box::pin(async move { ensure_box(self.dirs, p, name, line).await.map_err(|e| e.to_string()) })
    }
}

/// A step's id and label, as the page shows it.
fn named(step: Step) -> (&'static str, &'static str) {
    match step {
        Step::Leftover => ("leftover", "Remove the VM an earlier test left"),
        Step::Vagrant => ("vagrant", "Vagrant is installed"),
        Step::Provider => ("provider", "A hypervisor is ready"),
        Step::Hypervisor => ("hypervisor", "Hypervisor responds"),
        Step::Box => ("box", "Get a small test VM image"),
        Step::Boot => ("boot", "Boot the test VM"),
        Step::Exec => ("exec", "Run a command inside the VM"),
        Step::Network => ("network", "VM reachable on a lab network"),
        _ => ("networks", "Remove the networks the test made"),
    }
}

/// What a passed (or skipped) step found, as its detail line.
fn detail(found: Found) -> Option<String> {
    match found {
        Found::Version(v) => Some(v),
        Found::Provider(p) => Some(p.id().to_owned()),
        Found::NothingToProbe => Some("nothing to probe for this provider".to_owned()),
        Found::Box { name, held: true } => Some(format!("{name} (ready)")),
        Found::Box { name, held: false } => Some(name.to_owned()),
        Found::Booted { name, provider } => Some(format!("{name} on {}", provider.id())),
        Found::Uname(said) => Some(said),
        Found::Reached { address } => Some(format!("this machine → {address}")),
        Found::AddressesTaken => Some(format!("every test address ({}…) is taken on this machine", ADDRESSES[0])),
        Found::NoPrivateNetwork(p) => Some(format!("{} has no private networks", p.id())),
        Found::Removed(from) => Some(format!("removed from {}", from.join(", "))),
        Found::NetworksRemoved(names) => Some(names.join(", ")),
        _ => None,
    }
}

/// Why a step failed, as the app's error: the runner's report as a lab's start says it (UTM
/// refused by the system: the way out), a VM of the test's name that is someone else's in the
/// app's words.
fn error(failure: Failure) -> Error {
    match failure {
        Failure::Runner(report) => crate::runtime::isothrum::errors::coded(&report),
        Failure::Foreign { hypervisor } => Error::Invalid(format!(
            "{hypervisor} has a VM named \"{VM_NAME}\" that isn't this test's, so it was left alone. Rename or remove it in {hypervisor}, then test again."
        )),
        other => Error::Invalid(other.to_string()),
    }
}

/// Says one step's progress through `r`; the networks' removal is kept in `networks`, said after
/// the VM's deletion.
fn say(r: &Reporter, networks: &Mutex<Option<StepProgress>>, progress: StepProgress) {
    match progress {
        Progress::Passed(step, Found::Line(line)) => {
            let (id, label) = named(step);
            r.progress(id, label, &line);
        }
        later @ (Progress::Passed(Step::Networks, _) | Progress::Failed(Step::Networks, _)) => {
            if let Ok(mut kept) = networks.lock() {
                *kept = Some(later);
            }
        }
        other => super::say(r, other, named, detail, error),
    }
}

/// The test on `preferred` (else the first hypervisor ready), its VM and runner in `dir`. What it
/// downloads goes to the app's runner record (Storage cleans it).
pub(super) async fn run_test(dirs: &AppDirs, dir: &Path, preferred: Option<Provider>, r: &Reporter) -> Result<()> {
    let cx = providers::machine_selftest();
    let vm = TestVm::new(VM_NAME, HOSTNAME);
    let networks = Mutex::new(None);
    let ran = hypervisor::run(&cx, dir, &vm, &Picks { dirs, preferred }, &|p| say(r, &networks, p)).await;
    let passed = ran.passed.map_err(error);
    // The box stays cached for the next test.
    if let Some(down) = ran.cleaned_up {
        r.cleanup("Delete the test VM", &passed, down.map_err(error), Some("test box kept for next time".into()));
    }
    let (id, label) = named(Step::Networks);
    match networks.lock().ok().and_then(|mut kept| kept.take()) {
        Some(Progress::Passed(_, found)) => r.send(id, label, StepState::Ok, detail(found)),
        Some(Progress::Failed(_, why)) => r.send(id, label, StepState::Fail, Some(why.to_string())),
        _ => {}
    }
    passed
}

#[cfg(test)]
mod tests;
