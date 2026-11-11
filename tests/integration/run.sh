#!/usr/bin/env bash
# The integration tests: real labs, end to end, on the real tools. The regression suite for what
# only real Docker, Vagrant, hypervisors and clouds show. Each scenario is an opt-in Rust test
# (`#[ignore]`) run with the knobs it needs; one verdict per scenario, and a failure doesn't stop
# the run. docs/testing/integration.md says what each scenario guards against; tests/README.md
# lists every knob and the results' format.
#
#   tests/integration/run.sh                                            the local tier: Docker, docker-vm, VMs here
#   CYBERCTF_TEST_INTEGRATION_TIERS="local aws" tests/integration/run.sh   and the AWS tier (billed while it runs)
#   CYBERCTF_TEST_INTEGRATION_TIERS=server tests/integration/run.sh        a lab on a Proxmox / ESXi host (infra/test-servers)
#   ONLY="attack-box-docker attack-box-vm" tests/integration/run.sh
#   tests/integration/run.sh --only vm-lab@virtualbox                   one scenario on one hypervisor (the rig's shard)
#   tests/integration/run.sh --results results.jsonl                    and a JSON record per scenario there
#   tests/integration/run.sh --list                                     the scenarios and the knobs each reads
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
#                       CYBERCTF_TEST_VM_PROVIDER, and the docker-vm one); may repeat
#   --results <file>    append a JSON record per scenario to <file> (tests/README.md, "Results")
#   --list              print a JSON line per scenario (its id, tier, form, test and knobs) and exit
#   -h, --help          this
#
# Knobs:
#   CYBERCTF_TEST_INTEGRATION_TIERS  local, aws, server (default: local)
#   CYBERCTF_TEST_INTEGRATION_LOGS   where each scenario's log goes (default: tests/integration/.state/logs)
#   CYBERCTF_TEST_INTEGRATION_TIMEOUT seconds a scenario may take (480; the attack-box ones 600)
#   CYBERCTF_TEST_ISOLOOM_DIR        an Isoloom checkout, for its example labs (default: ../isoloom)
#   CYBERCTF_TEST_VM_PROVIDER        the hypervisor for VM labs (default: qemu)
#   CYBERCTF_TEST_DOCKER_VM_PROVIDER the one for a container lab in a VM (default: utm on macOS, else
#                                    CYBERCTF_TEST_VM_PROVIDER)
#   CYBERCTF_TEST_AWS_REGION         the AWS tier's region (default: eu-west-3)
#   CYBERCTF_TEST_CORPUS_LABS        lab folders, comma-separated, for the corpus-docker scenario
#   CYBERCTF_TEST_PVE_ENDPOINT, _TOKEN, _NODE, _BRIDGE, _RUNNER_HOME (and _JUMP when the lab bridge
#                                    is internal to the node): the Proxmox host of server-lab-proxmox
#   CYBERCTF_TEST_ESXI_HOST, _PASSWORD (and _DATASTORE, _NETWORK): the ESXi host of server-lab-esxi
#                                    (infra/test-servers/README.md makes either host)
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
    --only)
      # <scenario>@<provider>: the scenario as ONLY, the provider as the hypervisor knobs (none, or
      # `docker`, names no hypervisor).
      only=${2:?--only needs a scenario}
      ONLY="${ONLY:+$ONLY }${only%%@*}"
      case $only in
        *@docker | *@) ;;
        *@*) export CYBERCTF_TEST_VM_PROVIDER=${only#*@} CYBERCTF_TEST_DOCKER_VM_PROVIDER=${only#*@} ;;
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
if [ "$(uname -s)" = Darwin ]; then DOCKER_VM_PROVIDER=${CYBERCTF_TEST_DOCKER_VM_PROVIDER:-utm}; else DOCKER_VM_PROVIDER=${CYBERCTF_TEST_DOCKER_VM_PROVIDER:-$VM_PROVIDER}; fi
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
# own words (runtime/launch/local.rs's refusal, runtime/providers/unavailable.rs's reasons, a
# missing tool, Docker down, an architecture a hypervisor doesn't run; a unit test in
# integration_tests/support.rs holds this to them): once seen on a provider, its remaining
# scenarios are skipped with that line as the reason.
PLATFORM_FAILURE="This lab can't run on|is not supported on|is not installed|was not found|Not installed\.|isn't responding|isn't turned on|isn't available on this machine|is too old|service isn't running|drives Hyper-V only for|added to the libvirt group|can't use /dev/kvm|Docker isn't running"
# Seconds a scenario may take: CAP, and BOX_CAP for the attack-box ones (their images are pulled
# until the rig's goldens carry them).
CAP=${CYBERCTF_TEST_INTEGRATION_TIMEOUT:-480}
BOX_CAP=${CYBERCTF_TEST_INTEGRATION_TIMEOUT:-600}

BOX=integration_tests::attack_box
LOCAL_BOX="$BOX::the_attack_box_is_a_machine_of_the_lab_on_every_local_form"
AWS_BOX="$BOX::the_attack_box_is_a_machine_of_an_aws_lab"
HELLO_STACK=integration_tests::docker_lab::hello_stack_runs_end_to_end
SEGMENTED=integration_tests::docker_lab::segmented_runs_end_to_end
DOCKER_VM_LAB=integration_tests::docker_vm_lab::a_container_lab_runs_in_a_local_vm_end_to_end
VM_LAB=integration_tests::vm_lab::a_vm_lab_runs_end_to_end_through_the_runner
PER_MACHINE=integration_tests::per_machine::a_dual_form_lab_runs_as_one_vm_per_machine_end_to_end
LAUNCH_COMMANDS=integration_tests::launch_commands::a_local_lab_launches_and_tears_down_through_the_commands
CORPUS=integration_tests::corpus_docker::corpus_docker_labs_run_end_to_end_on_the_runner
REPLACE=integration_tests::replace::a_running_lab_is_not_replaced_under_it
PROXMOX=integration_tests::server_lab::a_vm_lab_runs_on_proxmox_through_the_runner
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
    "$DOCKER_VM_LAB") echo CYBERCTF_TEST_DOCKER_VM_LAB CYBERCTF_TEST_VM_PROVIDER ;;
    "$VM_LAB") echo CYBERCTF_TEST_VM_LAB CYBERCTF_TEST_VM_PROVIDER CYBERCTF_TEST_VM_INPUTS ;;
    "$PER_MACHINE") echo CYBERCTF_TEST_PER_MACHINE_LAB CYBERCTF_TEST_VM_PROVIDER CYBERCTF_TEST_VM_INPUTS ;;
    "$LAUNCH_COMMANDS") echo CYBERCTF_TEST_LOCAL_LAB ;;
    "$CORPUS") echo CYBERCTF_TEST_CORPUS_LABS ;;
    "$REPLACE") echo CYBERCTF_TEST_REPLACE_LAB CYBERCTF_TEST_REPLACE_FORM CYBERCTF_TEST_VM_PROVIDER CYBERCTF_TEST_VM_INPUTS ;;
    "$PROXMOX") echo CYBERCTF_TEST_PVE_LAB CYBERCTF_TEST_PVE_ENDPOINT CYBERCTF_TEST_PVE_TOKEN CYBERCTF_TEST_PVE_NODE CYBERCTF_TEST_PVE_BRIDGE CYBERCTF_TEST_PVE_JUMP CYBERCTF_TEST_PVE_RUNNER_HOME ;;
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
# scenario <id> <form> <provider> <test> [VAR=value...]: the opt-in test, alone, with its knobs,
# capped. It passes when the test ran and passed; a test that returned early for a missing knob,
# or because the launcher refuses its lab on that provider (a VM on a network, on UTM), is
# skipped, and so is every scenario while NOT_HERE says why the tier can't run, or once a scenario
# on the same provider failed on the platform itself (platform_failure). The form and the
# provider (empty where none applies) go to its record.
scenario() {
  local name=$1 form=$2 provider=$3 test=$4 start=$SECONDS log unset refused cap earlier
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
  case $name in attack-box-*) cap=$BOX_CAP ;; *) cap=$CAP ;; esac
  if logged "$name" "$log" "$cap" capped env "$@" cargo test -q --lib -- --ignored --nocapture --test-threads=1 --exact "$test" &&
    grep -q "test result: ok. 1 passed" "$log"; then
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
# (src-tauri/src/runtime/attack_machine/place.rs), which a unit test there holds to this list.
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
  scenario attack-box-docker-vm docker-vm "$DOCKER_VM_PROVIDER" "$LOCAL_BOX" CYBERCTF_TEST_BOX_LAB="$SOLO" CYBERCTF_TEST_BOX_FORM=docker-vm \
    CYBERCTF_TEST_VM_PROVIDER="$DOCKER_VM_PROVIDER" CYBERCTF_TEST_BOX_SHARED_FOLDER="$(shared_for "$DOCKER_VM_PROVIDER" attack-box-docker-vm)"
  scenario attack-box-vm vm "$VM_PROVIDER" "$LOCAL_BOX" CYBERCTF_TEST_BOX_LAB="$SOLO" CYBERCTF_TEST_BOX_FORM=vm \
    CYBERCTF_TEST_VM_PROVIDER="$VM_PROVIDER" CYBERCTF_TEST_BOX_SHARED_FOLDER="$(shared_for "$VM_PROVIDER" attack-box-vm)"
  # A lab's whole life on each local form: up, status, park and resume, checks, down, nothing left.
  scenario docker-lab-hello-stack docker "" "$HELLO_STACK" CYBERCTF_TEST_HELLO_LAB="$EXAMPLES/hello-stack"
  scenario docker-lab-segmented docker "" "$SEGMENTED" CYBERCTF_TEST_SEGMENTED_LAB="$EXAMPLES/segmented"
  scenario docker-vm-lab docker-vm "$DOCKER_VM_PROVIDER" "$DOCKER_VM_LAB" \
    CYBERCTF_TEST_DOCKER_VM_LAB="$SOLO" CYBERCTF_TEST_VM_PROVIDER="$DOCKER_VM_PROVIDER"
  scenario vm-lab vm "$VM_PROVIDER" "$VM_LAB" CYBERCTF_TEST_VM_LAB="$SOLO" CYBERCTF_TEST_VM_PROVIDER="$VM_PROVIDER"
  scenario per-machine per-machine "$VM_PROVIDER" "$PER_MACHINE" \
    CYBERCTF_TEST_PER_MACHINE_LAB="$EXAMPLES/supplier-portal-api" CYBERCTF_TEST_VM_PROVIDER="$VM_PROVIDER" \
    CYBERCTF_TEST_VM_INPUTS=API_URL=http://localhost:3206,LAUNCH_TOKEN=test-token
  scenario launch-commands docker "" "$LAUNCH_COMMANDS" CYBERCTF_TEST_LOCAL_LAB="$EXAMPLES/hello-stack"
  # Representative CTF labs of one's own (private, so named by folder only), each up to down.
  scenario corpus-docker docker "" "$CORPUS"
  # A running lab's folder is never replaced under it: an update and a re-import are refused
  # while it is up on each local form, and go through once it is stopped.
  scenario replace-docker docker "" "$REPLACE" CYBERCTF_TEST_REPLACE_LAB="$EXAMPLES/hello-stack" CYBERCTF_TEST_REPLACE_FORM=docker
  scenario replace-docker-vm docker-vm "$DOCKER_VM_PROVIDER" "$REPLACE" CYBERCTF_TEST_REPLACE_LAB="$SOLO" CYBERCTF_TEST_REPLACE_FORM=docker-vm \
    CYBERCTF_TEST_VM_PROVIDER="$DOCKER_VM_PROVIDER"
  scenario replace-vm vm "$VM_PROVIDER" "$REPLACE" CYBERCTF_TEST_REPLACE_LAB="$SOLO" CYBERCTF_TEST_REPLACE_FORM=vm \
    CYBERCTF_TEST_VM_PROVIDER="$VM_PROVIDER"
  # What a player does that breaks a lab (tests/resilience.md): a container or VM removed, stopped
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
  scenario attack-box-aws-vm cloud-vm aws "$AWS_BOX" CYBERCTF_TEST_BOX_LAB="$SOLO" CYBERCTF_TEST_BOX_FORM=cloud-vm \
    CYBERCTF_TEST_AWS_REGION="$AWS_REGION" CYBERCTF_TEST_CLOUD_REAL_IP=1
