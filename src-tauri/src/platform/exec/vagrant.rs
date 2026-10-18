//! Vagrant commands on a lab's machines, elevated when the hypervisor needs it.
//!
//! Hyper-V only takes management commands from an administrator, and Vagrant's Hyper-V provider
//! refuses to run otherwise (issue #185). On Windows, commands on Hyper-V machines go through
//! [`super::elevate`]'s script: elevated (one UAC prompt per [`Vagrant::batch`]) when the app may
//! not manage Hyper-V itself, their output streamed as usual. Everywhere else (and on every other
//! provider) they run exactly as any other tool, through [`super::stream`] and [`super::run_env`].

use std::path::Path;

use super::elevate::{self, Mode};
use crate::error::Result;

/// When a step of a [`Vagrant::batch`] runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum When {
    /// Only while every step before it succeeded.
    IfOk,
}

/// One Vagrant command of a batch: its arguments and when it runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub args: Vec<String>,
    pub when: When,
}

impl Step {
    pub fn new(args: &[&str]) -> Step {
        Step { args: args.iter().map(ToString::to_string).collect(), when: When::IfOk }
    }
}

/// A step's result: `None` when it was skipped (an earlier step failed), else its output
/// (stdout and stderr lines, in order) or its failure.
pub type Outcome = Option<Result<String>>;

/// What a batch reports while it runs.
#[derive(Debug)]
pub enum Event {
    /// A step starts.
    Start,
    /// An output line of step `i` (its order in the batch).
    Line(usize, String),
    /// A step ended (its outcome is in the batch's returned [`Outcome`]s).
    End,
}

impl Event {
    /// The event as a test reads it: `start`, `line <i> <text>`, `end`.
    #[cfg(test)]
    pub(super) fn describe(&self) -> String {
        match self {
            Event::Start => "start".into(),
            Event::Line(i, l) => format!("line {i} {l}"),
            Event::End => "end".into(),
        }
    }
}

/// The environment variable naming the Hyper-V virtual switch a VM's first adapter goes on (set
/// by the script that runs Vagrant on Hyper-V, see [`super::elevate`], unless the user set it):
/// with several switches and none named, Vagrant stops to ask which one. Isoloom's own name, so
/// one value serves both kinds of Vagrantfile: the lab's (Isoloom reads it, and stops `vagrant
/// up` when no switch has that name) and the launcher's own (attack VM, self-test).
pub const HYPERV_SWITCH_ENV: &str = crate::runtime::lab::HYPERV_SWITCH_ENV;

/// The Vagrantfile line attaching `target` (`config`, or a machine's variable) to that switch,
/// when it is set.
pub fn hyperv_switch_line(indent: &str, target: &str) -> String {
    format!("{indent}{target}.vm.network \"public_network\", bridge: ENV[\"{HYPERV_SWITCH_ENV}\"] if ENV[\"{HYPERV_SWITCH_ENV}\"]\n")
}

/// Whether the machines Vagrant tracks in `dir` were created on `provider` (a provider id).
pub fn machines_on(dir: &Path, provider: &str) -> bool {
    std::fs::read_dir(dir.join(".vagrant").join("machines")).is_ok_and(|it| it.flatten().any(|m| m.path().join(provider).is_dir()))
}

/// Whether the machines Vagrant tracks in `dir` run on Hyper-V.
pub fn on_hyperv(dir: &Path) -> bool {
    machines_on(dir, "hyperv")
}

/// Vagrant in one folder, with the environment its Vagrantfile reads.
pub struct Vagrant<'a> {
    dir: &'a Path,
    env: &'a [(String, String)],
    mode: Mode,
}

