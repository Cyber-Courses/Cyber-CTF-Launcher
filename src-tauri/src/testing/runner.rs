//! Runs as the Isothrum runner records them, for tests that start from a run already there: saved
//! and read back through the runner's public API (its store), never as files of the runner's
//! home, whose layout is the runner's own.

use std::path::Path;

use isothrum_runner::store::Store;
use isothrum_runner::{Desired, RunName, RunRecord, RunState, Runner, RunnerConfig};

/// Saves `record` (a run record, as the runner's JSON) in the store of the runner whose home is
/// `home`, in the state the record says its last operation left it in, wanted as that state
/// says (gone when absent, parked when parked, else running). A run already recorded is replaced.
pub fn save_run(home: &Path, record: &serde_json::Value) {
    let record: RunRecord = serde_json::from_value(record.clone()).unwrap();
    let desired = match record.state {
        RunState::Absent => Desired::Gone,
        RunState::Parked => Desired::Parked,
        _ => Desired::Running,
    };
    std::fs::create_dir_all(home.join("runs").join(record.name.as_str())).unwrap();
    Store::open(home)
        .unwrap()
        .write(|tx| {
            tx.forget_run(&record.name)?;
            tx.save_record(&record, desired)
        })
        .unwrap();
}

/// Run `name`'s record in the runner whose home is `home`, as the runner's JSON.
pub fn run_record(home: &Path, name: &str) -> serde_json::Value {
    let runner = Runner::new(RunnerConfig::new(home)).unwrap();
    let record = runner.run_record(&RunName::new(name).unwrap()).unwrap().unwrap();
    serde_json::to_value(record).unwrap()
}

/// Makes the store of the runner whose home is `home` unreadable, as a damaged disk would.
pub fn damage_store(home: &Path) {
    std::fs::create_dir_all(home).unwrap();
    for f in ["state.db", "state.db-wal", "state.db-shm"] {
        let _ = std::fs::remove_file(home.join(f));
    }
    std::fs::write(home.join("state.db"), "not a database").unwrap();
}

/// Whether the runner whose home is `home` records run `name`.
pub fn is_recorded(home: &Path, name: &str) -> bool {
    Runner::new(RunnerConfig::new(home)).unwrap().run_record(&RunName::new(name).unwrap()).unwrap().is_some()
}
