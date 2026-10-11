//! The Proxmox self-test: a throwaway VM from a minimal Terraform module (same provider and
//! image as a real lab), booted, probed over SSH, destroyed.

use std::path::Path;
use std::time::Duration;

use crate::AppHandle;

use super::{BOOT, CLEANUP, CORES, DISK_GB, MEMORY_MB, PREPARE, Ran, Reporter, wait_tcp, wait_tcp_from};
use crate::error::{Error, Result};
use crate::runtime::{server, ssh, terraform};

const MAIN_TF: &str = include_str!("templates/main.tf");
const VARIABLES_TF: &str = include_str!("templates/variables.tf");
const OUTPUTS_TF: &str = include_str!("templates/outputs.tf");
/// Lock file for the bpg/proxmox provider, hashed for every platform the launcher runs on
/// (regenerate with `terraform providers lock -platform=linux_amd64 -platform=linux_arm64
/// -platform=darwin_amd64 -platform=darwin_arm64 -platform=windows_amd64` when bumping it).
const LOCK_HCL: &str = include_str!("templates/terraform.lock.hcl");

/// The module's folder name under the work dir (its `proxmox` component routes it to the
/// Terraform container on macOS, like a lab's Proxmox module).
const TARGET: &str = "proxmox-selftest";

fn module_dir(work: &Path) -> std::path::PathBuf {
    work.join("terraform").join(TARGET)
}

fn state_dir(work: &Path) -> std::path::PathBuf {
    work.join("state")
}

/// Destroys the VM a previous run left behind (its cleanup failed), from that run's state.
pub(super) async fn leftover(r: &Reporter, conn: &server::Connection, work: &Path) -> Result<()> {
    let module = module_dir(work);
    write_module(&module)?;
    terraform::destroy(&module, &state_dir(work), &conn.tf_vars, &conn.tf_env, |l| r.progress("prepare", PREPARE, l)).await
}

pub(super) async fn run(app: &AppHandle, id: &str, r: &Reporter, conn: &server::Connection, work: &Path) -> Ran {
    let (key, pubkey) = match ssh::ensure_key(app).await {
        Ok(k) => k,
        Err(e) => return Ran { result: Err(e), cleaned: true },
    };
    // A host-internal lab bridge: the VM is only reachable from the node, so probe from there.
    let jump = match server::proxmox_jump_for_host(app, id, &key, &pubkey).await {
        Ok(j) => j,
        Err(e) => return Ran { result: Err(e), cleaned: true },
    };
    let module = module_dir(work);
    let state = state_dir(work);

    let prepared = r
        .step("prepare", PREPARE, async {
            write_module(&module)?;
            Ok(((), None))
        })
        .await;
    if let Err(e) = prepared {
        return Ran { result: Err(e), cleaned: true };
    }

    let vars = test_vars(&conn.tf_vars, pubkey);

    // `apply` creates the VM and, because the agent is enabled, blocks until it boots and
    // reports an address. Terraform's own output streams as the step's detail.
    let apply = r
        .step("apply", BOOT, async {
            terraform::apply(&module, &state, &vars, &conn.tf_env, |l| r.progress("apply", BOOT, l)).await?;
            Ok(((), Some("VM created".into())))
        })
        .await;

    // Whatever happened, tear down anything that was created before returning.
    if apply.is_err() {
        let cleaned = destroy(r, conn, &module, &state).await.is_ok();
        return Ran { result: apply, cleaned };
    }

    let ip = r
        .step("boot", "VM reports a network address", async {
            match terraform::ssh_endpoint(&state) {
                Some((ip, _)) => Ok((ip.clone(), Some(ip))),
                None => Err(Error::Invalid("the VM didn't report an address (guest agent didn't answer)".into())),
            }
        })
        .await;

    if let Ok(ip) = ip {
        let _ = r
            .step("ssh", "SSH answers on the VM", async {
                match &jump {
                    None => wait_tcp(&ip, 22, Duration::from_secs(90)).await?,
                    Some(login) => wait_tcp_from(login, &key, &ip, 22, Duration::from_secs(90)).await?,
                }
                Ok(((), Some(ssh_detail(&ip, jump.is_some()))))
            })
            .await;
    }

    // Always clean up, and report it as its own step.
    let cleaned = destroy(r, conn, &module, &state).await.is_ok();
    Ran { result: Ok(()), cleaned }
}

