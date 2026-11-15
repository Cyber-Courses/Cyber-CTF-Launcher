//! The values a cloud-services lab's module is run with: its lab file's `vars` (the launch's
//! values filled in) and this machine's public IP in its `player_ip_var`, each checked against
//! the variables the module declares (read by the runner from the module itself, nothing run).
//! Isothrum hands each to Terraform as `TF_VAR_<name>`: a variable the module marks sensitive
//! goes as a secret, never recorded; the others as plain values, which the runner replays on the
//! run's later operations.

use isothrum_runner::InputVariable;

use super::lab_file::{self, LabFile};
use crate::error::{Error, Result};

/// The module's values for a start, by variable.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct ModuleValues {
    /// The values the module may show, recorded by the runner for the run's later operations.
    pub(crate) plain: Vec<(String, String)>,
    /// The values of the variables the module marks sensitive: passed to this start only.
    pub(crate) secret: Vec<(String, String)>,
}

/// The values the module, which declares `declared`, is started with: the lab file's `vars`
/// with the launch's values `launch` filled in, and `player_ip` (this machine's public IP) in the
/// lab file's `player_ip_var` as a one-address block (a list of one where the variable is a list
/// or a set).
///
/// # Errors
///
/// When the lab file sets a variable the module doesn't declare, takes a launch value the launch
/// didn't give, or leaves a variable the module needs (no default) without a value.
pub(crate) fn module_values(file: &LabFile, declared: &[InputVariable], launch: &[(String, String)], player_ip: Option<&str>) -> Result<ModuleValues> {
    let variable = |name: &str| {
        declared
            .iter()
            .find(|v| v.name == name)
            .ok_or_else(|| Error::Invalid(format!("this lab's cloud/lab.yml gives a value to `{name}`, which its module doesn't declare")))
    };
    let mut given: Vec<(&InputVariable, String)> = Vec::new();
    for (name, text) in &file.vars {
        let filled = lab_file::fill(text, "inputs", |input| launch.iter().find(|(k, _)| k == input).map(|(_, v)| v.clone()))
            .map_err(|input| Error::Invalid(format!("this lab's `{name}` takes the launch's `{input}`, which the launch didn't give")))?;
        given.push((variable(name)?, filled));
    }
    if let (Some(name), Some(ip)) = (file.player_ip_var.as_deref(), player_ip) {
        let var = variable(name)?;
        given.push((var, allow_list_value(var, ip)));
    }
    let missing: Vec<&str> = declared.iter().filter(|v| v.default.is_none() && !given.iter().any(|(g, _)| g.name == v.name)).map(|v| v.name.as_str()).collect();
    if !missing.is_empty() {
        return Err(Error::Invalid(format!("this lab's module needs a value for {}, which its cloud/lab.yml doesn't give", quoted(&missing))));
    }
    let (secret, plain): (Vec<_>, Vec<_>) = given.into_iter().partition(|(v, _)| v.sensitive);
    let named = |all: Vec<(&InputVariable, String)>| all.into_iter().map(|(v, value)| (v.name.clone(), value)).collect();
    Ok(ModuleValues { plain: named(plain), secret: named(secret) })
}

/// This machine's public IP `ip` as variable `var` takes it: a one-address block (`/32`, or
/// `/128` for an IPv6 address), in a list of one when the variable is a list or a set.
fn allow_list_value(var: &InputVariable, ip: &str) -> String {
    let block = if ip.contains(':') { format!("{ip}/128") } else { format!("{ip}/32") };
    let collection = var.type_constraint.as_deref().is_some_and(|t| t.trim_start().starts_with("list") || t.trim_start().starts_with("set"));
    if collection { serde_json::json!([block]).to_string() } else { block }
}

