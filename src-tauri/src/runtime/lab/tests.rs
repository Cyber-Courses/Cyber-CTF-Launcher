use super::*;
use crate::runtime::Runtime;
use std::path::PathBuf;

#[test]
fn isolooms_spec_and_generators_are_used_here_alone_and_the_run_manifest_by_the_runner_module() {
    // Regression: the door was said to be this module's alone while the tier evidence, the
    // attack box, the start place and the runner's view used Isoloom's crates directly.
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    // Spelled in two, so this file doesn't name the manifest's crate itself.
    let manifest = ["isoloom", "_manifest::"].concat();
    for s in crate::testing::source_scan::crate_sources() {
        let rel = s.path.strip_prefix(&src).unwrap().to_string_lossy().replace('\\', "/");
        let text = format!("{}\n{}", s.code, s.tests);
        let here = rel == "runtime/lab.rs" || rel.starts_with("runtime/lab/");
        let runner = rel == "runtime/isothrum.rs" || rel.starts_with("runtime/isothrum/");
        for krate in ["isoloom_spec::", "isoloom_targets::", "isoloom_plan::", "isoloom_compose::", "isoloom_render::"] {
            assert!(here || !text.contains(krate), "{rel} uses {krate} outside runtime/lab");
        }
        assert!(runner || !text.contains(&manifest), "{rel} reads the run manifest outside runtime/isothrum");
    }
}

const SPEC: &str = "version: 1\nname: t\nnetworks:\n  lab: { cidr: 10.30.0.0/24 }\ninputs: [CTF_LAUNCH_TOKEN]\nmachines:\n  web:\n    networks: { lab: 10 }\n    ports: { web: { port: 80, protocol: http } }\n    files: [{ path: /run/lab/token, input: CTF_LAUNCH_TOKEN }]\n    app: { image: nginx:1.27 }\n";

fn lab(spec: &str) -> crate::testing::TempDir {
    crate::runtime::test_support::write_lab(spec)
}

#[test]
fn generates_the_targets_files_into_the_lab() {
    let dir = lab(SPEC);
    prepare(&dir, Target::Docker).unwrap();
    let compose = std::fs::read_to_string(compose_file(&dir)).unwrap();
    assert!(compose.contains("isoloom.port.web"), "{compose}");
    // Named after the launcher's project for the lab, not the spec.
    let project = super::super::docker::project(dir.file_name().unwrap().to_str().unwrap());
    assert!(compose.contains(&format!("\nname: {project}\n")), "{compose}");
    prepare(&dir, Target::CloudDocker).unwrap();
    assert!(dir.join(".isoloom/cloud-docker/aws/main.tf").is_file());
    std::fs::remove_dir_all(dir).unwrap();
}

/// A machine building its image and running `setup:` scripts, beside one on a pulled image:
/// the fixture lab's `database` (the supplier portal), whose init job shares the built image.
const BUILT_WITH_INIT: &str = "version: 1\nname: t\nnetworks:\n  lab: { cidr: 10.30.0.0/24 }\nmachines:\n  database:\n    networks: { lab: 32 }\n    ports: { mysql: 3306 }\n    setup: [build/database/evidence.sh]\n    app: { build: build/database }\n  web:\n    networks: { lab: 31 }\n    ports: { web: { port: 80, protocol: http } }\n    setup: [seed.sh]\n    app: { image: nginx:1.27 }\n";

