//! Where a VM lab on an ESXi server runs, as the runner is told it.

use isothrum_runner::{RunOptions, SecretValue};

use super::super::lab;

/// Where a VM lab on an ESXi server runs (Vagrant's `vmware_esxi`): the server's id, so an
/// operation asked for elsewhere is refused; its settings as the manifest's params; its password,
/// given to every operation of the run (status and park too). The runner relays the host's SSH
/// and HTTPS itself where this machine can't reach it (macOS).
#[derive(Clone)]
pub(crate) struct EsxiPlace {
    /// The server's id.
    pub(crate) label: String,
    /// The server's name, as Servers shows it.
    pub(crate) name: String,
    /// The host's settings, by the manifest's param names.
    pub(crate) params: Vec<(String, String)>,
    password: SecretValue,
}

impl std::fmt::Debug for EsxiPlace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EsxiPlace").field("label", &self.label).field("params", &self.params).finish_non_exhaustive()
    }
}

impl EsxiPlace {
    /// Server `label`'s place (`name` in Servers), from the environment the launcher keeps for its
    /// labs (`lab::esxi_env`'s names): the settings the manifest takes, and the password.
    pub(crate) fn from_env(label: &str, name: &str, env: &[(String, String)]) -> EsxiPlace {
        let value = |name: &str| env.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone());
        EsxiPlace {
            label: label.to_owned(),
            name: name.to_owned(),
            params: lab::esxi::PARAMS.iter().filter_map(|name| value(name).map(|v| ((*name).to_owned(), v))).collect(),
            password: SecretValue::new(value(lab::esxi::PASSWORD).unwrap_or_default()),
        }
    }

    /// `opts` on this host: its id recorded, its settings as environment, its password among
    /// the secrets.
    pub(crate) fn onto(&self, opts: RunOptions) -> RunOptions {
        let mut opts = self.params.iter().fold(opts.with_host_label(self.label.clone()), |o, (k, v)| o.with_env(k.clone(), v.clone()));
        opts.secrets.insert(lab::esxi::PASSWORD, self.password.clone());
        opts
    }
}
