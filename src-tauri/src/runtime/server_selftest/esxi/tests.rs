use super::super::lab_run::scripted::*;
use super::*;

/// A host's connection with its settings `env` (`lab::esxi_env`'s names).
fn conn(env: &[(&str, &str)]) -> server::Connection {
    server::Connection {
        id: "esxi1".into(),
        provider: Provider::VmwareEsxi,
        name: "ESXi".into(),
        env: env.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect(),
        tf_vars: Vec::new(),
        tf_env: Vec::new(),
        region: None,
        auto_stop_hours: None,
    }
}

/// The course on a host with datastore `ssd1` and port group `Lab Net`, no box store.
/// The test VM's name in these tests.
const TEST_VM: &str = "cyberctf-server-selftest-k3x9a2-probe";

fn esxi() -> Esxi {
    Esxi::new(&conn(&[(DATASTORE, "ssd1"), (VIRTUAL_NETWORK, "Lab Net, Other")]), None, TEST_VM.into())
}

/// Vagrant's and the plugin's lines of a run that went all the way, the box downloaded.
const ALL_THE_WAY: [&str; 13] = [
    "Building the environment's VMs (Windows machines take a while)…",
    "Bringing machine 'probe' up with 'vmware_esxi' provider...",
    "==> probe: Adding box 'bento/debian-12' (v202510.26.0) for provider: vmware_desktop (amd64)",
    "    probe: Downloading: https://vagrantcloud.com/bento/boxes/debian-12/versions/202510.26.0/providers/vmware_desktop/amd64/vagrant.box",
    "==> probe: --- Box             : bento/debian-12",
    "==> probe: --- Box Ver         : 202510.26.0",
    "Opening VMX source: /tmp/probe.vmx",
    "Transfer Completed",
    "==> probe: --- VMID            : 12",
    "==> probe: --- VM has been Powered On...",
    "==> probe: --- Waiting for state \"running\"",
    "==> probe: --- Success, state is now \"running\"",
    "==> probe: Running provisioner: project...",
];

#[tokio::test]
async fn a_run_that_works_reports_every_step_of_the_plan_and_tears_down() {
    let (r, seen) = reporter();
    let run = Scripted::new(&ALL_THE_WAY, None);
    let ran = lab_run::test(&r, &run, esxi()).await;
    assert!(ran.result.is_ok() && ran.cleaned);
    assert_eq!(ends(&seen), ["image:ok", "vm:ok", "address:ok", "ssh:ok", "command:ok", "cleanup:ok"]);
    // The steps the plan lists after the connection and the preparation, in its order.
    assert_eq!(ends(&seen), PLAN[2..].iter().map(|s| format!("{s}:ok")).collect::<Vec<_>>());
    assert_eq!(run.calls(), ["up", "check", "down"]);
    // The image step names the box and its architecture (a wrong one would show), and where.
    assert_eq!(detail(&seen, "image", "ok"), "Box bento/debian-12 v202510.26.0 (amd64), uploaded to datastore ssd1");
    assert!(showed(&seen, "image", "Transfer Completed"), "the upload's lines show under its step");
    assert_eq!(detail(&seen, "address", "ok"), "On port group Lab Net");
    assert!(showed(&seen, "ssh", "Running provisioner"), "the provisioning over SSH shows under the SSH step");
    assert_eq!(detail(&seen, "command", "ok"), "Linux x86_64");
}

#[tokio::test]
async fn a_failure_mid_way_fails_its_step_in_plain_words_and_still_cleans_up() {
    let (r, seen) = reporter();
    let raw = "There was an error talking to ESXi.\n  Failed, timeout waiting for \"running\". (exited with code 1)";
    let run = Scripted::new(&ALL_THE_WAY[..10], Some(raw));
    let ran = lab_run::test(&r, &run, esxi()).await;
    let err = ran.result.unwrap_err().to_string();
    assert!(err.starts_with("The test VM got no address on port group Lab Net within 4 minutes"), "{err}");
    assert!(ran.cleaned);
    assert_eq!(ends(&seen), ["image:ok", "vm:ok", "address:fail", "cleanup:ok"]);
    assert_eq!(run.calls(), ["up", "down"], "no command after a failed start, and always the teardown");
}

