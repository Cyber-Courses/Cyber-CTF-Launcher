//! Real cloud labs through the launcher's own Terraform cloud path, end to end, on a real AWS
//! account. This is the path [`launch::start_terraform`] takes for a cloud host: generate the
//! lab's Isoloom module for the cloud (`lab::prepare`), the launcher's run variables for it
//! (`lab::run_vars`, the firewall locked to this machine), `terraform::apply` (which also waits
//! on the lab host's `ready_file` over SSH), the status read from the state, then
//! `terraform::destroy` — run whatever happened, so nothing is left billing. The caller proves
//! zero AWS leftovers with the AWS CLI afterwards (the resources carry `managed-by=isoloom`).
//!
//! The apply's ready-wait is bounded here (`READY_BUDGET`), because the launcher's cloud path
//! applies the module but never delivers the lab's project over SSH (only the Isothrum runner,
//! from the module's `run.json`, does that), so the lab host can never report `ready`: the wait
//! would otherwise run to the launcher's own 40-minute timeout. The teardown runs regardless.
//!
//! Opt-in (it needs a real AWS account — ambient default-profile credentials — and the lab):
//!
//! ```text
//! CYBERCTF_TEST_CLOUD_LAB=<isoloom checkout>/examples/solo-web AWS_REGION=us-east-1 \
//! cargo test real_cloud_labs -- --ignored --nocapture --test-threads=1
//! ```

use std::path::{Path, PathBuf};
use std::time::Duration;

use super::{Runtime, lab, ssh, terraform};

/// How long the apply's ready-wait may run before this test abandons it (the teardown still
/// runs). Enough to see the lab host boot and the launcher start polling it over SSH.
const READY_BUDGET: Duration = Duration::from_secs(9 * 60);

/// The region every cloud run here uses (the account is kept empty elsewhere; smallest sizes).
const REGION: &str = "us-east-1";

fn region() -> String {
    std::env::var("AWS_REGION").ok().filter(|r| !r.is_empty()).unwrap_or_else(|| REGION.to_owned())
}

/// A fresh copy of the lab in `CYBERCTF_TEST_CLOUD_LAB`, without its generated files.
fn copy_lab(name: &str) -> PathBuf {
    let src = PathBuf::from(std::env::var("CYBERCTF_TEST_CLOUD_LAB").expect("set CYBERCTF_TEST_CLOUD_LAB to the lab's checkout"));
    let dir = std::env::temp_dir().join(name);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(std::process::Command::new("cp").arg("-R").arg(&src).arg(&dir).status().unwrap().success(), "copy the lab");
    let _ = std::fs::remove_dir_all(dir.join(".isoloom"));
    dir
}

/// The launcher's SSH key, generated once in `home` and registered as the launcher's (what
/// `terraform::apply`/`destroy` read through `ssh::launcher_key`).
fn launcher_key(home: &Path) -> (PathBuf, String) {
    std::fs::create_dir_all(home).unwrap();
    let key = home.join("id_ed25519");
    if !key.is_file() {
        let path = key.to_string_lossy().to_string();
        assert!(std::process::Command::new("ssh-keygen").args(["-q", "-t", "ed25519", "-N", "", "-f", &path]).status().unwrap().success(), "ssh-keygen");
    }
    ssh::set_launcher_key_for_test(key.clone());
    let public = std::fs::read_to_string(key.with_extension("pub")).unwrap().trim().to_owned();
    (key, public)
}

/// This machine's public IPv4, for the lab firewall (the launcher fetches it the same way).
async fn public_ip() -> String {
    let body = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap()
        .get("https://checkip.amazonaws.com")
        .send()
        .await
        .expect("fetch public IP")
        .text()
        .await
        .expect("read public IP");
    body.trim().to_owned()
}

/// The launcher's host variables for an AWS cloud host (what `lab::host_vars` builds from a
/// saved AWS profile): where, the auto-stop the reaper reads, and the Docker VM's size.
fn aws_host_vars(docker: bool) -> Vec<(String, String)> {
    let mut vars = vec![("region".to_owned(), region()), ("auto_stop_hours".to_owned(), "1".to_owned()), ("auto_stop_minutes".to_owned(), "60".to_owned())];
    if docker {
        // cloud-docker takes an `instance_type`; keep it the smallest that runs Docker.
        vars.push(("instance_type".to_owned(), "t3.micro".to_owned()));
    }
    vars
}

