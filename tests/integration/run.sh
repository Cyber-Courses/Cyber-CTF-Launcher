#!/usr/bin/env bash
# The integration tests: real labs, end to end, on the real tools. The regression suite for what
# only real Docker, Vagrant, hypervisors and clouds show. Each scenario is an opt-in Rust test
# (`#[ignore]`) run with the knobs it needs; one verdict per scenario, and a failure doesn't stop
# the run. docs/testing/integration.md says what each scenario guards against; tests/README.md
# lists every knob.
#
#   tests/integration/run.sh                                            the local tier: Docker, docker-vm, VMs here
#   CYBERCTF_TEST_INTEGRATION_TIERS="local aws" tests/integration/run.sh   and the AWS tier (billed while it runs)
#   CYBERCTF_TEST_INTEGRATION_TIERS=server tests/integration/run.sh        a lab on a Proxmox / ESXi host (infra/test-servers)
#   ONLY="attack-box-docker attack-box-vm" tests/integration/run.sh
#
# Knobs:
#   CYBERCTF_TEST_INTEGRATION_TIERS  local, aws, server (default: local)
#   CYBERCTF_TEST_INTEGRATION_LOGS   where each scenario's log goes (default: tests/integration/.state/logs)
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

ISOLOOM=$(cd "${CYBERCTF_TEST_ISOLOOM_DIR:-$ROOT/../isoloom}" 2>/dev/null && pwd) || {
  echo "no Isoloom checkout: set CYBERCTF_TEST_ISOLOOM_DIR" >&2
  exit 2
}
EXAMPLES="$ISOLOOM/examples"
TIERS=${CYBERCTF_TEST_INTEGRATION_TIERS:-local}
VM_PROVIDER=${CYBERCTF_TEST_VM_PROVIDER:-qemu}
if [ "$(uname -s)" = Darwin ]; then DOCKER_VM_PROVIDER=${CYBERCTF_TEST_DOCKER_VM_PROVIDER:-utm}; else DOCKER_VM_PROVIDER=${CYBERCTF_TEST_DOCKER_VM_PROVIDER:-$VM_PROVIDER}; fi
AWS_REGION=${CYBERCTF_TEST_AWS_REGION:-eu-west-3}
LOGS=${CYBERCTF_TEST_INTEGRATION_LOGS:-$ROOT/tests/integration/.state/logs}
mkdir -p "$LOGS"
cd "$ROOT/src-tauri" || exit 2

# A scenario's checks (`ok:` / `FAILED:`) are its summary on screen; its log keeps the rest.
# shellcheck disable=SC2034 # read by lib/suite.sh's logged, which shellcheck doesn't follow
SUITE_SHOW='^(ok|FAILED):|panicked'

# How a test says it returned early for a knob that isn't set (integration_tests::support::SKIPPED,
# which a unit test holds to this line): cargo counts that return as a pass.
KNOB_UNSET="integration test skipped, not set:"

# scenario <name> <test> [VAR=value...]: the opt-in test, alone, with its knobs. It passes when
# the test ran and passed; a test that returned early for a missing knob is skipped.
scenario() {
  local name=$1 test=$2 start=$SECONDS log unset
  shift 2
  wanted "$name" || return 0
  log="$LOGS/$name.log"
  if logged "$name" "$log" 0 env "$@" cargo test -q --lib -- --ignored --nocapture --test-threads=1 --exact "$test" &&
    grep -q "test result: ok. 1 passed" "$log"; then
    unset=$(grep -F "$KNOB_UNSET" "$log" | head -1)
    if [ -n "$unset" ]; then
      record "$name" skip $((SECONDS - start)) "${unset#"$KNOB_UNSET" } isn't set"
    else
      record "$name" ok $((SECONDS - start)) ""
    fi
  else
    record "$name" FAIL $((SECONDS - start)) "$log"
  fi
}
tier() { listed "$1" "$TIERS"; }
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

BOX=integration_tests::attack_box
LOCAL_BOX="$BOX::the_attack_box_is_a_machine_of_the_lab_on_every_local_form"
AWS_BOX="$BOX::the_attack_box_is_a_machine_of_an_aws_lab"
SOLO="$EXAMPLES/solo-web"

