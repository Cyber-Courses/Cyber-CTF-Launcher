//! A start and an update of a run, tried once more after UTM's passing scripting refusal
//! ([`errors::try_again_after_utm`]), from where the failed try left the run.

use super::super::paths;
use super::{Error, Inputs, LabRun, Result, errors, remote, view};

impl LabRun {
    /// Brings the running lab up to date with its regenerated files (a machine added to the lab
    /// or taken out), with the parameters it was started with. A VM restarted for its new
    /// settings boots like a started one, into the same passing UTM refusal: see
    /// [`LabRun::after_utm_refusal`].
    pub(crate) async fn update(&self, log: &mut impl FnMut(String)) -> Result<()> {
        // Boxed: each try is a whole run's operation, a large future.
        match Box::pin(self.attempt(Op::Update, log)).await {
            Err(e) => Box::pin(self.after_utm_refusal(Op::Update, e, log)).await,
            done => done,
        }
    }

    /// [`LabRun::up`] on `provider`; on UTM, a start its passing scripting refusal cut short goes
    /// on as [`LabRun::after_utm_refusal`] says.
    pub(crate) async fn start(&self, inputs: Inputs<'_>, provider: &str, log: &mut impl FnMut(String)) -> Result<()> {
        let op = Op::Up { inputs, provider };
        match Box::pin(self.attempt(op, log)).await {
            Err(e) if provider == super::super::providers::Provider::Utm.id() => Box::pin(self.after_utm_refusal(op, e, log)).await,
            done => done,
        }
    }

    /// `op`, once.
    async fn attempt(&self, op: Op<'_>, log: &mut impl FnMut(String)) -> Result<()> {
        match op {
            Op::Up { inputs, provider } => self.up(inputs, Some(provider), log).await,
            Op::Update => {
                self.require_up()?;
                self.act_placed(remote::Act::Update, log).await
            }
        }
    }

    /// What follows `op` that failed with `failed`: when that is UTM's passing scripting refusal
    /// ([`errors::try_again_after_utm`], said in `log`), one more try, else the error. The try is
    /// `op` again while the run still runs; when the failed attempt left it down (a restart for
    /// new settings shut the VM down before UTM refused its boot), an update would be refused
    /// ("isn't running"), so the try is an `up` on the current files, on the run's provider, which
    /// brings the run back with what `op` was adding.
    async fn after_utm_refusal(&self, op: Op<'_>, failed: Error, log: &mut impl FnMut(String)) -> Result<()> {
        if !errors::try_again_after_utm(&failed, log) {
            return Err(failed);
        }
        let Some(provider) = self.left_down().await else { return self.attempt(op, log).await };
        log("The run isn't running after that: starting it again on its current files.".into());
        match op {
            Op::Up { inputs, provider } => self.up(inputs, Some(provider), log).await,
            // An update carries no launch's values, and neither does the start that replaces it;
            // a run whose status names no provider is on the one its lab folder records.
            Op::Update => self.up(Inputs::default(), provider.or_else(|| paths::local_vm(&self.dir)).as_deref(), log).await,
        }
    }

    /// The run's provider when the run is down (nothing of it running), as its status says; none
    /// when it runs. A status that can't be read is logged, and taken for a run that runs.
    async fn left_down(&self) -> Option<Option<String>> {
        match self.run_status().await {
            Ok(Some(status)) if !view::runs(&status) => Some(status.provider.map(|p| isothrum_runner::wire_name(&p))),
            Ok(_) => None,
            Err(e) => {
                log::warn!("couldn't read the run's status after UTM's refusal ({e}): trying the same operation again");
                None
            }
        }
    }
}

/// An operation on a run that UTM's passing refusal may cut short ([`LabRun::after_utm_refusal`]).
#[derive(Clone, Copy)]
enum Op<'a> {
    /// A start with the launch's `inputs`, on `provider`.
    Up { inputs: Inputs<'a>, provider: &'a str },
    /// Bringing the running run up to date with its files.
    Update,
}
