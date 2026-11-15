use super::{Place, fill_logins, on_server, primary_url, with_parked};
use crate::runtime::port::{GuestPort, HostPort};
use crate::runtime::providers::Provider;
use crate::runtime::test_support::status_with;
use crate::runtime::{Park, Runtime};

#[test]
fn only_a_lab_that_is_down_reads_as_parked() {
    assert_eq!(with_parked(status_with(false, &[]), Some(Park::Pause)).parked, Some(Park::Pause));
    assert_eq!(with_parked(status_with(true, &["web"]), Some(Park::Pause)).parked, None, "started again by hand");
    assert_eq!(with_parked(status_with(false, &[]), None).parked, None);
}

#[test]
fn each_port_gets_the_login_its_spec_declares() {
    let spec = crate::runtime::lab::parse(
            "version: 1\nname: t\nnetworks:\n  lab: { cidr: 10.30.0.0/24 }\nmachines:\n  jenkins:\n    networks: { lab: 10 }\n    ports: { web: { port: 8080, protocol: http, publish: true, login: { user: admin, password: admin } }, agent: 50000 }\n    app: { image: nginx:1.27 }\n",
        )
        .unwrap();
    let mut s = status_with(true, &["jenkins", "other"]);
    for m in &mut s.machines {
        m.ports = vec![
            crate::runtime::Port { published: Some(HostPort::new(51234)), target: GuestPort::new(8080), login: None },
            crate::runtime::Port { published: None, target: GuestPort::new(50000), login: None },
        ];
    }
    fill_logins(&mut s, &spec);
    let logins: Vec<Vec<Option<(&str, &str)>>> =
        s.machines.iter().map(|m| m.ports.iter().map(|p| p.login.as_ref().map(|l| (l.user.as_str(), l.password.as_str()))).collect()).collect();
    assert_eq!(logins, [vec![Some(("admin", "admin")), None], vec![None, None]], "the declared port only, on its own machine");
}

/// The connection of saved host `id`, named `name`, on `provider`.
fn conn(id: &str, name: &str, provider: Provider) -> crate::runtime::server::Connection {
    crate::runtime::server::Connection {
        id: id.into(),
        provider,
        name: name.into(),
        env: Vec::new(),
        tf_vars: Vec::new(),
        tf_env: Vec::new(),
        region: None,
        auto_stop_hours: None,
    }
}

#[test]
fn a_server_lab_says_where_it_runs() {
    let s = on_server(status_with(true, &["web"]), &conn("h-aws", "My AWS", Provider::Aws));
    assert_eq!((s.host.as_deref(), s.host_id.as_deref(), s.provider.as_deref()), (Some("My AWS"), Some("h-aws"), Some("aws")));
    assert!(matches!(s.place, Some(Place::Cloud)));
    let s = on_server(status_with(true, &["web"]), &conn("h-pve", "pve", Provider::Proxmox));
    assert!(matches!(s.place, Some(Place::Server)));
    assert_eq!((s.host_id.as_deref(), s.provider.as_deref()), (Some("h-pve"), Some("proxmox")));
}

