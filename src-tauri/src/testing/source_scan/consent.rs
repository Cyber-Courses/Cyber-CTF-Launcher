//! Nothing accepts a licence, an agreement or terms on the player's behalf: no shipped code of the
//! crate (comments aside) and no script it runs (the attack box's provisioning) holds a switch that
//! accepts one for them. Where a vendor asks for it, its own window shows it and the player
//! accepts it there (Docker Desktop's Subscription Service Agreement, at its first start).
//! Isothrum holds its own installs to the same rule (its `consent` guardrail).

use std::path::Path;

use super::crate_sources;

/// What accepts a licence, an agreement or terms on the player's behalf, in lower case: Docker's
/// and VirtualBox's Extension Pack's `--accept-license`, winget's `--accept-*-agreements`,
/// Xcode's `-license accept`, debconf's licence answers, Microsoft's `ACCEPT_EULA`, installers'
/// EULA switches, `--agree-tos`.
const ACCEPTS: [&str; 13] = [
    "--accept-license",
    "--accept-licence",
    "--accept-package-agreements",
    "--accept-source-agreements",
    "--accept-eula",
    "--accept-terms",
    "--agree-tos",
    "-license accept",
    "accepted-license",
    "license select true",
    "accept_eula",
    "accepteula",
    "/eula",
];

/// The lines of `text` (comments aside) that accept something for the player, as `path:line`.
fn accepting(path: &str, text: &str) -> Vec<String> {
    text.lines()
        .enumerate()
        .filter(|(_, line)| {
            let line = line.trim_start();
            !line.starts_with("//") && !line.starts_with('#')
        })
        .filter(|(_, line)| {
            let lower = line.to_ascii_lowercase();
            ACCEPTS.iter().any(|a| lower.contains(a))
        })
        .map(|(n, line)| format!("{path}:{}: {}", n + 1, line.trim()))
        .collect()
}

#[test]
fn a_switch_that_accepts_for_the_player_is_named_with_its_line() {
    let text = "install --accept-license --user=x\n// never `--accept-license`\n# nor --accept-eula\nwinget install --Accept-Source-Agreements\napt-get install -y kali-linux-headless\n";
    assert_eq!(accepting("a.sh", text), ["a.sh:1: install --accept-license --user=x", "a.sh:4: winget install --Accept-Source-Agreements"]);
}

/// The shipped code and the attack box's scripts accept nothing for the player.
#[test]
fn nothing_is_accepted_on_the_players_behalf() {
    let mut found: Vec<String> = crate_sources().iter().flat_map(|s| accepting(&s.path.display().to_string(), &s.code)).collect();
    let scripts = Path::new(env!("CARGO_MANIFEST_DIR")).join("attack-box");
    let mut stack = vec![scripts];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if let Ok(text) = std::fs::read_to_string(&path) {
                found.extend(accepting(&path.display().to_string(), &text));
            }
        }
    }
    assert!(found.is_empty(), "accepts for the player:\n{}", found.join("\n"));
}
