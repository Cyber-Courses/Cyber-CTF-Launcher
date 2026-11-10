use std::path::{Path, PathBuf};

use super::*;
use crate::platform::exec::fake::{Reply, Tools, fail, ok};
use crate::testing::test_app::TestApp;

/// A Mach-O header: a thin 64-bit file for `cpu`.
fn thin(cpu: u32) -> Vec<u8> {
    let mut h = MH_MAGIC_64.to_le_bytes().to_vec();
    h.extend(cpu.to_le_bytes());
    h.resize(64, 0);
    h
}

const X86_64: u32 = 0x0100_0007;
const ARM64: u32 = 0x0100_000c;

/// A new, empty folder of the test's own.
fn fresh(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("cyberctf-ovftool-{tag}-{}", random_hex(6)));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write(path: &Path, body: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
}

fn executable(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    #[cfg(not(unix))]
    let _ = path;
}

/// The OVF Tool as Broadcom's download unpacks: the program, its libraries in `lib/`, and data.
fn download(root: &Path) -> PathBuf {
    let exe = root.join(EXE);
    write(&exe, &thin(X86_64));
    executable(&exe);
    write(&root.join("lib").join("libvmomi.dylib"), &thin(X86_64));
    write(&root.join("env").join("en").join("messages.vmsg"), b"messages");
    write(&root.join("certs").join("cacert.pem"), b"-----BEGIN CERTIFICATE-----");
    root.to_path_buf()
}

/// Every file under `dir`, relative, sorted.
fn files(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if e.file_type().unwrap().is_dir() {
            out.extend(files(&e.path()).into_iter().map(|f| format!("{name}/{f}")));
        } else {
            out.push(name);
        }
    }
    out.sort();
    out
}

/// `codesign` as macOS answers for VMware's own signature; every other tool (xattr) succeeds.
fn vmware_signed() -> Tools {
    Tools::new(|program, args| match program {
        "/usr/bin/codesign" if args.first() == Some(&"-d") => {
            ok(format!("designated => identifier ovftool and anchor apple generic and certificate leaf[subject.OU] = {VMWARE_TEAM_ID}\n"))
        }
        _ => ok(""),
    })
}

const VMWARE_TEAM_ID: &str = "EG7KH642X6";

fn placed_path(outcome: &Outcome) -> PathBuf {
    match outcome {
        Outcome::Placed { path, .. } => PathBuf::from(path),
        refused @ Outcome::Refused { .. } => panic!("refused: {refused:?}"),
    }
}

#[tokio::test]
async fn a_signed_download_is_copied_into_the_apps_folder_and_the_download_left_as_it_was() {
    let t = TestApp::new();
    let picked = download(&fresh("signed"));
    let before = files(&picked);
    let tools = vmware_signed();
    let outcome = place(&t.dirs(), &picked).await.unwrap();
    let dest = managed_dir(t.data());
    assert_eq!(placed_path(&outcome), dest.join(EXE));
    assert_eq!(files(&dest), before, "the whole tool is copied");
    assert_eq!(files(&picked), before, "the player's download is only read");
    assert!(is_program(&dest.join(EXE)), "the copy is still a program");
    if cfg!(target_os = "macos") {
        // The program and its library are both checked against VMware's team.
        let calls = tools.calls();
        let checked =
            |name: &str| calls.iter().any(|c| c.contains("-R=anchor apple generic and certificate leaf[subject.OU] = \"EG7KH642X6\"") && c.ends_with(name));
        assert!(checked(&format!("/{EXE}")) && checked("lib/libvmomi.dylib"), "{calls:?}");
        assert!(
            !calls.iter().any(|c| c.starts_with("/usr/bin/codesign") && c.contains(&*picked.to_string_lossy())),
            "the download itself isn't run against: {calls:?}"
        );
    } else {
        assert!(tools.calls().is_empty(), "no signature check off macOS: {:?}", tools.calls());
    }
    // Nothing of the copy's making is left beside it.
    let tools_dir = dest.parent().unwrap();
    assert_eq!(files(tools_dir).iter().filter(|f| f.starts_with(".ovftool-")).count(), 0, "{:?}", files(tools_dir));
    let _ = std::fs::remove_dir_all(&picked);
}

