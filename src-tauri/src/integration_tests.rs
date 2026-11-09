//! Real labs, end to end, on the real tools: Docker, local hypervisors, cloud accounts and
//! servers, driven through the launcher's own code. Every test here is opt-in (`#[ignore]`): it
//! reads its knobs from `CYBERCTF_TEST_*` and returns early without them. `scripts/integration-tests.sh`
//! runs them by tier; docs/testing/integration.md says what each one guards against.

mod attack_box;
mod commands;
mod corpus;
mod docker;
mod docker_vm;
mod flag_contract;
mod per_machine;
mod servers;
pub(crate) mod support;
mod vm;
