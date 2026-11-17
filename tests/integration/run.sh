#!/usr/bin/env bash
# The integration tests: real labs, end to end, on the real tools. The regression suite for what
# only real Docker, Vagrant, hypervisors and clouds show. Each scenario is an opt-in Rust test
# (`#[ignore]`) run with the knobs it needs; one verdict per scenario, and a failure doesn't stop
# the run. docs/contributing/testing/integration.md says what each scenario guards against; tests/README.md
# lists every knob and the results' format.
#
#   tests/integration/run.sh                                            the local tier: Docker, VMs here
#   CYBERCTF_TEST_INTEGRATION_TIERS="local aws" tests/integration/run.sh   and the AWS tier (billed while it runs)
#   CYBERCTF_TEST_INTEGRATION_TIERS=server tests/integration/run.sh        a lab on a Proxmox / ESXi host (its settings below)
#   ONLY="attack-box-docker attack-box-vm" tests/integration/run.sh
#   tests/integration/run.sh --only vm-lab@virtualbox                   one scenario on one hypervisor (the rig's shard)
#   tests/integration/run.sh --results results.jsonl                    and a JSON record per scenario there
#   tests/integration/run.sh --list                                     the scenarios and the knobs each reads
#
# Each scenario runs with a temp folder of its own (TMPDIR, TMP and TEMP: `cyberctf-it.XXXXXX`, on
# disk: see temp_base), which its log names and which goes when it ends, passed, failed or killed
# past its cap: what a test makes there (a lab's copy, the runner's home, a test thread's folder,
# its app's folders: CYBERCTF_TEST_APP_ROOT) never piles up on the machine, and a process still
# running from it (a VM or a QEMU hub a killed test started, daemonized) is stopped first. It holds
# the labs' runs, their VMs' disks among them: by default on Linux in /var/tmp, not /tmp (a tmpfs
# of half the RAM on Debian 13 and Fedora); CYBERCTF_TEST_INTEGRATION_TEMP names another folder.
# macOS's own TMPDIR (/var/folders/…) isn't used: too long for the Unix sockets made there (104
# bytes). The guided setup's scenario keeps the system's: its folder is kept from pass to pass.
#
# Fail fast: each scenario is killed past its cap (8 minutes; the attack-box ones 10, until the
# goldens carry their images), and once a scenario fails on the platform itself (the launcher's
# own refusals: a lab that can't run on that hypervisor, an architecture it doesn't support, a
# tool or plugin not installed, a hypervisor not responding, Docker not running), the provider's
# remaining scenarios are skipped with that failure as their reason; a failure in the lab fails
# that scenario alone.
#
# Options:
#   --only <scenario>[@<provider>]  only that scenario (as ONLY), on that hypervisor (as
#                       CYBERCTF_TEST_VM_PROVIDER); may repeat
#   --results <file>    append a JSON record per scenario to <file> (tests/README.md, "Results")
#   --list              print a JSON line per scenario (its id, tier, form, test and knobs) and exit
#   --tiers <tiers>     the tiers to run, as CYBERCTF_TEST_INTEGRATION_TIERS (`--tiers server`:
#                       the servers' tests, which `pnpm test:servers` runs)
#   -h, --help          this
#
# Knobs:
#   CYBERCTF_TEST_INTEGRATION_TIERS  local, aws, server (default: local)
#   CYBERCTF_TEST_INTEGRATION_LOGS   where each scenario's log goes (default: tests/integration/.state/logs)
#   CYBERCTF_TEST_INTEGRATION_TIMEOUT seconds a scenario may take (480; the attack-box ones 600)
#   CYBERCTF_TEST_ISOLOOM_DIR        an Isoloom checkout, for its example labs (default: ../isoloom)
#   CYBERCTF_TEST_VM_PROVIDER        the hypervisor for VM labs (default: qemu)
#   CYBERCTF_TEST_AWS_REGION         the AWS tier's region (default: eu-west-3)
#   CYBERCTF_TEST_CORPUS_LABS        lab folders, comma-separated, for the corpus-docker scenario
#   CYBERCTF_TEST_PVE_HOST, _PASSWORD (and _TOKEN_ID, _TOKEN_SECRET, _SSH_KEY, _FINGERPRINT, _STORAGE,
#                                    _BRIDGE, _NODE): the Proxmox host of server-lab-proxmox-connection
#   CYBERCTF_TEST_PVE_ENDPOINT, _TOKEN_ID, _TOKEN_SECRET, _NODE, _BRIDGE, _RUNNER_HOME (and _JUMP
#                                    when the lab bridge is internal to the node): server-lab-proxmox
#   CYBERCTF_TEST_ESXI_HOST, _PASSWORD (and _DATASTORE, _NETWORK): the ESXi host of server-lab-esxi
#   ONLY, SKIP, STRICT               as every suite runner (scripts/lib/suite.sh)
#
# Old names still read, while the workspace's test rig sets them (remove once it has moved):
# TIERS, LOGS, ISOLOOM, VM_PROVIDER, CYBERCTF_TEST_RUNNER_HOME.
set -uo pipefail

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
# shellcheck source=../../scripts/lib/suite.sh
. "$ROOT/scripts/lib/suite.sh"
suite_help "$0" "$@"

LISTING=""
while [ $# -gt 0 ]; do
  case $1 in
    --results) suite_results_to integration "${2:?--results needs a file}"; shift ;;
    --list) LISTING=1 ;;
    --tiers) CYBERCTF_TEST_INTEGRATION_TIERS=${2:?--tiers needs a list}; shift ;;
    --only)
      # <scenario>@<provider>: the scenario as ONLY, the provider as the hypervisor knobs (none, or
      # `docker`, names no hypervisor).
      only=${2:?--only needs a scenario}
      ONLY="${ONLY:+$ONLY }${only%%@*}"
      case $only in
        *@docker | *@) ;;
        *@*) export CYBERCTF_TEST_VM_PROVIDER=${only#*@} ;;
      esac
      export ONLY
      shift
      ;;
    *) echo "run.sh: unknown argument: $1 (see --help)" >&2; exit 2 ;;
  esac
  shift
