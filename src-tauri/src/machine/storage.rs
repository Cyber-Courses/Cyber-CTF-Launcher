//! What Cyber CTF stores on this machine, for the Machine page: the container images and
//! Vagrant boxes installed labs (and the self-tests) use, with sizes, and a clean-up that removes
//! only those. Images still used by a container, or boxes used by a VM, are left alone
//! (Docker/Vagrant refuse).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::AppHandle;
use serde::Serialize;

use crate::error::Result;
use crate::exec::run;
use crate::machine::{installed_labs, selftest};
use crate::runtime::providers::Provider;

/// `docker image inspect` format: the image's name and its size in bytes. An image pulled only by
/// digest (the pinned attack box) has no tag, so its name is then its `repo@sha256:…` reference.
const IMAGE_FORMAT: &str = "{{if .RepoTags}}{{index .RepoTags 0}}{{else}}{{index .RepoDigests 0}}{{end}}\t{{.Size}}";

/// The local hypervisors the VM self-test can use, whose test boxes count as the app's.
const LOCAL_PROVIDERS: [Provider; 7] =
    [Provider::Virtualbox, Provider::VmwareDesktop, Provider::Parallels, Provider::Utm, Provider::Libvirt, Provider::Qemu, Provider::Hyperv];

#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct StoredItem {
    pub name: String,
    pub bytes: u64,
}

#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct Storage {
    /// Container images labs, the attack box and the self-test use, present locally.
    pub images: Vec<StoredItem>,
    /// Vagrant boxes VM labs and the self-test use, present locally.
    pub boxes: Vec<StoredItem>,
}

impl Storage {
    fn total(&self) -> u64 {
        self.images.iter().chain(&self.boxes).map(|i| i.bytes).sum()
    }
}

/// Images an installed Docker lab's compose file references.
async fn lab_images(dir: &Path, id: &str) -> Vec<String> {
    let project = format!("cyberctf-{id}");
    let file = crate::runtime::lab::compose_file(dir).display().to_string();
    run("docker", &["compose", "-p", &project, "-f", &file, "config", "--images"], Some(dir))
        .await
        .map(|o| o.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect())
        .unwrap_or_default()
}

/// Boxes a Vagrantfile names (`config.vm.box = "..."`, any variable name).
fn vagrantfile_boxes(path: &Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(path) else { return Vec::new() };
    text.lines().filter_map(box_assignment).collect()
}

/// The box a Vagrantfile line sets, quoted either way.
fn box_assignment(line: &str) -> Option<String> {
    let l = line.trim();
    // A commented-out alternative is not a box the lab uses (Cleanup would remove it).
    if l.starts_with('#') {
        return None;
    }
    let i = l.find(".vm.box")?;
    let rest = l[i + ".vm.box".len()..].trim_start();
    let rest = rest.strip_prefix('=')?.trim_start();
    let q = rest.chars().next().filter(|c| *c == '"' || *c == '\'')?;
    let rest = &rest[1..];
    let name = &rest[..rest.find(q)?];
    // A box is `owner/name` (or a bare name): nothing that would point at Vagrant's boxes folder
    // itself (empty, `..`) or read as an option, which Cleanup would measure and remove.
    let plain =
        !name.is_empty() && !name.starts_with('-') && !name.contains('\\') && name.split('/').all(|part| !part.is_empty() && part != "." && part != "..");
    plain.then(|| name.to_string())
}

fn vagrant_home() -> Option<PathBuf> {
    std::env::var_os("VAGRANT_HOME").map(PathBuf::from).or_else(|| crate::platform::home_dir().map(|h| h.join(".vagrant.d")))
}

/// Where Vagrant keeps a box (`owner/name` -> `owner-VAGRANTSLASH-name`).
fn box_dir(vagrant_home: &Path, name: &str) -> PathBuf {
    vagrant_home.join("boxes").join(name.replace('/', "-VAGRANTSLASH-"))
}

fn dir_size(path: &Path) -> u64 {
    let Ok(meta) = std::fs::symlink_metadata(path) else { return 0 };
    if meta.is_file() {
        return meta.len();
    }
    if !meta.is_dir() {
        return 0;
    }
    std::fs::read_dir(path).map(|it| it.flatten().map(|e| dir_size(&e.path())).sum()).unwrap_or(0)
}

/// Every image and box Cyber CTF uses, by name (present or not).
async fn wanted(app: &AppHandle, extra_images: &[String]) -> (BTreeSet<String>, BTreeSet<String>) {
    let mut images: BTreeSet<String> = extra_images.iter().filter(|i| crate::runtime::valid_image(i)).cloned().collect();
    images.insert(selftest::IMAGE.to_string());
    let mut boxes = BTreeSet::new();
    for (id, dir) in installed_labs(app) {
        // The lab's generated files (written when it last ran on that target).
        if crate::runtime::lab::compose_file(&dir).is_file() {
            images.extend(lab_images(&dir, &id).await);
        }
        let out = crate::runtime::lab::out(&dir);
        boxes.extend(vagrantfile_boxes(&out.join("vagrant/Vagrantfile")));
        boxes.extend(vagrantfile_boxes(&out.join("docker-vm/Vagrantfile")));
    }
    let arm = std::env::consts::ARCH == "aarch64";
    for p in LOCAL_PROVIDERS {
        boxes.extend(selftest::candidate_boxes(p, arm).iter().map(|b| b.to_string()));
    }
    (images, boxes)
}

