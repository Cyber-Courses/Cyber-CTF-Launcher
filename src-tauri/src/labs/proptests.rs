//! Property tests for unpacking a lab archive: whatever paths and entry kinds the tarball holds,
//! nothing is ever written outside the destination, and a well-formed archive lands whole.

use std::path::{Path, PathBuf};

use proptest::prelude::*;

use super::*;
use crate::testing::proptest_support::config;

/// One archive entry: its raw path, kind and content.
#[derive(Debug, Clone)]
struct Entry {
    path: String,
    kind: tar::EntryType,
    link: String,
    data: Vec<u8>,
}

/// A path component an attacker (or a sloppy archiver) could write.
fn component() -> impl Strategy<Value = String> {
    prop_oneof![
        4 => "[a-z]{1,6}",
        2 => Just("..".to_string()),
        1 => Just(".".to_string()),
        1 => Just(String::new()),
        1 => Just("lab-abc".to_string()),
        1 => "[\\\\:~ a-z.]{1,5}",
        1 => "\\PC{1,8}",
        1 => "[a-z]{90,140}",
    ]
}

fn raw_path() -> impl Strategy<Value = String> {
    (prop_oneof![Just(""), Just("/"), Just("./"), Just("../"), Just("//")], proptest::collection::vec(component(), 1..6), prop_oneof![Just(""), Just("/")])
        .prop_map(|(lead, comps, trail)| format!("{lead}{}{trail}", comps.join("/")))
}

fn entry() -> impl Strategy<Value = Entry> {
    let kind = prop_oneof![
        5 => Just(tar::EntryType::Regular),
        2 => Just(tar::EntryType::Directory),
        1 => Just(tar::EntryType::Symlink),
        1 => Just(tar::EntryType::Link),
        1 => Just(tar::EntryType::Continuous),
        1 => Just(tar::EntryType::Fifo),
    ];
    let link = prop_oneof![Just("../../../outside".to_string()), Just("/tmp".to_string()), raw_path()];
    (raw_path(), kind, link, proptest::collection::vec(any::<u8>(), 0..64)).prop_map(|(path, kind, link, data)| Entry { path, kind, link, data })
}

/// Writes `bytes` as a GNU long-name record (`././@LongLink`), so any path fits, `..` included.
fn long_name(builder: &mut tar::Builder<impl std::io::Write>, kind: tar::EntryType, bytes: &[u8]) {
    let mut h = tar::Header::new_gnu();
    h.as_old_mut().name[..13].copy_from_slice(b"././@LongLink");
    h.set_mode(0o644);
    h.set_entry_type(kind);
    h.set_size(bytes.len() as u64 + 1);
    h.set_cksum();
    let mut data = bytes.to_vec();
    data.push(0);
    builder.append(&h, data.as_slice()).unwrap();
}

/// A gzipped tarball of `entries`, raw paths kept as they are (`tar::Builder`'s path setters
/// refuse `..`, which is the point here).
fn tarball(entries: &[Entry]) -> Vec<u8> {
    let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast()));
    for e in entries {
        let path = e.path.as_bytes();
        if path.len() > 100 {
            long_name(&mut builder, tar::EntryType::GNULongName, path);
        }
        let mut h = tar::Header::new_gnu();
        let n = path.len().min(100);
        h.as_old_mut().name[..n].copy_from_slice(&path[..n]);
        h.set_mode(0o644);
        h.set_entry_type(e.kind);
        if matches!(e.kind, tar::EntryType::Symlink | tar::EntryType::Link) {
            let link = e.link.as_bytes();
            if link.len() > 100 {
                long_name(&mut builder, tar::EntryType::GNULongLink, link);
            }
            let n = link.len().min(100);
            h.as_old_mut().linkname[..n].copy_from_slice(&link[..n]);
        }
        let data: &[u8] = if e.kind.is_file() || e.kind == tar::EntryType::Continuous { &e.data } else { &[] };
        h.set_size(data.len() as u64);
        h.set_cksum();
        builder.append(&h, data).unwrap();
    }
    builder.into_inner().unwrap().finish().unwrap()
}

/// Every path under `dir`, without following links.
fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        out.push(p.clone());
        if e.file_type().is_ok_and(|t| t.is_dir()) {
            walk(&p, out);
        }
    }
}

