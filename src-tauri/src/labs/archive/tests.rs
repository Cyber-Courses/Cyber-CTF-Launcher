use super::*;
use crate::error::Result;

/// A lab archive as GitHub serves one: each `(path, data)` a file, its path written raw (`..`
/// included, which `append_data` refuses).
pub(crate) fn tarball(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast()));
    for (path, data) in entries {
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_mode(0o644);
        header.set_entry_type(tar::EntryType::Regular);
        // Write the raw name so tests can include `..` (append_data refuses it).
        let name = header.as_old_mut().name.as_mut();
        name[..path.len()].copy_from_slice(path.as_bytes());
        header.set_cksum();
        builder.append(&header, *data).unwrap();
    }
    builder.into_inner().unwrap().finish().unwrap()
}

#[test]
fn extracts_without_the_top_level_folder() {
    let dir = crate::testing::temp_dir("extract");
    extract(&tarball(&[("lab-abc/docker-compose.yml", b"services: {}"), ("lab-abc/evidence/claim.sh", b"#!/bin/sh")]), &dir).unwrap();
    assert_eq!(std::fs::read_to_string(dir.join("docker-compose.yml")).unwrap(), "services: {}");
    assert!(dir.join("evidence/claim.sh").is_file());
}

#[test]
fn names_windows_cant_hold_are_told_apart() {
    for bad in ["0 ", "notes.", "aux", "AUX.c", "con.txt", "Lpt1", "com9.log", "a:b", "what?", "tab\there", "x\"y"] {
        assert!(windows_name_problem(bad).is_some(), "{bad:?}");
    }
    for good in ["0", "a b", ".env", "auxiliary.c", "com10", "lpt0", "console", "README.md", "caf\u{e9}"] {
        assert_eq!(windows_name_problem(good), None, "{good:?}");
    }
}

#[cfg(windows)]
#[test]
fn a_lab_archive_with_a_name_windows_cant_hold_is_refused_plainly() {
    let dir = crate::testing::temp_dir("extract");
    let err = extract(&tarball(&[("lab-abc/ok.txt", b"x"), ("lab-abc/notes ", b"y")]), &dir).unwrap_err().to_string();
    assert!(err.contains("can't be installed on Windows") && err.contains("ending in a space"), "{err}");
}

#[test]
fn links_and_the_top_level_folder_entry_are_skipped() {
    let mut builder = tarball_with_dir("docs");
    let mut link = tar::Header::new_gnu();
    link.set_entry_type(tar::EntryType::Symlink);
    link.set_size(0);
    builder.append_link(&mut link, "lab-abc/passwd", "/etc/passwd").unwrap();
    let gz = builder.into_inner().unwrap().finish().unwrap();
    let dest = crate::testing::temp_dir("extract");
    extract(&gz, &dest).unwrap();
    assert!(dest.join("docs").is_dir());
    assert!(std::fs::symlink_metadata(dest.join("passwd")).is_err());
}

#[test]
fn an_internal_symlink_is_recreated_a_tree_escaping_one_is_not() {
    let mut builder = tarball_with_dir("contracts");
    // An internal relative link, like a lab's `client/src/contracts -> ../../contracts`: kept.
    let mut inside = tar::Header::new_gnu();
    inside.set_entry_type(tar::EntryType::Symlink);
    inside.set_size(0);
    builder.append_link(&mut inside, "lab-abc/here", "contracts").unwrap();
    // A relative link that climbs out of the tree: left out, so nothing points outside.
    let mut out = tar::Header::new_gnu();
    out.set_entry_type(tar::EntryType::Symlink);
    out.set_size(0);
    builder.append_link(&mut out, "lab-abc/escape", "../../../outside").unwrap();
    let gz = builder.into_inner().unwrap().finish().unwrap();
    let dest = crate::testing::temp_dir("extract");
    extract(&gz, &dest).unwrap();
    if cfg!(unix) {
        let here = std::fs::symlink_metadata(dest.join("here"));
        assert!(here.is_ok_and(|m| m.file_type().is_symlink()), "the internal link is recreated");
        assert_eq!(std::fs::read_link(dest.join("here")).unwrap(), std::path::Path::new("contracts"));
    }
    assert!(std::fs::symlink_metadata(dest.join("escape")).is_err(), "the escaping link is left out");
}