/// `docker image inspect` lines for `images`. One call for all; it fails if any image is missing
/// but still prints the ones it found, so then each is asked on its own.
async fn inspect_images(images: &BTreeSet<String>) -> String {
    let mut args = vec!["image", "inspect", "--format", IMAGE_FORMAT];
    args.extend(images.iter().map(String::as_str));
    if let Ok(o) = run("docker", &args, None).await {
        return o;
    }
    let mut o = String::new();
    for img in images {
        if let Ok(line) = run("docker", &["image", "inspect", "--format", IMAGE_FORMAT, img], None).await {
            o.push_str(&line);
        }
    }
    o
}

/// The images in `docker image inspect` lines of `name\tsize`, each once: two references to one
/// image (the pinned attack box and its old `latest` tag) are one download on disk.
fn parse_images(out: &str) -> Vec<StoredItem> {
    let mut seen = BTreeSet::new();
    out.lines()
        .filter_map(|line| {
            let (name, size) = line.split_once('\t')?;
            Some(StoredItem { name: name.to_string(), bytes: size.trim().parse().unwrap_or(0) })
        })
        .filter(|i| seen.insert(i.name.clone()))
        .collect()
}

/// The boxes of `boxes` downloaded under `home`, with their size on disk.
fn present_boxes(home: &Path, boxes: &BTreeSet<String>) -> Vec<StoredItem> {
    boxes
        .iter()
        .filter_map(|b| {
            let dir = box_dir(home, b);
            dir.is_dir().then(|| StoredItem { name: b.clone(), bytes: dir_size(&dir) })
        })
        .collect()
}

async fn present(images: &BTreeSet<String>, boxes: &BTreeSet<String>) -> Storage {
    let found_images = if images.is_empty() { Vec::new() } else { parse_images(&inspect_images(images).await) };
    let found_boxes = match vagrant_home() {
        Some(home) => present_boxes(&home, boxes),
        None => Vec::new(),
    };
    Storage { images: found_images, boxes: found_boxes }
}

/// `extra_images`: images the UI knows about that the backend doesn't (the attack-box image setting).
#[tauri::command]
pub async fn machine_storage(app: AppHandle, extra_images: Vec<String>) -> Storage {
    let (images, boxes) = wanted(&app, &extra_images).await;
    present(&images, &boxes).await
}