/// One cloud target through the launcher's own path. Returns the lab's instance name prefix (for
/// the caller's AWS-CLI leftover check) and whether the lab host ever reported ready.
async fn cloud_through_launcher(name: &str, runtime: Runtime, docker: bool) {
    let dir = copy_lab(name);
    let home = std::env::temp_dir().join(format!("{name}-home"));
    let (key, public) = launcher_key(&home);
    let (module, target) = lab::terraform(&dir, runtime, "aws").expect("resolve the aws module");
    let prepared = lab::prepare(&dir, target).expect("generate the aws module");
    let ip = public_ip().await;
    let inputs = lab::inputs_json(&prepared.spec, &[]);
    let vars = lab::run_vars(&aws_host_vars(docker), inputs, false, public, &key, Some(&ip));
    let env = vec![("AWS_REGION".to_owned(), region()), ("AWS_DEFAULT_REGION".to_owned(), region())];
    let state = home.join("state");
    std::fs::create_dir_all(&state).unwrap();
    super::paths::record_runtime(&state, runtime).ok();

    let mut log = |l: String| println!("[{name}] {l}");
    println!("[{name}] applying the {} aws module (firewall to {ip}/32)…", if docker { "cloud-docker" } else { "cloud-vm" });
    let applied = tokio::time::timeout(READY_BUDGET, terraform::apply(&module, &state, &vars, &env, &mut log)).await;
    match &applied {
        Ok(Ok(())) => println!("[{name}] apply returned ok (lab host confirmed, or SSH gave up)"),
        Ok(Err(e)) => println!("[{name}] apply returned error: {e}"),
        Err(_) => println!("[{name}] apply did NOT return within {}s — the ready-wait is stuck (no project delivered; see report)", READY_BUDGET.as_secs()),
    }
    let status = terraform::status(&state);
    println!("[{name}] status: running={} machines={:?}", status.running, status.machines.iter().map(|m| (&m.name, &m.state, &m.ip)).collect::<Vec<_>>());
    let host_ip = status.machines.first().map(|m| m.ip.clone());
    if let Some(hip) = &host_ip {
        reachability(name, hip).await;
    }

    // Teardown, whatever happened above (the Stop button's path).
    println!("[{name}] destroying…");
    let to_destroy = lab::terraform_to_destroy(&dir, runtime, "aws").expect("the destroy module");
    let destroyed = terraform::destroy(&to_destroy, &state, &aws_host_vars(docker), &env, &mut log).await;
    destroyed.expect("destroy");
    assert!(!terraform::deployed(&state), "the state should hold nothing after destroy");
    println!("[{name}] destroyed; state clear. Verify zero AWS leftovers with the AWS CLI (tag managed-by=isoloom).");
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&home);
}

/// Whether the lab's published port answers from this machine (the firewall is open to it). A
/// best-effort probe; its result is logged, not asserted (the lab may never have come up). Both
/// cloud targets publish solo-web's web port on 8080.
async fn reachability(name: &str, ip: &str) {
    let url = format!("http://{ip}:8080/");
    let got = reqwest::Client::builder().timeout(Duration::from_secs(15)).build().unwrap().get(&url).send().await;
    println!("[{name}] reachability {url}: {:?}", got.map(|r| r.status()));
}

#[tokio::test]
#[ignore = "needs a real AWS account and CYBERCTF_TEST_CLOUD_LAB; see the module docs"]
async fn real_cloud_labs_cloud_vm_aws() {
    cloud_through_launcher("ci-cloud-vm-aws", Runtime::Vm, false).await;
}

#[tokio::test]
#[ignore = "needs a real AWS account and CYBERCTF_TEST_CLOUD_LAB; see the module docs"]
async fn real_cloud_labs_cloud_docker_aws() {
    cloud_through_launcher("ci-cloud-docker-aws", Runtime::Docker, true).await;
}
