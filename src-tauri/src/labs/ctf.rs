//! What a lab says about itself in its `.ctf/` folder, beside its `isoloom.yml`: its catalogue
//! entry (`metadata.json`: title, description, category, difficulty, the skills it exercises) and
//! its challenges (`objectives.json`). The catalogue reads them when a lab is published; the
//! launcher reads them from an installed lab's own copy, for a lab with no catalogue entry (one
//! imported from a folder).
//!
//! Each file is optional, and read on its own: one that is missing is simply absent, one that
//! can't be read or doesn't match its schema is reported (which file, and why) instead of read,
//! the other file still read. What a challenge checks (its answers, its dev flag) and its hint
//! texts never leave this module: only how many hints it has.

use std::path::Path;

use serde::Deserialize;

/// The folder a lab describes itself in, at its root.
pub(crate) const DIR: &str = ".ctf";
/// The lab's catalogue entry, in [`DIR`].
const METADATA: &str = "metadata.json";
/// The lab's challenges, in [`DIR`].
const OBJECTIVES: &str = "objectives.json";
/// The most of either file read: a description and its challenges, not a dump.
const MAX_BYTES: u64 = 1024 * 1024;

/// A lab's catalogue entry as `.ctf/metadata.json` gives it: the title, and what the lab page
/// shows besides when it is there.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "LabMetadata")]
pub struct Metadata {
    /// The lab's title.
    pub title: String,
    /// What the lab is about, when it says.
    pub description: Option<String>,
    /// Its category (`web`, `network`…), when it gives one.
    pub category: Option<String>,
    /// 1 easy, 2 medium, 3 hard, when it gives one.
    pub difficulty: Option<u8>,
    /// The skills it exercises, by id (`rest-api-read-http`).
    pub capabilities: Vec<String>,
}

/// A challenge of the lab, as `.ctf/objectives.json` lists it: what it asks, never its answer.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "LabChallenge")]
pub struct Challenge {
    /// Its key, unique in the lab.
    pub key: String,
    /// Its title (its key when it has none).
    pub title: String,
    /// What it asks the player, when it says.
    pub prompt: Option<String>,
    /// Not needed to complete the lab.
    pub optional: bool,
    /// What solving it is worth, when it says.
    pub points: Option<u32>,
    /// How many hints it has (their texts stay in the file).
    pub hint_count: u32,
    /// Its guided steps, in order.
    pub steps: Vec<Step>,
}

/// A guided step of a challenge: what it asks, never its answer.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "LabChallengeStep")]
pub struct Step {
    /// Its key, unique in the lab.
    pub key: String,
    /// Its title (its key when it has none).
    pub title: String,
    /// What it asks, when it says.
    pub prompt: Option<String>,
    /// How many hints it has.
    pub hint_count: u32,
}

/// A file of `.ctf/` that is there but can't be read: which one, and why.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "LabFileProblem")]
pub struct Problem {
    /// The file's name (`metadata.json`).
    pub file: String,
    /// Why it can't be read (a parse error, with its line and column).
    pub error: String,
}

/// What a lab's `.ctf/` folder gives: each file when it is there and reads, and why each one that
/// is there doesn't.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct Ctf {
    /// `metadata.json`, when it is there and reads.
    pub(crate) metadata: Option<Metadata>,
    /// `objectives.json`'s challenges, when it is there and reads.
    pub(crate) objectives: Option<Vec<Challenge>>,
    /// Each file that is there but can't be read, with why.
    pub(crate) problems: Vec<Problem>,
}

/// Reads the `.ctf/` folder of the lab in `lab_dir` (see the module's doc).
pub(crate) fn read(lab_dir: &Path) -> Ctf {
    let dir = lab_dir.join(DIR);
    let mut problems = Vec::new();
    let metadata = kept(&mut problems, METADATA, file::<MetadataFile>(&dir, METADATA).and_then(|f| f.map(MetadataFile::checked).transpose()));
    let objectives = kept(&mut problems, OBJECTIVES, file::<ObjectivesFile>(&dir, OBJECTIVES).and_then(|f| f.map(ObjectivesFile::checked).transpose()));
    Ctf { metadata, objectives, problems }
}

