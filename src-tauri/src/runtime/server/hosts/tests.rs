use super::*;
use crate::runtime::providers::Provider;
use crate::runtime::server::HOST_MARKER;
use crate::runtime::server::profile::tests::profile;

/// Saved host `id` on `provider`, the test profile's other fields.
pub(crate) fn host(id: &str, provider: Provider) -> HostProfile {
    HostProfile { id: id.into(), ..profile(provider) }
}

/// A store of `hosts`, `default` its default.
pub(crate) fn store(hosts: Vec<HostProfile>, default: Option<&str>) -> Store {
    Store { default: default.map(str::to_string), hosts }
}

#[test]
fn a_host_moves_only_with_nothing_on_it_and_renewed_keys_never_move_it() {
    let aws = |region: &str, key: &str| HostProfile { host: region.into(), username: key.into(), ..host("a1", Provider::Aws) };
    let (now, keys) = (aws("eu-west-3", "AKIAOLD"), aws("eu-west-3", "AKIANEW"));
    // A new access key (the old one deleted or expired) is how to sign in, not where.
    keeps_placement(&now, &keys, 2).unwrap();
    let err = keeps_placement(&now, &aws("us-east-1", "AKIAOLD"), 2).unwrap_err().to_string();
    assert_eq!(err, "2 labs are still on this account: stop them before changing its address or region.");
    keeps_placement(&now, &aws("us-east-1", "AKIAOLD"), 0).unwrap();
    // A server pointed at another machine or node, or turned into another provider.
    let pve = host("p1", Provider::Proxmox);
    let err = keeps_placement(&pve, &HostProfile { node: Some("pve2".into()), host: "10.0.0.9".into(), ..pve.clone() }, 1).unwrap_err().to_string();
    assert_eq!(err, "1 lab is still on this host: stop it before changing its address or region, node or organization.");
    assert!(keeps_placement(&pve, &HostProfile { provider: Provider::VmwareEsxi, ..pve.clone() }, 1).unwrap_err().to_string().contains("provider"));
    keeps_placement(&pve, &HostProfile { username: "root@pam!token".into(), datastore: Some("other".into()), ..pve.clone() }, 1).unwrap();
    // An AWS CLI account switched to another profile (likely another AWS account), or from
    // stored keys to the CLI.
    let cli = HostProfile { use_cli_creds: true, aws_profile: Some("labs".into()), ..host("a2", Provider::Aws) };
    let err = keeps_placement(&cli, &HostProfile { aws_profile: Some("personal".into()), ..cli.clone() }, 1).unwrap_err().to_string();
    assert!(err.ends_with("changing its AWS CLI profile."), "{err}");
    assert!(keeps_placement(&HostProfile { use_cli_creds: false, aws_profile: None, ..cli.clone() }, &cli, 1).is_err());
}

#[test]
fn the_first_server_saved_becomes_the_default_but_never_a_cloud() {
    let mut s = store(Vec::new(), None);
    put_host(&mut s, host("aws1", Provider::Aws));
    assert_eq!(s.default, None);
    put_host(&mut s, host("pve1", Provider::Proxmox));
    assert_eq!(s.default.as_deref(), Some("pve1"));
    put_host(&mut s, host("esx1", Provider::VmwareEsxi));
    assert_eq!(s.default.as_deref(), Some("pve1"));
    // Saving again replaces the entry instead of adding one.
    put_host(&mut s, HostProfile { name: "Renamed".into(), ..host("pve1", Provider::Proxmox) });
    assert_eq!(s.hosts.len(), 3);
    assert_eq!(s.hosts.iter().find(|h| h.id == "pve1").unwrap().name, "Renamed");
}

#[test]
fn removing_the_default_hands_it_to_another_server() {
    let mut s = store(vec![host("aws1", Provider::Aws), host("pve1", Provider::Proxmox), host("esx1", Provider::VmwareEsxi)], Some("pve1"));
    remove_host(&mut s, "pve1");
    assert_eq!(s.default.as_deref(), Some("esx1"));
    assert_eq!(s.hosts.len(), 2);
    // Removing a non-default leaves the default alone.
    remove_host(&mut s, "aws1");
    assert_eq!(s.default.as_deref(), Some("esx1"));
    // No server left: no default (the cloud account never takes it).
    let mut s = store(vec![host("aws1", Provider::Aws), host("esx1", Provider::VmwareEsxi)], Some("esx1"));
    remove_host(&mut s, "esx1");
    assert_eq!(s.default, None);
}

