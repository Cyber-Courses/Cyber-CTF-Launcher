//! What the tools the launcher runs print, read in one place per tool, so no caller matches a
//! tool's words where it acts on them: each reader returns a typed value (an enum with an explicit
//! `Other`, an `Option`), and each wording it knows is backed by a fixture of the text as a named
//! version of the tool prints it (`tool_output/fixtures/<tool>/<version>-<case>`). The tools are
//! spawned with their messages in the C locale and their output decoded from the code page they
//! wrote it in ([`text`]), so what they say is in the bytes and words these readers know.
//!
//! The Docker CLI's failures are read by [`super::exec::DockerSaid`], Terraform's state by
//! `runtime::terraform::state` and Proxmox's API by `runtime::proxmox::api`.

pub mod apple_events;
pub mod brew;
pub mod gcloud;
pub mod ping;
pub mod pkexec;
pub mod text;
pub mod vboxmanage;
pub mod win32;