/// What file `name` read as, or none with why it couldn't be added to `problems`.
fn kept<T>(problems: &mut Vec<Problem>, name: &str, read: Result<Option<T>, String>) -> Option<T> {
    read.unwrap_or_else(|error| {
        problems.push(Problem { file: name.to_owned(), error });
        None
    })
}

/// The JSON file `name` in `dir` as `T`: none when it isn't there, why not when it can't be read
/// or parsed (too large, not UTF-8, not `T`).
fn file<T: serde::de::DeserializeOwned>(dir: &Path, name: &str) -> Result<Option<T>, String> {
    let path = dir.join(name);
    let meta = match std::fs::metadata(&path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.to_string()),
    };
    if !meta.is_file() {
        return Err("not a file".into());
    }
    if meta.len() > MAX_BYTES {
        return Err(format!("larger than {} KiB", MAX_BYTES / 1024));
    }
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    // Each file is one object: serde would read a struct from an array too, field by field.
    if !text.trim_start().starts_with('{') {
        return Err("not a JSON object".into());
    }
    serde_json::from_str(&text).map(Some).map_err(|e| e.to_string())
}

/// `metadata.json` as it is written: the fields the launcher reads, typed, the others (the
/// evidence's kind and parameters, the providers) left to the catalogue.
#[derive(Deserialize)]
struct MetadataFile {
    title: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    difficulty: Option<u8>,
    #[serde(default)]
    capabilities: Vec<String>,
}

impl MetadataFile {
    /// The entry, once its values are ones the page can show: a title, a difficulty of 1 to 3.
    /// Blank optional texts are absent.
    fn checked(self) -> Result<Metadata, String> {
        let title = self.title.trim().to_owned();
        if title.is_empty() {
            return Err("`title` is empty".into());
        }
        if let Some(d) = self.difficulty
            && !(1..=3).contains(&d)
        {
            return Err(format!("`difficulty` is {d}, not 1, 2 or 3"));
        }
        let text = |s: Option<String>| s.map(|s| s.trim().to_owned()).filter(|s| !s.is_empty());
        Ok(Metadata {
            title,
            description: text(self.description),
            category: text(self.category),
            difficulty: self.difficulty,
            capabilities: self.capabilities.into_iter().map(|c| c.trim().to_owned()).filter(|c| !c.is_empty()).collect(),
        })
    }
}

/// `objectives.json` as it is written.
#[derive(Deserialize)]
struct ObjectivesFile {
    objectives: Vec<ChallengeFile>,
}

/// A challenge as it is written. Its `check` and `dev` are not read: they are the answers.
#[derive(Deserialize)]
struct ChallengeFile {
    #[serde(flatten)]
    step: StepFile,
    #[serde(default)]
    optional: bool,
    #[serde(default)]
    points: Option<u32>,
    #[serde(default)]
    steps: Vec<StepFile>,
}

/// A step (or a challenge's own fields) as it is written.
#[derive(Deserialize)]
struct StepFile {
    key: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    prompt: Option<String>,
    #[serde(default)]
    hints: Vec<String>,
}

impl StepFile {
    fn checked(self, keys: &mut Vec<String>) -> Result<Step, String> {
        let key = self.key.trim().to_owned();
        if key.is_empty() {
            return Err("an objective has an empty `key`".into());
        }
        if keys.contains(&key) {
            return Err(format!("the key `{key}` is used twice"));
        }
        keys.push(key.clone());
        let title = self.title.map(|t| t.trim().to_owned()).filter(|t| !t.is_empty()).unwrap_or_else(|| key.clone());
        let prompt = self.prompt.filter(|p| !p.trim().is_empty());
        Ok(Step { key, title, prompt, hint_count: u32::try_from(self.hints.len()).unwrap_or(u32::MAX) })
    }
}