/// The quarantine is lifted from the copy only, and only after every signature check passed.
#[cfg(target_os = "macos")]
#[tokio::test]
async fn the_quarantine_is_lifted_from_the_checked_copy_only() {
    let t = TestApp::new();
    let picked = download(&fresh("quarantine"));
    let tools = vmware_signed();
    place(&t.dirs(), &picked).await.unwrap();
    let calls = tools.calls();
    let lifted: Vec<&String> = calls.iter().filter(|c| c.starts_with("/usr/bin/xattr")).collect();
    assert_eq!(lifted.len(), 1, "{calls:?}");
    assert!(lifted[0].starts_with("/usr/bin/xattr -d -r com.apple.quarantine "), "{}", lifted[0]);
    let target = lifted[0].trim_start_matches("/usr/bin/xattr -d -r com.apple.quarantine ");
    assert!(Path::new(target).starts_with(t.data().join("tools")), "lifted from the app's copy: {target}");
    assert!(!target.contains(&*picked.to_string_lossy()), "never from the download: {target}");
    let last_check = calls.iter().rposition(|c| c.starts_with("/usr/bin/codesign --verify")).unwrap();
    let lift = calls.iter().position(|c| c.starts_with("/usr/bin/xattr")).unwrap();
    assert!(last_check < lift, "lifted only once the checks passed: {calls:?}");
    let _ = std::fs::remove_dir_all(&picked);
}

/// The real `xattr` lifts the quarantine a download carries from every file of the folder.
#[cfg(target_os = "macos")]
#[tokio::test]
async fn lifting_the_quarantine_clears_it_from_every_file_of_the_folder() {
    let dir = download(&fresh("xattr"));
    let quarantined = |f: &Path| std::process::Command::new("/usr/bin/xattr").args(["-p", "com.apple.quarantine"]).arg(f).output().unwrap().status.success();
    for f in [dir.join(EXE), dir.join("lib").join("libvmomi.dylib")] {
        let set = std::process::Command::new("/usr/bin/xattr").args(["-w", "com.apple.quarantine", "0081;00000000;Safari;"]).arg(&f).status().unwrap();
        assert!(set.success() && quarantined(&f));
    }
    lift_quarantine(&dir).await.unwrap();
    assert!(!quarantined(&dir.join(EXE)) && !quarantined(&dir.join("lib").join("libvmomi.dylib")));
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(target_os = "macos")]
#[tokio::test]
async fn a_tool_signed_by_another_team_is_refused_with_that_team_and_nothing_is_placed() {
    let t = TestApp::new();
    let picked = download(&fresh("other-team"));
    let tools = Tools::new(|program, args| match program {
        "/usr/bin/codesign" if args.iter().any(|a| a.starts_with("-R=")) => fail("test-requirement: code failed to satisfy specified code requirement(s)"),
        "/usr/bin/codesign" if args.first() == Some(&"-d") => {
            ok("designated => identifier ovftool and anchor apple generic and certificate leaf[subject.OU] = ABCDE12345\n")
        }
        _ => ok(""),
    });
    let outcome = place(&t.dirs(), &picked).await.unwrap();
    assert_eq!(outcome, Outcome::Refused { reason: Refusal::NotVmware, detail: Some("ABCDE12345".into()) });
    assert!(!managed_dir(t.data()).exists(), "nothing placed");
    assert!(!tools.called("/usr/bin/xattr"), "the quarantine stays: {:?}", tools.calls());
    assert_eq!(files(&t.data().join("tools")), Vec::<String>::new(), "no copy left behind");
    let _ = std::fs::remove_dir_all(&picked);
}

