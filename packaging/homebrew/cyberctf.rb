# Cyber CTF's Homebrew cask, Casks/cyberctf.rb in Cyber-Courses/homebrew-tap. Its source is
# packaging/homebrew/cyberctf.rb in Cyber-Courses/Cyber-CTF-Launcher: each release's distribute
# workflow fills in the version and the DMG's checksum and replaces the tap's copy, so edit it
# there, not in the tap.
cask "cyberctf" do
  version "__VERSION__"
  sha256 "__SHA256__"

  url "https://github.com/Cyber-Courses/Cyber-CTF-Launcher/releases/download/v#{version}/Cyber.CTF_#{version}_universal.dmg"
  name "Cyber CTF"
  desc "Run Cyber CTF labs on your own machine"
  homepage "https://github.com/Cyber-Courses/Cyber-CTF-Launcher"

  depends_on :macos

  # No `auto_updates true`: a Homebrew install is updated by `brew upgrade`, not by the app's own
  # updater, which a Homebrew install doesn't use.

  app "Cyber CTF.app"

  # Not removed by zap: the login keychain's items of service "org.cyberctf.desktop" (the
  # sign-in session, and a password per saved server or cloud account; delete them in Keychain
  # Access, or with `security delete-generic-password -s org.cyberctf.desktop` until it finds
  # none), and ~/CyberCTF/shared, the folder shared with the attack box, which holds the user's
  # own files.
  zap trash: [
    "~/Library/Application Support/org.cyberctf.desktop",
    "~/Library/Caches/org.cyberctf.desktop",
    "~/Library/Logs/org.cyberctf.desktop",
    "~/Library/WebKit/org.cyberctf.desktop",
  ]
end
