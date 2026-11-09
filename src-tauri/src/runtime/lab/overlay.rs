//! The launcher's additions to a lab's spec: machines it adds to the lab (the attack box), kept
//! beside the lab in [`FILE`], a YAML layer merged into its `isoloom.yml` with Isoloom's own merge
//! each time the lab is read. A machine of the layer may replace one of the spec wholesale (its
//! name in the layer's `replace:` list, which only the launcher reads): the spec's machine is
//! dropped before the merge, so none of its fields carry over. The published spec is never
//! written: taking a machine out removes it from the layer, and a lab with no additions has no
//! layer at all.

use std::path::Path;

use serde_yaml_ng::{Mapping, Value};

use crate::error::{Error, Result};

/// The layer, in the lab's folder.
pub(crate) const FILE: &str = ".cyberctf-overlay.yml";

/// The layer's own key: the spec machines its machines replace wholesale.
const REPLACE: &str = "replace";

/// The lab's spec text in `dir`, with the layer merged in when there is one. The outer error is
/// the layer's (it exists but can't be read: never taken for no layer, which would quietly drop
/// the launcher's machines from the lab); the inner one the spec's.
pub(super) fn spec_text(dir: &Path) -> Result<isoloom_spec::Result<String>> {
    let layer = read(dir)?;
    Ok(merged(dir, layer))
}

/// The spec in `dir` with `layer` merged in.
fn merged(dir: &Path, layer: Option<Value>) -> isoloom_spec::Result<String> {
    let path = isoloom_spec::find(dir)?;
    let text = std::fs::read_to_string(&path).map_err(|source| isoloom_spec::Error::Read { path, source })?;
    let Some(mut layer) = layer else { return Ok(text) };
    let replaced = layer.as_mapping_mut().and_then(|m| m.remove(REPLACE));
    let Some(replaced) = replaced.as_ref().and_then(Value::as_sequence).filter(|r| !r.is_empty()) else {
        return isoloom_spec::defaults::override_spec_with(&text, Some(&layer), &[]);
    };
    let mut spec: Value = serde_yaml_ng::from_str(&text)?;
    if let Some(machines) = spec.get_mut("machines").and_then(Value::as_mapping_mut) {
        for name in replaced {
            machines.remove(name);
        }
    }
    isoloom_spec::defaults::override_spec_with(&serde_yaml_ng::to_string(&spec)?, Some(&layer), &[])
}

/// Adds machine `name`, as `machine` (its spec, as `isoloom.yml` writes a machine), to the layer;
/// with `replaces`, in place of the spec's machine of that name (none of its fields kept).
pub(crate) fn add_machine(dir: &Path, name: &str, machine: Value, replaces: bool) -> Result<()> {
    let mut layer = read(dir)?.unwrap_or_else(|| Value::Mapping(Mapping::new()));
    let root = layer.as_mapping_mut().ok_or_else(|| Error::Invalid(format!("{FILE} isn't a YAML mapping")))?;
    let machines = root.entry(Value::from("machines")).or_insert_with(|| Value::Mapping(Mapping::new()));
    let machines = machines.as_mapping_mut().ok_or_else(|| Error::Invalid(format!("{FILE}: `machines` isn't a mapping")))?;
    machines.insert(Value::from(name), machine);
    if replaces {
        let list = root.entry(Value::from(REPLACE)).or_insert_with(|| Value::Sequence(Vec::new()));
        let list = list.as_sequence_mut().ok_or_else(|| Error::Invalid(format!("{FILE}: `{REPLACE}` isn't a list")))?;
        if !list.iter().any(|n| n.as_str() == Some(name)) {
            list.push(Value::from(name));
        }
    }
    write(dir, &layer)
}

/// The layer's machines, by name.
pub(crate) fn machines(dir: &Path) -> Result<Vec<String>> {
    Ok(read(dir)?
        .and_then(|l| l.get("machines").and_then(Value::as_mapping).map(|m| m.keys().filter_map(Value::as_str).map(str::to_owned).collect()))
        .unwrap_or_default())
}

