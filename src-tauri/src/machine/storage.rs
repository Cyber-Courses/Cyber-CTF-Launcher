//! What Cyber CTF stores on this machine, for the Machine page: the container images and
//! Vagrant boxes it downloaded itself (see `owned`), with sizes, and a clean-up that removes only
//! those. An image or box the player had before, or pulled for their own projects, is never
//! listed nor removed, even when a lab uses the same one. Images still used by a container, or
//! boxes used by a VM, are left alone (Docker/Vagrant refuse).

pub(crate) mod owned;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::AppHandle;
use serde::Serialize;

use crate::error::Result;
use crate::exec::run;
use owned::{Owned, OwnedBox};

/// `docker image inspect` format: the image's id and its size in bytes.
const IMAGE_FORMAT: &str = "{{.Id}}\t{{.Size}}";

/// The attack box's own repository. Its images are the app's by name: earlier launchers pulled
/// them before downloads were recorded, and nobody else publishes it.
const ATTACK_BOX_REPOSITORY: &str = "cyberctf/attack-box";

#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct StoredItem {
    pub name: String,
    pub bytes: u64,
}

#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct Storage {
    /// Container images the app downloaded, present locally.
    pub images: Vec<StoredItem>,
    /// Vagrant boxes the app downloaded, present locally.
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

/// The boxes a lab's generated Vagrantfiles name (one VM per machine, or Docker in one VM).
fn lab_boxes(dir: &Path) -> Vec<String> {
    let out = crate::runtime::lab::out(dir);
    ["vagrant", "docker-vm"].iter().flat_map(|target| vagrantfile_boxes(&out.join(target).join("Vagrantfile"))).collect()
}

/// Records as the app's what a lab start on this machine downloaded: the images its Compose file
/// names and the boxes its Vagrantfiles name, that weren't here `before` it.
pub(crate) async fn claim_lab(app: &AppHandle, dir: &Path, id: &str, before: &owned::Before) {
    let Ok(store) = owned::path(app) else { return };
    let images = if before.tracks_images() && crate::runtime::lab::compose_file(dir).is_file() { lab_images(dir, id).await } else { Vec::new() };
    owned::claim(&store, before, &images, &lab_boxes(dir)).await;
}

/// An image reference's repository, without a Docker Hub prefix, tag or digest.
fn repository(image: &str) -> &str {
    let image = image.split('@').next().unwrap_or(image);
    let image = match image.rfind(':') {
        Some(i) if !image[i..].contains('/') => &image[..i],
        _ => image,
    };
    image.strip_prefix("docker.io/").unwrap_or(image)
}

/// The images to list: the recorded ones, and of `extra_images` (the attack-box images the UI
/// knows about) those of the attack box's own repository, whatever image they name now.
fn image_candidates(owned: &Owned, extra_images: &[String]) -> Vec<(String, Option<String>)> {
    let launcher = extra_images.iter().filter(|i| crate::runtime::valid_image(i) && repository(i) == ATTACK_BOX_REPOSITORY).map(|i| (i.clone(), None));
    owned.images.iter().map(|i| (i.reference.clone(), Some(i.id.clone()))).chain(launcher).collect()
}

/// `docker image inspect` of `reference`: its image id and size.
async fn inspect_image(reference: &str) -> Option<(String, u64)> {
    run("docker", &["image", "inspect", "--format", IMAGE_FORMAT, reference], None).await.ok().and_then(|o| parse_image_line(&o))
}

/// An `id\tsize` line (a size that isn't a number reads as 0).
fn parse_image_line(out: &str) -> Option<(String, u64)> {
    let (id, size) = out.lines().next()?.split_once('\t')?;
    Some((id.trim().to_string(), size.trim().parse().unwrap_or(0))).filter(|(id, _)| !id.is_empty())
}

/// The app's images present now, as (reference, image id, size): a recorded reference only while
/// it still names the image the app pulled.
async fn present_images(candidates: &[(String, Option<String>)]) -> Vec<(String, String, u64)> {
    let mut found = Vec::new();
    for (reference, want) in candidates {
        if let Some((id, size)) = inspect_image(reference).await
            && want.as_ref().is_none_or(|w| *w == id)
        {
            found.push((reference.clone(), id, size));
        }
    }
    found
}

