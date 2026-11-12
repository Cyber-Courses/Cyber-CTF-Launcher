#!/usr/bin/env bash
# The leftover audit's snapshot on Linux and macOS (snapshot.ps1: Windows; snapshot-server.sh: a
# Proxmox or ESXi server): what this machine holds of every kind of resource the launcher's stack
# can make (tests/leftovers.md), as one JSON document. Read-only and quick: each tool is asked
# once, with a time limit, and a tool that's missing, refuses or doesn't answer is recorded as
# such (the diff makes it a `skip`), never an error.
#
#   tests/audit/snapshot.sh [--out <file>] [--root <dir>]... [--sudo] [--only <group>,...]
#
#   --out <file>  where the snapshot goes (default: standard output)
#   --root <dir>  a folder of the run's own (CYBERCTF_WDIO_ROOT, a runner's work folder): the
#                 `.vagrant` folders and pid and lock files under it are recorded too
#   --sudo        read what only root can (iptables, nftables, other users' sockets, libvirt's
#                 system domains) through `sudo -n`, when it lets in without a password; without
#                 it those kinds are recorded as needing root
#   --only <groups>  only these groups of kinds (default: all): docker, vagrant, files,
#                 registration, process, hypervisors, network, credentials
#
# The document: {"audit": 1, "os", "host", "user", "taken" (epoch seconds), "roots", "probes": [{
# "kind", "tool", "status" (ok, missing, denied, timeout, failed), "note", "output"}]}: each
# probe the tool's own output, which tests/audit/inventory.ts reads (its unit tests hold each
# parser to a recorded output). tests/audit/run.sh diffs two of them.
# The `sh -c` scripts below expand their variables in their own shell, on purpose.
# shellcheck disable=SC2016
set -uo pipefail
export LC_ALL=C

OUT=-
ROOTS=()
SUDO=0
ONLY=''
while [ $# -gt 0 ]; do
  case $1 in
    --out)
      OUT=${2:?--out takes a file}
      shift
      ;;
    --root)
      ROOTS+=("${2:?--root takes a folder}")
      shift
      ;;
    --sudo) SUDO=1 ;;
    --only)
      ONLY=${2:?--only takes groups, comma separated}
      shift
      ;;
    -h | --help)
      sed -n '2,/^[^#]/p' "$0" | sed '$d' | sed 's/^# \{0,1\}//'
      exit 0
      ;;
    *)
      echo "snapshot.sh: unknown argument $1 (--help says how)" >&2
      exit 2
      ;;
  esac
  shift
done

# GUI-installed tools are not always on a shell's PATH (Homebrew, Docker Desktop, VirtualBox).
export PATH="$PATH:/usr/local/bin:/opt/homebrew/bin:/Applications/Docker.app/Contents/Resources/bin:/Applications/VirtualBox.app/Contents/MacOS"
OS=linux
[ "$(uname -s)" = Darwin ] && OS=macos
LIMIT=${CYBERCTF_AUDIT_TIMEOUT:-20}
WORK=$(mktemp -d "${TMPDIR:-/tmp}/audit-snapshot.XXXXXX") || exit 2
trap 'rm -rf "$WORK"' EXIT
PROBES=$WORK/probes

# What only root reads, through sudo when --sudo is given and it asks no password.
ROOT=()
if [ "$(id -u)" -ne 0 ] && [ "$SUDO" = 1 ] && sudo -n true 2>/dev/null; then ROOT=(sudo -n); fi
is_root() { [ "$(id -u)" -eq 0 ] || [ "${#ROOT[@]}" -gt 0 ]; }

# json <text>: the text as a JSON string (control characters but tab and line breaks dropped).
json() {
  printf '%s' "$1" | tr -d '\000-\010\013\014\016-\037' |
    sed -e 's/\\/\\\\/g' -e 's/"/\\"/g' -e $'s/\t/\\\\t/g' -e $'s/\r/\\\\r/g' |
    awk 'BEGIN { printf "\"" } { printf "%s%s", sep, $0; sep = "\\n" } END { printf "\"" }'
}