/// Takes machine `name` out of the layer (the spec's own machine of that name comes back); the
/// layer goes once it holds nothing.
pub(crate) fn remove_machine(dir: &Path, name: &str) -> Result<()> {
    let Some(mut layer) = read(dir)? else { return Ok(()) };
    if let Some(machines) = layer.get_mut("machines").and_then(Value::as_mapping_mut) {
        machines.remove(name);
        if machines.is_empty() {
            layer.as_mapping_mut().map(|m| m.remove("machines"));
        }
    }
    if let Some(list) = layer.get_mut(REPLACE).and_then(Value::as_sequence_mut) {
        list.retain(|n| n.as_str() != Some(name));
        if list.is_empty() {
            layer.as_mapping_mut().map(|m| m.remove(REPLACE));
        }
    }
    if layer.as_mapping().is_none_or(Mapping::is_empty) {
        return match std::fs::remove_file(dir.join(FILE)) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
            _ => Ok(()),
        };
    }
    write(dir, &layer)
}

/// The layer, when the lab has one. One that exists but can't be read or isn't YAML is an error.
fn read(dir: &Path) -> Result<Option<Value>> {
    let unreadable = |e: &dyn std::fmt::Display| Error::Invalid(format!("the launcher's additions to this lab ({FILE}) can't be read: {e}"));
    let text = match std::fs::read_to_string(dir.join(FILE)) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(unreadable(&e)),
    };
    serde_yaml_ng::from_str(&text).map(Some).map_err(|e| unreadable(&e))
}

fn write(dir: &Path, layer: &Value) -> Result<()> {
    let text = serde_yaml_ng::to_string(layer).map_err(|e| Error::Invalid(format!("{FILE}: {e}")))?;
    std::fs::write(dir.join(FILE), text)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPEC: &str = "version: 1\nname: t\nnetworks: { lan: { cidr: 10.70.0.0/24 } }\nmachines:\n  web: { os: debian@12, networks: { lan: 10 } }\n";

    fn lab() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("cyberctf-overlay-{}", rand::random::<u32>()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("isoloom.yml"), SPEC).unwrap();
        dir
    }

    #[test]
    fn a_machine_added_is_in_the_spec_read_and_gone_once_taken_out() {
        let dir = lab();
        assert_eq!(spec_text(&dir).unwrap().unwrap(), SPEC, "no layer: the spec as published");
        let machine: Value = serde_yaml_ng::from_str("{ os: kali@rolling, networks: { lan: 249 } }").unwrap();
        add_machine(&dir, "extra", machine, false).unwrap();
        assert_eq!(machines(&dir).unwrap(), ["extra"]);
        let spec = isoloom_spec::parse(&spec_text(&dir).unwrap().unwrap()).unwrap();
        let names: Vec<&str> = spec.machines.keys().map(String::as_str).collect();
        assert_eq!(names, ["web", "extra"]);
        assert_eq!(std::fs::read_to_string(dir.join("isoloom.yml")).unwrap(), SPEC, "never written");
        remove_machine(&dir, "extra").unwrap();
        assert!(!dir.join(FILE).exists(), "an empty layer goes");
        assert_eq!(spec_text(&dir).unwrap().unwrap(), SPEC);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_machine_that_replaces_one_of_the_spec_keeps_none_of_its_fields() {
        let dir = lab();
        let machine: Value = serde_yaml_ng::from_str("{ os: kali@rolling, networks: { lan: 10 } }").unwrap();
        add_machine(&dir, "web", machine, true).unwrap();
        let text = spec_text(&dir).unwrap().unwrap();
        isoloom_spec::parse(&text).unwrap();
        assert!(text.contains("kali@rolling") && !text.contains("debian@12"), "the layer's machine, not a merge: {text}");
        assert_eq!(machines(&dir).unwrap(), ["web"]);
        remove_machine(&dir, "web").unwrap();
        assert!(!dir.join(FILE).exists(), "nothing left: the layer goes");
        assert_eq!(spec_text(&dir).unwrap().unwrap(), SPEC, "the spec's own machine is back");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_corrupt_layer_is_an_error_never_taken_for_none() {
        let dir = lab();
        std::fs::write(dir.join(FILE), "machines: [unclosed\n").unwrap();
        let unreadable = |e: Error| assert!(e.to_string().contains(FILE), "names the layer: {e}");
        unreadable(spec_text(&dir).unwrap_err());
        unreadable(machines(&dir).unwrap_err());
        unreadable(add_machine(&dir, "extra", Value::Mapping(Mapping::new()), false).unwrap_err());
        unreadable(remove_machine(&dir, "extra").unwrap_err());
        assert_eq!(std::fs::read_to_string(dir.join(FILE)).unwrap(), "machines: [unclosed\n", "left as it was");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
