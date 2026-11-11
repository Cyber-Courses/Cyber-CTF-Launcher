//! KVM on Linux: whether a provider boots its VMs through `/dev/kvm` (libvirt, and QEMU with
//! `accel=kvm`), whether this account may open it, and the groups a player joins for it, which
//! apply from their next login.

use super::{Provider, Unavailable};

/// Why KVM's hypervisors can't run VMs here although they are installed: vagrant-libvirt and
/// vagrant-qemu boot KVM guests (QEMU with `accel=kvm`, as every lab asks on Linux), so they need
/// `/dev/kvm`, and the player must be allowed to open it.
pub(crate) fn kvm_problem() -> Option<Unavailable> {
    if devices_assumed_ready() {
        return None;
    }
    let dev = std::path::Path::new(KVM_DEVICE);
    if !dev.exists() {
        return Some(Unavailable::NoKvm);
    }
    if std::fs::OpenOptions::new().read(true).write(true).open(dev).is_err() {
        return Some(Unavailable::KvmDenied { group: kvm_group() });
    }
    None
}

/// The KVM device.
const KVM_DEVICE: &str = "/dev/kvm";

#[cfg(test)]
thread_local! {
    /// A test's word that this host's devices (`/dev/kvm`, VirtualBox's driver) are usable.
    static DEVICES_READY: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// For a test whose hypervisors are fakes (the tools answer as told): takes this host's devices
/// (`/dev/kvm`, VirtualBox's driver) as usable on the test's thread, whatever the machine running
/// the tests has (a CI runner's `/dev/kvm` is often root's alone, and no driver is loaded).
#[cfg(test)]
pub(crate) fn assume_devices_ready() {
    DEVICES_READY.with(|ready| ready.set(true));
}

/// Whether a test said this host's devices are usable ([`assume_devices_ready`]); never outside
/// the tests.
pub(super) fn devices_assumed_ready() -> bool {
    #[cfg(test)]
    {
        DEVICES_READY.with(std::cell::Cell::get)
    }
    #[cfg(not(test))]
    {
        false
    }
}

/// The group a player joins to open `/dev/kvm`: the one it belongs to (`kvm` wherever a
/// distribution makes it `0660`), else `kvm`, the name every distribution gives it.
pub(crate) fn kvm_group() -> String {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        let gid = std::fs::metadata(KVM_DEVICE).ok().map(|m| m.gid());
        let groups = std::fs::read_to_string("/etc/group").unwrap_or_default();
        group_of(gid, &groups)
    }
    #[cfg(not(unix))]
    {
        "kvm".to_owned()
    }
}

/// The name of group `gid` in `etc_group` (`/etc/group`'s text), when it is one a player joins
/// (not root's); else `kvm`.
#[cfg(any(unix, test))]
fn group_of(gid: Option<u32>, etc_group: &str) -> String {
    gid.filter(|g| *g != 0)
        .and_then(|gid| {
            etc_group.lines().find_map(|l| {
                let mut fields = l.split(':');
                let name = fields.next()?;
                (fields.nth(1)?.parse::<u32>().ok()? == gid).then(|| name.to_owned())
            })
        })
        .unwrap_or_else(|| "kvm".to_owned())
}

/// The groups libvirt's socket and `/dev/kvm` are opened through.
const LIBVIRT_GROUPS: [&str; 2] = ["libvirt", "kvm"];

/// Whether `provider` boots KVM guests on `os`, through `/dev/kvm`: libvirt, and QEMU on Linux
/// (vagrant-qemu asks for `accel=kvm` there, as every lab's Vagrantfile does; HVF on a Mac).
pub fn uses_kvm(provider: Provider, os: &str) -> bool {
    provider == Provider::Libvirt || (provider == Provider::Qemu && os == "linux")
}

/// The groups a player joins to use `provider` on `os`, which apply from their next login:
/// libvirt's and `/dev/kvm`'s for libvirt, `/dev/kvm`'s for QEMU on Linux ([`kvm_group`]); none
/// for another.
pub fn login_groups(provider: Provider, os: &str) -> Vec<String> {
    match provider {
        Provider::Libvirt => {
            let mut groups: Vec<String> = LIBVIRT_GROUPS.map(str::to_owned).to_vec();
            let device = kvm_group();
            if !groups.contains(&device) {
                groups.push(device);
            }
            groups
        }
        Provider::Qemu if uses_kvm(provider, os) => vec![kvm_group()],
        _ => Vec::new(),
    }
}

/// The group a new login applies for `provider` ([`Unavailable::NewLogin`]): libvirt's for
/// libvirt, `/dev/kvm`'s for QEMU.
pub(super) fn new_login_group(provider: Provider) -> String {
    if provider == Provider::Libvirt { "libvirt".to_owned() } else { kvm_group() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_kvm_group_is_the_one_dev_kvm_belongs_to() {
        let etc_group = "root:x:0:\nkvm:x:993:\nlibvirt:x:994:ada\nhypervisors:x:1200:\n";
        assert_eq!(group_of(Some(993), etc_group), "kvm");
        assert_eq!(group_of(Some(1200), etc_group), "hypervisors", "a distribution's own name for it");
        assert_eq!(group_of(Some(0), etc_group), "kvm", "root's: a player doesn't join it");
        assert_eq!(group_of(Some(4242), etc_group), "kvm", "a group /etc/group doesn't name");
        assert_eq!(group_of(None, etc_group), "kvm", "no /dev/kvm");
    }
}