if tier local; then
  # The attack box, a machine of the lab, on each local form: added, reached from, the shared
  # folder seen both ways from inside it (where the form can share one), taken out.
  scenario attack-box-docker "$LOCAL_BOX" CYBERCTF_TEST_BOX_LAB="$SOLO" CYBERCTF_TEST_BOX_FORM=docker \
    CYBERCTF_TEST_BOX_SHARED_FOLDER="$LOGS/shared/attack-box-docker"
  scenario attack-box-docker-vm "$LOCAL_BOX" CYBERCTF_TEST_BOX_LAB="$SOLO" CYBERCTF_TEST_BOX_FORM=docker-vm CYBERCTF_TEST_VM_PROVIDER="$DOCKER_VM_PROVIDER" \
    CYBERCTF_TEST_BOX_SHARED_FOLDER="$(shared_for "$DOCKER_VM_PROVIDER" attack-box-docker-vm)"
  scenario attack-box-vm "$LOCAL_BOX" CYBERCTF_TEST_BOX_LAB="$SOLO" CYBERCTF_TEST_BOX_FORM=vm CYBERCTF_TEST_VM_PROVIDER="$VM_PROVIDER" \
    CYBERCTF_TEST_BOX_SHARED_FOLDER="$(shared_for "$VM_PROVIDER" attack-box-vm)"
  # A lab's whole life on each local form: up, status, park and resume, checks, down, nothing left.
  scenario docker-lab-hello-stack integration_tests::docker_lab::hello_stack_runs_end_to_end CYBERCTF_TEST_HELLO_LAB="$EXAMPLES/hello-stack"
  scenario docker-lab-segmented integration_tests::docker_lab::segmented_runs_end_to_end CYBERCTF_TEST_SEGMENTED_LAB="$EXAMPLES/segmented"
  scenario docker-vm-lab integration_tests::docker_vm_lab::a_container_lab_runs_in_a_local_vm_end_to_end \
    CYBERCTF_TEST_DOCKER_VM_LAB="$SOLO" CYBERCTF_TEST_VM_PROVIDER="$DOCKER_VM_PROVIDER"
  scenario vm-lab integration_tests::vm_lab::a_vm_lab_runs_end_to_end_through_the_runner \
    CYBERCTF_TEST_VM_LAB="$SOLO" CYBERCTF_TEST_VM_PROVIDER="$VM_PROVIDER"
  scenario per-machine integration_tests::per_machine::a_dual_form_lab_runs_as_one_vm_per_machine_end_to_end \
    CYBERCTF_TEST_PER_MACHINE_LAB="$EXAMPLES/supplier-portal-api" CYBERCTF_TEST_VM_PROVIDER="$VM_PROVIDER" \
    CYBERCTF_TEST_VM_INPUTS=API_URL=http://localhost:3206,LAUNCH_TOKEN=test-token
  scenario launch-commands integration_tests::launch_commands::a_local_lab_launches_and_tears_down_through_the_commands \
    CYBERCTF_TEST_LOCAL_LAB="$EXAMPLES/hello-stack"
  # Representative CTF labs of one's own (private, so named by folder only), each up to down.
  scenario corpus-docker integration_tests::corpus_docker::corpus_docker_labs_run_end_to_end_on_the_runner
  # A running lab's folder is never replaced under it: an update and a re-import are refused
  # while it is up on each local form, and go through once it is stopped.
  REPLACE=integration_tests::replace::a_running_lab_is_not_replaced_under_it
  scenario replace-docker "$REPLACE" CYBERCTF_TEST_REPLACE_LAB="$EXAMPLES/hello-stack" CYBERCTF_TEST_REPLACE_FORM=docker
  scenario replace-docker-vm "$REPLACE" CYBERCTF_TEST_REPLACE_LAB="$SOLO" CYBERCTF_TEST_REPLACE_FORM=docker-vm \
    CYBERCTF_TEST_VM_PROVIDER="$DOCKER_VM_PROVIDER"
  scenario replace-vm "$REPLACE" CYBERCTF_TEST_REPLACE_LAB="$SOLO" CYBERCTF_TEST_REPLACE_FORM=vm CYBERCTF_TEST_VM_PROVIDER="$VM_PROVIDER"
fi

if tier aws; then
  # Signed in first: a run whose sign-in runs out halfway leaves the lab behind.
  if aws sts get-caller-identity >/dev/null 2>&1; then
    # Real tests ask for this machine's real public IP: the lab's firewall opens to it alone.
    scenario attack-box-aws-docker "$AWS_BOX" CYBERCTF_TEST_BOX_LAB="$SOLO" CYBERCTF_TEST_BOX_FORM=cloud-docker \
      CYBERCTF_TEST_AWS_REGION="$AWS_REGION" CYBERCTF_TEST_CLOUD_REAL_IP=1
    scenario attack-box-aws-vm "$AWS_BOX" CYBERCTF_TEST_BOX_LAB="$SOLO" CYBERCTF_TEST_BOX_FORM=cloud-vm \
      CYBERCTF_TEST_AWS_REGION="$AWS_REGION" CYBERCTF_TEST_CLOUD_REAL_IP=1
  else
    skip attack-box-aws-docker "the AWS CLI isn't signed in (aws login)"
    skip attack-box-aws-vm "the AWS CLI isn't signed in (aws login)"
  fi
fi

if tier server; then
  # A lab on a server through the runner, as the launcher drives it; each host by its own knobs
  # (infra/test-servers/README.md makes a throwaway one).
  # Every setting a host's test reads checked first: one missing made the test panic, recorded FAIL.
  needs=$(unset_of CYBERCTF_TEST_PVE_ENDPOINT CYBERCTF_TEST_PVE_TOKEN CYBERCTF_TEST_PVE_NODE CYBERCTF_TEST_PVE_BRIDGE CYBERCTF_TEST_PVE_RUNNER_HOME)
  if [ -z "$needs" ]; then
    scenario server-lab-proxmox integration_tests::server_lab::a_vm_lab_runs_on_proxmox_through_the_runner CYBERCTF_TEST_PVE_LAB="$SOLO"
  else
    skip server-lab-proxmox "not set: $needs (infra/test-servers/README.md)"
  fi
  needs=$(unset_of CYBERCTF_TEST_ESXI_HOST CYBERCTF_TEST_ESXI_PASSWORD)
  if [ -z "$needs" ]; then
    scenario server-lab-esxi integration_tests::server_lab::a_vm_lab_runs_on_esxi_through_the_runner CYBERCTF_TEST_ESXI_LAB="$SOLO"
  else
    skip server-lab-esxi "not set: $needs (infra/test-servers/README.md)"
  fi
fi

suite_end "integration tests ($TIERS), logs in $LOGS"