/// Removes the images and boxes `machine_storage` lists. Ones still in use stay. Returns
/// the bytes freed.
#[tauri::command]
pub async fn machine_storage_clean(app: AppHandle, extra_images: Vec<String>) -> Result<u64> {
    let (images, boxes) = wanted(&app, &extra_images).await;
    let before = present(&images, &boxes).await;
    for img in &before.images {
        let _ = run("docker", &["image", "rm", &img.name], None).await;
    }
    for b in &before.boxes {
        // No --force: Vagrant refuses to remove a box a VM still uses.
        let _ = run("vagrant", &["box", "remove", &b.name, "--all"], None).await;
    }
    let after = present(&images, &boxes).await;
    Ok(before.total().saturating_sub(after.total()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_boxes_from_vagrantfiles() {
        let dir = std::env::temp_dir().join("cyberctf-vf-test");
        std::fs::create_dir_all(&dir).unwrap();
        let vf = dir.join("Vagrantfile");
        std::fs::write(&vf, "Vagrant.configure('2') do |c|\n  c.vm.box = \"bento/debian-12\"\n  dc.vm.box = 'gusztavvargadr/windows-server'\nend\n").unwrap();
        assert_eq!(vagrantfile_boxes(&vf), vec!["bento/debian-12", "gusztavvargadr/windows-server"]);
    }

    #[test]
    fn box_lines_need_an_assignment_and_quotes() {
        assert_eq!(box_assignment("config.vm.box_version = \"1\""), None);
        assert_eq!(box_assignment("config.vm.box = box_name"), None);
        assert_eq!(box_assignment("  x.vm.box='a/b' # comment").as_deref(), Some("a/b"));
    }

    #[test]
    fn box_lines_without_spaces_or_with_look_alike_keys_and_image_lines_on_windows_endings() {
        assert_eq!(box_assignment("config.vm.box=\"a/b\"").as_deref(), Some("a/b"));
        assert_eq!(box_assignment("  m.vm.box = \"a/b\" if ENV[\"X\"]").as_deref(), Some("a/b"));
        // `box_url`, `box_version` and `box_check_update` share the prefix but set no box.
        for other in ["config.vm.box_url = \"https://x/a.box\"", "config.vm.box_check_update = \"no\"", "config.vm.box_download_insecure = \"1\""] {
            assert_eq!(box_assignment(other), None, "{other}");
        }
        assert_eq!(box_assignment("config.vm.box = \"\""), None, "an empty box names no box");
        // `docker image inspect` on Windows: CRLF, and a size that isn't a number reads as 0.
        let items = parse_images("busybox:1.36\t4261550\r\nother:1\t-5\r\n");
        assert_eq!(items.iter().map(|i| (i.name.as_str(), i.bytes)).collect::<Vec<_>>(), [("busybox:1.36", 4_261_550), ("other:1", 0)]);
    }

    #[test]
    fn a_commented_out_box_line_is_not_a_box_the_lab_uses() {
        // A Vagrantfile keeping an alternative box in a comment lists that box as the lab's: it
        // shows under storage and `vagrant box remove --all` is run on it. Fix: skip a line
        // whose first non-blank character is `#` (and the part of a line after an unquoted `#`).
        assert_eq!(box_assignment("# config.vm.box = \"old/box\""), None);
        assert_eq!(box_assignment("config.vm.box = \"a/b\" # was \"old/box\"").as_deref(), Some("a/b"));
    }

    #[test]
    fn boxes_live_under_vagrants_slash_escaped_names() {
        assert_eq!(box_dir(Path::new("/h/.vagrant.d"), "bento/debian-12"), Path::new("/h/.vagrant.d/boxes/bento-VAGRANTSLASH-debian-12"));
    }

    #[test]
    fn image_lines_give_names_and_sizes() {
        let items = parse_images("busybox:1.36\t4261550\ncyberctf/attack-box:latest\tnope\ngarbage\n");
        assert_eq!(items.len(), 2);
        assert_eq!((items[0].name.as_str(), items[0].bytes), ("busybox:1.36", 4261550));
        assert_eq!(items[1].bytes, 0);
    }

    #[test]
    fn present_boxes_are_the_downloaded_ones_with_their_size() {
        let home = std::env::temp_dir().join(format!("cyberctf-vhome-{}", rand::random::<u32>()));
        let bx = box_dir(&home, "bento/debian-12").join("0").join("virtualbox");
        std::fs::create_dir_all(&bx).unwrap();
        std::fs::write(bx.join("box.vmdk"), vec![0u8; 1000]).unwrap();
        std::fs::write(bx.join("metadata.json"), "{}").unwrap();
        let wanted: BTreeSet<String> = ["bento/debian-12".to_string(), "generic/alpine319".to_string()].into();
        let found = present_boxes(&home, &wanted);
        assert_eq!(found.len(), 1);
        assert_eq!((found[0].name.as_str(), found[0].bytes), ("bento/debian-12", 1002));
        assert_eq!(dir_size(&home.join("missing")), 0);
        let storage = Storage { images: vec![StoredItem { name: "i".into(), bytes: 5 }], boxes: found };
        assert_eq!(storage.total(), 1007);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[tokio::test]
    async fn storage_lists_the_labs_images_and_cleans_them() {
        use crate::exec::fake::{Tools, fail, ok};
        let t = crate::test_app::TestApp::new();
        let lab = t.lab("web", &[]);
        let compose = crate::runtime::lab::compose_file(&lab);
        std::fs::create_dir_all(compose.parent().unwrap()).unwrap();
        std::fs::write(&compose, "services: {}\n").unwrap();
        let tools = Tools::new(|_, args| match args {
            [.., "config", "--images"] => ok("nginx:1.27\n\n"),
            // One call for all fails (an image is missing): each is then asked on its own.
            ["image", "inspect", "--format", _, "nginx:1.27"] => ok("nginx:1.27\t1000\n"),
            ["image", "inspect", ..] => fail("No such image"),
            _ => ok(""),
        });
        let s = machine_storage(t.handle().clone(), vec!["ghcr.io/x/box:1".into(), "-bad".into()]).await;
        assert_eq!(s.images.iter().map(|i| (i.name.as_str(), i.bytes)).collect::<Vec<_>>(), [("nginx:1.27", 1000)]);
        assert!(!tools.called("-bad"), "an invalid image is never passed to docker");
        let freed = machine_storage_clean(t.handle().clone(), Vec::new()).await.unwrap();
        assert!(tools.called("docker image rm nginx:1.27"));
        assert_eq!(freed, 0, "docker still lists it, so nothing was freed");
    }

    #[test]
    fn a_missing_vagrantfile_names_no_boxes() {
        assert!(vagrantfile_boxes(Path::new("/definitely/not/here/Vagrantfile")).is_empty());
        assert_eq!(box_assignment("config.vm.box = \"unterminated"), None);
        assert_eq!(box_assignment("no box here"), None);
    }
}

#[cfg(test)]
mod proptests;
