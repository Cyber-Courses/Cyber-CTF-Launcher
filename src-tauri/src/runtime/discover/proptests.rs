//! Property tests for the running-labs scan's parsers: whatever Docker, VirtualBox or vmrun
//! print, what comes back is a plausible lab id (never a stray token) and real listings read back.

use std::collections::HashMap;

use proptest::prelude::*;

use super::*;
use crate::proptest_support::config;

fn lab_id() -> impl Strategy<Value = String> {
    "[a-zA-Z0-9_-]{1,64}"
}

/// A lab's name as isoloom-core accepts it: kebab-case.
fn kebab() -> impl Strategy<Value = String> {
    "[a-z0-9]{1,8}(-[a-z0-9]{1,8}){0,3}"
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn parsers_never_panic_and_only_give_lab_ids(out in "(?s).{0,300}", path_ish in "[/\\\\a-z.ls_ -]{0,60}") {
        for text in [&out, &path_ish] {
            for id in lab_ids_from_docker_projects(text).into_iter().chain(lab_ids_from_vmrun(text)) {
                prop_assert!(is_lab_id(&id), "{:?}", id);
            }
            let _ = vbox_vm_names(text);
            let _ = isoloom_name(text);
            if let Some(id) = id_after_labs(text) {
                prop_assert!(is_lab_id(&id), "{:?}", id);
            }
        }
    }

    #[test]
    fn docker_projects_read_back_sorted_and_unique(
        ids in proptest::collection::vec(lab_id(), 0..6),
        others in proptest::collection::vec(prop_oneof!["[a-z-]{0,12}", "cyberctf-[a-z0-9]{1,6}-attack\\.[a-z]{1,3}", "cyberctf-"], 0..4),
        pad in "[ \t]{0,2}",
    ) {
        let lines: Vec<String> = ids.iter().map(|id| format!("{pad}cyberctf-{id}{pad}")).chain(others.iter().cloned()).collect();
        let got = lab_ids_from_docker_projects(&lines.join("\n"));
        let mut want: Vec<String> = ids.clone();
        want.extend(others.iter().filter_map(|o| o.strip_prefix("cyberctf-")).filter(|id| is_lab_id(id)).map(str::to_string));
        want.sort();
        want.dedup();
        prop_assert_eq!(got, want);
    }

    /// A VM under `<anything>/labs/<id>/.isoloom/...` names its lab, Unix or Windows path, even
    /// below another folder called `labs`.
    #[test]
    fn vmrun_paths_name_their_lab(
        id in lab_id(),
        decoy in proptest::option::of(lab_id()),
        prefix in proptest::collection::vec("[A-Za-z0-9 ._-]{1,10}", 0..4),
        layout in prop_oneof![Just(".isoloom/vagrant/.vagrant/machines/dc01/vmware_desktop/abc"), Just(".isoloom-x/y"), Just(".cyberctf-attack-vm/.vagrant/m")],
        windows in any::<bool>(),
    ) {
        let mut segs: Vec<String> = vec![if windows { "C:".into() } else { String::new() }];
        if let Some(d) = &decoy {
            segs.extend(["labs".to_string(), d.clone()]);
        }
        segs.extend(prefix);
        segs.extend(["labs".to_string(), id.clone(), layout.to_string(), "vm.vmx".to_string()]);
        let path = segs.join(if windows { "\\" } else { "/" });
        prop_assert_eq!(id_after_labs(&path), Some(id.clone()));
        let listing = format!("Total running VMs: 1\n{path}\n");
        prop_assert_eq!(lab_ids_from_vmrun(&listing), vec![id]);
    }

    #[test]
    fn vbox_names_read_back(names in proptest::collection::vec("[^\"\n\r]{0,20}", 0..5), uuid in "[0-9a-f-]{36}") {
        let out: String = names.iter().map(|n| format!("\"{n}\" {{{uuid}}}\n")).collect();
        prop_assert_eq!(vbox_vm_names(&out), names);
    }

    /// The name the scan reads is the one the spec parser (and so the VM names) reads, however the
    /// line is written.
    #[test]
    fn isoloom_name_agrees_with_the_yaml_parser(
        name in kebab(),
        quote in prop_oneof![Just(""), Just("\""), Just("'")],
        sep in "[ ]{1,3}",
        comment in proptest::option::of("[ \t]{1,3}#[ -~]{0,20}"),
        trailing in "[ ]{0,2}",
        crlf in any::<bool>(),
        before in proptest::collection::vec(prop_oneof!["version: [0-9]", "# name: [a-z]{1,5}", "machines:\n  [a-z]{1,5}:\n    name: [a-z]{1,5}", "namespace: [a-z]{1,4}"], 0..3),
    ) {
        let nl = if crlf { "\r\n" } else { "\n" };
        let line = format!("name:{sep}{quote}{name}{quote}{}{trailing}", comment.unwrap_or_default());
        let yaml = before.iter().map(|b| b.replace('\n', nl)).chain([line, "networks: {}".into()]).collect::<Vec<_>>().join(nl);
        // Two of the same key before it is not a spec.
        let parsed: Result<serde_yaml_ng::Value, _> = serde_yaml_ng::from_str(&yaml);
        prop_assume!(parsed.is_ok());
        let parsed = parsed.unwrap();
        // A bare `0` or `true` is not a string: no spec has it as its name.
        prop_assume!(parsed["name"].as_str() == Some(name.as_str()));
        prop_assert_eq!(isoloom_name(&yaml), Some(name), "{:?}", yaml);
    }

    #[test]
    fn vms_map_only_to_installed_labs_and_never_by_the_attacker_alone(
        labs in proptest::collection::hash_map("[a-z0-9-]{1,10}", lab_id(), 0..4),
        vms in proptest::collection::vec((prop_oneof!["[a-z0-9-]{1,10}", "\\PC{0,12}"], prop_oneof![Just("attacker".to_string()), "[a-z0-9]{1,6}"], any::<bool>()), 0..8),
    ) {
        let by_name: HashMap<String, String> = labs.clone();
        let names: Vec<String> = vms
            .iter()
            .map(|(lab, machine, known)| {
                let lab = if *known { labs.keys().next().cloned().unwrap_or_else(|| lab.clone()) } else { lab.clone() };
                format!("{lab}{VM_NAME_SEP}{machine}")
            })
            .collect();
        let got = lab_ids_from_vm_names(names.iter().map(String::as_str), &by_name);
        prop_assert!(got.windows(2).all(|w| w[0] < w[1]), "sorted and unique");
        for id in &got {
            let name = by_name.iter().find(|(_, v)| *v == id).map(|(k, _)| k.clone()).unwrap();
            prop_assert!(names.iter().any(|n| n.split_once(VM_NAME_SEP).is_some_and(|(l, m)| l == name && m != "attacker")));
        }
    }
}