/// The listed images: one per image (two references to one image are one download on disk).
fn image_items(found: &[(String, String, u64)]) -> Vec<StoredItem> {
    let mut seen = BTreeSet::new();
    found.iter().filter(|(_, id, _)| seen.insert(id.clone())).map(|(reference, _, bytes)| StoredItem { name: reference.clone(), bytes: *bytes }).collect()
}

/// Where Vagrant keeps a box version for a provider: `<box>/<version>/<provider>`, or with the
/// architecture between (`<box>/<version>/<arch>/<provider>`).
fn box_version_dir(home: &Path, b: &OwnedBox) -> Option<PathBuf> {
    let version = box_dir(home, &b.name).join(&b.version);
    let direct = version.join(&b.provider);
    if direct.is_dir() {
        return Some(direct);
    }
    std::fs::read_dir(&version).ok()?.flatten().map(|arch| arch.path().join(&b.provider)).find(|p| p.is_dir())
}

/// The app's boxes downloaded under `home`, with their size on disk.
fn present_boxes(home: &Path, boxes: &BTreeSet<OwnedBox>) -> Vec<(OwnedBox, u64)> {
    boxes.iter().filter_map(|b| box_version_dir(home, b).map(|dir| (b.clone(), dir_size(&dir)))).collect()
}

/// How a box shows in the list: `name (provider, version)`, as `vagrant box list` says it.
fn box_label(b: &OwnedBox) -> String {
    format!("{} ({}, {})", b.name, b.provider, b.version)
}

/// What of the app's downloads is here now.
struct Present {
    images: Vec<(String, String, u64)>,
    boxes: Vec<(OwnedBox, u64)>,
}

impl Present {
    /// What of `owned` (and of `extra_images`) is here, boxes under Vagrant's `home`.
    async fn find(owned: &Owned, extra_images: &[String], home: Option<&Path>) -> Present {
        let images = present_images(&image_candidates(owned, extra_images)).await;
        let boxes = home.map(|home| present_boxes(home, &owned.boxes)).unwrap_or_default();
        Present { images, boxes }
    }

    fn storage(&self) -> Storage {
        Storage { images: image_items(&self.images), boxes: self.boxes.iter().map(|(b, bytes)| StoredItem { name: box_label(b), bytes: *bytes }).collect() }
    }
}

/// `extra_images`: the attack-box images the UI knows about; only those of the attack box's own
/// repository count (see [`image_candidates`]), any other is the player's unless recorded.
#[tauri::command]
pub async fn machine_storage(app: AppHandle, extra_images: Vec<String>) -> Result<Storage> {
    let owned = owned::load(&owned::path(&app)?);
    Ok(Present::find(&owned, &extra_images, vagrant_home().as_deref()).await.storage())
}

/// Removes what `machine_storage` lists (see [`clean`]). Returns the bytes freed.
#[tauri::command]
pub async fn machine_storage_clean(app: AppHandle, extra_images: Vec<String>) -> Result<u64> {
    clean(&owned::path(&app)?, &extra_images, vagrant_home().as_deref()).await
}