#[tokio::test]
async fn a_failed_upload_says_what_ovftool_said_and_a_failed_teardown_what_is_left() {
    let (r, seen) = reporter();
    let mut run =
        Scripted::new(&ALL_THE_WAY[..7].iter().copied().chain(["Error: No space left on device"]).collect::<Vec<_>>(), Some("There was an OVF Tool error."));
    run.down = Some("vagrant destroy failed");
    let ran = lab_run::test(&r, &run, esxi()).await;
    assert!(!ran.cleaned, "the folder stays so the next test removes the VM first");
    // The run's error is the failed upload; the teardown's step says what is left.
    assert_eq!(ran.result.unwrap_err().to_string(), detail(&seen, "image", "fail"));
    let left = detail(&seen, "cleanup", "fail");
    assert!(left.starts_with("Couldn't remove everything the test created on the ESXi host (vagrant destroy failed)"), "{left}");
    assert!(left.contains(TEST_VM), "{left}");
    assert_eq!(ends(&seen), ["image:fail", "cleanup:fail"]);
    let why = detail(&seen, "image", "fail");
    assert!(why.contains("to datastore ssd1") && why.ends_with("Details: Error: No space left on device"), "{why}");
}

#[test]
fn the_failures_people_hit_belong_to_their_step() {
    let e = esxi();
    assert_eq!(e.explain("Error: Duplicate name 'cyberctf-server-selftest-probe'").unwrap().0, 0);
    assert!(e.explain("Duplicate name").unwrap().1.contains("delete it in the ESXi host client"));
    assert_eq!(e.explain("The box 'bento/debian-12' could not be found or could not be accessed").unwrap().0, 0);
    assert_eq!(e.explain("There was an error talking to ESXi.\n  Unable to power on VM:\n  …").unwrap().0, 1);
    assert_eq!(e.explain("Timed out while waiting for the machine to boot.").unwrap().0, 3);
    assert!(e.explain("Permission denied (publickey)").is_none());
}

#[test]
fn a_box_already_downloaded_is_named_with_the_architecture_vagrant_stored_it_for() {
    let boxes = std::env::temp_dir().join(format!("cyberctf-esxi-boxes-{}", rand::random::<u32>()));
    let version = boxes.join("bento-VAGRANTSLASH-debian-12").join("202510.26.0");
    std::fs::create_dir_all(version.join("arm64").join("vmware_desktop")).unwrap();
    std::fs::create_dir_all(version.join("amd64").join("virtualbox")).unwrap();
    let mut e = Esxi::new(&conn(&[]), Some(boxes.clone()), TEST_VM.into());
    // No `Adding box` line: the plugin's summary names the box, the store its architecture.
    for line in &ALL_THE_WAY[4..6] {
        assert_eq!(e.ends(line), None);
    }
    assert_eq!(e.detail(0).as_deref(), Some("Box bento/debian-12 v202510.26.0 (arm64)"), "only the VMware build counts");
    std::fs::create_dir_all(version.join("amd64").join("vmware_desktop")).unwrap();
    assert_eq!(e.detail(0).as_deref(), Some("Box bento/debian-12 v202510.26.0 (amd64 or arm64)"));
    std::fs::remove_dir_all(&boxes).unwrap();
    assert_eq!(e.detail(0).as_deref(), Some("Box bento/debian-12 v202510.26.0 (architecture unknown)"));
    assert_eq!(Esxi::new(&conn(&[]), None, TEST_VM.into()).detail(0), None, "no box named yet");
    assert_eq!(e.network, DEFAULT_NETWORK);
}

#[tokio::test]
async fn the_upload_carries_the_size_of_the_box_build_it_uploads_once_stored() {
    let boxes = std::env::temp_dir().join(format!("cyberctf-esxi-size-{}", rand::random::<u32>()));
    let version = boxes.join("bento-VAGRANTSLASH-debian-12").join("202510.26.0");
    for (arch, bytes) in [("amd64", 3000), ("arm64", 5)] {
        std::fs::create_dir_all(version.join(arch).join("vmware_desktop")).unwrap();
        std::fs::write(version.join(arch).join("vmware_desktop").join("disk-s001.vmdk"), vec![0u8; bytes]).unwrap();
    }
    std::fs::create_dir_all(version.join("amd64").join("virtualbox")).unwrap();
    std::fs::write(version.join("amd64").join("virtualbox").join("box.vmdk"), vec![0u8; 900]).unwrap();
    let (r, seen) = reporter();
    let run = Scripted::new(&ALL_THE_WAY, None);
    let course = Esxi::new(&conn(&[(DATASTORE, "ssd1")]), Some(boxes.clone()), TEST_VM.into());
    assert!(lab_run::test(&r, &run, course).await.result.is_ok());
    assert_eq!(running_size(&seen, "image"), Some(3000), "the amd64 VMware build Vagrant added, not another");
    assert_eq!(running_size(&seen, "vm"), None);
    // Not stored (yet): no size; with no architecture named, the only VMware build stored.
    assert_eq!(stored_size(&boxes, "bento/debian-12", "202510.1.0", None), None);
    std::fs::remove_dir_all(version.join("arm64")).unwrap();
    assert_eq!(stored_size(&boxes, "bento/debian-12", "202510.26.0", None), Some(3000));
    std::fs::remove_dir_all(&boxes).unwrap();
}

