//! The labs imported from a folder ([`super::local_id`]) installed here: what their lab page shows
//! of them, read from the installed copy (the one their run uses), since no catalogue lists them.

use std::path::Path;

use super::ctf::{self, Challenge, Metadata, Problem};
use crate::platform::AppDirs;
use crate::runtime::{self, is_lab_id};

/// How an imported lab's id starts ([`super::local_id`]); a catalogue lab's is a UUID.
pub(crate) const PREFIX: &str = "local-";

/// An imported lab as its page shows it.
#[derive(Debug, PartialEq, Eq, serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "ImportedLab")]
pub struct Imported {
    /// Its id (`local-<name>`).
    pub id: String,
    /// The name its `isoloom.yml` gives (its id when it gives none, or can't be read): its title
    /// when `.ctf/metadata.json` gives none.
    pub name: String,
    /// Its `.ctf/metadata.json`, when there and readable.
    pub metadata: Option<Metadata>,
    /// Its `.ctf/objectives.json`'s challenges, when there and readable.
    pub objectives: Option<Vec<Challenge>>,
    /// The files of its `.ctf/` that are there but can't be read, and why.
    pub problems: Vec<Problem>,
}

/// Every imported lab installed here, by id.
///
/// # Errors
///
/// No data folder.
pub(crate) fn list(dirs: &AppDirs) -> crate::error::Result<Vec<Imported>> {
    let labs = dirs.data()?.join("labs");
    // No labs folder yet: nothing installed.
    let Ok(entries) = std::fs::read_dir(&labs) else { return Ok(Vec::new()) };
    let mut found: Vec<Imported> = entries
        .filter_map(|e| {
            let e = e.ok()?;
            let id = e.file_name().into_string().ok()?;
            // A staging copy (`.local-….staging`) or a stray file isn't an installed lab.
            (id.starts_with(PREFIX) && is_lab_id(&id) && e.path().is_dir()).then(|| read(&e.path(), id))
        })
        .collect();
    found.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(found)
}

/// The imported lab `id` installed in `dir`.
fn read(dir: &Path, id: String) -> Imported {
    let name = runtime::lab::load(dir).ok().map(|spec| spec.name).filter(|n| !n.trim().is_empty()).unwrap_or_else(|| id.clone());
    let ctf::Ctf { metadata, objectives, problems } = ctf::read(dir);
    Imported { id, name, metadata, objectives, problems }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::test_app::TestApp;

    const SPEC: &str = "version: 1\nname: invoice-portal\nnetworks:\n  lab: { cidr: 10.30.0.0/24 }\nmachines:\n  web:\n    networks: { lab: 10 }\n    app: { image: nginx:1.27 }\n";

    #[test]
    fn only_imported_labs_are_listed_each_with_its_own_ctf_folder() {
        let t = TestApp::new();
        assert_eq!(list(&t.dirs()).unwrap(), [], "no labs folder yet");
        t.lab("66d71cbf-9b4e-4a78-b3cf-8208227a96b2", &[("isoloom.yml", SPEC), (".ctf/metadata.json", r#"{ "title": "Catalogue" }"#)]);
        t.lab("local-invoice", &[("isoloom.yml", SPEC), (".ctf/metadata.json", r#"{ "title": "Supplier portal API", "category": "web" }"#)]);
        t.lab("local-bare", &[("isoloom.yml", SPEC)]);
        t.lab("local-broken", &[(".ctf/metadata.json", "{")]);
        t.lab(".local-invoice.staging", &[("isoloom.yml", SPEC)]);
        std::fs::write(t.data().join("labs").join("local-file"), "").unwrap();

        let labs = list(&t.dirs()).unwrap();
        let ids: Vec<_> = labs.iter().map(|l| l.id.as_str()).collect();
        assert_eq!(ids, ["local-bare", "local-broken", "local-invoice"]);
        // No .ctf: the spec's name, nothing else.
        assert_eq!(labs[0], Imported { id: "local-bare".into(), name: "invoice-portal".into(), metadata: None, objectives: None, problems: vec![] });
        // No spec it can read: named by its id, and the unreadable file said.
        assert_eq!(labs[1].name, "local-broken");
        assert_eq!(labs[1].problems.iter().map(|p| p.file.as_str()).collect::<Vec<_>>(), ["metadata.json"]);
        let invoice = labs[2].metadata.as_ref().unwrap();
        assert_eq!((invoice.title.as_str(), invoice.category.as_deref()), ("Supplier portal API", Some("web")));
    }
}
