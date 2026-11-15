apt-get install -y $keep kali-desktop-xfce lightdm
# LightDM signs the player in to XFCE, on the VM's own screen; its X server never blanks it.
mkdir -p /etc/lightdm/lightdm.conf.d
printf '[Seat:*]\nautologin-user=player\nautologin-user-timeout=0\nautologin-session=xfce\nuser-session=xfce\nxserver-command=X -s 0 -dpms\n' > /etc/lightdm/lightdm.conf.d/50-cyberctf-autologin.conf
groupadd -f -r autologin
usermod -a -G autologin player
# No screen lock: Kali's lockers don't start for the player, and at each sign-in the session
# turns off blanking, the screensaver's lock, the power manager's and the session's own.
mkdir -p /home/player/.config/autostart
for locker in light-locker xfce4-screensaver; do
  printf '[Desktop Entry]\nHidden=true\n' > "/home/player/.config/autostart/$locker.desktop"
done
chown -R player: /home/player/.config
cat > /usr/local/bin/cyberctf-no-screen-lock <<'NOLOCK'
#!/bin/sh
xset s off -dpms
off() { xfconf-query -c "$1" -p "$2" -n -t bool -s false; }
off xfce4-screensaver /saver/enabled
off xfce4-screensaver /lock/enabled
off xfce4-power-manager /xfce4-power-manager/dpms-enabled
off xfce4-power-manager /xfce4-power-manager/lock-screen-suspend-hibernate
off xfce4-session /shutdown/LockScreen
exit 0
NOLOCK
chmod 0755 /usr/local/bin/cyberctf-no-screen-lock
printf '[Desktop Entry]\nType=Application\nName=No screen lock\nExec=/usr/local/bin/cyberctf-no-screen-lock\nNoDisplay=true\n' > /etc/xdg/autostart/cyberctf-no-screen-lock.desktop
systemctl set-default graphical.target
systemctl enable lightdm
systemctl restart lightdm
