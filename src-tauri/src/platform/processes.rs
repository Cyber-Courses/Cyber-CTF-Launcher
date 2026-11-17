//! Every program the launcher starts goes through one runner, Isothrum's
//! [`SystemProcesses`]: headless on Windows, each tool's whole tree ended when its call is cut
//! short (a timeout, a cancellation) and at the app's exit ([`end_all_now`]), its messages in the
//! C locale. Each tool is recorded while it runs in the ledger in the app's data folder
//! ([`keep_ledger_in`]), so the next start ends what a crash or a kill left running
//! ([`end_left_behind`]).

use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, OnceLock};

use isothrum_runner::{BoxFuture, Exit, OnLine, ProcessError, ProcessRunner, ProcessSpec, SystemProcesses};

/// The ledger's folder in the app's data folder.
pub const LEDGER_DIR: &str = "processes";

/// The processes this app runs: one set, whose clones share the tools running and stopping.
static PROCESSES: LazyLock<SystemProcesses> = LazyLock::new(SystemProcesses::default);

/// The ledger the tools are recorded in, once the app knows its data folder.
static LEDGER: OnceLock<PathBuf> = OnceLock::new();

/// Records every tool started from now on in the folder `dir` ([`LEDGER_DIR`] in the app's data
/// folder). The first folder given stays.
pub fn keep_ledger_in(dir: PathBuf) {
    // A later call (a second window's setup) names the same folder.
    let _ = LEDGER.set(dir);
}

/// Ends what a previous process of the app recorded in the ledger `dir` and left running (the app
/// or a deploy worker killed outright, a crash); the ids it ended.
pub async fn end_left_behind(dir: &Path) -> Vec<u32> {
    let ended = isothrum_runner::ledger::end_orphans(dir).await;
    if !ended.is_empty() {
        log::info!("ended {} tool(s) a previous run of the app left running: {ended:?}", ended.len());
    }
    ended
}

/// Ends every tool running now, at once: the app is exiting, and nothing would end them after it.
pub fn end_all_now() {
    PROCESSES.end_all_now();
}

/// The runner every program of the app starts through, each tool recorded in the ledger.
pub fn system() -> Arc<dyn ProcessRunner> {
    Arc::new(Ledgered(PROCESSES.clone()))
}

/// [`SystemProcesses`], each tool recorded in the app's ledger unless its call names a folder.
struct Ledgered(SystemProcesses);

impl ProcessRunner for Ledgered {
    fn run<'a>(&'a self, spec: &'a ProcessSpec, on_line: OnLine<'a>) -> BoxFuture<'a, Result<Exit, ProcessError>> {
        match LEDGER.get().filter(|_| spec.ledger_dir.is_none()) {
            Some(dir) => {
                let mut recorded = spec.clone();
                recorded.ledger_dir = Some(dir.clone());
                Box::pin(async move { self.0.run(&recorded, on_line).await })
            }
            None => self.0.run(spec, on_line),
        }
    }
}