impl ObjectivesFile {
    /// The challenges, once each has a key used nowhere else in the lab.
    fn checked(self) -> Result<Vec<Challenge>, String> {
        let mut keys = Vec::new();
        self.objectives
            .into_iter()
            .map(|c| {
                let Step { key, title, prompt, hint_count } = c.step.checked(&mut keys)?;
                let steps = c.steps.into_iter().map(|s| s.checked(&mut keys)).collect::<Result<_, _>>()?;
                Ok(Challenge { key, title, prompt, optional: c.optional, points: c.points, hint_count, steps })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A lab folder with `files` (paths under it), in an app data folder removed with it.
    struct Lab {
        /// Kept for as long as the folder is: it removes it.
        _app: crate::testing::test_app::TestApp,
        dir: std::path::PathBuf,
    }

    impl Lab {
        fn with(files: &[(&str, &str)]) -> Lab {
            let app = crate::testing::test_app::TestApp::new();
            let dir = app.lab("local-t", files);
            Lab { _app: app, dir }
        }
    }

    const METADATA_JSON: &str = r#"{
      "project_name": "cyberlabs-invoice-portal-api", "slug": "invoice-portal-api", "title": "Supplier portal API",
      "description": "Look an invoice up.", "category": "web", "difficulty": 1, "evidence_kind": "AMOUNT",
      "evidence_params": { "min": 1000 }, "dev_evidence": "15600.00",
      "capabilities": ["rest-api-read-http", "http-request-headers"], "question": "Report the amount.", "providers": ["virtualbox"]
    }"#;

    const OBJECTIVES_JSON: &str = r#"{ "objectives": [
      { "key": "main", "title": "Read the flag", "prompt": "Find it.", "check": { "evidence": "SECRET", "format": "FLAG{%s}" },
        "dev": "FLAG{dev}", "hints": ["first", "second"],
        "steps": [ { "key": "ace", "title": "Name the ACE", "prompt": "Which right?", "check": { "answer": ["genericall"] } } ] },
      { "key": "bonus", "optional": true, "points": 50 }
    ] }"#;

    #[test]
    fn a_full_ctf_folder_gives_the_entry_and_the_challenges_without_their_answers() {
        let lab = Lab::with(&[(".ctf/metadata.json", METADATA_JSON), (".ctf/objectives.json", OBJECTIVES_JSON)]);
        let ctf = read(&lab.dir);
        assert_eq!(ctf.problems, []);
        assert_eq!(
            ctf.metadata,
            Some(Metadata {
                title: "Supplier portal API".into(),
                description: Some("Look an invoice up.".into()),
                category: Some("web".into()),
                difficulty: Some(1),
                capabilities: vec!["rest-api-read-http".into(), "http-request-headers".into()],
            })
        );
        let challenges = ctf.objectives.unwrap();
        assert_eq!(
            challenges,
            [
                Challenge {
                    key: "main".into(),
                    title: "Read the flag".into(),
                    prompt: Some("Find it.".into()),
                    optional: false,
                    points: None,
                    hint_count: 2,
                    steps: vec![Step { key: "ace".into(), title: "Name the ACE".into(), prompt: Some("Which right?".into()), hint_count: 0 }],
                },
                Challenge { key: "bonus".into(), title: "bonus".into(), prompt: None, optional: true, points: Some(50), hint_count: 0, steps: vec![] },
            ]
        );
        // The answers, the dev flag and the hint texts stay in the file.
        let sent = serde_json::to_string(&challenges).unwrap();
        for secret in ["genericall", "FLAG{dev}", "SECRET", "first", "check"] {
            assert!(!sent.contains(secret), "{secret} leaked: {sent}");
        }
    }