#[cfg(target_os = "macos")]
#[tokio::test]
async fn an_unsigned_library_is_refused_with_what_codesign_said() {
    let t = TestApp::new();
    let picked = download(&fresh("unsigned"));
    let tools = Tools::new(|program, args| match program {
        "/usr/bin/codesign" if args.last().is_some_and(|f| f.ends_with("libvmomi.dylib")) => fail("libvmomi.dylib: code object is not signed at all"),
        _ => ok(""),
    });
    let outcome = place(&t.dirs(), &picked).await.unwrap();
    assert_eq!(
        outcome,
        Outcome::Refused { reason: Refusal::Unsigned, detail: Some("lib/libvmomi.dylib: libvmomi.dylib: code object is not signed at all".into()) }
    );
    assert!(!managed_dir(t.data()).exists());
    assert!(!tools.called("/usr/bin/xattr"));
    let _ = std::fs::remove_dir_all(&picked);
}

/// The signature check itself, on every OS (only macOS runs it): every Mach-O file of the copy is
/// verified, then held to VMware's team; the first that fails says why.
#[tokio::test]
async fn the_signature_check_holds_every_program_and_library_to_vmwares_team() {
    let dir = download(&fresh("signature"));
    let all = vmware_signed();
    assert_eq!(signature_refusal(&dir).await.unwrap(), None);
    assert_eq!(all.calls().iter().filter(|c| c.starts_with("/usr/bin/codesign --verify --strict -R=")).count(), 2, "{:?}", all.calls());
    drop(all);
    let _other = Tools::new(|_, args| {
        if args.iter().any(|a| a.starts_with("-R=")) {
            fail("test-requirement: code failed to satisfy specified code requirement(s)")
        } else if args.first() == Some(&"-d") {
            ok("designated => identifier ovftool and certificate leaf[subject.OU] = ABCDE12345\n")
        } else {
            ok("")
        }
    });
    assert_eq!(signature_refusal(&dir).await.unwrap(), Some((Refusal::NotVmware, Some("ABCDE12345".into()))));
    let _unsigned = Tools::new(|_, _| fail("code object is not signed at all"));
    let refusal = signature_refusal(&dir).await.unwrap().unwrap();
    assert_eq!(refusal.0, Refusal::Unsigned);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn a_folder_without_the_program_is_refused_before_anything_is_copied() {
    let t = TestApp::new();
    let picked = fresh("no-tool");
    write(&picked.join("lib").join("libvmomi.dylib"), &thin(X86_64));
    write(&picked.join("README.txt"), b"VMware OVF Tool");
    let tools = vmware_signed();
    assert_eq!(place(&t.dirs(), &picked).await.unwrap(), Outcome::Refused { reason: Refusal::NoTool, detail: None });
    assert!(!t.data().join("tools").exists(), "nothing copied");
    assert!(tools.calls().is_empty(), "{:?}", tools.calls());
    let _ = std::fs::remove_dir_all(&picked);
}

#[tokio::test]
async fn the_program_alone_without_its_libraries_is_refused() {
    let t = TestApp::new();
    let picked = fresh("no-lib");
    write(&picked.join(EXE), &thin(X86_64));
    executable(&picked.join(EXE));
    let _tools = vmware_signed();
    assert_eq!(place(&t.dirs(), &picked).await.unwrap(), Outcome::Refused { reason: Refusal::NoLibraries, detail: None });
    assert!(!t.data().join("tools").exists());
    let _ = std::fs::remove_dir_all(&picked);
}

#[cfg(unix)]
#[tokio::test]
async fn a_program_that_isnt_executable_is_refused() {
    let t = TestApp::new();
    let picked = download(&fresh("not-exec"));
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(picked.join(EXE), std::fs::Permissions::from_mode(0o644)).unwrap();
    }
    let _tools = vmware_signed();
    assert_eq!(place(&t.dirs(), &picked).await.unwrap(), Outcome::Refused { reason: Refusal::NoTool, detail: None });
    assert!(!managed_dir(t.data()).exists());
    let _ = std::fs::remove_dir_all(&picked);
}

