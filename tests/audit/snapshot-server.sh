#!/bin/sh
# The leftover audit's snapshot of a Proxmox VE node or an ESXi host (snapshot.sh: Linux and
# macOS; snapshot.ps1: Windows): what the server holds of every kind of resource the stack makes
# there (tests/leftovers.md, "Servers"), as one JSON document on standard output. POSIX sh, as
# ESXi's shell is BusyBox's: run on the server itself, as root, for example from the machine that
# drives it:
#
#   ssh root@<server> sh -s < tests/audit/snapshot-server.sh > before.json
#
# Read-only: it lists, never changes. The document is snapshot.sh's ({"audit": 1, "os": "proxmox"
# or "esxi", ..., "probes": [...]}); a probe whose tool fails is recorded with its error.
# The probes' scripts expand their variables in their own shell, on purpose.
# shellcheck disable=SC2016
export LC_ALL=C
LIMIT=${CYBERCTF_AUDIT_TIMEOUT:-30}
WORK=$(mktemp -d /tmp/audit-snapshot.XXXXXX) || exit 2
trap 'rm -rf "$WORK"' EXIT
: >"$WORK/probes"

# json <text>: the text as a JSON string.
json() {
  printf '%s' "$1" | tr -d '\000-\010\013\014\016-\037' | awk '
    BEGIN { printf "\"" }
    { gsub(/\\/, "\\\\"); gsub(/"/, "\\\""); gsub(/\t/, "\\t"); gsub(/\r/, ""); printf "%s%s", sep, $0; sep = "\\n" }
    END { printf "\"" }'
}

# probe <kind> <tool> <script>: the script's output as the kind's probe (sh -c, within the time
# limit when the server has timeout).
probe() {
  if command -v timeout >/dev/null 2>&1; then
    timeout "$LIMIT" sh -c "$3" >"$WORK/out" 2>"$WORK/err" </dev/null
  else
    sh -c "$3" >"$WORK/out" 2>"$WORK/err" </dev/null
  fi
  rc=$?
  case $rc in
    0) status=ok note='' ;;
    124 | 143) status=timeout note="no answer in ${LIMIT}s" ;;
    *) status=failed note="exit $rc: $(grep -v '^[[:space:]]*$' "$WORK/err" | tail -n 1)" ;;
  esac
  printf '{"kind":"%s","tool":"%s","status":"%s","note":%s,"output":%s}\n' "$1" "$2" "$status" "$(json "$note")" "$(json "$(cat "$WORK/out")")" >>"$WORK/probes"
}

if command -v qm >/dev/null 2>&1; then
  OS=proxmox
  node=$(hostname)
  probe proxmox-vm qm 'qm list'
  probe proxmox-ct pct 'pct list'
  probe proxmox-sdn-zone pvesh 'pvesh get /cluster/sdn/zones --output-format json'
  probe proxmox-sdn-vnet pvesh 'pvesh get /cluster/sdn/vnets --output-format json'
  probe proxmox-network pvesh "pvesh get /nodes/$node/network --output-format json"
  probe proxmox-pool pvesh 'pvesh get /pools --output-format json'
  probe proxmox-user pvesh 'pvesh get /access/users --output-format json'
  probe proxmox-token pvesh 'pvesh get /access/users --full 1 --output-format json'
  probe proxmox-ipset pvesh 'pvesh get /cluster/firewall/ipset --output-format json'
  probe proxmox-mapping pvesh 'pvesh get /cluster/mapping/dir --output-format json'
  probe proxmox-volume pvesm 'for s in $(pvesm status | awk "NR > 1 { print \$1 }"); do pvesm list "$s" 2>/dev/null | sed "s|^|$s |"; done'
elif command -v vim-cmd >/dev/null 2>&1; then
  OS=esxi
  probe esxi-vm vim-cmd 'vim-cmd vmsvc/getallvms'
  probe esxi-portgroup esxcli 'esxcli network vswitch standard portgroup list'
  probe esxi-vswitch esxcli 'esxcli network vswitch standard list'
  # The datastores by name (their links in /vmfs/volumes), each top folder and file.
  probe esxi-file ls 'cd /vmfs/volumes && for d in *; do [ -L "$d" ] && ls -1 "$d/" | sed "s|^|$d/|"; done; true'
else
  echo "snapshot-server.sh: neither qm (Proxmox VE) nor vim-cmd (ESXi) here" >&2
  exit 2
fi

printf '{"audit":1,"os":"%s","host":%s,"user":%s,"taken":%s,"roots":[],"probes":[\n' "$OS" "$(json "$(hostname)")" "$(json "$(id -un 2>/dev/null || echo root)")" "$(date +%s)"
sed '$!s/$/,/' "$WORK/probes"
printf ']}\n'
