#!/bin/sh
# The attack box's player account, made by root where it is missing: "$account", with a home, a
# login shell and sudo without a password. Where the shared folder ("$shared", mounted from the
# computer running the lab) belongs to a user the box doesn't have, the account takes that user's
# uid, and it joins the folder's group: the player writes there as that computer's user does.
#
# Run when the image is built (no shared folder then), at each start of a container box (its image
# may predate the account, or the folder's owner may differ from the image's), and once when a VM
# box is set up. The launcher names the account (`attack_machine::PLAYER`) and the folder.
#
#   account=<name> [shared=<folder>] sh player-account.sh
set -e
: "${account:?the account to make}"
shared=${shared:-}

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