#[tokio::test]
async fn the_one_folder_inside_the_picked_one_that_holds_the_tool_is_used() {
    // The Windows and Linux downloads unpack into an `ovftool` folder.
    let t = TestApp::new();
    let picked = fresh("nested");
    download(&picked.join("ovftool"));
    let _tools = vmware_signed();
    let outcome = place(&t.dirs(), &picked).await.unwrap();
    assert_eq!(placed_path(&outcome), managed_dir(t.data()).join(EXE));
    // Two candidates: which one is meant isn't guessed.
    download(&picked.join("ovftool-older"));
    assert_eq!(place(&t.dirs(), &picked).await.unwrap(), Outcome::Refused { reason: Refusal::NoTool, detail: None });
    let _ = std::fs::remove_dir_all(&picked);
}

#[tokio::test]
async fn a_new_copy_replaces_the_earlier_one_whole() {
    let t = TestApp::new();
    let dest = managed_dir(t.data());
    write(&dest.join("lib").join("libold.dylib"), &thin(X86_64));
    write(&dest.join(EXE), b"old");
    let picked = download(&fresh("replace"));
    let _tools = vmware_signed();
    place(&t.dirs(), &picked).await.unwrap();
    assert_eq!(files(&dest), files(&picked), "nothing of the earlier copy is left");
    assert_eq!(files(dest.parent().unwrap()), files(&picked).iter().map(|f| format!("ovftool/{f}")).collect::<Vec<_>>(), "no copy aside is left");
    let _ = std::fs::remove_dir_all(&picked);
}

#[cfg(target_os = "macos")]
#[tokio::test]
async fn a_refused_copy_keeps_the_earlier_one() {
    let t = TestApp::new();
    let dest = managed_dir(t.data());
    write(&dest.join(EXE), b"the copy that works");
    let picked = download(&fresh("keep"));
    let _tools = Tools::new(|program, _| if program == "/usr/bin/codesign" { fail("code object is not signed at all") } else { ok("") });
    assert!(matches!(place(&t.dirs(), &picked).await.unwrap(), Outcome::Refused { reason: Refusal::Unsigned, .. }));
    assert_eq!(std::fs::read(dest.join(EXE)).unwrap(), b"the copy that works");
    let _ = std::fs::remove_dir_all(&picked);
}

#[tokio::test]
async fn placing_is_refused_when_the_policy_turns_tool_installs_off() {
    let _policy = crate::platform::policy::Override::with(&[Key::DisableToolInstalls]);
    let t = TestApp::new();
    let picked = download(&fresh("policy"));
    let _tools = vmware_signed();
    let err = place(&t.dirs(), &picked).await.unwrap_err();
    assert!(matches!(err.code(), crate::error::Code::TurnedOffByPolicy { .. }), "{err}");
    assert!(!t.data().join("tools").exists());
    let _ = std::fs::remove_dir_all(&picked);
}

#[test]
fn a_folder_far_bigger_than_the_tool_isnt_copied() {
    let from = download(&fresh("budget"));
    let to = fresh("budget-to").join("copy");
    let err = copy_tree(&from, &to, &mut Budget { files: 2, bytes: MAX_BYTES }).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::FileTooLarge);
    let to2 = to.with_file_name("copy2");
    assert_eq!(copy_tree(&from, &to2, &mut Budget { files: MAX_FILES, bytes: 10 }).unwrap_err().kind(), std::io::ErrorKind::FileTooLarge);
    copy_tree(&from, &to.with_file_name("copy3"), &mut Budget::default()).unwrap();
    let _ = std::fs::remove_dir_all(&from);
    let _ = std::fs::remove_dir_all(to.parent().unwrap());
}