    #[test]
    fn a_metadata_only_folder_has_no_challenges() {
        let ctf = read(&Lab::with(&[(".ctf/metadata.json", METADATA_JSON)]).dir);
        assert_eq!(ctf.metadata.map(|m| m.title).as_deref(), Some("Supplier portal API"));
        assert_eq!(ctf.objectives, None);
        assert_eq!(ctf.problems, []);
    }

    #[test]
    fn a_lab_without_a_ctf_folder_gives_nothing_and_no_problem() {
        assert_eq!(read(&Lab::with(&[("isoloom.yml", "version: 1\n")]).dir), Ctf::default());
        // An empty folder too.
        let lab = Lab::with(&[]);
        std::fs::create_dir_all(lab.dir.join(DIR)).unwrap();
        assert_eq!(read(&lab.dir), Ctf::default());
    }

    #[test]
    fn a_malformed_file_is_reported_by_name_and_the_other_still_read() {
        let lab = Lab::with(&[(".ctf/metadata.json", "{ \"title\": \"A\", "), (".ctf/objectives.json", OBJECTIVES_JSON)]);
        let ctf = read(&lab.dir);
        assert_eq!(ctf.metadata, None);
        assert_eq!(ctf.objectives.map(|o| o.len()), Some(2));
        assert_eq!(ctf.problems.len(), 1);
        assert_eq!(ctf.problems[0].file, "metadata.json");
        assert!(ctf.problems[0].error.contains("line 1"), "{:?}", ctf.problems);

        // Each way a file can be wrong says why.
        let cases = [
            (".ctf/metadata.json", r#"{ "description": "no title" }"#, "missing field `title`"),
            (".ctf/metadata.json", r#"{ "title": "  " }"#, "`title` is empty"),
            (".ctf/metadata.json", r#"{ "title": "A", "difficulty": 7 }"#, "`difficulty` is 7"),
            (".ctf/metadata.json", r#"{ "title": "A", "difficulty": "hard" }"#, "invalid type"),
            (".ctf/metadata.json", r#"["A"]"#, "not a JSON object"),
            (".ctf/metadata.json", "", "not a JSON object"),
            (".ctf/objectives.json", r#"{ "objectives": {} }"#, "invalid type"),
            (".ctf/objectives.json", r#"{ "objectives": [ { "title": "no key" } ] }"#, "missing field `key`"),
            (".ctf/objectives.json", r#"{ "objectives": [ { "key": "a" }, { "key": "b", "steps": [ { "key": "a" } ] } ] }"#, "`a` is used twice"),
            (".ctf/objectives.json", r#"{ "objectives": [ { "key": "a", "hints": [1] } ] }"#, "invalid type"),
        ];
        for (file, body, why) in cases {
            let ctf = read(&Lab::with(&[(file, body)]).dir);
            assert!(ctf.metadata.is_none() && ctf.objectives.is_none(), "{body}");
            let [problem] = ctf.problems.as_slice() else { panic!("{body}: {:?}", ctf.problems) };
            assert!(file.ends_with(&problem.file), "{body}: {problem:?}");
            assert!(problem.error.contains(why), "{body}: {problem:?}");
        }
    }

    #[test]
    fn a_file_that_is_too_large_or_not_a_file_is_reported_not_read() {
        let lab = Lab::with(&[(".ctf/metadata.json", &format!("{{ \"title\": \"{}\" }}", "a".repeat(1024 * 1024)))]);
        std::fs::create_dir_all(lab.dir.join(DIR).join(OBJECTIVES)).unwrap();
        let ctf = read(&lab.dir);
        assert_eq!(ctf.metadata, None);
        let why: Vec<_> = ctf.problems.iter().map(|p| (p.file.as_str(), p.error.as_str())).collect();
        assert_eq!(why, [("metadata.json", "larger than 1024 KiB"), ("objectives.json", "not a file")]);
    }
}