#[test]
fn hosts_with_running_labs_are_not_removed() {
    assert!(no_labs_running(0).is_ok());
    assert_eq!(no_labs_running(1).unwrap_err().to_string(), "1 lab still running on this host. Stop it before removing the host.");
    assert_eq!(no_labs_running(3).unwrap_err().to_string(), "3 labs still running on this host. Stop them before removing the host.");
}

#[test]
fn only_a_saved_server_can_be_the_default() {
    let mut s = store(vec![host("aws1", Provider::Aws), host("pve1", Provider::Proxmox)], None);
    set_default_host(&mut s, Some("pve1".into())).unwrap();
    assert_eq!(s.default.as_deref(), Some("pve1"));
    assert_eq!(set_default_host(&mut s, Some("aws1".into())).unwrap_err().to_string(), "a cloud account can't be the default host");
    assert!(set_default_host(&mut s, Some("nope".into())).is_err());
    assert_eq!(s.default.as_deref(), Some("pve1"));
    set_default_host(&mut s, None).unwrap();
    assert_eq!(s.default, None);
}

#[test]
fn what_is_left_on_a_host_keeps_it_from_being_removed() {
    // Regression: only the launcher's own Terraform states under deployments/ were counted, so a
    // lab the runner still ran on a host (moved elsewhere since, or uninstalled) didn't keep it:
    // removed, nothing could stop that lab any more.
    use crate::runtime::test_support::record_run;
    let t = crate::testing::test_app::TestApp::new();
    let dirs = t.dirs();
    let mark = |lab: &str, host: Option<&str>| {
        let dir = t.lab(lab, &[]);
        crate::runtime::server::mark_lab(&dir, host).unwrap();
    };
    assert_eq!(leftovers_on(&dirs, "aws1").unwrap(), 0);
    // Runs the runner records on aws1: one still marked there (counted by its marker, not
    // here), one moved elsewhere, one uninstalled, one taken down, one on another host.
    mark("web-1", Some("aws1"));
    record_run(t.data(), "web-1", Some("aws1"), "cloud-docker", "ready");
    mark("db-1", Some("pve1"));
    record_run(t.data(), "db-1", Some("aws1"), "cloud-vm", "failed");
    record_run(t.data(), "gone-1", Some("aws1"), "cloud-docker", "parked");
    record_run(t.data(), "down-1", Some("aws1"), "cloud-docker", "absent");
    record_run(t.data(), "api-1", Some("aws2"), "cloud-docker", "ready");
    assert_eq!(leftovers_on(&dirs, "aws1").unwrap(), 2);
    assert_eq!(leftovers_on(&dirs, "aws2").unwrap(), 1);
    assert_eq!(labs_on(&dirs, "aws1").unwrap(), 3, "two left and one marked");
    // A Terraform state of the launcher's own path, awaiting its hand-over, counts too.
    let state = t.data().join("deployments").join("old-1").join("aws");
    std::fs::create_dir_all(&state).unwrap();
    std::fs::write(state.join("terraform.tfstate"), r#"{"outputs":{},"resources":[{"type":"aws_instance"}]}"#).unwrap();
    std::fs::write(state.join("host"), "aws1").unwrap();
    assert_eq!(leftovers_on(&dirs, "aws1").unwrap(), 3);
    // A record that can't be read is an error, not "nothing left".
    std::fs::write(t.data().join("runs/cyberctf-api-1/record.json"), "{").unwrap();
    assert!(leftovers_on(&dirs, "aws1").is_err() && labs_on(&dirs, "aws1").is_err());
}

#[test]
fn markers_are_read_from_each_lab_folder() {
    let labs = std::env::temp_dir().join(format!("cyberctf-markers-{}", rand::random::<u32>()));
    assert_eq!(markers_in(&labs), Vec::<String>::new());
    for (lab, marker) in [("a", Some("pve1\n")), ("b", Some("pve1")), ("c", None), ("d", Some(""))] {
        std::fs::create_dir_all(labs.join(lab)).unwrap();
        if let Some(m) = marker {
            std::fs::write(labs.join(lab).join(HOST_MARKER), m).unwrap();
        }
    }
    let mut markers = markers_in(&labs);
    markers.sort();
    assert_eq!(markers, ["", "pve1", "pve1"]);
    assert_eq!(count_by_host(&markers).get("pve1"), Some(&2));
    std::fs::remove_dir_all(labs).unwrap();
}

#[test]
fn running_labs_count_per_host_and_skip_blank_markers() {
    let markers: Vec<String> = ["ab", "cd", "ab", ""].iter().map(ToString::to_string).collect();
    let counts = count_by_host(&markers);
    assert_eq!(counts.get("ab"), Some(&2));
    assert_eq!(counts.get("cd"), Some(&1));
    assert_eq!(counts.len(), 2);
}