/// Removes the app's downloads recorded in `store` (and the attack box's of `extra_images`):
/// each image reference it pulled (untagged, so an image the player tagged too stays), each box
/// version for its provider (never `--all`). Ones still in use stay. Then forgets what is gone.
/// Returns the bytes freed.
async fn clean(store: &Path, extra_images: &[String], home: Option<&Path>) -> Result<u64> {
    let before = Present::find(&owned::load(store), extra_images, home).await;
    for (reference, _, _) in &before.images {
        let _ = run("docker", &["image", "rm", reference], None).await;
    }
    for (b, _) in &before.boxes {
        // No --force: Vagrant refuses to remove a box a VM still uses.
        let _ = run("vagrant", &["box", "remove", &b.name, "--provider", &b.provider, "--box-version", &b.version], None).await;
    }
    let after = Present::find(&owned::load(store), extra_images, home).await;
    let kept_images: BTreeSet<&str> = after.images.iter().map(|(r, _, _)| r.as_str()).collect();
    let kept_boxes: Vec<&OwnedBox> = after.boxes.iter().map(|(b, _)| b).collect();
    owned::update(store, |o| {
        o.images.retain(|i| kept_images.contains(i.reference.as_str()));
        o.boxes.retain(|b| kept_boxes.contains(&b));
    })?;
    Ok(before.storage().total().saturating_sub(after.storage().total()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exec::fake::{Reply, Tools, fail, ok};

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
        assert_eq!(parse_image_line("sha256:a\t4261550\r\n"), Some(("sha256:a".into(), 4_261_550)));
        assert_eq!(parse_image_line("sha256:b\t-5\r\n"), Some(("sha256:b".into(), 0)));
        assert_eq!(parse_image_line("garbage"), None);
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
    fn two_references_to_one_image_are_listed_once() {
        let found =
            [("cyberctf/attack-box:latest".to_string(), "sha256:a".to_string(), 3000), ("cyberctf/attack-box@sha256:d".into(), "sha256:a".into(), 3000)];
        let items = image_items(&found);
        assert_eq!(items.iter().map(|i| (i.name.as_str(), i.bytes)).collect::<Vec<_>>(), [("cyberctf/attack-box:latest", 3000)]);
    }

    #[test]
    fn only_the_attack_boxs_own_repository_is_the_apps_by_name() {
        let extra =
            ["cyberctf/attack-box@sha256:abc".to_string(), "docker.io/cyberctf/attack-box:latest".into(), "kalilinux/kali-rolling".into(), "-bad".into()];
        let names: Vec<String> = image_candidates(&Owned::default(), &extra).into_iter().map(|(r, want)| format!("{r} {want:?}")).collect();
        assert_eq!(names, ["cyberctf/attack-box@sha256:abc None", "docker.io/cyberctf/attack-box:latest None"]);
    }

    /// A box version downloaded under `home` (`<arch>` between version and provider when given).
    fn download_box(home: &Path, b: &OwnedBox, arch: Option<&str>, bytes: usize) {
        let mut dir = box_dir(home, &b.name).join(&b.version);
        if let Some(arch) = arch {
            dir = dir.join(arch);
        }
        let dir = dir.join(&b.provider);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("box.vmdk"), vec![0u8; bytes]).unwrap();
    }

    fn owned_box(name: &str, provider: &str, version: &str) -> OwnedBox {
        OwnedBox { name: name.into(), provider: provider.into(), version: version.into() }
    }

    #[test]
    fn present_boxes_are_the_apps_downloaded_versions_with_their_size() {
        let home = std::env::temp_dir().join(format!("cyberctf-vhome-{}", rand::random::<u32>()));
        let alpine = owned_box("generic/alpine319", "virtualbox", "4.3.12");
        let debian = owned_box("bento/debian-12", "parallels", "202510.26.0");
        download_box(&home, &alpine, None, 1000);
        download_box(&home, &debian, Some("arm64"), 500);
        // Another version of the same box (the player's): not the app's.
        download_box(&home, &owned_box("generic/alpine319", "virtualbox", "4.3.11"), None, 9999);
        let gone = owned_box("utm/bookworm", "utm", "1");
        let found = present_boxes(&home, &[alpine.clone(), debian.clone(), gone].into());
        assert_eq!(found, [(debian, 500), (alpine, 1000)]);
        assert_eq!(dir_size(&home.join("missing")), 0);
        let _ = std::fs::remove_dir_all(&home);
    }

    /// Docker as the tests see it: `images` (reference, id, size) are on the machine, and
    /// `pulled` among them only once the lab started (`up`).
    fn docker(images: &'static [(&'static str, &'static str, u64)], pulled: &'static [&'static str]) -> Tools {
        let here = move |r: &str, up: bool| images.iter().find(|(name, _, _)| *name == r).filter(|(name, _, _)| up || !pulled.contains(name));
        let mut up = false;
        Tools::new(move |_, args| -> Option<Reply> {
            match args {
                a if a.contains(&"up") && a.contains(&"--pull") => {
                    up = true;
                    ok("")
                }
                ["image", "ls", "-q", "--no-trunc"] => {
                    ok(images.iter().filter(|(n, _, _)| up || !pulled.contains(n)).map(|(_, id, _)| format!("{id}\n")).collect::<String>())
                }
                ["image", "inspect", "--format", format, reference] => match here(reference, up) {
                    Some((_, id, size)) if format.contains("Size") => ok(format!("{id}\t{size}\n")),
                    Some((_, id, _)) => ok(format!("{id}\n")),
                    None => fail("No such image"),
                },
                [.., "config", "--images"] => ok(images.iter().map(|(n, _, _)| format!("{n}\n")).collect::<String>()),
                _ => ok(""),
            }
        })
    }

    const SPEC: &str = "version: 1\nname: t\nnetworks:\n  lab: { cidr: 10.30.0.0/24 }\nmachines:\n  web:\n    networks: { lab: 10 }\n    services: [{ port: 80, http: true }]\n    docker: { image: nginx:1.27 }\n  db:\n    networks: { lab: 20 }\n    docker: { image: postgres:16 }\n";

    #[tokio::test]
    async fn an_image_present_before_any_lab_ran_is_never_removed() {
        // Regression: Clean up removed every image an installed lab names, by name: a
        // developer's own postgres:16 went with it.
        let t = crate::test_app::TestApp::new();
        let dir = t.lab("web", &[("isoloom.yml", SPEC)]);
        let tools = docker(&[("nginx:1.27", "sha256:nginx", 1000), ("postgres:16", "sha256:postgres", 4000)], &["nginx:1.27"]);
        crate::runtime::start(t.handle(), &dir, "web", crate::runtime::Runtime::Docker, None, None, &[], |_| {}).await.unwrap();
        let listed = machine_storage(t.handle().clone(), Vec::new()).await.unwrap();
        assert_eq!(listed.images.iter().map(|i| (i.name.as_str(), i.bytes)).collect::<Vec<_>>(), [("nginx:1.27", 1000)]);
        machine_storage_clean(t.handle().clone(), Vec::new()).await.unwrap();
        assert!(tools.called("docker image rm nginx:1.27"));
        assert!(!tools.called("image rm postgres"), "{:?}", tools.calls());
    }

    #[tokio::test]
    async fn clean_up_removes_only_the_apps_downloads_and_forgets_what_went() {
        let t = crate::test_app::TestApp::new();
        let store = owned::path(t.handle()).unwrap();
        let home = t.data().join("vagrant-home");
        let alpine = owned_box("generic/alpine319", "virtualbox", "4.3.12");
        download_box(&home, &alpine, None, 1000);
        // The player's own box, of a name the self-test also uses.
        download_box(&home, &owned_box("bento/debian-12", "virtualbox", "1"), None, 1000);
        owned::update(&store, |o| {
            o.images.insert(owned::OwnedImage { reference: "nginx:1.27".into(), id: "sha256:nginx".into() });
            // Pulled again by the player since (another image under that name now): theirs.
            o.images.insert(owned::OwnedImage { reference: "redis:7".into(), id: "sha256:old-redis".into() });
            o.boxes.insert(alpine.clone());
        })
        .unwrap();
        let tools = docker(
            &[
                ("nginx:1.27", "sha256:nginx", 1000),
                ("redis:7", "sha256:new-redis", 50),
                ("cyberctf/attack-box:latest", "sha256:ab", 3000),
                ("kalilinux/kali-rolling", "sha256:kali", 500),
            ],
            &[],
        );
        let extra = ["cyberctf/attack-box:latest".to_string(), "kalilinux/kali-rolling".into()];
        clean(&store, &extra, Some(&home)).await.unwrap();
        let calls = tools.calls();
        assert!(
            calls.contains(&"docker image rm nginx:1.27".to_string()) && calls.contains(&"docker image rm cyberctf/attack-box:latest".to_string()),
            "{calls:?}"
        );
        assert!(calls.contains(&"vagrant box remove generic/alpine319 --provider virtualbox --box-version 4.3.12".to_string()), "{calls:?}");
        assert!(
            !calls
                .iter()
                .any(|c| c.contains("redis") && c.contains(" rm ") || c.contains("kali") && c.contains(" rm ") || c.contains("debian") || c.contains("--all")),
            "{calls:?}"
        );
        // Still here (the fake Docker and Vagrant removed nothing): still recorded.
        assert_eq!(owned::load(&store).images.len(), 1, "the redis record went: it names another image now");
        std::fs::remove_dir_all(box_dir(&home, &alpine.name)).unwrap();
        clean(&store, &[], Some(&home)).await.unwrap();
        assert!(owned::load(&store).boxes.is_empty(), "a box gone is forgotten");
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