#[cfg(unix)]
#[test]
fn a_chain_of_links_reading_as_internal_but_resolving_outside_is_left_out() {
    // Regression: `sub/a -> ..` and `b -> sub/a/../secret` each read as inside the tree by their
    // path (`sub/a/..` is `sub`), so both were recreated, and `b` resolved to a file beside the lab:
    // a lab's tree, built into its image and mounted in its machines, could read the app's data.
    let mut builder = tarball_with_dir("sub");
    for (at, target) in [("lab-abc/sub/a", ".."), ("lab-abc/b", "sub/a/../secret"), ("lab-abc/ok", "sub/a/sub")] {
        let mut link = tar::Header::new_gnu();
        link.set_entry_type(tar::EntryType::Symlink);
        link.set_size(0);
        builder.append_link(&mut link, at, target).unwrap();
    }
    let gz = builder.into_inner().unwrap().finish().unwrap();
    let outside = crate::testing::temp_dir("extract");
    std::fs::write(outside.join("secret"), "s").unwrap();
    let dest = outside.join("lab");
    extract(&gz, &dest).unwrap();
    assert!(dest.join("sub/a").is_symlink(), "an internal link is kept");
    assert!(std::fs::symlink_metadata(dest.join("b")).is_err(), "the chain to the secret is left out");
    assert!(dest.join("ok").is_dir(), "a chain that stays inside is kept");
}

#[test]
fn rejects_path_traversal_in_archives() {
    let dir = crate::testing::temp_dir("extract");
    let err = extract(&tarball(&[("lab-abc/../../evil", b"x")]), &dir);
    assert!(err.is_err());
}

#[test]
fn an_archive_unpacking_past_the_limit_is_refused() {
    let dir = crate::testing::temp_dir("extract");
    // Highly compressible, like a gzip bomb: 3 MiB of zeros in a few KiB.
    let zeros = vec![0u8; 3 * 1024 * 1024];
    let archive = tarball(&[("lab-abc/a.bin", &zeros), ("lab-abc/b.bin", &zeros)]);
    assert!(archive.len() < 64 * 1024);
    let err = extract_within(&archive, &dir, 4 * 1024 * 1024).unwrap_err().to_string();
    assert_eq!(err, "the lab archive unpacks to more than 4 MB; refused");
    // The entry past the limit was never written.
    assert!(!dir.join("b.bin").exists());
    let _ = std::fs::remove_dir_all(&dir);
    extract_within(&archive, &dir, 6 * 1024 * 1024).unwrap();
    assert_eq!(std::fs::metadata(dir.join("b.bin")).unwrap().len(), zeros.len() as u64);
}

#[test]
fn an_empty_archive_installs_nothing_and_fails_nothing() {
    let dir = extract_to(&[]).unwrap();
    // The destination is created by the caller (`stage`); nothing appears on its own.
    assert!(!dir.exists() || std::fs::read_dir(&dir).unwrap().next().is_none());
    // An entry that is only the top-level folder name (no slash) is the folder itself: skipped.
    let dir = extract_to(&[("lab-abc", b"not a file we want")]).unwrap();
    assert!(!dir.exists() || std::fs::read_dir(&dir).unwrap().next().is_none());
}

#[test]
fn paths_outside_the_top_folder_land_inside_the_destination_or_are_refused() {
    // An absolute path and a leading `..` lose their first component like the top folder
    // does, so they can only land inside the destination.
    let dir = extract_to(&[("/etc/passwd", b"x"), ("../evil", b"y")]).unwrap();
    assert_eq!(std::fs::read_to_string(dir.join("etc/passwd")).unwrap(), "x");
    assert_eq!(std::fs::read_to_string(dir.join("evil")).unwrap(), "y");
    // A `..` anywhere after the top folder is refused, wherever it sits.
    for path in ["lab-abc/a/../b", "lab-abc/..", "lab-abc/a/b/../../../c"] {
        let err = extract_to(&[(path, b"x")]).unwrap_err().to_string();
        assert!(err.starts_with("unsafe path in lab archive"), "{path}: {err}");
    }
    // A `.` in the middle is nothing; a backslash is a plain character on Unix, not a separator.
    let dir = extract_to(&[("lab-abc/./a", b"x")]).unwrap();
    assert!(dir.join("a").is_file());
}

