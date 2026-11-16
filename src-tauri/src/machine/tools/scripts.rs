//! The shell script behind the macOS casks' installs: the cask downloaded with Homebrew, then
//! opened in its own installer. (Linux's root scripts are isothrum-machine's.)

use super::steps::Step;

// Download the installer with brew (no admin needed), then open it so the tool's own
// native installer runs and the user finishes there (trusted OS prompts, no custom dialog).
// A disk image is mounted quietly and its .pkg opened in Installer, which comes to the front;
// only an app to drag into Applications (Docker Desktop, UTM) is shown in Finder, brought
// forward. Either way the log says where to finish: a window behind the app went unnoticed.
// `brew fetch` doesn't update Homebrew, and an old Homebrew can crash reading today's cask
// data (`undefined method 'first' for nil` in api/cask.rb), so a failed fetch updates
// Homebrew and tries once more.
pub fn fetch_and_open(brew: &str, cask: &str, name: &str) -> Step {
    let script = format!(
        r#"set -e
if ! '{brew}' fetch --cask {cask}; then
  echo "Homebrew couldn't download {name}. Updating Homebrew and trying again (this can take a few minutes)…"
  '{brew}' update || true
  if ! '{brew}' fetch --cask {cask}; then
    echo "Homebrew still can't download {name}. Run 'brew update' in Terminal, or install {name} from its website, then try again." >&2
    exit 1
  fi
fi
f="$('{brew}' --cache --cask {cask})"
case "$f" in
  *.dmg)
    m="$(hdiutil attach -nobrowse -noautoopen "$f" | awk -F'\t' '/\/Volumes\//{{print $NF; exit}}')"
    p="$(find "$m" -maxdepth 1 -name '*.pkg' | head -n 1)"
    if [ -n "$p" ]; then
      open "$p"
      echo "The {name} installer is open in its own window. Finish it there; this step updates by itself."
    else
      open "$m"
      osascript -e 'tell application "Finder" to activate' >/dev/null 2>&1 || true
      echo "A Finder window with {name} is open. Drag it into Applications, then open it once; this step updates by itself."
    fi
    ;;
  *)
    open "$f"
    echo "The {name} installer is open in its own window. Finish it there; this step updates by itself."
    ;;
esac"#
    );
    Step { program: "sh".into(), args: vec!["-c".into(), script], note: Some(format!("Downloading {name}…")), private: None }
}

#[cfg(test)]
mod tests;
