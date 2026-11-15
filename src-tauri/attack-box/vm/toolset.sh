if ! dpkg-query -W -f='${Status}' kali-linux-headless 2>/dev/null | grep -q 'ok installed'; then
  apt-get update
  apt-get install -y $keep kali-linux-headless
fi
