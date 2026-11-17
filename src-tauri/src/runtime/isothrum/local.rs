//! What only a run on this machine or on an ESXi server is asked: its page (parked included, as
//! its record says), where it is reachable here, and the hypervisor it runs on.

use super::deployment::Deployment;
use super::{Result, view, web};
use crate::runtime::{LabStatus, Place};

impl Deployment {
    /// The lab as its page shows it, here or on ESXi: its containers or its VMs, parked as its
    /// record says; not running before its files were ever generated.
    pub(super) async fn local_status(&self) -> Result<LabStatus> {
        let place = if self.suspends() { Place::LocalVm } else { Place::Container };
        if !self.files_back().await? {
            return Ok(LabStatus { place: Some(place), ..LabStatus::default() });
        }
        let status = self.read().await?;
        let parked = self.recorded().await?.as_ref().and_then(crate::runtime::recorded::parked);
        Ok(LabStatus { place: Some(place), ..view::lab_status(&self.described()?, &status, parked) })
    }

    /// The provider the run's record says it runs on, none before it ever ran: no tool runs.
    pub(crate) async fn recorded_provider(&self) -> Result<Option<String>> {
        Ok(self.recorded().await?.and_then(|r| r.provider).map(|p| isothrum_runner::wire_name(&p)))
    }

    /// Where the lab is reachable on this computer while it runs ([`web::local_url`]): none before
    /// its files exist, or while nothing of it is published.
    pub(crate) async fn local_url(&self) -> Result<Option<String>> {
        let Some(status) = self.run_status().await? else { return Ok(None) };
        Ok(web::local_url(&self.described()?, &status))
    }
}