#[test]
fn an_init_job_of_a_machine_that_builds_its_image_is_rendered_buildable_so_the_start_pulls_nothing_for_it() {
    // Regression: an init job that ran the built image with `image:` alone made the start's `docker
    // compose pull --ignore-buildable` (Isothrum's pull step) leave the machine out but ask a
    // registry for the job's `localhost/isoloom/…` name, and the start failed ("failed to resolve
    // reference localhost/isoloom/…-database:latest"). The job carries the machine's `build:` too,
    // so the pull leaves it out and `up --build` builds once for both. Rendered through the pinned
    // Isoloom crates: the pin holds it. Also as an instance (`.cyberctf-instance`), the app's own
    // path for an installed lab: its files under `.isoloom-<n>/`, its images named
    // `<lab>-<n>-<machine>`.
    for instance in [None, Some(7)] {
        let dir = lab(BUILT_WITH_INIT);
        std::fs::create_dir_all(dir.join("build/database")).unwrap();
        std::fs::write(dir.join("build/database/Dockerfile"), "FROM mysql:8.0\n").unwrap();
        std::fs::write(dir.join("build/database/evidence.sh"), "#!/bin/sh\n").unwrap();
        std::fs::write(dir.join("seed.sh"), "#!/bin/sh\n").unwrap();
        if let Some(n) = instance {
            std::fs::write(dir.join(INSTANCE_MARKER), n.to_string()).unwrap();
        }
        prepare(&dir, Target::Docker).unwrap();
        let compose = std::fs::read_to_string(compose_file(&dir)).unwrap();
        let file: serde_yaml_ng::Value = serde_yaml_ng::from_str(&compose).unwrap();
        let services = file["services"].as_mapping().unwrap_or_else(|| panic!("no services in\n{compose}"));
        let service = |name: &str| services.get(name).unwrap_or_else(|| panic!("no service {name} in\n{compose}"));
        let (database, job) = (service("database"), service("isoloom-init-database-1"));
        let built = database["image"].as_str().unwrap_or_default();
        let named = instance.map_or("localhost/isoloom/t-database".to_owned(), |n| format!("localhost/isoloom/t-{n}-database"));
        assert!(built == named && database["build"].is_mapping(), "the machine builds its image ({instance:?}): {database:?}");
        assert_eq!(job["image"].as_str(), Some(built), "the job runs the image the machine builds ({instance:?})");
        // `pull --ignore-buildable` skips a service with a `build`; a `pull_policy` wouldn't
        // do, as the pull's `--policy` overrides it.
        assert_eq!(job["build"], database["build"], "the job is as buildable as the machine, with its build: ({instance:?})\n{compose}");
        let web = service("isoloom-init-web-1");
        assert!(web["build"].is_null() && web["image"].as_str() == Some("nginx:1.27"), "a pulled image's job is pulled ({instance:?}): {web:?}");
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[test]
fn explains_a_target_the_lab_cant_use() {
    let dir = lab(SPEC);
    // An image's filesystem can't be reproduced on a fresh VM: no VMs.
    let err = prepare(&dir, Target::Vagrant).unwrap_err().to_string();
    assert!(err.starts_with("this lab can't run there: vagrant") && err.contains("machines.web.app.image"), "{err}");
    std::fs::remove_dir_all(dir).unwrap();
}

/// `CyberCTF/cached` at `commit`, any source allowed.
fn cached(commit: &str) -> Source<'_> {
    Source { repository: "CyberCTF/cached", commit, check: |_, _| Ok(()) }
}

#[tokio::test]
async fn a_source_its_check_refuses_is_never_fetched() {
    let t = crate::testing::test_app::TestApp::new();
    let refused = Source { check: |r, _| Err(Error::Invalid(format!("refusing {r}"))), ..cached("3333333333333333333333333333333333333333") };
    let fetch = |url: String| async move { panic!("fetched {url}") };
    let err = lab_at_with(&t.dirs(), "cached", refused, fetch).await.unwrap_err();
    assert_eq!(err.to_string(), "refusing CyberCTF/cached");
}

#[tokio::test]
async fn a_spec_not_installed_is_fetched_once_per_commit() {
    // Regression: a lab page's requirements, its servers and its start place each downloaded
    // the same isoloom.yml: three fetches for one page and its Start.
    let t = crate::testing::test_app::TestApp::new();
    let fetches = std::sync::atomic::AtomicUsize::new(0);
    let fetch = |url: String| {
        fetches.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        assert_eq!(url, "https://raw.githubusercontent.com/CyberCTF/cached/1111111111111111111111111111111111111111/isoloom.yml");
        async { Ok(SPEC.to_owned()) }
    };
    let commit = "1111111111111111111111111111111111111111";
    for _ in 0..3 {
        let lab = lab_at_with(&t.dirs(), "cached", cached(commit), fetch).await.unwrap();
        assert_eq!(lab.spec().name, "t");
    }
    assert_eq!(fetches.load(std::sync::atomic::Ordering::SeqCst), 1);
    // A failed fetch is not remembered: the next read tries again.
    let other = "2222222222222222222222222222222222222222";
    let failing = |_: String| async { Err(Error::Invalid("couldn't read the lab's spec: HTTP 503".into())) };
    assert!(lab_at_with(&t.dirs(), "cached", cached(other), failing).await.is_err());
    let ok = |_: String| async { Ok(SPEC.to_owned()) };
    assert!(lab_at_with(&t.dirs(), "cached", cached(other), ok).await.is_ok());
}

#[test]
fn requirements_size_a_single_form_lab_as_one_value() {
    // SPEC's web machine is a container only (app.image), so every form it runs is a Docker
    // one: all container-sized, and the lightest-to-heaviest range collapses to one value.
    let r = requirements(&parsed(SPEC).unwrap()).expect("a lab with a machine has requirements");
    assert_eq!(r.min, r.max);
    assert_eq!(r.min.memory_mb, isoloom_spec::CONTAINER_MEMORY_MB);
}

#[test]
fn a_cloud_services_lab_has_no_machine_footprint() {
    let lab =
        parsed("version: 1\nname: t\ncloud:\n  provider: aws\n  terraform: terraform\n  vars: { region: us-east-1 }\n  outputs: { app: app_url }\n").unwrap();
    assert!(requirements(&lab).is_none());
}

#[test]
fn a_backend_refusal_keeps_its_reason() {
    // Every target either renders (with the snapshot) or refuses in Isoloom's words, named
    // after the target: what the player reads after "can't run there".
    let dir = lab(SPEC);
    let lab = super::lab(&dir).unwrap();
    for target in &Target::ALL {
        match generate(&lab, *target, None) {
            Ok(files) => assert!(files.iter().any(|f| f.path == isoloom_targets::resolved::PATH), "{target:?}: the snapshot goes with every target"),
            Err(e) => assert!(e.to_string().starts_with(target.id()), "{target:?}: {e}"),
        }
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn the_snapshot_read_here_is_the_version_its_fields_were_checked_against() {
    // `tools`, `topology` and `blocks_clear` read the resolved snapshot's `networks.*.cidr`,
    // `docker_cidr`, `internet`, `machines.*.addresses` and `tools`, none of which version 5
    // (a machine's role `member`, was `target`) changed. A new version changes or drops
    // fields: this fails so they are read again.
    assert_eq!(isoloom_targets::resolved::RESOLVED_VERSION, 5);
    let dir = lab(SPEC);
    assert_eq!(isoloom_targets::resolved::resolve(&super::lab(&dir).unwrap())["resolved_version"], 5);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_container_labs_vagrant_folder_is_the_one_an_earlier_version_ran_its_helper_vm_from() {
    let d = Path::new("/l");
    assert_eq!(vagrant_dir(d, Runtime::Docker), PathBuf::from("/l/.isoloom/docker-vm"));
    assert_eq!(vagrant_dir(d, Runtime::Docker), docker_vm_dir(d));
    assert_eq!(vagrant_dir(d, Runtime::Vm), PathBuf::from("/l/.isoloom/vagrant"));
}

#[test]
fn an_instance_skips_numbers_whose_blocks_docker_already_has() {
    let labs = std::env::temp_dir().join(format!("cyberctf-labs-{}", rand::random::<u32>()));
    let a = labs.join("a");
    std::fs::create_dir_all(&a).unwrap();
    std::fs::write(a.join("isoloom.yml"), SPEC).unwrap();
    // SPEC is at 10.30.0.0/24: instance 1 would be 10.31, 2 is 10.32. Another program's
    // network covers 10.31 (a /16 here), so the lab gets 2.
    assert_eq!(ensure_instance(&labs, &a, &["10.31.0.0/16".into(), "172.17.0.0/16".into()]).unwrap(), 2);
    // Instance 2 is held by another installed lab: 3 (10.33) then.
    std::fs::remove_file(a.join(INSTANCE_MARKER)).unwrap();
    std::fs::create_dir_all(labs.join("b")).unwrap();
    std::fs::write(labs.join("b").join(INSTANCE_MARKER), "2").unwrap();
    assert_eq!(ensure_instance(&labs, &a, &["10.31.0.0/16".into()]).unwrap(), 3);
    std::fs::remove_dir_all(labs).unwrap();
}

#[test]
fn a_stopped_labs_instance_moves_when_its_blocks_are_taken_now() {
    // Regression: the instance was picked once at install, with Docker down (no networks to
    // avoid), and kept for good: the start then failed on "Pool overlaps".
    let labs = std::env::temp_dir().join(format!("cyberctf-labs-{}", rand::random::<u32>()));
    let a = labs.join("a");
    std::fs::create_dir_all(&a).unwrap();
    std::fs::write(a.join("isoloom.yml"), SPEC).unwrap();
    assert_eq!(ensure_instance(&labs, &a, &[]).unwrap(), 1, "picked blind");
    // Docker now has a network on instance 1's block (10.31): the start moves the lab to 2.
    assert_eq!(recheck_instance(&labs, &a, &["10.31.0.0/24".into()]).unwrap(), 2);
    assert_eq!(instance(&a), Some(2));
    // Clear: kept.
    assert_eq!(recheck_instance(&labs, &a, &["10.31.0.0/24".into()]).unwrap(), 2);
    // Without a number yet, one is picked.
    std::fs::remove_file(a.join(INSTANCE_MARKER)).unwrap();
    assert_eq!(recheck_instance(&labs, &a, &[]).unwrap(), 1);
    std::fs::remove_dir_all(labs).unwrap();
}

#[test]
fn labs_picking_their_instances_at_once_get_different_ones() {
    let labs = std::env::temp_dir().join(format!("cyberctf-labs-{}", rand::random::<u32>()));
    let dirs: Vec<_> = (0..8).map(|i| labs.join(format!("lab{i}"))).collect();
    for d in &dirs {
        std::fs::create_dir_all(d).unwrap();
    }
    let picks: Vec<u8> = dirs
        .iter()
        .map(|d| {
            let (labs, d) = (labs.clone(), d.clone());
            std::thread::spawn(move || ensure_instance(&labs, &d, &[]).unwrap())
        })
        .collect::<Vec<_>>()
        .into_iter()
        .map(|h| h.join().unwrap())
        .collect();
    let mut unique = picks.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), picks.len(), "{picks:?}");
    std::fs::remove_dir_all(labs).unwrap();
}

#[test]
fn cidr_ranges() {
    assert_eq!(ipv4_range("10.31.0.0/24"), Some((u32::from(std::net::Ipv4Addr::new(10, 31, 0, 0)), u32::from(std::net::Ipv4Addr::new(10, 31, 0, 255)))));
    assert_eq!(ipv4_range("10.31.5.7/16").map(|r| r.0), Some(u32::from(std::net::Ipv4Addr::new(10, 31, 0, 0))));
    assert_eq!(ipv4_range("fd00::/64"), None);
}

#[test]
fn observers_come_with_their_addresses() {
    let dir = lab(&format!("{SPEC}tools:\n  shell: {{}}\n"));
    let found = tools(&dir, Runtime::Docker).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].name, "shell");
    // Isoloom's toolbox, pinned by its digest.
    let image = found[0].image.as_deref().unwrap();
    assert!(image.starts_with("nicolaka/netshoot:") && image.contains("@sha256:"), "{image}");
    assert_eq!(found[0].addresses.len(), 1);
    assert_eq!(found[0].addresses[0].network, "lab");
    assert!(tools(&dir, Runtime::Cloud).unwrap().is_empty(), "none beside cloud services");
    assert!(tools(&dir, Runtime::Vm).unwrap()[0].addresses[0].ip.starts_with("10.30.0."));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn the_labs_own_defaults_file_reaches_the_generated_files() {
    let dir = lab(SPEC);
    std::fs::write(dir.join(isoloom_spec::defaults::PROJECT_FILE), "cloud:\n  aws: { region: us-east-1 }\n").unwrap();
    prepare(&dir, Target::CloudDocker).unwrap();
    let tf = std::fs::read_to_string(dir.join(".isoloom/cloud-docker/aws/main.tf")).unwrap();
    assert!(tf.contains("us-east-1"), "{tf}");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn picks_the_module_for_each_run() {
    let d = Path::new("/l");
    assert_eq!(terraform(d, Runtime::Docker, "aws"), (PathBuf::from("/l/.isoloom/cloud-docker/aws"), Target::CloudDocker));
    // Proxmox runs one VM per machine, a container lab's too: no helper VM there.
    for runtime in [Runtime::Docker, Runtime::Vm] {
        assert_eq!(terraform(d, runtime, "proxmox"), (PathBuf::from("/l/.isoloom/proxmox"), Target::Proxmox));
    }
    assert_eq!(terraform(d, Runtime::Vm, "aws"), (PathBuf::from("/l/.isoloom/cloud-vm/aws"), Target::CloudVm));
    // Every cloud now has a cloud-vm module, not only AWS.
    assert_eq!(terraform(d, Runtime::Vm, "azure"), (PathBuf::from("/l/.isoloom/cloud-vm/azure"), Target::CloudVm));
    assert_eq!(terraform(d, Runtime::Vm, "oci"), (PathBuf::from("/l/.isoloom/cloud-vm/oci"), Target::CloudVm));
}

#[test]
fn an_instance_marker_is_read_only_between_one_and_the_maximum() {
    let dir = lab(SPEC);
    for (text, want) in
        [("1", Some(1)), ("99", Some(99)), (" 7\r\n", Some(7)), ("0", None), ("100", None), ("", None), ("-1", None), ("x", None), ("1 2", None)]
    {
        std::fs::write(dir.join(INSTANCE_MARKER), text).unwrap();
        assert_eq!(instance(&dir), want, "{text:?}");
    }
    // A marker that is a folder, or none: no instance.
    std::fs::remove_file(dir.join(INSTANCE_MARKER)).unwrap();
    assert_eq!(instance(&dir), None);
    std::fs::create_dir(dir.join(INSTANCE_MARKER)).unwrap();
    assert_eq!(instance(&dir), None);
    assert_eq!(out(&dir), dir.join(".isoloom"), "no instance: the plain output folder");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_machine_with_every_instance_taken_refuses_a_new_lab() {
    let labs = std::env::temp_dir().join(format!("cyberctf-labs-full-{}", rand::random::<u32>()));
    let a = labs.join("a");
    std::fs::create_dir_all(&a).unwrap();
    // No spec: every number is clear on the address side; other labs hold 1..=99.
    for n in 1..=isoloom_spec::instance::MAX {
        let other = labs.join(format!("other{n}"));
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(other.join(INSTANCE_MARKER), n.to_string()).unwrap();
    }
    let err = ensure_instance(&labs, &a, &[]).unwrap_err().to_string();
    assert!(err.starts_with("no free instance for this lab"), "{err}");
    assert!(!a.join(INSTANCE_MARKER).exists(), "nothing written on failure");
    // One short of full: the last number is free.
    std::fs::remove_dir_all(labs.join("other99")).unwrap();
    assert_eq!(ensure_instance(&labs, &a, &[]).unwrap(), 99);
    std::fs::remove_dir_all(labs).unwrap();
}

#[test]
fn a_staging_folder_left_by_a_crashed_install_still_holds_its_instance() {
    // The install copies the instance marker into `.<id>.staging` before the swap; a crash
    // between the two leaves that folder, and its number stays taken (never reused by
    // another lab, which would then share its blocks with the lab once the swap completes).
    let labs = std::env::temp_dir().join(format!("cyberctf-labs-stage-{}", rand::random::<u32>()));
    let staging = labs.join(".a.staging");
    std::fs::create_dir_all(&staging).unwrap();
    std::fs::write(staging.join(INSTANCE_MARKER), "1").unwrap();
    // A stray file in the labs folder is not a lab.
    std::fs::write(labs.join("notes.txt"), "1").unwrap();
    let b = labs.join("b");
    std::fs::create_dir_all(&b).unwrap();
    assert_eq!(ensure_instance(&labs, &b, &[]).unwrap(), 2);
    std::fs::remove_dir_all(labs).unwrap();
}

#[test]
fn cidr_ranges_at_the_prefix_boundaries() {
    let all = ipv4_range("0.0.0.0/0").unwrap();
    assert_eq!(all, (0, u32::MAX));
    let one = ipv4_range("10.1.2.3/32").unwrap();
    assert_eq!(one.0, one.1);
    assert_eq!(one.0, u32::from(std::net::Ipv4Addr::new(10, 1, 2, 3)));
    for bad in ["10.0.0.0/33", "10.0.0.0/-1", "10.0.0.0/", "/24", "10.0.0.0", "256.0.0.0/8", "10.0.0.0/24/8", " 10.0.0.0/24", "10.0.0.0/ 24"] {
        assert_eq!(ipv4_range(bad), None, "{bad}");
    }
}

#[test]
fn a_spec_with_windows_line_endings_reads_like_any_other() {
    let dir = lab(&SPEC.replace('\n', "\r\n"));
    let lab = super::lab(&dir).unwrap();
    assert_eq!(lab.spec().name, "t");
    assert_eq!(lab.spec().networks["lab"].cidr, "10.30.0.0/24");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_spec_with_a_field_this_build_does_not_know_asks_for_a_newer_launcher() {
    // Labs adopt spec additions before every player has updated: a top-level key this build's
    // Isoloom doesn't have means a newer format, so the player is told to update rather than
    // shown a mistake of the lab's. A mistake of the lab's own stays Isoloom's words.
    let dir = lab(&format!("{SPEC}observability:\n  traces: true\n"));
    let err = super::lab(&dir).unwrap_err().to_string();
    assert!(err.starts_with("This lab needs a newer version of Cyber CTF. Update it (Settings > About), then start the lab again."), "{err}");
    assert!(err.contains("isoloom.yml:"), "{err}");
    std::fs::remove_dir_all(dir).unwrap();
    let dir = lab(&SPEC.replace("10.30.0.0/24", "not-a-cidr"));
    let err = super::lab(&dir).unwrap_err().to_string();
    assert!(!err.contains("newer version"), "{err}");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn prepare_replaces_the_previous_files_but_keeps_other_ones_in_the_output_folder() {
    let dir = lab(SPEC);
    prepare(&dir, Target::Docker).unwrap();
    let compose = compose_file(&dir);
    std::fs::write(&compose, "stale").unwrap();
    // Something else in the output folder (a check runner's log, a pinned-ports file).
    std::fs::write(compose.with_file_name("notes.txt"), "mine").unwrap();
    prepare(&dir, Target::Docker).unwrap();
    assert!(std::fs::read_to_string(&compose).unwrap().contains("services"), "regenerated");
    assert_eq!(std::fs::read_to_string(compose.with_file_name("notes.txt")).unwrap(), "mine");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn an_instance_at_the_maximum_still_generates_and_resolves() {
    let dir = lab(SPEC);
    std::fs::write(dir.join(INSTANCE_MARKER), "99").unwrap();
    let prepared = prepare(&dir, Target::Docker).unwrap();
    assert_eq!(prepared.spec.name, "t-99");
    assert!(compose_file(&dir).starts_with(dir.join(".isoloom-99")));
    assert!(compose_file(&dir).is_file());
    std::fs::remove_dir_all(dir).unwrap();
}
