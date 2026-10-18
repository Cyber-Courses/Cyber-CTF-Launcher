//! The `ovftool` wrapper: vagrant-vmware-esxi uploads VMs with ovftool to a `vi://` URL it builds
//! from the host name alone, so the wrapper adds the HTTPS relay's port before running the real one.

use std::path::PathBuf;

/// `ovftool` that adds the relay's port to a `vi://…@127.0.0.1` target (the plugin builds the URL
/// from the host name alone, so it can't carry a port) and runs the real one.
const WRAPPER: &str = r#"#!/bin/sh
# Written by Cyber CTF: sends ovftool's ESXi target through the app's loopback relay.
n=$#; i=0
while [ "$i" -lt "$n" ]; do
  a=$1; shift
  case "$a" in
    vi://*@127.0.0.1*) a=$(printf '%s' "$a" | /usr/bin/sed -E "s#@127\.0\.0\.1(/|\$)#@127.0.0.1:${CYBERCTF_OVFTOOL_PORT}\1#") ;;
  esac
  set -- "$@" "$a"; i=$((i + 1))
done
exec "$CYBERCTF_REAL_OVFTOOL" "$@"
"#;

/// The ovftool the wrapper hands over to: the one on PATH (VMware's own bundle as a fallback).
pub(super) fn real_ovftool() -> Option<String> {
    let from_path = std::env::var_os("PATH").and_then(|p| std::env::split_paths(&p).map(|d| d.join("ovftool")).find(|f| f.is_file()));
    from_path
        .or_else(|| Some(PathBuf::from("/Applications/VMware Fusion.app/Contents/Library/VMware OVF Tool/ovftool")).filter(|f| f.is_file()))
        .map(|p| p.display().to_string())
}

/// A folder holding the wrapper as `ovftool` (written when missing or changed), to put first on
/// PATH.
pub(super) fn wrapper_dir() -> std::io::Result<PathBuf> {
    let dir = std::env::temp_dir().join("cyberctf-lan-relay");
    std::fs::create_dir_all(&dir)?;
    let f = dir.join("ovftool");
    if std::fs::read_to_string(&f).ok().as_deref() != Some(WRAPPER) {
        std::fs::write(&f, WRAPPER)?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o755))?;
    }
    Ok(dir)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn wrapper_adds_the_relay_port_to_the_vi_target() {
        let dir = wrapper_dir().unwrap();
        let out = std::process::Command::new(dir.join("ovftool"))
            .args(["--noSSLVerify", "box.vmx", "vi://root:p%40ss@127.0.0.1/pool"])
            .env("CYBERCTF_OVFTOOL_PORT", "40443")
            .env("CYBERCTF_REAL_OVFTOOL", "/bin/echo")
            .output()
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "--noSSLVerify box.vmx vi://root:p%40ss@127.0.0.1:40443/pool");
    }
}
