//! The launcher's additions to a lab's spec: machines it adds to the lab (the attack box on a
//! cloud lab run as one VM per machine), kept beside the lab in [`FILE`], a YAML layer merged into
//! its `isoloom.yml` with Isoloom's own merge each time the lab is read. The published spec is
//! never written: taking a machine out removes it from the layer, and a lab with no additions has
//! no layer at all.

use std::path::Path;

use serde_yaml_ng::{Mapping, Value};

use crate::error::{Error, Result};

/// The layer, in the lab's folder.
pub(crate) const FILE: &str = ".cyberctf-overlay.yml";

/// The lab's spec text in `dir`, with the layer merged in when there is one.
pub(super) fn spec_text(dir: &Path) -> isoloom_spec::Result<String> {
    let path = isoloom_spec::find(dir)?;
    let text = std::fs::read_to_string(&path).map_err(|source| isoloom_spec::Error::Read { path, source })?;
    let Some(layer) = read(dir) else { return Ok(text) };
    isoloom_spec::defaults::override_spec_with(&text, Some(&layer), &[])
}

/// Adds machine `name`, as `machine` (its spec, as `isoloom.yml` writes a machine), to the layer.
pub(crate) fn add_machine(dir: &Path, name: &str, machine: Value) -> Result<()> {
    let mut layer = read(dir).unwrap_or_else(|| Value::Mapping(Mapping::new()));
    let root = layer.as_mapping_mut().ok_or_else(|| Error::Invalid(format!("{FILE} isn't a YAML mapping")))?;
    let machines = root.entry(Value::from("machines")).or_insert_with(|| Value::Mapping(Mapping::new()));
    let machines = machines.as_mapping_mut().ok_or_else(|| Error::Invalid(format!("{FILE}: `machines` isn't a mapping")))?;
    machines.insert(Value::from(name), machine);
    write(dir, &layer)
}

/// Takes machine `name` out of the layer; the layer goes once it holds nothing.
pub(crate) fn remove_machine(dir: &Path, name: &str) -> Result<()> {
    let Some(mut layer) = read(dir) else { return Ok(()) };
    if let Some(machines) = layer.get_mut("machines").and_then(Value::as_mapping_mut) {
        machines.remove(name);
        if machines.is_empty() {
            layer.as_mapping_mut().map(|m| m.remove("machines"));
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

/// Whether the layer holds machine `name`.
pub(crate) fn has_machine(dir: &Path, name: &str) -> bool {
    read(dir).is_some_and(|l| l.get("machines").and_then(|m| m.get(name)).is_some())
}

/// The layer, when the lab has one.
fn read(dir: &Path) -> Option<Value> {
    let text = std::fs::read_to_string(dir.join(FILE)).ok()?;
    serde_yaml_ng::from_str(&text).ok()
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
        assert_eq!(spec_text(&dir).unwrap(), SPEC, "no layer: the spec as published");
        let machine: Value = serde_yaml_ng::from_str("{ os: kali@rolling, networks: { lan: 249 } }").unwrap();
        add_machine(&dir, "extra", machine).unwrap();
        assert!(has_machine(&dir, "extra"));
        let spec = isoloom_spec::parse(&spec_text(&dir).unwrap()).unwrap();
        let names: Vec<&str> = spec.machines.keys().map(String::as_str).collect();
        assert_eq!(names, ["web", "extra"]);
        assert_eq!(std::fs::read_to_string(dir.join("isoloom.yml")).unwrap(), SPEC, "never written");
        remove_machine(&dir, "extra").unwrap();
        assert!(!dir.join(FILE).exists(), "an empty layer goes");
        assert_eq!(spec_text(&dir).unwrap(), SPEC);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
