# The Cyber CTF attack box: Kali with its standard toolset, ready the moment a shell opens.
# The bare kalilinux/kali-rolling image ships no tools; kali-linux-headless is Kali's own
# default selection minus the desktop. Built for amd64 and arm64, so it is the same box on
# macOS (Intel and Apple Silicon), Linux and Windows.
FROM kalilinux/kali-rolling

ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update \
 && apt-get install -y kali-linux-headless \
 && apt-get clean \
 && rm -rf /var/lib/apt/lists/*

# A clean prompt and one stable window title, whatever terminal the launcher opens.
COPY bashrc.cyberctf /tmp/bashrc.cyberctf
RUN cat /tmp/bashrc.cyberctf >> /root/.bashrc && rm /tmp/bashrc.cyberctf

WORKDIR /root
CMD ["sleep", "infinity"]