done

# old_name <knob> <its old name>: the knob from its old name when only that one is set.
old_name() {
  if [ -z "${!1:-}" ] && [ -n "${!2:-}" ]; then
    printf -v "$1" '%s' "${!2}"
    export "${1?}"
  fi
}
old_name CYBERCTF_TEST_INTEGRATION_TIERS TIERS
old_name CYBERCTF_TEST_INTEGRATION_LOGS LOGS
old_name CYBERCTF_TEST_ISOLOOM_DIR ISOLOOM
old_name CYBERCTF_TEST_VM_PROVIDER VM_PROVIDER
old_name CYBERCTF_TEST_PVE_RUNNER_HOME CYBERCTF_TEST_RUNNER_HOME

if [ -n "$LISTING" ]; then
  # Listing runs nothing: no checkout, no logs.
  ISOLOOM="${CYBERCTF_TEST_ISOLOOM_DIR:-$ROOT/../isoloom}"
else
  ISOLOOM=$(cd "${CYBERCTF_TEST_ISOLOOM_DIR:-$ROOT/../isoloom}" 2>/dev/null && pwd) || {
    echo "no Isoloom checkout: set CYBERCTF_TEST_ISOLOOM_DIR" >&2
    exit 2
  }
fi
EXAMPLES="$ISOLOOM/examples"
TIERS=${CYBERCTF_TEST_INTEGRATION_TIERS:-local}
VM_PROVIDER=${CYBERCTF_TEST_VM_PROVIDER:-qemu}
AWS_REGION=${CYBERCTF_TEST_AWS_REGION:-eu-west-3}
LOGS=${CYBERCTF_TEST_INTEGRATION_LOGS:-$ROOT/tests/integration/.state/logs}
[ -n "$LISTING" ] || mkdir -p "$LOGS"
cd "$ROOT/src-tauri" || exit 2

# A scenario's checks (`ok:` / `FAILED:`) are its summary on screen; its log keeps the rest.
# shellcheck disable=SC2034 # read by lib/suite.sh's logged, which shellcheck doesn't follow
SUITE_SHOW='^(ok|FAILED):|panicked'

# How a test says it returned early for a knob that isn't set (integration_tests::support::SKIPPED,
# which a unit test holds to this line): cargo counts that return as a pass.
KNOB_UNSET="integration test skipped, not set:"
# How it says it returned because the launcher refuses its lab on the provider it was given
# (integration_tests::support::REFUSED): skipped, the launcher's words the reason.
REFUSED="integration test skipped, refused by the launcher:"
# How a failed check or a panic says the platform itself failed the scenario, in the launcher's
# own words (runtime/launch/local.rs's refusal, the machine check's reasons (runtime/providers.rs's said), a
# missing tool, Docker down, an architecture a hypervisor doesn't run; a unit test in
# integration_tests/support.rs holds this to them): once seen on a provider, its remaining
# scenarios are skipped with that line as the reason.
PLATFORM_FAILURE="This lab can't run on|is not supported on|is not installed|was not found|Not installed\.|isn't responding|isn't turned on|isn't available on this machine|is too old|service isn't running|drives Hyper-V only for|group since you logged in|can't use /dev/kvm|Docker isn't running"
# Seconds a scenario may take: CAP, and BOX_CAP for the attack-box ones (their images are pulled
# until the rig's goldens carry them).
CAP=${CYBERCTF_TEST_INTEGRATION_TIMEOUT:-480}
BOX_CAP=${CYBERCTF_TEST_INTEGRATION_TIMEOUT:-600}

BOX=integration_tests::attack_box
LOCAL_BOX="$BOX::the_attack_box_is_a_machine_of_the_lab_on_every_local_form"
AWS_BOX="$BOX::the_attack_box_is_a_machine_of_an_aws_lab"
HELLO_STACK=integration_tests::docker_lab::hello_stack_runs_end_to_end
SEGMENTED=integration_tests::docker_lab::segmented_runs_end_to_end
INVOICE_PORTAL=integration_tests::docker_lab::supplier_portal_runs_end_to_end
VM_LAB=integration_tests::vm_lab::a_vm_lab_runs_end_to_end_through_the_runner
PER_MACHINE=integration_tests::per_machine::a_dual_form_lab_runs_as_one_vm_per_machine_end_to_end
LAUNCH_COMMANDS=integration_tests::launch_commands::a_local_lab_launches_and_tears_down_through_the_commands
CLI_LAB=integration_tests::cli_lab::a_docker_lab_runs_end_to_end_through_the_command_line
CORPUS=integration_tests::corpus_docker::corpus_docker_labs_run_end_to_end_on_the_runner
REPLACE=integration_tests::replace::a_running_lab_is_not_replaced_under_it
PROXMOX_CONNECTION=integration_tests::server_lab::a_proxmox_host_passes_the_connection_test
PROXMOX=integration_tests::server_lab::a_vm_lab_runs_on_proxmox_through_the_runner
PROXMOX_VM_TEST=integration_tests::server_lab::the_vm_test_passes_on_proxmox
ESXI=integration_tests::server_lab::a_vm_lab_runs_on_esxi_through_the_runner
SETUP=integration_tests::setup::the_guided_setup_readies_this_machine
RES=integration_tests::resilience
RES_REMOVED="$RES::a_container_removed_while_the_lab_runs_shows_down_and_a_reset_brings_it_back"
RES_STOPPED="$RES::a_container_stopped_while_the_lab_runs_shows_down_and_a_resume_brings_it_back"
RES_REMOVED_STARTING="$RES::a_container_removed_while_the_start_waits_fails_it_or_shows_it_and_a_start_again_works"
RES_PORT="$RES::a_port_the_lab_publishes_taken_at_start_is_named_and_a_start_once_free_works"
RES_KILLED="$RES::a_start_killed_midway_is_failed_and_a_start_again_works"
RES_DAEMON="$RES::docker_stopped_while_the_lab_starts_says_so_and_a_start_once_it_is_back_works"
RES_VM=integration_tests::resilience_vm
RES_VM_DESTROYED="$RES_VM::a_vm_destroyed_while_the_lab_runs_shows_gone_and_a_start_again_rebuilds_it"
RES_VM_OFF="$RES_VM::a_vm_powered_off_while_the_lab_runs_shows_off_and_a_resume_brings_it_back"
RES_VM_KILLED="$RES_VM::a_vm_start_killed_while_it_provisions_is_failed_and_a_start_again_works"
RES_VM_HYPERVISOR="$RES_VM::a_vm_killed_by_its_hypervisor_while_it_provisions_fails_the_start_and_a_start_again_works"