# timed <command...>: the command with the time limit (timeout, else perl's alarm, else none);
# exit 124 when it ran out.
timed() {
  if command -v timeout >/dev/null; then
    timeout "$LIMIT" "$@"
  elif command -v perl >/dev/null; then
    perl -e '$SIG{ALRM} = sub { kill "TERM", $pid; exit 124 }; alarm shift; $pid = fork // exit 125;
      if (!$pid) { exec @ARGV or exit 127 } waitpid $pid, 0; exit($? >> 8)' "$LIMIT" "$@"
  else
    "$@"
  fi
}

# emit <kind> <tool> <status> <note> <output file>: a probe's record.
emit() {
  printf '{"kind":"%s","tool":"%s","status":"%s","note":%s,"output":%s}\n' "$1" "$2" "$3" "$(json "$4")" "$(json "$(cat "$5" 2>/dev/null)")" >>"$PROBES"
}

# probe <kind> <tool> <command...>: the command's output as the kind's probe; missing when <tool>
# isn't on PATH, timeout or failed (its last error line the note) otherwise.
probe() {
  local kind=$1 tool=$2 rc
  shift 2
  if ! command -v "$tool" >/dev/null && [ ! -x "$tool" ]; then
    : >"$WORK/out"
    emit "$kind" "$tool" missing "no $tool on PATH" "$WORK/out"
    return 0
  fi
  timed "$@" >"$WORK/out" 2>"$WORK/err" </dev/null
  rc=$?
  case $rc in
    0) emit "$kind" "$tool" ok "" "$WORK/out" ;;
    124) emit "$kind" "$tool" timeout "no answer in ${LIMIT}s" "$WORK/out" ;;
    *) emit "$kind" "$tool" failed "exit $rc: $(grep -v '^\s*$' "$WORK/err" | tail -n 1)" "$WORK/out" ;;
  esac
}

# rooted <kind> <tool> <command...>: a probe of what only root reads; denied without it.
rooted() {
  if ! is_root; then
    : >"$WORK/out"
    command -v "$2" >/dev/null || {
      emit "$1" "$2" missing "no $2 on PATH" "$WORK/out"
      return 0
    }
    emit "$1" "$2" denied "needs root (run with --sudo where sudo asks no password)" "$WORK/out"
    return 0
  fi
  local kind=$1 tool=$2
  shift 2
  probe "$kind" "$tool" ${ROOT[@]+"${ROOT[@]}"} "$@"
}

# answers <command...>: whether a daemon answers (Docker, libvirt): a probe of one that doesn't is
# `missing`, saying so.
answers() { timed "$@" >/dev/null 2>&1 </dev/null; }

# --- Docker: one JSON object a line, its labels and creation time as Docker has them ----------

docker_probes() {
  if ! command -v docker >/dev/null || ! answers docker version --format '{{.Server.Version}}'; then
    : >"$WORK/out"
    local why="no docker on PATH"
    command -v docker >/dev/null && why="Docker isn't running (docker version has no server)"
    for kind in docker-container docker-network docker-volume docker-image; do emit "$kind" docker missing "$why" "$WORK/out"; done
    return 0
  fi
  probe docker-container docker sh -c 'ids=$(docker ps -aq --no-trunc); [ -z "$ids" ] || docker inspect --format "{\"id\":{{json .Id}},\"name\":{{json .Name}},\"created\":{{json .Created}},\"labels\":{{json .Config.Labels}},\"state\":{{json .State.Status}}}" $ids'
  probe docker-network docker sh -c 'docker network inspect --format "{\"id\":{{json .Id}},\"name\":{{json .Name}},\"created\":{{json .Created}},\"labels\":{{json .Labels}},\"driver\":{{json .Driver}}}" $(docker network ls -q --no-trunc)'
  probe docker-volume docker sh -c 'names=$(docker volume ls -q); [ -z "$names" ] || docker volume inspect --format "{\"name\":{{json .Name}},\"created\":{{json .CreatedAt}},\"labels\":{{json .Labels}},\"mountpoint\":{{json .Mountpoint}}}" $names'
  probe docker-image docker docker image ls --no-trunc --format '{{json .}}'
}

