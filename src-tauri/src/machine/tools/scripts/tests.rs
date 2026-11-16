use super::*;

#[test]
fn a_failed_cask_fetch_updates_homebrew_and_retries_once() {
    // A Homebrew whose fetch always fails (an old one crashing on the cask data).
    let dir = std::env::temp_dir().join(format!("cyberctf-brew-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let brew = dir.join("brew");
    let log = dir.join("calls");
    crate::testing::write_executable(&brew, format!("#!/bin/sh\necho \"$1\" >> '{}'\n[ \"$1\" = update ]\n", log.display()));

    let step = fetch_and_open(brew.to_str().unwrap(), "vagrant", "Vagrant");
    let out = std::process::Command::new(&step.program).args(&step.args).output().unwrap();
    let calls = std::fs::read_to_string(&log).unwrap();
    std::fs::remove_dir_all(&dir).ok();

    assert!(!out.status.success());
    assert_eq!(calls.lines().collect::<Vec<_>>(), ["fetch", "update", "fetch"]);
    assert!(String::from_utf8_lossy(&out.stderr).contains("Run 'brew update' in Terminal"));
}
