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

# A clean prompt and one stable window title, whatever terminal the launcher opens: root's, and
# every account's made after (its skeleton).
COPY bashrc.cyberctf /tmp/bashrc.cyberctf
RUN cat /tmp/bashrc.cyberctf >> /root/.bashrc \
 && cat /tmp/bashrc.cyberctf >> /etc/skel/.bashrc \
 && rm /tmp/bashrc.cyberctf

# The account the player works as (the launcher's shell opens as it): a home, bash, sudo without a
# password. The image's own user stays root, so the launcher can still set the box up; it makes the
# account again where the box starts from an image without it (player-account.sh). The name is the
# launcher's (`attack_machine::PLAYER`, which a test there holds this to).
ARG PLAYER=player
COPY player-account.sh /tmp/player-account.sh
RUN account=$PLAYER sh /tmp/player-account.sh && rm /tmp/player-account.sh

WORKDIR /root
CMD ["sleep", "infinity"]