# --- Vagrant: its machine index and its boxes, read from VAGRANT_HOME (no Ruby to start) -------

vagrant_probes() {
  local home=${VAGRANT_HOME:-$HOME/.vagrant.d}
  if [ ! -d "$home" ]; then
    : >"$WORK/out"
    emit vagrant-machine vagrant missing "no Vagrant home ($home)" "$WORK/out"
    emit vagrant-box vagrant missing "no Vagrant home ($home)" "$WORK/out"
  else
    probe vagrant-machine cat sh -c 'f="$1/data/machine-index/index"; [ ! -f "$f" ] || cat "$f"' sh "$home"
    probe vagrant-box find sh -c 'd="$1/boxes"; [ ! -d "$d" ] || (cd "$d" && find . -mindepth 3 -maxdepth 4 -name metadata.json | sed "s|^\./||; s|/metadata.json$||")' sh "$home"
  fi
}

# --- The launcher's own folders: its data, temp folders, run folders, pid and lock files ------

# The launcher's data folders (its identifier, tauri.conf.json), Isothrum's, the run's roots.
data_dirs() {
  if [ "$OS" = macos ]; then
    printf '%s\n' "$HOME/Library/Application Support/org.cyberctf.desktop" "$HOME/Library/Caches/org.cyberctf.desktop"
  else
    printf '%s\n' "${XDG_DATA_HOME:-$HOME/.local/share}/org.cyberctf.desktop" "${XDG_CONFIG_HOME:-$HOME/.config}/org.cyberctf.desktop" "${XDG_CACHE_HOME:-$HOME/.cache}/org.cyberctf.desktop"
  fi
  printf '%s\n' "$HOME/.cyberctf" "${ISOTHRUM_HOME:-$HOME/.isothrum}" ${ROOTS[@]+"${ROOTS[@]}"}
}

# What the stack registers with the system: the cyberctf:// handler's desktop entry (Linux), the
# app's own entry a package installs.
registration_probes() {
  if [ "$OS" = macos ]; then
    probe os-registration find sh -c 'for d in "$HOME/Library/LaunchAgents" /Library/LaunchAgents /Library/LaunchDaemons; do [ -d "$d" ] && find "$d" -maxdepth 1 -iname "*cyberctf*"; done; true'
  else
    probe os-registration find sh -c 'for d in "${XDG_DATA_HOME:-$HOME/.local/share}/applications" /usr/share/applications /usr/local/share/applications; do [ -d "$d" ] && find "$d" -maxdepth 1 \( -iname "*cyber*ctf*" -o -iname "*cyberctf*" \); done; true'
  fi
}

files_probes() {
  local dirs=() d
  while IFS= read -r d; do [ -d "$d" ] && dirs+=("$d"); done < <(data_dirs)
  : >"$WORK/files"
  # The temp folders the stack names (tests/leftovers.md, "Files"), at the top of each temp folder.
  # -H: a temp folder that is a symlink is searched, not listed as itself (macOS's /tmp is one, to
  # /private/tmp, which find alone never looks into).
  for d in "${TMPDIR:-/tmp}" /tmp /var/tmp; do
    [ -d "$d" ] || continue
    find -H "$d" -mindepth 1 -maxdepth 1 \( -name 'cyberctf*' -o -name '.cyberctf*' -o -name 'org_cyberctf*' -o -name 'isothrum*' -o -name 'isoloom*' \) 2>/dev/null
  done | sort -u >"$WORK/temp"
  probe temp-folder cat cat "$WORK/temp"
  if [ "${#dirs[@]}" -eq 0 ]; then
    probe vagrant-dotfolder cat cat "$WORK/files"
    probe run-file cat cat "$WORK/files"
    return 0
  fi
  probe vagrant-dotfolder find find ${dirs[@]+"${dirs[@]}"} -maxdepth 8 -type d -name .vagrant -prune
  # A lab's own lock files (a package manager's) are its sources, not the stack's.
  probe run-file find find ${dirs[@]+"${dirs[@]}"} -maxdepth 8 \( -name node_modules -o -name .git \) -prune -o -type f \
    \( -name '*.pid' -o -name '*.lock' -o -name '*.sock' -o -name lock \) \
    ! \( -name yarn.lock -o -name Pipfile.lock -o -name poetry.lock -o -name composer.lock -o -name Gemfile.lock -o -name Cargo.lock -o -name flake.lock -o -name pnpm-lock.yaml \) -print
}