# reads <test>: the knobs the test reads (a unit test, tests/integration/run.test.ts, holds this to
# the scenarios' sources).
reads() {
  case $1 in
    "$LOCAL_BOX") echo CYBERCTF_TEST_BOX_LAB CYBERCTF_TEST_BOX_FORM CYBERCTF_TEST_BOX_TARGET CYBERCTF_TEST_BOX_SHARED_FOLDER CYBERCTF_TEST_VM_PROVIDER ;;
    "$AWS_BOX") echo CYBERCTF_TEST_BOX_LAB CYBERCTF_TEST_BOX_FORM CYBERCTF_TEST_BOX_TARGET CYBERCTF_TEST_AWS_REGION CYBERCTF_TEST_CLOUD_REAL_IP ;;
    "$HELLO_STACK") echo CYBERCTF_TEST_HELLO_LAB CYBERCTF_TEST_BOX_IMAGE ;;
    "$SEGMENTED") echo CYBERCTF_TEST_SEGMENTED_LAB CYBERCTF_TEST_BOX_IMAGE ;;
    "$INVOICE_PORTAL") echo CYBERCTF_TEST_INVOICE_LAB CYBERCTF_TEST_BOX_IMAGE ;;
    "$VM_LAB") echo CYBERCTF_TEST_VM_LAB CYBERCTF_TEST_VM_PROVIDER CYBERCTF_TEST_VM_INPUTS ;;
    "$PER_MACHINE") echo CYBERCTF_TEST_PER_MACHINE_LAB CYBERCTF_TEST_VM_PROVIDER CYBERCTF_TEST_VM_INPUTS ;;
    "$LAUNCH_COMMANDS") echo CYBERCTF_TEST_LOCAL_LAB ;;
    "$CLI_LAB") echo CYBERCTF_TEST_LOCAL_LAB ;;
    "$CORPUS") echo CYBERCTF_TEST_CORPUS_LABS ;;
    "$REPLACE") echo CYBERCTF_TEST_REPLACE_LAB CYBERCTF_TEST_REPLACE_FORM CYBERCTF_TEST_VM_PROVIDER CYBERCTF_TEST_VM_INPUTS ;;
    "$PROXMOX_CONNECTION") echo CYBERCTF_TEST_PVE_HOST CYBERCTF_TEST_PVE_PASSWORD CYBERCTF_TEST_PVE_TOKEN_ID CYBERCTF_TEST_PVE_TOKEN_SECRET CYBERCTF_TEST_PVE_SSH_KEY CYBERCTF_TEST_PVE_FINGERPRINT CYBERCTF_TEST_PVE_STORAGE CYBERCTF_TEST_PVE_BRIDGE CYBERCTF_TEST_PVE_NODE ;;
    "$PROXMOX") echo CYBERCTF_TEST_PVE_LAB CYBERCTF_TEST_PVE_ENDPOINT CYBERCTF_TEST_PVE_TOKEN_ID CYBERCTF_TEST_PVE_TOKEN_SECRET CYBERCTF_TEST_PVE_NODE CYBERCTF_TEST_PVE_BRIDGE CYBERCTF_TEST_PVE_JUMP CYBERCTF_TEST_PVE_RUNNER_HOME CYBERCTF_TEST_PVE_SSH_KEY CYBERCTF_TEST_PVE_STORAGE CYBERCTF_TEST_PVE_FINGERPRINT ;;
    "$ESXI") echo CYBERCTF_TEST_ESXI_LAB CYBERCTF_TEST_ESXI_HOST CYBERCTF_TEST_ESXI_PASSWORD CYBERCTF_TEST_ESXI_DATASTORE CYBERCTF_TEST_ESXI_NETWORK ;;
    "$RES_REMOVED" | "$RES_STOPPED" | "$RES_REMOVED_STARTING") echo CYBERCTF_TEST_RESILIENCE_LAB CYBERCTF_TEST_RESILIENCE_MACHINE ;;
    "$RES_PORT") echo CYBERCTF_TEST_RESILIENCE_LAB ;;
    "$RES_KILLED") echo CYBERCTF_TEST_RESILIENCE_LAB ;;
    "$RES_DAEMON") echo CYBERCTF_TEST_RESILIENCE_LAB CYBERCTF_TEST_RESILIENCE_THROWAWAY CYBERCTF_TEST_RESILIENCE_DOCKER_STOP CYBERCTF_TEST_RESILIENCE_DOCKER_START ;;
    "$RES_VM_DESTROYED" | "$RES_VM_OFF") echo CYBERCTF_TEST_RESILIENCE_VM_LAB CYBERCTF_TEST_RESILIENCE_MACHINE CYBERCTF_TEST_VM_PROVIDER CYBERCTF_TEST_VM_INPUTS ;;
    "$RES_VM_KILLED") echo CYBERCTF_TEST_RESILIENCE_VM_LAB CYBERCTF_TEST_VM_PROVIDER CYBERCTF_TEST_VM_INPUTS ;;
    "$RES_VM_HYPERVISOR") echo CYBERCTF_TEST_RESILIENCE_VM_LAB CYBERCTF_TEST_RESILIENCE_THROWAWAY CYBERCTF_TEST_RESILIENCE_VM_KILL CYBERCTF_TEST_VM_PROVIDER CYBERCTF_TEST_VM_INPUTS ;;
    "$SETUP") echo CYBERCTF_TEST_SETUP_PATHS CYBERCTF_TEST_SETUP_HYPERVISORS CYBERCTF_TEST_SETUP_LOGS CYBERCTF_TEST_SETUP_LAB CYBERCTF_TEST_SETUP_CHECK_ONLY CYBERCTF_TEST_RESULTS ;;
  esac
}

