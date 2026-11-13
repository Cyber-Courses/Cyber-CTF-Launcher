//! What the runner tests share: an installed lab under a fresh runner home, a runner on that
//! home, and the scripted processes (the runner's fake ones) its tools are answered by.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use isothrum_runner::fake::{Call, FakeProcesses, Reply};
use isothrum_runner::{FakePorts, ProcessRunner, RunnerConfig};

use super::lab;

/// What each runner test module imports: the fixture, the fake processes' calls and replies, and
/// the model's lock.
pub(super) mod prelude {
    pub(in crate::runtime::isothrum) use std::sync::Mutex;

    pub(in crate::runtime::isothrum) use isothrum_runner::fake::{Call, Reply};

    pub(in crate::runtime::isothrum) use super::{Home, Scripted, locked};
}

/// A fresh runner home, `root`, holding an installed lab `dir` (`<root>/labs/web`) with its
/// files generated; removed when dropped.
pub(super) struct Home {
    pub(super) root: PathBuf,
    pub(super) dir: PathBuf,
}

impl Home {
    /// The lab's `spec` and its other `files` written, then generated for `target`; `tag` names
    /// the home's temporary folder.
    pub(super) fn new(tag: &str, spec: &str, files: &[(&str, &str)], target: lab::Target) -> Home {
        let root = std::env::temp_dir().join(format!("cyberctf-isothrum{tag}-{}", rand::random::<u32>()));
        let dir = root.join("labs").join("web");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("isoloom.yml"), spec).unwrap();
        for (name, body) in files {
            std::fs::write(dir.join(name), body).unwrap();
        }
        lab::prepare(&dir, target).unwrap();
        Home { root, dir }
    }

    /// A runner on this home: its processes `fake`, its free ports `ports`, or a real machine's
    /// worth of them (41000 and up) when none is given: the runner pins every published port to
    /// a free one where the preferred number isn't fixed (Isothrum 2.1.3), and a double with no
    /// port fails every start with "no free host port", which no real machine does.
    pub(super) fn config(&self, fake: &Arc<FakeProcesses>, ports: impl IntoIterator<Item = u16>) -> RunnerConfig {
        let mut config = RunnerConfig::new(&self.root);
        config.processes = Arc::clone(fake) as Arc<dyn ProcessRunner>;
        let given: Vec<u16> = ports.into_iter().collect();
        config.ports = Arc::new(FakePorts::new(if given.is_empty() { (41000..41064).collect() } else { given }));
        config
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// An installed lab, and the fake processes its tools are answered by from a model, `S`, of
/// what they run.
pub(super) struct Scripted<S> {
    pub(super) home: Home,
    model: Arc<Mutex<S>>,
    pub(super) fake: Arc<FakeProcesses>,
}

impl<S: Default + Send + 'static> Scripted<S> {
    /// Each call answered by `answer` from the model, which starts as its default.
    pub(super) fn answered_by(home: Home, answer: fn(&Mutex<S>, &Call) -> Reply) -> Scripted<S> {
        let model = Arc::new(Mutex::new(S::default()));
        let script = Arc::clone(&model);
        Scripted { home, model, fake: Arc::new(FakeProcesses::new(move |call| answer(&script, call))) }
    }

    /// The model, to read or change between calls.
    pub(super) fn model(&self) -> MutexGuard<'_, S> {
        locked(&self.model)
    }
}

/// `model` locked, a test's earlier panic notwithstanding.
pub(super) fn locked<S>(model: &Mutex<S>) -> MutexGuard<'_, S> {
    model.lock().unwrap_or_else(PoisonError::into_inner)
}