#[test]
fn a_top_folder_with_a_leading_dot_is_stripped_too() {
    // Regression: GNU tar (`tar czf x.tgz .`) writes `./` before every entry, and a lab archive built
    // that way installed under `<dest>/<repo>-<sha>/`, where nothing looks.
    let dir = extract_to(&[("./lab-abc/isoloom.yml", b"name: x")]).unwrap();
    assert!(dir.join("isoloom.yml").is_file(), "{:?}", std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().path()).collect::<Vec<_>>());
}

#[test]
fn a_file_under_a_path_that_is_a_file_fails_cleanly_and_a_truncated_archive_fails() {
    let err = extract_to(&[("lab-abc/sub", b"a file"), ("lab-abc/sub/x", b"under a file")]).unwrap_err();
    assert!(matches!(err, Error::Io(_)), "{err}");
    // Cut mid-stream: an error, never a lab installed from half an archive.
    let big = vec![7u8; 20_000];
    let gz = tarball(&[("lab-abc/isoloom.yml", b"name: x"), ("lab-abc/big", &big)]);
    let dir = crate::testing::temp_dir("extract");
    assert!(extract(&gz[..gz.len() / 2], &dir).is_err());
    assert!(extract(b"not a gzip at all", &dir).is_err());
}

#[test]
fn long_unicode_paths_and_hard_links_in_an_archive() {
    // A path past tar's 100-byte name field (GNU long-name extension) with non-ASCII names
    // extracts whole; a hard link (not a file or folder) is skipped like a symlink.
    let deep = format!("lab-abc/{}/{}/fichier é.txt", "dossier-très-long-".repeat(4), "ü".repeat(40));
    let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast()));
    let mut file = tar::Header::new_gnu();
    file.set_size(2);
    file.set_mode(0o644);
    file.set_entry_type(tar::EntryType::Regular);
    builder.append_data(&mut file, &deep, &b"ok"[..]).unwrap();
    let mut link = tar::Header::new_gnu();
    link.set_entry_type(tar::EntryType::Link);
    link.set_size(0);
    builder.append_link(&mut link, "lab-abc/alias", &deep).unwrap();
    let gz = builder.into_inner().unwrap().finish().unwrap();
    let dir = crate::testing::temp_dir("extract");
    extract(&gz, &dir).unwrap();
    assert_eq!(std::fs::read_to_string(dir.join(&deep["lab-abc/".len()..])).unwrap(), "ok");
    assert!(std::fs::symlink_metadata(dir.join("alias")).is_err(), "a hard link is skipped");
}

/// A gzip tar builder with the lab's top folder `lab-abc/` and one `subdir/` already in it, so
/// a symlink test adds only the link it exercises.
fn tarball_with_dir(subdir: &str) -> tar::Builder<flate2::write::GzEncoder<Vec<u8>>> {
    let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast()));
    let mut dir = tar::Header::new_gnu();
    dir.set_entry_type(tar::EntryType::Directory);
    dir.set_size(0);
    dir.set_mode(0o755);
    builder.append_data(&mut dir.clone(), "lab-abc/", &[][..]).unwrap();
    builder.append_data(&mut dir, format!("lab-abc/{subdir}/"), &[][..]).unwrap();
    builder
}

/// `entries` extracted into a fresh folder, removed when dropped.
fn extract_to(entries: &[(&str, &[u8])]) -> Result<crate::testing::TempDir> {
    let dir = crate::testing::temp_dir("extract");
    extract(&tarball(entries), &dir).map(|()| dir)
}