/// `names` as a sentence lists them: `a`, `b` and `c`.
fn quoted(names: &[&str]) -> String {
    let quoted: Vec<String> = names.iter().map(|n| format!("`{n}`")).collect();
    match quoted.split_last() {
        Some((last, rest)) if !rest.is_empty() => format!("{} and {last}", rest.join(", ")),
        _ => quoted.join(""),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A module's variable, as the runner reads it from its `variable` block.
    fn var(name: &str, type_constraint: Option<&str>, default: Option<i64>, sensitive: bool) -> InputVariable {
        serde_json::from_value(serde_json::json!({ "name": name, "type": type_constraint, "default": default, "sensitive": sensitive })).unwrap()
    }

    fn file(text: &str) -> LabFile {
        LabFile::parse(text).unwrap()
    }

    fn launch() -> Vec<(String, String)> {
        vec![("CTF_LAUNCH_TOKEN".into(), "t0k".into()), ("CTF_API_URL".into(), "https://api.example".into())]
    }

    #[test]
    fn the_lab_files_values_fill_the_launchs_and_a_sensitive_variable_goes_as_a_secret() {
        let declared = [var("region", Some("string"), None, false), var("token", Some("string"), None, true), var("size", None, Some(2), false)];
        let f = file("provider: aws\nvars:\n  region: us-east-1\n  token: \"{{ inputs.CTF_LAUNCH_TOKEN }}@{{ inputs.CTF_API_URL }}\"\n");
        let values = module_values(&f, &declared, &launch(), None).unwrap();
        assert_eq!(values.plain, [("region".to_owned(), "us-east-1".to_owned())]);
        assert_eq!(values.secret, [("token".to_owned(), "t0k@https://api.example".to_owned())], "never recorded");
    }

    #[test]
    fn the_player_ip_goes_in_its_variable_as_one_address_or_a_list_of_one() {
        let f = file("provider: aws\nplayer_ip_var: allowed\n");
        let string = module_values(&f, &[var("allowed", Some("string"), None, false)], &[], Some("203.0.113.7")).unwrap();
        assert_eq!(string.plain, [("allowed".to_owned(), "203.0.113.7/32".to_owned())]);
        let list = module_values(&f, &[var("allowed", Some("list(string)"), None, false)], &[], Some("2001:db8::7")).unwrap();
        assert_eq!(list.plain, [("allowed".to_owned(), r#"["2001:db8::7/128"]"#.to_owned())]);
        let set = module_values(&f, &[var("allowed", Some(" set(string)"), None, false)], &[], Some("203.0.113.7")).unwrap();
        assert_eq!(serde_json::from_str::<Vec<String>>(&set.plain[0].1).unwrap(), ["203.0.113.7/32"]);
    }

    #[test]
    fn a_value_the_module_doesnt_take_one_the_launch_lacks_or_one_it_needs_is_refused_by_name() {
        let declared = [var("region", None, None, false), var("name", None, None, false), var("zone", None, None, false)];
        let refused = |text: &str, ip: Option<&str>| module_values(&file(text), &declared, &launch(), ip).unwrap_err().to_string();
        assert_eq!(
            refused("provider: aws\nvars: { regoin: x, region: y, name: n, zone: z }\n", None),
            "this lab's cloud/lab.yml gives a value to `regoin`, which its module doesn't declare"
        );
        assert_eq!(
            refused("provider: aws\nvars: { region: \"{{ inputs.CTF_FLAG }}\" }\n", None),
            "this lab's `region` takes the launch's `CTF_FLAG`, which the launch didn't give"
        );
        assert_eq!(
            refused("provider: aws\nvars: { region: x }\n", None),
            "this lab's module needs a value for `name` and `zone`, which its cloud/lab.yml doesn't give"
        );
        assert_eq!(
            refused("provider: aws\nvars: { region: x, name: n, zone: z }\nplayer_ip_var: allowed\n", Some("203.0.113.7")),
            "this lab's cloud/lab.yml gives a value to `allowed`, which its module doesn't declare"
        );
    }

    #[test]
    fn a_list_of_names_reads_as_a_sentence() {
        assert_eq!(quoted(&["a"]), "`a`");
        assert_eq!(quoted(&["a", "b", "c"]), "`a`, `b` and `c`");
    }
}