#[cfg(unix)]
#[test]
fn a_link_inside_the_folder_is_copied_as_a_link_and_one_leading_out_left_out() {
    let from = download(&fresh("links"));
    std::os::unix::fs::symlink("libvmomi.dylib", from.join("lib").join("libvmomi.1.dylib")).unwrap();
    std::os::unix::fs::symlink("/etc/passwd", from.join("lib").join("passwd")).unwrap();
    std::os::unix::fs::symlink("../certs", from.join("lib").join("certs")).unwrap();
    let to = fresh("links-to").join("copy");
    copy_tree(&from, &to, &mut Budget::default()).unwrap();
    assert_eq!(std::fs::read_link(to.join("lib").join("libvmomi.1.dylib")).unwrap(), PathBuf::from("libvmomi.dylib"));
    assert!(std::fs::symlink_metadata(to.join("lib").join("passwd")).is_err());
    assert!(std::fs::symlink_metadata(to.join("lib").join("certs")).is_err());
    let _ = std::fs::remove_dir_all(&from);
    let _ = std::fs::remove_dir_all(to.parent().unwrap());
}

#[test]
fn a_leftover_of_an_interrupted_placement_is_removed_once_old() {
    let dir = fresh("leftovers");
    let leftover = dir.join(".ovftool-new-abc");
    std::fs::create_dir(&leftover).unwrap();
    std::fs::create_dir(dir.join("ovftool")).unwrap();
    remove_leftovers(&dir);
    assert!(leftover.exists(), "a young one may be another placement's");
    let old = SystemTime::now() - LEFTOVER_AFTER - Duration::from_secs(1);
    std::fs::File::open(&leftover).unwrap().set_modified(old).unwrap();
    remove_leftovers(&dir);
    assert!(!leftover.exists());
    assert!(dir.join("ovftool").exists(), "the placed copy stays");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_hypervisors_copy_comes_first_then_the_apps_then_the_path() {
    let root = fresh("order");
    let fusion = root.join("Fusion").join(EXE);
    let data = root.join("data");
    let managed = managed_dir(&data).join(EXE);
    let on_path = root.join("bin").join(EXE);
    let path = std::env::join_paths([root.join("empty"), root.join("bin")]).unwrap();
    let bundled = || vec![(Source::Fusion, fusion.clone())];
    assert_eq!(locate_in(bundled(), Some(&data), Some(&path)), None, "none yet");
    write(&on_path, b"");
    assert_eq!(locate_in(bundled(), Some(&data), Some(&path)), Some((Source::Path, on_path.clone())));
    write(&managed, b"");
    assert_eq!(locate_in(bundled(), Some(&data), Some(&path)), Some((Source::Managed, managed.clone())));
    write(&fusion, b"");
    assert_eq!(locate_in(bundled(), Some(&data), Some(&path)), Some((Source::Fusion, fusion.clone())));
    // No data folder known: the hypervisor's, then the PATH.
    std::fs::remove_file(&fusion).unwrap();
    assert_eq!(locate_in(bundled(), None, Some(&path)), Some((Source::Path, on_path)));
    let _ = std::fs::remove_dir_all(&root);
}

/// A Windows environment's Program Files folders.
fn program_files(name: &str) -> Option<OsString> {
    match name {
        "ProgramFiles(x86)" => Some(OsString::from(r"C:\Program Files (x86)")),
        "ProgramFiles" => Some(OsString::from(r"C:\Program Files")),
        _ => None,
    }
}

#[test]
fn the_hypervisor_copies_are_fusions_on_macos_and_workstations_on_windows() {
    let found = bundled(program_files);
    if cfg!(target_os = "macos") {
        assert_eq!(found, [(Source::Fusion, PathBuf::from("/Applications/VMware Fusion.app/Contents/Library/VMware OVF Tool/ovftool"))]);
    } else if cfg!(windows) {
        assert_eq!(found.iter().map(|(s, _)| *s).collect::<Vec<_>>(), [Source::Workstation, Source::Workstation]);
        assert_eq!(found[0].1, workstation_dir(Path::new(r"C:\Program Files (x86)")).join("ovftool.exe"));
    } else {
        // Workstation for Linux puts it in /usr/bin, on the PATH.
        assert!(found.is_empty(), "{found:?}");
    }
}

/// Workstation's copy, which the check reports, is on the app's PATH on Windows, so a lab's run
/// finds it too.
#[test]
fn workstations_copy_is_on_the_windows_path() {
    let on_path = crate::platform::env_path::windows_dirs(program_files);
    for pf in [r"C:\Program Files (x86)", r"C:\Program Files"] {
        assert!(on_path.contains(&workstation_dir(Path::new(pf))), "{pf}: {on_path:?}");
    }
}

/// The check runs the found file's `--version`, its folder first on `PATH`, and says where it is.
#[tokio::test]
async fn the_check_asks_the_found_copy_its_version() {
    let tools =
        Tools::new(|program, args| (program == "ovftool" && args == ["--version"]).then(|| Reply::Out("VMware ovftool 5.1.0 (build-24936374)\n".into())));
    let dir = fresh("probe");
    let file = dir.join(EXE);
    let tool = version_of(&file).await;
    assert_eq!((tool.installed, tool.version.as_deref()), (true, Some("VMware ovftool 5.1.0 (build-24936374)")));
    let path = tools.env_of("ovftool --version", "PATH").unwrap();
    assert_eq!(std::env::split_paths(&path).next(), Some(dir.clone()), "{path}");
    // One that doesn't run (Rosetta missing, Gatekeeper) reads as not installed.
    let _broken = Tools::new(|_, _| fail("Bad CPU type in executable"));
    let tool = version_of(&file).await;
    assert!(!tool.installed && tool.version.is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

/// A real run: the program found by name through the `PATH` given is the found file.
#[cfg(unix)]
#[tokio::test]
async fn the_found_copy_answers_for_real_through_the_path_it_is_given() {
    let dir = fresh("probe-real");
    let file = dir.join(EXE);
    write(&file, b"#!/bin/sh\necho 'VMware ovftool 9.9.9 (build-1)'\n");
    executable(&file);
    let tool = version_of(&file).await;
    assert_eq!(tool.version.as_deref(), Some("VMware ovftool 9.9.9 (build-1)"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// The machine check reports the copy the app placed, and where, as soon as it is there.
#[tokio::test]
async fn a_placed_copy_is_reported_by_the_next_check() {
    let t = TestApp::new();
    let picked = download(&fresh("report"));
    {
        let _tools = vmware_signed();
        place(&t.dirs(), &picked).await.unwrap();
    }
    let _tools = Tools::new(|program, _| (program == "ovftool").then(|| Reply::Out("VMware ovftool 5.1.0\n".into())));
    let (tool, location) = probe(Some(t.data())).await;
    assert!(tool.installed);
    let location = location.unwrap();
    // A hypervisor's own copy wins when there is one (this machine may have Fusion).
    if bundled(|n| std::env::var_os(n)).iter().all(|(_, f)| !f.is_file()) {
        assert_eq!(location.source, Source::Managed);
        assert_eq!(PathBuf::from(location.path), managed_dir(t.data()).join(EXE));
    }
    let _ = std::fs::remove_dir_all(&picked);
}

#[test]
fn the_cpus_come_from_the_mach_o_header() {
    assert_eq!(cpus(&thin(X86_64)), [Cpu::X86_64]);
    assert_eq!(cpus(&thin(ARM64)), [Cpu::Arm64]);
    // A universal binary: big-endian, its count, then one 20-byte entry per CPU.
    let mut fat = FAT_MAGIC.to_be_bytes().to_vec();
    fat.extend(2u32.to_be_bytes());
    for cpu in [X86_64, ARM64] {
        let mut entry = cpu.to_be_bytes().to_vec();
        entry.resize(20, 0);
        fat.extend(entry);
    }
    assert_eq!(cpus(&fat), [Cpu::X86_64, Cpu::Arm64]);
    assert!(is_mach_o(&fat) && is_mach_o(&thin(X86_64)));
    assert_eq!(cpus(b"#!/bin/sh\n"), []);
    assert!(!is_mach_o(b"#!/bin/sh\n") && !is_mach_o(b"MZ") && !is_mach_o(b""));
    assert!(intel_only(&cpus(&thin(X86_64))));
    assert!(!intel_only(&cpus(&fat)) && !intel_only(&cpus(&thin(ARM64))) && !intel_only(&[]));
}

#[test]
fn only_mach_o_files_are_signature_checked() {
    let dir = download(&fresh("macho"));
    let found: Vec<String> = mach_o_files(&dir).iter().map(|f| f.strip_prefix(&dir).unwrap().to_string_lossy().replace('\\', "/")).collect();
    assert_eq!(found, ["lib/libvmomi.dylib".to_string(), EXE.to_string()]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_signing_team_comes_from_the_designated_requirement() {
    let dr = "designated => identifier ovftool and anchor apple generic and certificate 1[field.1.2.840.113635.100.6.2.6] /* exists */ and certificate leaf[subject.OU] = EG7KH642X6\n";
    assert_eq!(signing_team(dr).as_deref(), Some("EG7KH642X6"));
    assert_eq!(signing_team("designated => certificate leaf[subject.OU] = \"ABCDE12345\"").as_deref(), Some("ABCDE12345"));
    assert_eq!(signing_team("designated => cdhash H\"0123\""), None);
}

#[test]
fn the_outcomes_reach_the_webview_tagged() {
    let placed = serde_json::to_value(Outcome::Placed { path: "/x/ovftool".into(), needs_rosetta: true }).unwrap();
    assert_eq!(placed, serde_json::json!({ "outcome": "placed", "path": "/x/ovftool", "needsRosetta": true }));
    let refused = serde_json::to_value(Outcome::refused(Refusal::NotVmware, Some("ABCDE12345".into()))).unwrap();
    assert_eq!(refused, serde_json::json!({ "outcome": "refused", "reason": "notVmware", "detail": "ABCDE12345" }));
}

/// The owner's real download, read only (its quarantine and place stay): placed into a test
/// app's folder with the real `codesign` and `xattr`. Opt-in, macOS:
///   `cargo test --lib real_download -- --ignored`
#[cfg(target_os = "macos")]
#[tokio::test]
#[ignore = "reads ~/Downloads/VMware OVF Tool, which only a machine with the download has"]
async fn real_download_is_placed_and_runs() {
    let picked = crate::platform::home_dir().unwrap().join("Downloads").join("VMware OVF Tool");
    let t = TestApp::new();
    let outcome = place(&t.dirs(), &picked).await.unwrap();
    let placed = placed_path(&outcome);
    let quarantined = |f: &Path| std::process::Command::new("/usr/bin/xattr").args(["-p", "com.apple.quarantine"]).arg(f).output().unwrap().status.success();
    assert!(!quarantined(&placed), "the copy runs");
    assert!(quarantined(&picked.join(EXE)), "the download keeps its quarantine");
    let tool = version_of(&placed).await;
    println!("{outcome:?} {:?} {:?}", tool.installed, tool.version);
    assert!(tool.installed || needs_rosetta(&placed), "{:?}", tool.version);
}

#[test]
fn what_the_esxi_test_reads_is_the_version_or_why_the_tool_cant_be_used() {
    let at = |needs_rosetta| Location { source: Source::Fusion, path: "/x/ovftool".into(), needs_rosetta };
    let tool = |installed, version: Option<&str>| Tool { installed, version: version.map(str::to_owned) };
    assert_eq!(usable_from(tool(true, Some("VMware ovftool 5.1.0")), Some(&at(false))), Ok("VMware ovftool 5.1.0".into()));
    assert_eq!(usable_from(tool(false, None), Some(&at(true))), Err("VMware's OVF Tool at /x/ovftool is built for Intel Macs and needs Rosetta".into()));
    assert_eq!(usable_from(tool(false, None), Some(&at(false))), Err("VMware's OVF Tool at /x/ovftool doesn't run".into()));
    assert!(usable_from(tool(false, None), None).unwrap_err().contains("wasn't found"));
}