# The tier the scenarios below belong to, and why they can't run here (empty: they can).
TIER=""
NOT_HERE=""

# in_tier <tier>: whether the scenarios of that tier are wanted (all of them, listing); sets TIER.
in_tier() {
  TIER=$1
  NOT_HERE=""
  [ -n "$LISTING" ] || listed "$1" "$TIERS"
}

# listing <id> <form> <test>: the scenario's line for --list.
listing() {
  printf '{"suite":"integration","case":%s,"tier":%s,"form":%s,"test":%s,"knobs":%s}\n' \
    "$(json_str "$1")" "$(json_str "$TIER")" "$(json_str "$2")" "$(json_str "$3")" "$(json_words "$(reads "$3")")"
}

# platform_failed <provider>: why the provider (`docker` for none) failed on the platform earlier
# in this run, empty while it hasn't (set by platform_failure).
platform_failed() {
  local var="PLATFORM_FAILED_${1:-docker}"
  printf '%s' "${!var:-}"
}
# platform_failure <provider> <log>: the line of a failed scenario's log that says the platform
# itself failed it (PLATFORM_FAILURE, among its failed checks and panics), kept for the
# provider's scenarios after it; nothing when the failure was the lab's.
platform_failure() {
  local line
  line=$(grep -E '^FAILED:|panicked' "$2" 2>/dev/null | grep -m 1 -E "$PLATFORM_FAILURE")
  [ -n "$line" ] || return 0
  printf -v "PLATFORM_FAILED_${1:-docker}" '%s' "$line"
}
# The temp folder of the scenario running (scenario_temp), and Windows' own path for it under Git
# Bash. A runner stopped midway stops that scenario (logged's SUITE_RUNNING) and drops its folder.
SCENARIO_TEMP=""
SCENARIO_TEMP_NATIVE=""
trap 'stop_running; drop_temp "$SCENARIO_TEMP" /dev/null vms' EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
# stop_running: the scenario `logged` runs, if any, ended (capped kills its process group).
stop_running() {
  [ -n "${SUITE_RUNNING:-}" ] && kill "$SUITE_RUNNING" 2>/dev/null && sleep 1
  return 0
}
# scenario_temp: a fresh temp folder for a scenario, its real path (macOS's /tmp is a link; Docker
# records the Compose files' paths), in SCENARIO_TEMP, in temp_base's folder.
scenario_temp() {
  local base
  base=$(temp_base)
  mkdir -p "$base"
  SCENARIO_TEMP=$(cd "$(mktemp -d "${base%/}/cyberctf-it.XXXXXX")" && pwd -P)
  SCENARIO_TEMP_NATIVE=$SCENARIO_TEMP
  if command -v cygpath >/dev/null 2>&1; then SCENARIO_TEMP_NATIVE=$(cygpath -w "$SCENARIO_TEMP"); fi
}
# temp_base: where the scenarios' temp folders go. They hold the labs' runs, their VMs' disks
# among them, so on disk (#317): CYBERCTF_TEST_INTEGRATION_TEMP when set; else TMPDIR when that
# is an absolute path (the test rig's Linux clones set one on their disk) but macOS's own
# (/var/folders/…, too long for the Unix sockets made there: 104 bytes); else /var/tmp on Linux,
# where /tmp is a tmpfs of half the RAM on Debian 13 and Fedora and /var/tmp is on disk, and short
# enough for a runner's socket inside it (108 bytes); else /tmp.
temp_base() {
  if [ -n "${CYBERCTF_TEST_INTEGRATION_TEMP:-}" ]; then echo "$CYBERCTF_TEST_INTEGRATION_TEMP"; return; fi
  case ${TMPDIR:-} in
    /var/folders/* | /private/var/folders/* | "" | [!/]*) ;;
    *) echo "$TMPDIR"; return ;;
  esac
  if [ "$(uname -s)" = Linux ]; then echo /var/tmp; else echo /tmp; fi
}
# drop_temp <folder> <log> [vms]: a scenario's temp folder gone, what it left with it: with `vms`
# (a scenario past its cap, a runner stopped midway) its labs' VMs first (drop_vms), then
# a process still naming it (a VM, a QEMU hub: daemonized, they outlive a killed test's process
# group) is stopped (TERM, then KILL), then the folder removed; the log says so.
drop_temp() {
  local dir=$1 log=$2 pids
  [ -n "$dir" ] && [ -d "$dir" ] || return 0
  [ "${3:-}" != vms ] || drop_vms "$dir" "$log"
  # By the folder's own name (random), whichever path to it a process was given (/tmp or /private/tmp).
  pids=$(pgrep -f -- "${dir##*/}" 2>/dev/null | tr '\n' ' ')
  if [ -n "${pids// /}" ]; then
    echo "suite: stopping what still runs from the scenario's temp folder: $pids" >>"$log"
    # shellcheck disable=SC2086 # one pid a word
    kill $pids 2>/dev/null
    sleep 2
    # shellcheck disable=SC2086
    kill -9 $pids 2>/dev/null
  fi
  rm -rf "$dir"
  [ -d "$dir" ] && echo "suite: couldn't remove all of the scenario's temp folder $dir" >>"$log"
  [ "$dir" = "$SCENARIO_TEMP" ] && SCENARIO_TEMP=""
  return 0
}
# tool_path <tool>: where the hypervisor or engine tool is: on PATH, else where its installer puts
# it, as the launcher looks (platform::env_path): a scenario's cleanup runs with the PATH the runner
# was started with, which on macOS lacks /usr/local/bin outside a login shell (VBoxManage, prlctl),
# and utmctl ships inside UTM's app, on no PATH. Fails when it's in none. With TOOL_ROOT (a test's),
# the known places alone, under it.
tool_path() {
  local tool=$1 place file
  [ -n "${TOOL_ROOT:-}" ] || ! command -v "$tool" 2>/dev/null || return 0
  for place in /usr/local/bin /opt/homebrew/bin /usr/bin "$HOME/.docker/bin" \
    /Applications/UTM.app/Contents/MacOS "$HOME/Applications/UTM.app/Contents/MacOS" \
    /Applications/VirtualBox.app/Contents/MacOS "/Applications/Parallels Desktop.app/Contents/MacOS" \
    "/Applications/VMware Fusion.app/Contents/Library" /Applications/Docker.app/Contents/Resources/bin \
    "/c/Program Files/Oracle/VirtualBox" "/c/Program Files (x86)/VMware/VMware Workstation" \
    "/c/Program Files/VMware/VMware Workstation" "/c/Program Files/Docker/Docker/resources/bin"; do
    for file in "${TOOL_ROOT:-}$place/$tool" "${TOOL_ROOT:-}$place/$tool.exe"; do
      [ -x "$file" ] && { echo "$file"; return 0; }
    done
  done
  return 1
}
# drop_vms <folder> <log>: the VMs Vagrant made for the folder's labs, each still defined stopped
# and deleted with its own hypervisor's tool (tool_path), the log saying so, then the folder's
# Compose projects (drop_compose). Vagrant keeps each machine's VM by its
# id in <state>/machines/<machine>/<provider>/id (the state folder a run's `vagrant` under its runner
# home's runs/<lab>, or a Vagrantfile's .vagrant), and the VMs live in the hypervisor's own place,
# outside the folder, so stopping what names the folder never reached them: a capped attack-box-vm
# left its VirtualBox VM running (2026-11-12), and on UTM its three VMs defined in UTM's library,
# two of them running (2026-11-14). Read before the folder goes.
drop_vms() {
  local dir=$1 log=$2 file id provider hypervisor stop del
  while IFS= read -r file; do
    id=$(tr -d '\r\n' <"$file")
    provider=${file%/id}
    provider=${provider##*/}
    [ -n "$id" ] || continue
    case $provider in
      virtualbox) drop_vbox "$id" "$log"; continue ;;
      libvirt) drop_libvirt "$id" "$log"; continue ;;
      utm) hypervisor=UTM stop=(utmctl stop --kill "$id") del=(utmctl delete "$id") ;;
      vmware_desktop) hypervisor=VMware stop=(vmrun stop "$id" hard) del=(vmrun deleteVM "$id") ;;
      parallels) hypervisor=Parallels stop=(prlctl stop "$id" --kill) del=(prlctl delete "$id") ;;
      hyperv)
        hypervisor=Hyper-V
        stop=(powershell -NoProfile -Command "Get-VM -Id '$id' | Stop-VM -TurnOff -Force")
        del=(powershell -NoProfile -Command "Get-VM -Id '$id' | Remove-VM -Force")
        ;;
      *) continue ;;
    esac
    stop[0]=$(tool_path "${stop[0]}") || continue
    del[0]=${stop[0]}
    echo "suite: stopping the scenario's $hypervisor VM $id: stopped and deleted" >>"$log"
    "${stop[@]}" >>"$log" 2>&1
    sleep 2
    "${del[@]}" >>"$log" 2>&1 || echo "suite: couldn't delete the $hypervisor VM $id" >>"$log"
  done < <(find "$dir" -maxdepth 10 -path '*/machines/*/*/id' -type f 2>/dev/null)
  drop_compose "$dir" "$log"
  return 0
}
# drop_compose <folder> <log>: the Compose projects the folder's labs started, each taken down with
# its volumes and orphans (`docker compose -p <project> down -v --remove-orphans`), then any volume
# still labelled with it removed, the log saying so. A project is the folder's when its containers'
# working folder (Compose's com.docker.compose.project.working_dir label) is inside it: a capped
# attack-box-docker left its project's three containers, network and volume (2026-11-14).
drop_compose() {
  local dir=$1 log=$2 project docker
  docker=$(tool_path docker) || return 0
  while IFS= read -r project; do
    [ -n "$project" ] || continue
    echo "suite: taking down the scenario's Compose project $project: containers, networks and volumes" >>"$log"
    "$docker" compose -p "$project" down -v --remove-orphans >>"$log" 2>&1 || echo "suite: couldn't take down the Compose project $project" >>"$log"
    "$docker" volume ls -q --filter "label=com.docker.compose.project=$project" 2>/dev/null | while IFS= read -r volume; do
      [ -z "$volume" ] || "$docker" volume rm -f "$volume" >>"$log" 2>&1
    done
  done < <("$docker" ps -a --filter label=com.docker.compose.project \
    --format '{{.Label "com.docker.compose.project"}}\t{{.Label "com.docker.compose.project.working_dir"}}' 2>/dev/null |
    awk -F '\t' -v dir="$dir" -v alt="/private$dir" '(index($2, dir "/") == 1 || $2 == dir || index($2, alt "/") == 1) && !seen[$1]++ { print $1 }')
  return 0
}
# drop_libvirt <id> <log>: the libvirt domain `id` (vagrant-libvirt's UUID) destroyed and undefined
# with its storage, from the user's session and the system instance (vagrant-libvirt's default).
drop_libvirt() {
  local id=$1 log=$2 uri virsh
  virsh=$(tool_path virsh) || return 0
  for uri in "" qemu:///system; do
    "$virsh" ${uri:+-c "$uri"} dominfo "$id" >/dev/null 2>&1 || continue
    echo "suite: stopping the scenario's libvirt domain $id${uri:+ ($uri)}: destroyed and undefined" >>"$log"
    "$virsh" ${uri:+-c "$uri"} destroy "$id" >>"$log" 2>&1
    "$virsh" ${uri:+-c "$uri"} undefine "$id" --remove-all-storage >>"$log" 2>&1 || echo "suite: couldn't undefine the libvirt domain $id" >>"$log"
  done
}
# drop_vbox <id> <log>: the VirtualBox VM `id`, still registered, powered off and deleted
# (`VBoxManage controlvm poweroff`, `unregistervm --delete`); VBoxHeadless names a UUID alone.
drop_vbox() {
  local id=$1 log=$2 name vbox
  vbox=$(tool_path VBoxManage) || return 0
  [[ $id =~ ^[0-9a-fA-F-]{36}$ ]] || return 0
  name=$("$vbox" showvminfo "$id" --machinereadable 2>/dev/null | sed -n 's/^name="\(.*\)"$/\1/p')
  [ -n "$name" ] || return 0
  echo "suite: stopping the scenario's VirtualBox VM $name ($id): powered off and deleted" >>"$log"
  "$vbox" controlvm "$id" poweroff >>"$log" 2>&1
  sleep 2
  "$vbox" unregistervm "$id" --delete >>"$log" 2>&1 || echo "suite: couldn't delete the VirtualBox VM $name ($id)" >>"$log"
}
# capped <command...>: the command in a process group of its own, killed whole (cargo and the test
# binary, which holds the VMs) when `logged` ends it past its cap.
capped() {
  local pid
  set -m
  "$@" &
  pid=$!
  set +m
  trap 'kill -- -'"$pid"' 2>/dev/null' TERM
  wait "$pid"
}
# cap_of <scenario> <provider>: its cap in seconds: CYBERCTF_TEST_INTEGRATION_TIMEOUT when set, else
# its own where it was measured slower than CAP allows (its measured time and about half again, the
# measure named), else BOX_CAP for the attack-box ones, else CAP. Raised one scenario on one
# provider at a time, never for all: a cap is how a hang shows.
cap_of() {
  [ -z "${CYBERCTF_TEST_INTEGRATION_TIMEOUT:-}" ] || { echo "$CYBERCTF_TEST_INTEGRATION_TIMEOUT"; return; }
  case $1@$2 in
    # per-machine passed in 441 s on QEMU, arm64 Mac (2026-11-14, runner 47b9852). UTM on the same
    # Mac has no complete run yet: QEMU's figure until it has.
    per-machine@qemu | per-machine@utm) echo 660 ;;
    # vm-lab passed end to end in 669 s on libvirt, x64 Debian (2026-11-14, runner 40efa9b, with a
    # 1200 s cap); on QEMU, arm64 Mac, it passed every check through re-provision and was cut at
    # 480 s before its stop (runner 47b9852). UTM has no complete run yet (its checks hung, F17).
    vm-lab@libvirt | vm-lab@qemu | vm-lab@utm) echo 1000 ;;
    attack-box-*) echo "$BOX_CAP" ;;
    # The VM test on the nested Proxmox test server (2026-11-17): its build, the image, the VM up
    # with its address and SSH took past 480 s under load, cut before its command and teardown.
    server-vm-test-proxmox@proxmox) echo 1200 ;;
    *) echo "$CAP" ;;
  esac
}
# scenario <id> <form> <provider> <test> [VAR=value...]: the opt-in test, alone, with its knobs,
# capped. It passes when the test ran and passed; a test that returned early for a missing knob,
# or because the launcher refuses its lab on that provider (a VM on a network, on UTM), is
# skipped, and so is every scenario while NOT_HERE says why the tier can't run, or once a scenario
# on the same provider failed on the platform itself (platform_failure). The form and the
# provider (empty where none applies) go to its record.
scenario() {
  local name=$1 form=$2 provider=$3 test=$4 start=$SECONDS log unset refused cap earlier passed
  shift 4
  if [ -n "$LISTING" ]; then
    listing "$name" "$form" "$test"
    return 0
  fi
  if [ -n "$NOT_HERE" ]; then
    skip "$name" "$NOT_HERE" "$form" "$provider"
    return 0
  fi
  earlier=$(platform_failed "$provider")
  if [ -n "$earlier" ]; then
    skip "$name" "${provider:-docker} failed on the platform earlier in this run: $earlier" "$form" "$provider"
    return 0
  fi
  wanted "$name" || return 0
  log="$LOGS/$name.log"
  cap=$(cap_of "$name" "$provider")
  # Its own temp folder (TMPDIR for Unix's tools, TMP and TEMP for Windows'), its app's folders
  # there too (CYBERCTF_TEST_APP_ROOT, read by the test's TestApp::scenario: a lab's runs, Vagrant's
  # state and a VM's disks, go with the folder, where the system's place kept a capped scenario's
  # VMs running, #308), but the guided setup's, kept from pass to pass in the system's.
  local temp=()
  if [ "$test" != "$SETUP" ]; then
    scenario_temp
    temp=("TMPDIR=$SCENARIO_TEMP" "TMP=$SCENARIO_TEMP_NATIVE" "TEMP=$SCENARIO_TEMP_NATIVE" "CYBERCTF_TEST_APP_ROOT=$SCENARIO_TEMP_NATIVE")
  fi
  logged "$name" "$log" "$cap" capped env ${temp[@]+"${temp[@]}"} "$@" cargo test -q --lib -- --ignored --nocapture --test-threads=1 --exact "$test"
  passed=$?
  if [ -n "$SCENARIO_TEMP" ]; then
    echo "suite: the scenario's temp folder was $SCENARIO_TEMP" >>"$log"
    # Past its cap, its lab's VMs go with the folder (a test that ended on its own
    # cleaned up itself, or the leftover audit says what it left).
    if grep -q "^suite: $name exceeded its" "$log"; then drop_temp "$SCENARIO_TEMP" "$log" vms; else drop_temp "$SCENARIO_TEMP" "$log"; fi
  fi
  if [ "$passed" -eq 0 ] && grep -q "test result: ok. 1 passed" "$log"; then
    unset=$(grep -F "$KNOB_UNSET" "$log" | head -1)
    refused=$(grep -F "$REFUSED" "$log" | head -1)
    if [ -n "$unset" ]; then
      record "$name" skip $((SECONDS - start)) "${unset#"$KNOB_UNSET" } isn't set" "$form" "$provider" "$log"
    elif [ -n "$refused" ]; then
      record "$name" skip $((SECONDS - start)) "${refused#"$REFUSED" }" "$form" "$provider" "$log"
    else
      record "$name" ok $((SECONDS - start)) "" "$form" "$provider" "$log"
    fi
  else
    record "$name" FAIL $((SECONDS - start)) "$(failure_of "$name" "$log")" "$form" "$provider" "$log"
    platform_failure "$provider" "$log"
  fi
}
# failure_of <name> <log>: what a failed scenario's log says first (a failed check, a panic),
# its cap when it ran past it, else the log.
failure_of() {
  local first over
  first=$(grep -m 1 -E '^FAILED:|panicked' "$2" 2>/dev/null)
  over=$(grep -m 1 "^suite: $1 exceeded its" "$2" 2>/dev/null)
  if [ -n "$over" ]; then
    echo "${over#suite: "$1" }${first:+ ($first)}"
  else
    echo "${first:-see $2}"
  fi
}
# unset_of <VAR...>: those of the settings named that aren't set, space-separated.
unset_of() {
  local v out=""
  for v in "$@"; do [ -n "${!v:-}" ] || out="$out $v"; done
  echo "${out# }"
}
# shared_for <provider> <scenario>: the shared folder a VM box gets on that hypervisor here, empty
# where it can't share one. The same providers as the launcher's `shares_folders`
# (src-tauri/src/runtime/attack_box/place.rs), which a unit test there holds to this list.
# QEMU only on Linux (virtiofsd); Hyper-V and any other hypervisor: none.
shared_for() {
  case "$1" in
    virtualbox | vmware_desktop | parallels | utm | libvirt) echo "$LOGS/shared/$2" ;;
    qemu) [ "$(uname -s)" = Linux ] && echo "$LOGS/shared/$2" ;;
    *) ;;
  esac
}