/// A fresh sandbox holding the destination, so anything written next to it shows.
fn sandbox() -> (PathBuf, PathBuf) {
    let root = std::env::temp_dir().join(format!("cyberctf-extract-prop-{}", rand::random::<u64>()));
    let dest = root.join("box").join("dest");
    std::fs::create_dir_all(&dest).unwrap();
    (root, dest)
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 128, ..config() })]

    #[test]
    fn nothing_is_ever_written_outside_the_destination(entries in proptest::collection::vec(entry(), 0..8)) {
        let (root, dest) = sandbox();
        let _ = extract(&tarball(&entries), &dest);
        let mut all = Vec::new();
        walk(&root, &mut all);
        let outside: Vec<&PathBuf> = all.iter().filter(|p| !p.starts_with(&dest) && **p != root.join("box")).collect();
        let links: Vec<&PathBuf> = all.iter().filter(|p| std::fs::symlink_metadata(p).is_ok_and(|m| m.file_type().is_symlink())).collect();
        let _ = std::fs::remove_dir_all(&root);
        prop_assert!(outside.is_empty(), "written outside: {:?}", outside);
        prop_assert!(links.is_empty(), "links made: {:?}", links);
    }

    #[test]
    fn any_bytes_are_refused_or_extracted_without_a_panic(bytes in proptest::collection::vec(any::<u8>(), 0..2048), gz in any::<bool>()) {
        let (root, dest) = sandbox();
        // Raw bytes, or the same bytes as a gzip stream of a (likely broken) tar.
        let input = if gz {
            use std::io::Write;
            let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
            e.write_all(&bytes).unwrap();
            e.finish().unwrap()
        } else {
            bytes
        };
        let _ = extract(&input, &dest);
        let mut all = Vec::new();
        walk(&root, &mut all);
        let _ = std::fs::remove_dir_all(&root);
        prop_assert!(all.iter().all(|p| p.starts_with(&dest) || *p == root.join("box")));
    }

    /// A GitHub-style archive (`<repo>-<sha>/` then plain paths) lands whole under the destination,
    /// without its top folder.
    #[test]
    fn a_well_formed_archive_lands_whole(
        top in "[a-zA-Z0-9._-]{1,20}-[0-9a-f]{7}",
        // Lowercase: the test may run on a case-insensitive file system. Some paths pass tar's
        // 100-byte name field (a GNU long name).
        // Names every OS can hold: Windows refuses a trailing space or dot, and device names
        // (an archive holding one is refused there, see `windows_name_problem`).
        files in proptest::collection::btree_map(proptest::collection::vec(prop_oneof![4 => "[a-z0-9_][a-z0-9_. -]{0,12}", 1 => "[a-z]{60,120}"].prop_filter("a name Windows can hold", |n| super::windows_name_problem(n).is_none()), 1..4), proptest::collection::vec(any::<u8>(), 0..100), 1..6),
        dot in any::<bool>(),
    ) {
        // A file's path can't also be another's folder.
        let paths: Vec<&Vec<String>> = files.keys().collect();
        prop_assume!(!paths.iter().any(|a| paths.iter().any(|b| b.len() > a.len() && b.starts_with(a))));
        let lead = if dot { "./" } else { "" };
        let mut entries = vec![Entry { path: format!("{lead}{top}/"), kind: tar::EntryType::Directory, link: String::new(), data: Vec::new() }];
        entries.extend(files.iter().map(|(p, data)| Entry { path: format!("{lead}{top}/{}", p.join("/")), kind: tar::EntryType::Regular, link: String::new(), data: data.clone() }));
        let (root, dest) = sandbox();
        let res = extract(&tarball(&entries), &dest);
        let mut read = Vec::new();
        for p in files.keys() {
            read.push(std::fs::read(p.iter().fold(dest.clone(), |d, c| d.join(c))).ok());
        }
        let _ = std::fs::remove_dir_all(&root);
        prop_assert!(res.is_ok(), "{:?}", res);
        for (got, want) in read.iter().zip(files.values()) {
            prop_assert_eq!(got.as_ref(), Some(want));
        }
    }
}
