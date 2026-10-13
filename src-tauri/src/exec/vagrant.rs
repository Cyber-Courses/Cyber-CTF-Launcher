//! Vagrant commands on a lab's machines, elevated when the hypervisor needs it.
//!
//! Hyper-V only takes management commands from an administrator, and Vagrant's Hyper-V provider
//! refuses to run otherwise (issue #185). On Windows, when the machines run on Hyper-V and the
//! app itself is not elevated, the commands go through [`super::elevate`]: one UAC prompt per
//! [`Vagrant::batch`], their output streamed as usual. Everywhere else (and on every other
//! provider) they run exactly as any other tool, through [`super::stream`] and [`super::run_env`].

use std::path::Path;

use super::elevate;
use crate::error::Result;

/// When a step of a [`Vagrant::batch`] runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum When {
    /// Only while every step before it succeeded.
    IfOk,
    /// Whatever happened before (a clean-up).
    Always,
}

/// One Vagrant command of a batch: its arguments and when it runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub args: Vec<String>,
    pub when: When,
}

impl Step {
    pub fn new(args: &[&str]) -> Step {
        Step { args: args.iter().map(|a| a.to_string()).collect(), when: When::IfOk }
    }

    /// A step that runs even after an earlier one failed.
    pub fn always(mut self) -> Step {
        self.when = When::Always;
        self
    }
}

/// A step's result: `None` when it was skipped (an earlier step failed), else its output
/// (stdout and stderr lines, in order) or its failure.
pub type Outcome = Option<Result<String>>;

/// What a batch reports while it runs.
#[derive(Debug)]
pub enum Event<'r> {
    /// Step `i` starts.
    Start(usize),
    /// An output line of step `i`.
    Line(usize, String),
    /// Step `i` ended.
    End(usize, &'r Result<String>),
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
    elevate: bool,
}

impl<'a> Vagrant<'a> {
    /// `hyperv`: the commands act on Hyper-V machines (they then run elevated where needed).
    pub fn new(dir: &'a Path, env: &'a [(String, String)], hyperv: bool) -> Vagrant<'a> {
        Vagrant { dir, env, elevate: elevate::needed(hyperv) }
    }

    /// Vagrant for the machines already in `dir`, elevated when they run on Hyper-V.
    pub fn of(dir: &'a Path, env: &'a [(String, String)]) -> Vagrant<'a> {
        Vagrant::new(dir, env, on_hyperv(dir))
    }

    /// Runs one command, forwarding each output line to `on_line` as it arrives.
    pub async fn stream(&self, args: &[&str], mut on_line: impl FnMut(String)) -> Result<()> {
        if !self.elevate {
            return super::stream("vagrant", args, Some(self.dir), self.env, on_line).await;
        }
        let outcomes = self
            .batch(&[Step::new(args)], |e| {
                if let Event::Line(_, l) = e {
                    on_line(l)
                }
            })
            .await?;
        only(outcomes).map(|_| ())
    }

    /// Runs one command and returns its output (stdout only when not elevated; both streams
    /// when elevated, where they share one log).
    pub async fn run(&self, args: &[&str]) -> Result<String> {
        if !self.elevate {
            return super::run_env("vagrant", args, Some(self.dir), self.env).await;
        }
        only(self.batch(&[Step::new(args)], |_| {}).await?)
    }

    /// Runs `steps` in order (a step [`When::IfOk`] is skipped once one has failed), reporting
    /// each one's start, lines and end to `on_event`. Elevated, the whole batch runs under one
    /// UAC prompt. `Err` only when the batch couldn't run at all (the prompt was declined);
    /// each step's own failure is in its [`Outcome`].
    pub async fn batch(&self, steps: &[Step], on_event: impl FnMut(Event<'_>)) -> Result<Vec<Outcome>> {
        #[cfg(windows)]
        if self.elevate {
            return elevate::batch(self.dir, self.env, steps, on_event).await;
        }
        Ok(self.batch_here(steps, on_event).await)
    }

    /// [`Vagrant::batch`] in this process, one tool run per step.
    async fn batch_here(&self, steps: &[Step], mut on_event: impl FnMut(Event<'_>)) -> Vec<Outcome> {
        let mut outcomes = Vec::with_capacity(steps.len());
        let mut failed = false;
        for (i, step) in steps.iter().enumerate() {
            if failed && step.when == When::IfOk {
                outcomes.push(None);
                continue;
            }
            on_event(Event::Start(i));
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
            on_event(Event::End(i, &result));
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
    use crate::exec::fake::{Tools, fail, ok};

    fn dir() -> std::path::PathBuf {
        std::env::temp_dir()
    }

    #[tokio::test]
    async fn a_batch_skips_what_follows_a_failure_except_clean_ups() {
        let tools = Tools::rules(&[("vagrant up", fail("boom")), ("vagrant provision", ok("p1\np2\n"))]);
        let d = dir();
        let v = Vagrant::new(&d, &[], true);
        let steps = [Step::new(&["provision"]), Step::new(&["up"]), Step::new(&["reload"]), Step::new(&["halt", "x"]).always()];
        let mut events = Vec::new();
        let outcomes = v
            .batch(&steps, |e| {
                events.push(match e {
                    Event::Start(i) => format!("start {i}"),
                    Event::Line(i, l) => format!("line {i} {l}"),
                    Event::End(i, r) => format!("end {i} {}", r.is_ok()),
                })
            })
            .await
            .unwrap();
        assert_eq!(tools.calls(), ["vagrant provision", "vagrant up", "vagrant halt x"]);
        assert!(matches!(&outcomes[0], Some(Ok(out)) if out == "p1\np2\n"));
        assert!(matches!(&outcomes[1], Some(Err(e)) if e.to_string().contains("boom")));
        assert!(outcomes[2].is_none(), "skipped after the failure");
        assert!(matches!(&outcomes[3], Some(Ok(_))), "a clean-up runs anyway");
        assert_eq!(events, ["start 0", "line 0 p1", "line 0 p2", "end 0 true", "start 1", "line 1 boom", "end 1 false", "start 3", "end 3 true"]);
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