SOLO="$EXAMPLES/solo-web"

if in_tier local; then
  # The attack box, a machine of the lab, on each local form: added, reached from, the shared
  # folder seen both ways from inside it (where the form can share one), taken out.
  scenario attack-box-docker docker "" "$LOCAL_BOX" CYBERCTF_TEST_BOX_LAB="$SOLO" CYBERCTF_TEST_BOX_FORM=docker \
    CYBERCTF_TEST_BOX_SHARED_FOLDER="$LOGS/shared/attack-box-docker"
  # The VM box bare: its set-up skips Kali's toolset (the owner's call, 2026-11-15: no baked box
  # image), which alone took the scenario past its cap; its account, folder, network and shell run.
  scenario attack-box-vm vm "$VM_PROVIDER" "$LOCAL_BOX" CYBERCTF_TEST_BOX_LAB="$SOLO" CYBERCTF_TEST_BOX_FORM=vm \
    CYBERCTF_TEST_VM_PROVIDER="$VM_PROVIDER" CYBERCTF_TEST_BOX_SHARED_FOLDER="$(shared_for "$VM_PROVIDER" attack-box-vm)" \
    CYBERCTF_TEST_BOX_SKIP_TOOLS=1
  # A lab's whole life on each local form: up, status, park and resume, checks, down, nothing left.
  scenario docker-lab-hello-stack docker "" "$HELLO_STACK" CYBERCTF_TEST_HELLO_LAB="$EXAMPLES/hello-stack"
  scenario docker-lab-segmented docker "" "$SEGMENTED" CYBERCTF_TEST_SEGMENTED_LAB="$EXAMPLES/segmented"
  # The catalogue's invoice-portal-api lab, from a checkout CYBERCTF_TEST_INVOICE_LAB names.
  scenario docker-lab-invoice-portal docker "" "$INVOICE_PORTAL"
  scenario vm-lab vm "$VM_PROVIDER" "$VM_LAB" CYBERCTF_TEST_VM_LAB="$SOLO" CYBERCTF_TEST_VM_PROVIDER="$VM_PROVIDER"
  scenario per-machine per-machine "$VM_PROVIDER" "$PER_MACHINE" \
    CYBERCTF_TEST_PER_MACHINE_LAB="$EXAMPLES/supplier-portal-api" CYBERCTF_TEST_VM_PROVIDER="$VM_PROVIDER" \
    CYBERCTF_TEST_VM_INPUTS=API_URL=http://localhost:3206,LAUNCH_TOKEN=test-token
  scenario launch-commands docker "" "$LAUNCH_COMMANDS" CYBERCTF_TEST_LOCAL_LAB="$EXAMPLES/hello-stack"
  # The command line (`cyberctf labs install`, `status`, `stop`) on a real Docker lab.
  scenario cli-lab-docker docker "" "$CLI_LAB" CYBERCTF_TEST_LOCAL_LAB="$EXAMPLES/hello-stack"
  # Representative CTF labs of one's own (private, so named by folder only), each up to down.
  scenario corpus-docker docker "" "$CORPUS"
  # A running lab's folder is never replaced under it: an update and a re-import are refused
  # while it is up on each local form, and go through once it is stopped.
  scenario replace-docker docker "" "$REPLACE" CYBERCTF_TEST_REPLACE_LAB="$EXAMPLES/hello-stack" CYBERCTF_TEST_REPLACE_FORM=docker
  scenario replace-vm vm "$VM_PROVIDER" "$REPLACE" CYBERCTF_TEST_REPLACE_LAB="$SOLO" CYBERCTF_TEST_REPLACE_FORM=vm \
    CYBERCTF_TEST_VM_PROVIDER="$VM_PROVIDER"
  # What a player does that breaks a lab (docs/contributing/testing/resilience.md): a container or VM removed, stopped
  # or powered off outside the app while the lab runs or starts, the start's process killed, a
  # port the lab publishes taken. Each checks what the launcher says, the way back, and that
  # nothing is left. They act on their own lab alone, so they run anywhere.
  HELLO="$EXAMPLES/hello-stack"
  scenario resilience-docker-removed docker "" "$RES_REMOVED" CYBERCTF_TEST_RESILIENCE_LAB="$HELLO"
  scenario resilience-docker-stopped docker "" "$RES_STOPPED" CYBERCTF_TEST_RESILIENCE_LAB="$HELLO"
  scenario resilience-docker-removed-starting docker "" "$RES_REMOVED_STARTING" CYBERCTF_TEST_RESILIENCE_LAB="$HELLO"
  scenario resilience-docker-port-taken docker "" "$RES_PORT" CYBERCTF_TEST_RESILIENCE_LAB="$HELLO"
  scenario resilience-docker-killed-start docker "" "$RES_KILLED" CYBERCTF_TEST_RESILIENCE_LAB="$HELLO"
  scenario resilience-vm-destroyed vm "$VM_PROVIDER" "$RES_VM_DESTROYED" CYBERCTF_TEST_RESILIENCE_VM_LAB="$SOLO" CYBERCTF_TEST_VM_PROVIDER="$VM_PROVIDER"
  scenario resilience-vm-powered-off vm "$VM_PROVIDER" "$RES_VM_OFF" CYBERCTF_TEST_RESILIENCE_VM_LAB="$SOLO" CYBERCTF_TEST_VM_PROVIDER="$VM_PROVIDER"
  scenario resilience-vm-killed-start vm "$VM_PROVIDER" "$RES_VM_KILLED" CYBERCTF_TEST_RESILIENCE_VM_LAB="$SOLO" CYBERCTF_TEST_VM_PROVIDER="$VM_PROVIDER"
  # These stop more than their own lab (Docker itself, a VM by its hypervisor's own tool): each
  # skips unless CYBERCTF_TEST_RESILIENCE_THROWAWAY=1, which only the test rig's throwaway
  # machines set, with the commands for their OS (tests/README.md, "Knobs").
  scenario resilience-docker-daemon-stopped docker "" "$RES_DAEMON" CYBERCTF_TEST_RESILIENCE_LAB="$HELLO"
  scenario resilience-vm-killed-provisioning vm "$VM_PROVIDER" "$RES_VM_HYPERVISOR" CYBERCTF_TEST_RESILIENCE_VM_LAB="$SOLO" \
    CYBERCTF_TEST_VM_PROVIDER="$VM_PROVIDER"
