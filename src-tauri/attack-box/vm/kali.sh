#!/bin/sh
# The Cyber CTF attack box: Kali's headless toolset, as the attack box image has it.
set -e
export DEBIAN_FRONTEND=noninteractive
# Configuration files kept as they are; on a Debian made Kali, Debian's libraries renamed in Kali
# (the t64 move) hand their files over to the new ones.
keep='-o Dpkg::Options::=--force-confdef -o Dpkg::Options::=--force-confold -o Dpkg::Options::=--force-overwrite'
if ! grep -q '^ID=kali' /etc/os-release; then
  apt-get update
  apt-get install -y ca-certificates curl gnupg
  set -e; d="$(mktemp -d)"; trap 'rm -rf "$d"' EXIT; printf 'DPkg::Lock::Timeout "600";\n' >"$d/apt.conf"; export APT_CONFIG="$d/apt.conf"
  command -v gpg >/dev/null || { if command -v apt-get >/dev/null; then apt-get update; apt-get install -y gnupg; else dnf install -y gnupg2; fi; }; mkdir -p -m 700 "$d/gnupg"
  curl -fsSL https://archive.kali.org/archive-keyring.gpg -o "$d/key"
  k="$(gpg --homedir "$d/gnupg" --batch --show-keys --with-colons "$d/key" | awk -F: '$1=="pub"{p=1} $1=="fpr"&&p{print $10; p=0}' | LC_ALL=C sort | paste -s -d ' ' -)"; [ "$k" = "44C6513A8E4FB3D30875F758ED444FF07D8D0BF6 827C8569F2518CC677FECA1AED65462EC8D5E4C5" ] || { echo "Kali's archive's signing key isn't the expected one (fingerprint 44C6513A8E4FB3D30875F758ED444FF07D8D0BF6 827C8569F2518CC677FECA1AED65462EC8D5E4C5, got: $k). Nothing was installed." >&2; exit 1; }
  install -m 0644 "$d/key" /usr/share/keyrings/kali-archive-keyring.gpg
  echo 'deb [signed-by=/usr/share/keyrings/kali-archive-keyring.gpg] http://http.kali.org/kali kali-rolling main contrib non-free non-free-firmware' > /etc/apt/sources.list.d/kali.list
  # Kali's archive only, over what Debian installed (its versions win, older ones included).
  rm -f /etc/apt/sources.list.d/debian.sources; : > /etc/apt/sources.list
  printf 'Package: *\nPin: release o=Kali\nPin-Priority: 1001\n' > /etc/apt/preferences.d/kali
  apt-get update
  # Its own kernel stays: Kali's needs more room than the image's boot partition has.
  apt-mark hold $(dpkg -l 'linux-image-*' | awk '/^ii/ {print $2}')
  apt-get full-upgrade -y --allow-downgrades $keep
  apt-get install -f -y $keep
fi
