//! A port inside a lab's machine and the port it is reached at on the host, kept apart by type so
//! one is never shown or looked up where the other is meant: the lab page's ports, the port a
//! clash names, the holder lookup. Both serialize as the bare number.

use std::fmt;

use serde::{Deserialize, Serialize};

/// A port inside a machine (a container's target port, a VM's own port): nothing on this computer
/// answers at it as it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct GuestPort(u16);

/// The port a machine's port is reached at on the host (this computer's loopback for a local lab).
/// What a port clash names and the holder lookup asks about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct HostPort(u16);

impl GuestPort {
    /// The port `number` inside a machine.
    pub const fn new(number: u16) -> GuestPort {
        GuestPort(number)
    }

    /// Its number.
    pub const fn get(self) -> u16 {
        self.0
    }
}

impl HostPort {
    /// The host port `number`, as the runner or the engine reports it.
    pub const fn new(number: u16) -> HostPort {
        HostPort(number)
    }

    /// Its number.
    pub const fn get(self) -> u16 {
        self.0
    }
}

impl fmt::Display for GuestPort {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl fmt::Display for HostPort {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_kind_is_the_bare_number_on_the_wire() {
        assert_eq!(serde_json::to_string(&HostPort::new(8080)).unwrap(), "8080");
        assert_eq!(serde_json::to_string(&GuestPort::new(80)).unwrap(), "80");
    }

    /// The lab page reads a port as `{published, target}`, `published` 0 when it isn't, with its
    /// `login` only when the lab declares one.
    #[test]
    fn a_lab_port_reads_as_the_ui_expects() {
        let port = |published, target| crate::runtime::Port { published, target: GuestPort::new(target), login: None };
        assert_eq!(serde_json::to_value(port(Some(HostPort::new(8080)), 80)).unwrap(), serde_json::json!({"published": 8080, "target": 80}));
        assert_eq!(serde_json::to_value(port(None, 22)).unwrap(), serde_json::json!({"published": 0, "target": 22}));
        let login = crate::runtime::Login { user: "admin".into(), password: "admin".into() };
        let with_login = crate::runtime::Port { login: Some(login), ..port(Some(HostPort::new(8080)), 80) };
        assert_eq!(
            serde_json::to_value(with_login).unwrap(),
            serde_json::json!({"published": 8080, "target": 80, "login": {"user": "admin", "password": "admin"}})
        );
    }
}
