//! What the tools the launcher runs print, read in one place per tool, so no caller matches a
//! tool's words where it acts on them: each reader returns a typed value (an enum with an explicit
//! `Other`, an `Option`), and each wording it knows is backed by a fixture of the text as a named
//! version of the tool prints it (`tool_output/fixtures/<tool>/<version>-<case>`). Isothrum's
//! runner starts the tools with their messages in the C locale, so what they say is in the words
//! these readers know.
//!
//! The Docker CLI's failures are read by isothrum-machine (`docker::says_denied`), Terraform's state by
//! `runtime::terraform::state` and Proxmox's API by `runtime::proxmox::api`.

pub mod apple_events;