fn tools(plugin: std::result::Result<(), &str>, ovftool: std::result::Result<&str, &str>) -> Tools {
    Tools { plugin: plugin.map_err(str::to_owned), ovftool: ovftool.map(str::to_owned).map_err(str::to_owned) }
}

#[tokio::test]
async fn a_missing_plugin_or_ovftool_fails_up_front_before_the_host_is_reached() {
    // No address set: had the check gone on to the host, it would say so instead.
    let host = conn(&[]);
    let err = reach_with(&host, tools(Err("Vagrant plugin `vagrant-vmware-esxi` is not installed"), Ok("VMware ovftool 5.1.0"))).await.unwrap_err().to_string();
    assert!(err.starts_with("Labs on ESXi run from this computer") && err.contains("`vagrant-vmware-esxi` is not installed"), "{err}");
    let err = reach_with(&host, tools(Ok(()), Err("VMware's OVF Tool (`ovftool`) wasn't found"))).await.unwrap_err().to_string();
    assert!(err.contains("`ovftool`) wasn't found") && !err.contains("vagrant-vmware-esxi"), "{err}");
    let err = reach_with(&host, tools(Err("no plugin"), Err("no ovftool"))).await.unwrap_err().to_string();
    assert!(err.contains("no plugin; no ovftool"), "{err}");
    // Both there: on to the host.
    assert_eq!(reach_with(&host, tools(Ok(()), Ok("v"))).await.unwrap_err().to_string(), "the host's address isn't set");
}

#[tokio::test]
async fn with_both_tools_the_first_step_reaches_the_hosts_ssh_port() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port().to_string();
    let host = conn(&[(HOSTNAME, "127.0.0.1"), (HOSTPORT, &port)]);
    let said = reach_with(&host, tools(Ok(()), Ok("VMware ovftool 5.1.0"))).await.unwrap();
    assert_eq!(said, format!("127.0.0.1:{port} open; vagrant-vmware-esxi and VMware ovftool 5.1.0 ready"));
}

/// The test lab generates for Vagrant with the ESXi provider the run drives, the VM named as
/// the teardown's note says, and its command as the lab's check.
#[test]
fn the_test_lab_generates_for_vagrant_on_esxi() {
    let work = std::env::temp_dir().join(format!("cyberctf-esxi-selftest-{}", rand::random::<u32>()));
    let dir = lab_run::write_lab(&work, lab::Target::Vagrant).unwrap();
    let generated = lab::vagrant_dir(&dir, Runtime::Vm);
    let vagrantfile = std::fs::read_to_string(generated.join("Vagrantfile")).unwrap();
    assert!(vagrantfile.contains("m.vm.provider \"vmware_esxi\""), "no ESXi provider block");
    // Named with the copy's key, which the run hands the Vagrantfile (`ISOLOOM_KEY`).
    let keyed = format!("v.guest_name = \"{LAB}-#{{ISOLOOM_KEY}}-{MACHINE}\"");
    assert!(vagrantfile.contains(&keyed), "the VM isn't named {keyed}");
    let manifest = std::fs::read_to_string(generated.join("run.json")).unwrap();
    assert!(manifest.contains("\"vmware_esxi\"") && manifest.contains("check-probe"), "{manifest}");
    assert_eq!(lab_run::kept_lab(&work), Some(dir), "a test cut short now is found by the next one");
    assert!(isothrum_runner::RunName::new(&crate::runtime::docker::project(&run_id("0123456789abcdef"))).is_ok());
    // The VM's name on a host is the same at every test there: the runner's key for the
    // test's run, the one the run hands the Vagrantfile. Another host's test VM is another.
    let home = Path::new("/data/runner");
    let name = vm_name_in(home, "0123456789abcdef").unwrap();
    let run = isothrum_runner::RunName::new(&crate::runtime::docker::project(&run_id("0123456789abcdef"))).unwrap();
    assert_eq!(name, format!("{LAB}-{}-{MACHINE}", isothrum_runner::copy_key(home, &run)));
    assert_eq!(vm_name_in(home, "0123456789abcdef").unwrap(), name);
    assert_ne!(vm_name_in(home, "fedcba9876543210").unwrap(), name);
    std::fs::remove_dir_all(work).unwrap();
}