async fn destroy(r: &Reporter, conn: &server::Connection, module: &Path, state: &Path) -> Result<()> {
    r.step("cleanup", CLEANUP, async {
        terraform::destroy(module, state, &conn.tf_vars, &conn.tf_env, |l| r.progress("cleanup", CLEANUP, l)).await?;
        Ok(((), Some("removed".into())))
    })
    .await
}

/// The host's Terraform variables plus the test VM's: the launcher's key and a small size.
fn test_vars(host_vars: &[(String, String)], pubkey: String) -> Vec<(String, String)> {
    let mut vars = host_vars.to_vec();
    vars.push(("ssh_public_key".into(), pubkey));
    vars.push(("cores".into(), CORES.into()));
    vars.push(("memory_mb".into(), MEMORY_MB.into()));
    vars.push(("disk_gb".into(), DISK_GB.into()));
    vars
}

/// The SSH step's detail: the VM's SSH port answered, directly or from the node.
fn ssh_detail(ip: &str, through_node: bool) -> String {
    if through_node { format!("{ip}:22 open (through the node)") } else { format!("{ip}:22 open") }
}

/// Writes the minimal, self-contained Terraform module (no lab fetch) to `dir`.
fn write_module(dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    std::fs::write(dir.join("main.tf"), MAIN_TF)?;
    std::fs::write(dir.join("variables.tf"), VARIABLES_TF)?;
    std::fs::write(dir.join("outputs.tf"), OUTPUTS_TF)?;
    // Ship a dependency lock so `terraform init` reuses the pinned provider versions/hashes
    // (reproducible, and no surprise upgrade) rather than re-resolving them. Covers every
    // platform the launcher runs on.
    std::fs::write(dir.join(".terraform.lock.hcl"), LOCK_HCL)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_module_declares_every_variable_the_test_passes() {
        for var in ["proxmox_endpoint", "proxmox_api_token", "proxmox_ssh_address", "node", "uplink_bridge", "ssh_public_key", "cores", "memory_mb", "disk_gb"]
        {
            assert!(VARIABLES_TF.contains(&format!("variable \"{var}\"")), "{var}");
        }
        assert!(OUTPUTS_TF.contains("output \"ip\""));
        assert!(LOCK_HCL.contains("registry.terraform.io/bpg/proxmox"));
    }

    #[test]
    fn the_test_vm_is_small_and_takes_the_launchers_key() {
        let vars = test_vars(&[("node".into(), "pve".into())], "ssh-ed25519 AAAA".into());
        let get = |k: &str| vars.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
        assert_eq!(get("node"), Some("pve"));
        assert_eq!(get("ssh_public_key"), Some("ssh-ed25519 AAAA"));
        assert_eq!((get("cores"), get("memory_mb"), get("disk_gb")), (Some(CORES), Some(MEMORY_MB), Some(DISK_GB)));
        assert_eq!(ssh_detail("10.0.0.9", false), "10.0.0.9:22 open");
        assert_eq!(ssh_detail("10.0.0.9", true), "10.0.0.9:22 open (through the node)");
    }

    #[test]
    fn write_module_lays_out_the_files() {
        let dir = std::env::temp_dir().join(format!("cyberctf-selftest-{}", rand::random::<u32>())).join(TARGET);
        write_module(&dir).unwrap();
        for f in ["main.tf", "variables.tf", "outputs.tf", ".terraform.lock.hcl"] {
            assert!(dir.join(f).is_file(), "{f}");
        }
        assert_eq!(std::fs::read_to_string(dir.join("main.tf")).unwrap(), MAIN_TF);
        std::fs::remove_dir_all(dir.parent().unwrap()).unwrap();
    }
}