fi

if in_tier server; then
  # A lab on a server through the runner, as the launcher drives it; each host by its own knobs
  # (infra/test-servers/README.md makes a throwaway one).
  # Every setting a host's test reads checked first: one missing made the test panic, recorded FAIL.
  needs=$(unset_of CYBERCTF_TEST_PVE_ENDPOINT CYBERCTF_TEST_PVE_TOKEN CYBERCTF_TEST_PVE_NODE CYBERCTF_TEST_PVE_BRIDGE CYBERCTF_TEST_PVE_RUNNER_HOME)
  [ -z "$needs" ] || NOT_HERE="not set: $needs (infra/test-servers/README.md)"
  scenario server-lab-proxmox vm proxmox "$PROXMOX" CYBERCTF_TEST_PVE_LAB="$SOLO"
  needs=$(unset_of CYBERCTF_TEST_ESXI_HOST CYBERCTF_TEST_ESXI_PASSWORD)
  NOT_HERE=""
  [ -z "$needs" ] || NOT_HERE="not set: $needs (infra/test-servers/README.md)"
  scenario server-lab-esxi vm esxi "$ESXI" CYBERCTF_TEST_ESXI_LAB="$SOLO"
fi

if [ -n "$LISTING" ]; then
  # Not run here: it installs real tools on the machine as an administrator. The test rig runs it
  # on a bare guest, or anywhere with CYBERCTF_TEST_SETUP_CHECK_ONLY=1, which only detects and
  # plans (docs/testing/integration.md, "Bare-machine tier"); listed for its knobs.
  TIER=bare-machine
  listing setup "" "$SETUP"
  exit 0
fi

suite_end "integration tests ($TIERS), logs in $LOGS"
