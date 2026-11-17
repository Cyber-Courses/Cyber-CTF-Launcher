use super::*;
use crate::runtime::server::secrets::{get_named, set_named};

fn values(pairs: &[(&str, &str)]) -> Values {
    pairs.iter().map(|(n, v)| ((*n).to_owned(), (*v).to_owned())).collect()
}

#[test]
fn a_runs_values_outlast_a_restart_and_go_when_forgotten() {
    let launched = values(&[("CTF_API_URL", "https://api"), ("CTF_LAUNCH_TOKEN", "tok-1"), ("CTF_FLAG_MAIN", "flag{1}")]);
    keep("cyberctf-a", &launched);
    assert_eq!(kept("cyberctf-a"), launched);
    restart();
    assert_eq!(kept("cyberctf-a"), launched, "read back from the store after a restart, in the launch's order");
    assert_eq!(kept("cyberctf-b"), Vec::new(), "another run's are its own");
    forget("cyberctf-a").unwrap();
    assert_eq!(kept("cyberctf-a"), Vec::new());
    restart();
    assert_eq!(kept("cyberctf-a"), Vec::new(), "gone from the store too");
    assert_eq!(get_named("launch-values:cyberctf-a").unwrap(), None);
    forget("cyberctf-a").unwrap();
}

#[test]
fn a_start_replaces_what_the_one_before_kept_and_a_start_with_none_forgets_it() {
    keep("cyberctf-a", &values(&[("CTF_LAUNCH_TOKEN", "old")]));
    keep("cyberctf-a", &values(&[("CTF_LAUNCH_TOKEN", "new")]));
    restart();
    assert_eq!(kept("cyberctf-a"), values(&[("CTF_LAUNCH_TOKEN", "new")]));
    // An import or an offline start has none: the run's old token isn't given to it later.
    keep("cyberctf-a", &[]);
    restart();
    assert_eq!(kept("cyberctf-a"), Vec::new());
    assert_eq!(get_named("launch-values:cyberctf-a").unwrap(), None);
}

#[test]
fn values_this_version_cant_read_are_none_and_never_logged() {
    set_named("launch-values:cyberctf-a", r#"{"CTF_LAUNCH_TOKEN": "secret-tok"#).unwrap();
    crate::testing::logs::take_warnings();
    assert_eq!(kept("cyberctf-a"), Vec::new());
    let warned = crate::testing::logs::take_warnings();
    assert_eq!(warned.len(), 1, "{warned:?}");
    assert!(!warned[0].contains("secret-tok"), "never the values: {warned:?}");
}

#[test]
fn a_sign_out_forgets_every_installed_labs_values_and_nothing_else() {
    let t = crate::testing::test_app::TestApp::new();
    t.lab("one", &[]);
    t.lab("two", &[]);
    keep("cyberctf-one", &values(&[("CTF_LAUNCH_TOKEN", "1")]));
    keep("cyberctf-two", &values(&[("CTF_LAUNCH_TOKEN", "2")]));
    set_named("attack-box:one", "box-password").unwrap();
    forget_all(&t.dirs());
    for run in ["cyberctf-one", "cyberctf-two"] {
        assert_eq!(kept(run), Vec::new(), "{run}");
        assert_eq!(get_named(&entry(run)).unwrap(), None, "{run}");
    }
    assert!(get_named("attack-box:one").unwrap().is_some(), "the attack box's password is the lab's, not the account's");
}