fi

if in_tier aws; then
  # Signed in first: a run whose sign-in runs out halfway leaves the lab behind.
  [ -n "$LISTING" ] || aws sts get-caller-identity >/dev/null 2>&1 || NOT_HERE="the AWS CLI isn't signed in (aws login)"
  # Real tests ask for this machine's real public IP: the lab's firewall opens to it alone.
  scenario attack-box-aws-docker cloud-docker aws "$AWS_BOX" CYBERCTF_TEST_BOX_LAB="$SOLO" CYBERCTF_TEST_BOX_FORM=cloud-docker \
    CYBERCTF_TEST_AWS_REGION="$AWS_REGION" CYBERCTF_TEST_CLOUD_REAL_IP=1
  # Bare, as attack-box-vm (its toolset left out).
  scenario attack-box-aws-vm cloud-vm aws "$AWS_BOX" CYBERCTF_TEST_BOX_LAB="$SOLO" CYBERCTF_TEST_BOX_FORM=cloud-vm \
    CYBERCTF_TEST_AWS_REGION="$AWS_REGION" CYBERCTF_TEST_CLOUD_REAL_IP=1 \
    CYBERCTF_TEST_BOX_SKIP_TOOLS=1
fi

if in_tier server; then
  # A server's connection test, then a lab on it through the runner, as the launcher drives them;
  # each host by its own knobs. Every setting a test needs checked first, so a missing one is a
  # skip that names it, not a panic recorded as FAIL.
  needs=$(unset_of CYBERCTF_TEST_PVE_HOST CYBERCTF_TEST_PVE_PASSWORD)
  [ -z "$needs" ] || NOT_HERE="not set: $needs"
  scenario server-lab-proxmox-connection "" proxmox "$PROXMOX_CONNECTION"
  needs=$(unset_of CYBERCTF_TEST_PVE_ENDPOINT CYBERCTF_TEST_PVE_TOKEN_ID CYBERCTF_TEST_PVE_TOKEN_SECRET CYBERCTF_TEST_PVE_NODE CYBERCTF_TEST_PVE_BRIDGE CYBERCTF_TEST_PVE_RUNNER_HOME CYBERCTF_TEST_PVE_SSH_KEY)
  NOT_HERE=""
  [ -z "$needs" ] || NOT_HERE="not set: $needs"
  scenario server-lab-proxmox vm proxmox "$PROXMOX" CYBERCTF_TEST_PVE_LAB="$SOLO"
  # The Servers page's VM test, on the same knobs (no lab).
  scenario server-vm-test-proxmox "" proxmox "$PROXMOX_VM_TEST"
  needs=$(unset_of CYBERCTF_TEST_ESXI_HOST CYBERCTF_TEST_ESXI_PASSWORD)
  NOT_HERE=""
  [ -z "$needs" ] || NOT_HERE="not set: $needs"
  scenario server-lab-esxi vm esxi "$ESXI" CYBERCTF_TEST_ESXI_LAB="$SOLO"
fi

if [ -n "$LISTING" ]; then
  # Not run here: it installs real tools on the machine as an administrator. The test rig runs it
  # on a bare guest, or anywhere with CYBERCTF_TEST_SETUP_CHECK_ONLY=1, which only detects and
  # plans (docs/contributing/testing/integration.md, "Bare-machine tier"); listed for its knobs.
  TIER=bare-machine
  listing setup "" "$SETUP"
  exit 0
fi

suite_end "integration tests ($TIERS), logs in $LOGS"
