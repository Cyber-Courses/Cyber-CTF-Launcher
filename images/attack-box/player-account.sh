#!/bin/sh
# The attack box's player account, made by root where it is missing: "$account", with a home, a
# login shell and sudo without a password. Where the shared folder ("$shared", mounted from the
# computer running the lab) belongs to a user the box doesn't have, the account takes that user's
# uid, and it joins the folder's group: the player writes there as that computer's user does.
# Where the launcher gave the account a password, "$password_hash" is a root-only file holding its
# crypt(3) hash: handed to chpasswd on its standard input, so neither the password nor its hash is
# ever on a command line. The screen locker, a login, su and SSH take that password; sudo asks none.
#
# Run when the image is built (no shared folder then), at each start of a container box (its image
# may predate the account, or the folder's owner may differ from the image's), and once when a VM
# box is set up. The launcher names the account (`attack_machine::PLAYER`) and the folder.
#
#   account=<name> [shared=<folder>] [password_hash=<file>] sh player-account.sh
set -e
: "${account:?the account to make}"
shared=${shared:-}
password_hash=${password_hash:-}

owner=0
group=0
if [ -n "$shared" ] && [ -d "$shared" ]; then
  owner=$(stat -c %u "$shared")
  group=$(stat -c %g "$shared")
fi
# The folder owner's uid, when it isn't root's and no account of the box has it.
uid=
if [ "$owner" != 0 ] && ! getent passwd "$owner" >/dev/null; then
  uid=$owner
fi
login=/bin/sh
if [ -x /bin/bash ]; then login=/bin/bash; fi

if ! id "$account" >/dev/null 2>&1; then
  if command -v useradd >/dev/null 2>&1; then
    useradd --create-home --shell "$login" ${uid:+--uid "$uid"} "$account"
  else
    adduser -D -s "$login" ${uid:+-u "$uid"} "$account"
  fi
elif [ -n "$uid" ]; then
  usermod --uid "$uid" "$account" || echo "player-account: $account keeps its uid ($shared belongs to uid $uid)" >&2
fi
if [ "$group" != 0 ]; then
  getent group "$group" >/dev/null || groupadd --gid "$group" shared || :
  name=$(getent group "$group" | cut -d: -f1)
  if [ -n "$name" ]; then
    usermod -a -G "$name" "$account" || echo "player-account: $account isn't in $shared's group" >&2
  fi
fi

mkdir -p /etc/sudoers.d
echo "$account ALL=(ALL:ALL) NOPASSWD: ALL" > "/etc/sudoers.d/$account"
chmod 0440 "/etc/sudoers.d/$account"

if [ -n "$password_hash" ] && [ -s "$password_hash" ]; then
  { printf '%s:' "$account"; cat "$password_hash"; } | chpasswd -e || echo "player-account: $account's password couldn't be set" >&2
fi
