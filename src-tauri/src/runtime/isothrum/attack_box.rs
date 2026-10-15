//! The attack box beside a container lab, as the runner's hooks: the player's box (started from
//! the lab page, on a network of its own) rejoins the lab's networks once the lab is up or
//! resumed, stops when the lab parks (to start again with it), and goes before the lab does, so
//! nothing holds the lab's networks when they are removed. Each is best effort, as on the
//! launcher's own Docker path: a lab never fails because of its attack box.

use isothrum_runner::{BoxFuture, HookError, Hooks, LabView};

use super::super::exegol;

/// The attack box of one lab (its id names the box and its network).
pub(super) struct AttackBox {
    lab: String,
}

impl AttackBox {
    /// Lab `id`'s attack box.
    pub(super) fn new(id: &str) -> AttackBox {
        AttackBox { lab: id.to_owned() }
    }
}

impl Hooks for AttackBox {
    fn after_up<'a>(&'a self, _lab: &'a LabView<'a>) -> BoxFuture<'a, Result<(), HookError>> {
        Box::pin(async move {
            exegol::rejoin(&self.lab).await;
            Ok(())
        })
    }

    fn before_down<'a>(&'a self, _lab: &'a LabView<'a>) -> BoxFuture<'a, Result<(), HookError>> {
        Box::pin(async move { exegol::stop(&self.lab, |_| {}).await.map_err(|e| HookError::new(e.to_string())) })
    }

    fn before_park<'a>(&'a self, _lab: &'a LabView<'a>) -> BoxFuture<'a, Result<(), HookError>> {
        Box::pin(async move {
            exegol::park(&self.lab).await;
            Ok(())
        })
    }

    fn after_resume<'a>(&'a self, _lab: &'a LabView<'a>) -> BoxFuture<'a, Result<(), HookError>> {
        Box::pin(async move {
            exegol::wake(&self.lab).await;
            Ok(())
        })
    }
}