# --- Processes and listening ports ---------------------------------------------------------

# The processes the stack starts (tests/leftovers.md, "Processes"): the app, its deploy workers
# and screen bridge, Isothrum, Vagrant, the hypervisors' VM processes, tunnels, Terraform. Only
# those are recorded: the whole process table would carry other programs' arguments. Vagrant by its
# program (the command's first word, or the `.../bin/vagrant` script its Ruby runs), never a word of
# a path: on a machine whose account is `vagrant` (a Vagrant box) everything runs under
# /home/vagrant/.
STACK_PROCESSES='cyber-ctf|cyberctf|org\.cyberctf|deploy --job|isothrum|isoloom|^ *[0-9]+ +[-0-9:]+ +([^ ]*/)?vagrant( |$)|/bin/vagrant( |$)|qemu-system|VBoxHeadless|VBoxNetDHCP|VBoxNetNAT|utmctl|QEMULauncher|prl_vm_app|vmware-vmx|terraform|xfreerdp|remmina|ssh .*-[LRDNw]'

process_probes() {
  # pid, its elapsed time (ps's etime: the diff dates it from the note's "now"), its command,
  # matched by the stack's patterns, this script's own lines left out.
  probe process ps sh -c '(ps -axww -o pid= -o etime= -o command= 2>/dev/null || ps -eww -o pid= -o etime= -o args=) |
    grep -E "$1" | grep -v -e "grep -E" -e "tests/audit/snapshot"; date +"now %s"' sh "$STACK_PROCESSES"
  if [ "$OS" = macos ]; then
    probe listening-port lsof sh -c 'lsof -nP -iTCP -sTCP:LISTEN -Fpcn; true'
  else
    probe listening-port ss ${ROOT[@]+"${ROOT[@]}"} ss -Hltnup
  fi
}

# --- Hypervisors -------------------------------------------------------------------------

# Each line prefixed by the connection it is on (system, session): libvirt's system domains need
# its group or root, its session ones are the user's. A system connection it can't open fails the
# probe (the kind is skipped, saying so), never reads as empty.
LIBVIRT_LIST='rc=0
for uri in qemu:///system qemu:///session; do
  tag=${uri#qemu:///}
  if ! virsh -c "$uri" uri >/dev/null 2>&1; then
    [ "$tag" = system ] && { echo "no access to $uri (libvirtd not running, or not in its group: --sudo)" >&2; rc=3; }
    continue
  fi
  case $1 in
    volume) for p in $(virsh -c "$uri" pool-list --all --name); do virsh -c "$uri" vol-list --pool "$p" | sed "1,2d; /^$/d; s|^|$tag $p |"; done ;;
    *) virsh -c "$uri" "$2" --all --name | sed "/^$/d; s|^|$tag |" ;;
  esac
done
exit $rc'

libvirt_probes() {
  probe libvirt-domain virsh ${ROOT[@]+"${ROOT[@]}"} sh -c "$LIBVIRT_LIST" sh domain list
  probe libvirt-network virsh ${ROOT[@]+"${ROOT[@]}"} sh -c "$LIBVIRT_LIST" sh network net-list
  probe libvirt-pool virsh ${ROOT[@]+"${ROOT[@]}"} sh -c "$LIBVIRT_LIST" sh pool pool-list
  probe libvirt-volume virsh ${ROOT[@]+"${ROOT[@]}"} sh -c "$LIBVIRT_LIST" sh volume -
}

vbox_probes() {
  probe vbox-vm VBoxManage VBoxManage list vms
  probe vbox-hostonly VBoxManage sh -c 'VBoxManage list hostonlyifs; VBoxManage list hostonlynets 2>/dev/null; true'
  probe vbox-natnet VBoxManage VBoxManage list natnets
  probe vbox-dhcp VBoxManage VBoxManage list dhcpservers
}