#[tokio::test]
async fn only_a_local_container_lab_has_a_url_here() {
    let dir = std::env::temp_dir().join(format!("cyberctf-status-url-{}", rand::random::<u32>()));
    std::fs::create_dir_all(&dir).unwrap();
    // A VM lab, a lab on a server host, a container lab in a local VM: no Docker probe at all.
    assert_eq!(primary_url(&dir, "t", Runtime::Vm).await, None);
    crate::runtime::paths::mark_local_vm(&dir, Some(Provider::Virtualbox)).unwrap();
    assert_eq!(primary_url(&dir, "t", Runtime::Docker).await, None);
    crate::runtime::paths::mark_local_vm(&dir, None).unwrap();
    std::fs::write(dir.join(crate::runtime::server::HOST_MARKER), "h").unwrap();
    assert_eq!(primary_url(&dir, "t", Runtime::Docker).await, None);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_lab_is_present_wherever_something_of_it_is_left() {
    // Regression: a failed website launch asked only whether the lab ran *here* (local Docker
    // and the lab folder's Vagrant record), so a lab up on a server host or cloud account, or
    // as VMs the runner started here, read as absent, and the failed start's teardown
    // destroyed it.
    use crate::runtime::test_support::record_run;
    let t = crate::testing::test_app::TestApp::new();
    let dirs = t.dirs();
    let dir = t.lab("present", &[]);
    assert!(!super::lab_present(&dirs, "present"), "installed, nothing of it up");
    // Its run, wherever the runner records it up: in a cloud account, as VMs here.
    record_run(t.data(), "present", Some("aws1"), "cloud-docker", "ready");
    assert!(super::lab_present(&dirs, "present"));
    record_run(t.data(), "present", None, "vagrant", "failed");
    assert!(super::lab_present(&dirs, "present"), "a failed start's VMs left here");
    // Taken down: absent.
    record_run(t.data(), "present", None, "vagrant", "absent");
    assert!(!super::lab_present(&dirs, "present"));
    assert!(super::lab_installed(&dirs, "present") && !super::lab_installed(&dirs, "gone"));
    // Parked here.
    crate::runtime::paths::mark_parked(&dir, Some(crate::runtime::Park::Pause)).unwrap();
    assert!(super::lab_present(&dirs, "present"));
}

#[test]
fn a_lab_whose_run_cant_be_read_is_judged_by_its_marker_and_says_why() {
    // Regression: why the run couldn't be read was dropped, and the failed start's teardown
    // was decided on the marker alone with nothing in the log.
    let t = crate::testing::test_app::TestApp::new();
    let dir = t.lab("present", &[]);
    crate::testing::runner::damage_store(&crate::runtime::paths::runner_home_in(t.data()));
    crate::testing::logs::take_warnings();
    assert!(!super::lab_present(&t.dirs(), "present"), "no marker: down");
    std::fs::write(dir.join(crate::runtime::server::HOST_MARKER), "aws1").unwrap();
    assert!(super::lab_present(&t.dirs(), "present"), "marked as on a host: up there");
    let logged = crate::testing::logs::take_warnings();
    assert_eq!(logged.len(), 2, "{logged:?}");
    assert!(logged[0].starts_with("couldn't tell whether present is up (") && logged[0].ends_with("taken for down by its host marker"), "{logged:?}");
    assert!(logged[1].ends_with("taken for up by its host marker"), "{logged:?}");
}

#[test]
fn what_is_still_up_is_read_from_the_runners_record_wherever_it_runs() {
    // Regression: an update, a re-import and a start elsewhere asked the lab folder (Vagrant's
    // `.vagrant`, local Docker), where the runner keeps nothing: a VM, a
    // docker-in-VM, an ESXi, a Proxmox or a cloud lab run by the runner read as down.
    use crate::runtime::test_support::record_run;
    let t = crate::testing::test_app::TestApp::new();
    let dirs = t.dirs();
    let dir = t.lab("web", &[]);
    let pve = t.host("pve1", "proxmox", serde_json::json!({}));
    assert_eq!(super::still_up(&dirs, "web").unwrap(), None, "never started");
    for (host, target) in
        [(None, "docker"), (None, "docker-vm"), (None, "vagrant"), (Some("esx1"), "vagrant"), (Some(pve.as_str()), "proxmox"), (Some("aws1"), "cloud-vm")]
    {
        record_run(t.data(), "web", host, target, "ready");
        let up = super::still_up(&dirs, "web").unwrap().unwrap_or_else(|| panic!("{target} on {host:?}"));
        assert_eq!((up.host.as_deref(), up.parked), (host, false), "{target}");
        let err = super::refuse_replace(&dirs, &dir, "web").unwrap_err();
        let expected = match host {
            None => crate::error::Code::LabRunningHere,
            // A saved host by its name; one no longer saved by its id.
            Some(h) => crate::error::Code::LabRunningOn { host: if h == pve { "Host pve1".into() } else { h.into() } },
        };
        assert_eq!(err.code(), expected, "{target}");
        assert!(err.is_refusal());
    }
    // Parked: only its parked mark lets the copy go (it is taken down before the swap).
    record_run(t.data(), "web", None, "vagrant", "parked");
    assert!(super::still_up(&dirs, "web").unwrap().is_some_and(|u| u.parked));
    assert!(super::refuse_replace(&dirs, &dir, "web").is_err());
    crate::runtime::paths::mark_parked(&dir, Some(crate::runtime::Park::Shutdown)).unwrap();
    assert!(super::refuse_replace(&dirs, &dir, "web").is_ok());
    // Down: free to replace; another lab's run is no business of this one's.
    record_run(t.data(), "web", None, "vagrant", "absent");
    record_run(t.data(), "web-2", None, "docker", "ready");
    assert_eq!(super::still_up(&dirs, "web").unwrap(), None);
    assert!(super::refuse_replace(&dirs, &dir, "web").is_ok());
    // A store that can't be read stops the replacement rather than read as "down".
    crate::testing::runner::damage_store(&crate::runtime::paths::runner_home_in(t.data()));
    assert!(super::still_up(&dirs, "web").is_err() && super::refuse_replace(&dirs, &dir, "web").is_err());
    // Nothing installed: nothing to replace.
    assert!(super::refuse_replace(&dirs, &t.data().join("labs/none"), "none").is_ok());
}

#[tokio::test]
async fn a_lab_marked_on_a_host_that_is_no_longer_saved_names_that_host() {
    use crate::platform::exec::fake::Tools;
    let t = crate::testing::test_app::TestApp::new();
    let dir = t.lab("web", &[]);
    std::fs::write(dir.join(crate::runtime::server::HOST_MARKER), "gone1\n").unwrap();
    let tools = Tools::strict_rules(&[]);
    for runtime in [Runtime::Docker, Runtime::Vm, Runtime::Cloud] {
        let err = super::status(&t.dirs(), &dir, "web", runtime).await.expect_err("refused").to_string();
        assert_eq!(err, "server host `gone1` not found", "{runtime:?}");
    }
    // Refused before anything local or remote is probed; the marker alone never makes it
    // run here either.
    assert!(tools.calls().is_empty(), "{:?}", tools.calls());
}

#[test]
fn a_parked_mark_is_reported_only_while_the_lab_is_down_whatever_the_mode() {
    for mode in [Park::Pause, Park::Shutdown] {
        assert_eq!(with_parked(status_with(false, &[]), Some(mode)).parked, Some(mode));
        // Half up (one machine running by hand): the status is "running", never "parked",
        // so the page offers Stop rather than a Resume that would do nothing.
        assert_eq!(with_parked(status_with(true, &["dc01"]), Some(mode)).parked, None);
    }
}

#[tokio::test]
async fn the_status_carries_what_the_page_may_do_where_the_lab_runs() {
    use crate::runtime::Control;
    let t = crate::testing::test_app::TestApp::new();
    let _tools = crate::platform::exec::fake::Tools::strict_rules(&[]);
    let dir = t.lab("web", &[]);
    let docker = super::status(&t.dirs(), &dir, "web", Runtime::Docker).await.unwrap().controls;
    // Containers stopped outside the app come back with a resume (tests/resilience.md M4).
    assert_eq!(docker, [Control::Shutdown, Control::Repair]);
    let vm = super::status(&t.dirs(), &dir, "web", Runtime::Vm).await.unwrap().controls;
    assert_eq!(vm, [Control::Pause, Control::Shutdown, Control::Provision, Control::Repair]);
    assert_eq!(super::status(&t.dirs(), &dir, "web", Runtime::Cloud).await.unwrap().controls, [] as [Control; 0]);
}
