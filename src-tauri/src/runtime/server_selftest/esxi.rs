//! The ESXi self-test: a throwaway VM from a minimal Vagrantfile (`vmware_esxi`), a command run
//! in it over SSH, then `vagrant destroy`.

use std::path::Path;

use super::{BOOT, CLEANUP, PREPARE, Ran, Reporter};
use crate::error::{Error, Result};
use crate::runtime::providers::Provider;
use crate::runtime::{server, vm};

/// Minimal throwaway VM (no lab provisioning), configured from the connection env.
const VAGRANTFILE: &str = include_str!("templates/Vagrantfile");

fn write_vagrantfile(work: &Path) -> Result<()> {
    std::fs::create_dir_all(work)?;
    std::fs::write(work.join("Vagrantfile"), VAGRANTFILE)?;
    Ok(())
}

/// Destroys the VM a previous run left behind (its cleanup failed), from that run's folder.
pub(super) async fn leftover(r: &Reporter, conn: &server::Connection, work: &Path) -> Result<()> {
    write_vagrantfile(work)?;
    vm::stop(work, &conn.env, |l| r.progress("prepare", PREPARE, l)).await
}

pub(super) async fn run(r: &Reporter, conn: &server::Connection, work: &Path) -> Ran {
    let prepared = r
        .step("prepare", PREPARE, async {
            write_vagrantfile(work)?;
            Ok(((), None))
        })
        .await;
    if let Err(e) = prepared {
        return Ran { result: Err(e), cleaned: true };
    }

    let up = r
        .step("up", BOOT, async {
            vm::start(work, Provider::VmwareEsxi, &conn.env, |l| r.progress("up", BOOT, l)).await?;
            Ok(((), Some("VM up".into())))
        })
        .await;

    if up.is_err() {
        let cleaned = destroy(r, conn, work).await.is_ok();
        return Ran { result: up, cleaned };
    }

    let _ = r
        .step("ssh", "Run a command in the VM", async {
            let out = crate::exec::run_env("vagrant", &["ssh", "-c", "echo cyberctf-ok"], Some(work), &conn.env).await?;
            if out.contains("cyberctf-ok") {
                Ok(((), Some("command ran in the VM".into())))
            } else {
                Err(Error::Invalid("the VM didn't answer over SSH".into()))
            }
        })
        .await;

    let cleaned = destroy(r, conn, work).await.is_ok();
    Ran { result: Ok(()), cleaned }
}

async fn destroy(r: &Reporter, conn: &server::Connection, work: &Path) -> Result<()> {
    r.step("cleanup", CLEANUP, async {
        vm::stop(work, &conn.env, |l| r.progress("cleanup", CLEANUP, l)).await?;
        Ok(((), Some("removed".into())))
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_vagrantfile_reads_the_connection_contract() {
        // The names `server::connection_env` sets for an ESXi host.
        for env in ["CYBERCTF_ESXI_HOSTNAME", "CYBERCTF_ESXI_HOSTPORT", "CYBERCTF_ESXI_USERNAME", "env:CYBERCTF_ESXI_PASSWORD", "ISOLOOM_SSH_PROXY_COMMAND"] {
            assert!(VAGRANTFILE.contains(env), "{env}");
        }
    }
}