mac_vm_probes() {
  local utm=/Applications/UTM.app/Contents/MacOS/utmctl
  if [ -x "$utm" ] && pgrep -xq UTM; then
    probe utm-vm "$utm" "$utm" list
  else
    : >"$WORK/out"
    local why="UTM isn't running (utmctl would start it)"
    [ -x "$utm" ] || why="no UTM ($utm)"
    emit utm-vm utmctl missing "$why" "$WORK/out"
  fi
  probe parallels-vm prlctl prlctl list -a -o uuid,name
}

# --- The network: firewall, hosts file, routes --------------------------------------------

network_probes() {
  probe hosts-entry cat cat /etc/hosts
  if [ "$OS" = macos ]; then
    probe route netstat netstat -rn -f inet
    # pf's anchors as files and in pf.conf (pfctl itself needs root).
    probe pf-anchor sh sh -c 'ls -1 /etc/pf.anchors 2>/dev/null | sed "s|^|file |"; grep -E "^[[:space:]]*(anchor|load anchor|nat-anchor|rdr-anchor)" /etc/pf.conf 2>/dev/null | sed "s|^|conf |"; true'
  else
    probe route ip ip -o route show
    rooted nft-table nft nft list tables
    rooted iptables-rule iptables-save iptables-save
  fi
}

# --- Credentials: the app's keychain entries (their names, never their secrets) ---------------

credential_probes() {
  if [ "$OS" = macos ]; then
    # The login keychain's items of the launcher's services: attributes only, no secret is read.
    probe keychain-item security sh -c 'security dump-keychain 2>/dev/null | awk '"'"'
      /^keychain: / { if (svce ~ /cyberctf|cyber-ctf|org\.cyberctf/) print svce "\t" acct; svce = ""; acct = "" }
      /"svce"<blob>=/ { svce = $0; sub(/.*<blob>="/, "", svce); sub(/"$/, "", svce) }
      /"acct"<blob>=/ { acct = $0; sub(/.*<blob>="/, "", acct); sub(/"$/, "", acct) }
      END { if (svce ~ /cyberctf|cyber-ctf|org\.cyberctf/) print svce "\t" acct }'"'"''
  elif command -v secret-tool >/dev/null && [ -n "${DBUS_SESSION_BUS_ADDRESS:-}" ]; then
    probe keychain-item secret-tool sh -c 'for s in org.cyberctf.desktop cyberctf cyber-ctf; do secret-tool search --all service "$s" 2>&1 | grep -E "^(label|attribute\.(service|username)) = "; done; true'
  else
    : >"$WORK/out"
    emit keychain-item secret-tool missing "no Secret Service (secret-tool and a session bus)" "$WORK/out"
  fi
}

: >"$PROBES"
# want <group>: whether --only lets it run.
want() { [ -z "$ONLY" ] || [[ ,$ONLY, == *,$1,* ]]; }
want docker && docker_probes
want vagrant && vagrant_probes
want files && files_probes
want registration && registration_probes
want process && process_probes
if want hypervisors; then
  if [ "$OS" = linux ]; then libvirt_probes; else mac_vm_probes; fi
  vbox_probes
fi
want network && network_probes
want credentials && credential_probes

{
  printf '{"audit":1,"os":"%s","host":%s,"user":%s,"taken":%s,"roots":[' "$OS" "$(json "$(hostname)")" "$(json "$(id -un)")" "$(date +%s)"
  sep=''
  for d in ${ROOTS[@]+"${ROOTS[@]}"}; do
    printf '%s%s' "$sep" "$(json "$d")"
    sep=,
  done
  printf '],"probes":[\n'
  sed '$!s/$/,/' "$PROBES"
  printf ']}\n'
} >"$WORK/snapshot.json"
if [ "$OUT" = - ]; then cat "$WORK/snapshot.json"; else cp "$WORK/snapshot.json" "$OUT"; fi
