//! Reading Docker here: whether it answers at all.
//! Where a running lab is reachable comes from the runner's status (`isothrum::web`).

/// Whether this machine's Docker answers: its daemon names its OS to this account
/// (isothrum-machine's [`daemon_os`](isothrum_machine::docker::daemon_os)). A Docker that isn't
/// installed or isn't running doesn't.
pub async fn answers() -> bool {
    isothrum_machine::docker::daemon_os(&crate::runtime::providers::machine_check()).await.is_ok()
}