impl<'a> Vagrant<'a> {
    /// `hyperv`: the commands act on Hyper-V machines (they then run elevated where needed).
    pub fn new(dir: &'a Path, env: &'a [(String, String)], hyperv: bool) -> Vagrant<'a> {
        Vagrant { dir, env, mode: elevate::mode(hyperv) }
    }

    /// Vagrant for the machines already in `dir`, elevated when they run on Hyper-V.
    pub fn of(dir: &'a Path, env: &'a [(String, String)]) -> Vagrant<'a> {
        Vagrant::new(dir, env, on_hyperv(dir))
    }

    /// Runs one command, forwarding each output line to `on_line` as it arrives.
    pub async fn stream(&self, args: &[&str], mut on_line: impl FnMut(String)) -> Result<()> {
        if self.mode == Mode::Direct {
            return super::stream("vagrant", args, Some(self.dir), self.env, on_line).await;
        }
        let outcomes = self
            .batch(&[Step::new(args)], |e| {
                if let Event::Line(_, l) = e {
                    on_line(l);
                }
            })
            .await?;
        only(outcomes).map(|_| ())
    }

    /// Runs one command and returns its output (stdout only, except through the Hyper-V script,
    /// where both streams share one log).
    pub async fn run(&self, args: &[&str]) -> Result<String> {
        if self.mode == Mode::Direct {
            return super::run_env("vagrant", args, Some(self.dir), self.env).await;
        }
        only(self.batch(&[Step::new(args)], |_| {}).await?)
    }

    /// Runs `steps` in order (a step [`When::IfOk`] is skipped once one has failed), reporting
    /// each one's start, lines and end to `on_event`. Elevated, the whole batch runs under one
    /// UAC prompt. `Err` only when the batch couldn't run at all (the prompt was declined);
    /// each step's own failure is in its [`Outcome`].
    pub async fn batch(&self, steps: &[Step], on_event: impl FnMut(Event)) -> Result<Vec<Outcome>> {
        #[cfg(windows)]
        if let Mode::Script { runas } = self.mode {
            // Boxed: following the script's run is a large future, which every caller of a batch
            // (a lab's launch, the VM self-test) would carry otherwise.
            return Box::pin(elevate::batch(self.dir, self.env, steps, runas, on_event)).await;
        }
        Ok(self.batch_here(steps, on_event).await)
    }

    /// [`Vagrant::batch`] in this process, one tool run per step.
    async fn batch_here(&self, steps: &[Step], mut on_event: impl FnMut(Event)) -> Vec<Outcome> {
        let mut outcomes = Vec::with_capacity(steps.len());
        let mut failed = false;
        for (i, step) in steps.iter().enumerate() {
            if failed && step.when == When::IfOk {
                outcomes.push(None);
                continue;
            }
            on_event(Event::Start);
            let args: Vec<&str> = step.args.iter().map(String::as_str).collect();
            let mut text = String::new();
            let result = super::stream("vagrant", &args, Some(self.dir), self.env, |l: String| {
                text.push_str(&l);
                text.push('\n');
                on_event(Event::Line(i, l));
            })
            .await
            .map(|()| text);
            failed |= result.is_err();
            on_event(Event::End);
            outcomes.push(Some(result));
        }
        outcomes
    }
}

/// The result of a batch of one step.
fn only(outcomes: Vec<Outcome>) -> Result<String> {
    outcomes.into_iter().next().flatten().unwrap_or_else(|| Err(crate::error::Error::Invalid("the Vagrant command did not run".into())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::exec::fake::{Tools, fail, ok};

    fn dir() -> std::path::PathBuf {
        std::env::temp_dir()
    }

    #[tokio::test]
    async fn single_commands_answer_as_the_plain_tools_do() {
        // In tests (and anywhere but a non-elevated Windows on Hyper-V) nothing is elevated.
        let tools = Tools::rules(&[("vagrant ssh", ok("Linux\n")), ("vagrant destroy", fail("locked"))]);
        let d = dir();
        let v = Vagrant::new(&d, &[], true);
        assert_eq!(v.run(&["ssh", "-c", "uname"]).await.unwrap(), "Linux\n");
        let mut lines = Vec::new();
        let err = v.stream(&["destroy", "-f"], |l| lines.push(l)).await.unwrap_err();
        assert!(err.to_string().contains("locked"), "{err}");
        assert_eq!(lines, ["locked"]);
        assert_eq!(tools.calls(), ["vagrant ssh -c uname", "vagrant destroy -f"]);
    }

    #[test]
    fn hyperv_machines_are_recognised_by_their_folder() {
        let d = std::env::temp_dir().join(format!("cyberctf-hyperv-{}", rand::random::<u32>()));
        assert!(!on_hyperv(&d));
        std::fs::create_dir_all(d.join(".vagrant/machines/default/virtualbox")).unwrap();
        assert!(!on_hyperv(&d) && machines_on(&d, "virtualbox"));
        std::fs::create_dir_all(d.join(".vagrant/machines/dc01/hyperv")).unwrap();
        assert!(on_hyperv(&d));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_batch_of_one_that_did_not_run_is_an_error() {
        assert!(only(vec![None]).is_err());
        assert!(only(vec![]).is_err());
        assert_eq!(only(vec![Some(Ok("x".into()))]).unwrap(), "x");
    }
}
