#!/usr/bin/env bash
# The integration tests: real labs, end to end, on the real tools. The regression suite for what
# only real Docker, Vagrant, hypervisors and clouds show. Each scenario is an opt-in Rust test (`#[ignore]`) run
# with the knobs it needs; one verdict per scenario, and a failure doesn't stop the run.
# docs/testing/integration.md says what each scenario guards against.
#
#   scripts/integration-tests.sh                       the local tier: Docker, docker-vm, VMs here
#   TIERS="local aws" scripts/integration-tests.sh     and the AWS tier (billed while it runs)
#   TIERS=server scripts/integration-tests.sh          a lab on a Proxmox / ESXi host (infra/test-servers)
#   ONLY="box-docker box-vm" scripts/integration-tests.sh
#
# Knobs:
#   TIERS               local, aws, server (default: local)
#   ISOLOOM             an Isoloom checkout, for its example labs (default: ../isoloom)
#   VM_PROVIDER         the hypervisor for VM labs (default: qemu)
#   DOCKER_VM_PROVIDER  the one for a container lab in a VM (default: utm on macOS)
#   AWS_REGION          the AWS tier's region (default: eu-west-3)
#   LOGS                where each scenario's log goes (default: .integration-tests)
#   ONLY, SKIP, STRICT  as every suite runner (scripts/lib/suite.sh)
set -uo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
# shellcheck source=lib/suite.sh
. "$ROOT/scripts/lib/suite.sh"
suite_help "$0" "$@"

ISOLOOM=$(cd "${ISOLOOM:-$ROOT/../isoloom}" 2>/dev/null && pwd) || { echo "no Isoloom checkout: set ISOLOOM" >&2; exit 2; }
EXAMPLES="$ISOLOOM/examples"
TIERS=${TIERS:-local}
VM_PROVIDER=${VM_PROVIDER:-qemu}
if [ "$(uname -s)" = Darwin ]; then DOCKER_VM_PROVIDER=${DOCKER_VM_PROVIDER:-utm}; else DOCKER_VM_PROVIDER=${DOCKER_VM_PROVIDER:-$VM_PROVIDER}; fi
AWS_REGION=${AWS_REGION:-eu-west-3}
LOGS=${LOGS:-$ROOT/.integration-tests}
mkdir -p "$LOGS"
cd "$ROOT/src-tauri" || exit 2

# A scenario's checks (`ok:` / `FAILED:`) are its summary on screen; its log keeps the rest.
SUITE_SHOW='^(ok|FAILED):|panicked'

# scenario <name> <test> [VAR=value...]: the opt-in test, alone, with its knobs. It passes when
# the test ran and passed (a missing knob makes it return early, which isn't a pass).
scenario() {
  local name=$1 test=$2 start=$SECONDS log
  shift 2
  wanted "$name" || return 0
  log="$LOGS/$name.log"
  if logged "$name" "$log" 0 env "$@" cargo test -q --lib -- --ignored --nocapture --test-threads=1 --exact "$test" &&
    grep -q "test result: ok. 1 passed" "$log"; then
    record "$name" ok $((SECONDS - start)) ""
  else
    record "$name" FAIL $((SECONDS - start)) "$log"
  fi
}
tier() { [[ " $TIERS " == *" $1 "* ]]; }
# shared_for <provider> <scenario>: the shared folder a VM box gets on that hypervisor here, empty
# where it can't share one (QEMU outside Linux: no virtiofsd; Hyper-V).
shared_for() {
  case "$1" in
    qemu) [ "$(uname -s)" = Linux ] && echo "$LOGS/shared/$2" ;;
    hyperv) ;;
    *) echo "$LOGS/shared/$2" ;;
  esac
}

BOX=integration_tests::attack_box
LOCAL_BOX="$BOX::the_attack_box_is_a_machine_of_the_lab_on_every_local_form"
AWS_BOX="$BOX::the_attack_box_is_a_machine_of_an_aws_lab"
SOLO="$EXAMPLES/solo-web"

if tier local; then
  # The attack box, a machine of the lab, on each local form: added, reached from, the shared
  # folder seen both ways from inside it (where the form can share one), taken out.
  scenario box-docker "$LOCAL_BOX" CYBERCTF_TEST_BOX_LAB="$SOLO" CYBERCTF_TEST_BOX_FORM=docker \
    CYBERCTF_TEST_SHARED_FOLDER="$LOGS/shared/box-docker"
  scenario box-docker-vm "$LOCAL_BOX" CYBERCTF_TEST_BOX_LAB="$SOLO" CYBERCTF_TEST_BOX_FORM=docker-vm CYBERCTF_TEST_VM_PROVIDER="$DOCKER_VM_PROVIDER" \
    CYBERCTF_TEST_SHARED_FOLDER="$(shared_for "$DOCKER_VM_PROVIDER" box-docker-vm)"
  scenario box-vm "$LOCAL_BOX" CYBERCTF_TEST_BOX_LAB="$SOLO" CYBERCTF_TEST_BOX_FORM=vm CYBERCTF_TEST_VM_PROVIDER="$VM_PROVIDER" \
    CYBERCTF_TEST_SHARED_FOLDER="$(shared_for "$VM_PROVIDER" box-vm)"
  # A lab's whole life on each local form: up, status, park and resume, checks, down, nothing left.
  scenario docker-lab integration_tests::docker::hello_stack_runs_end_to_end CYBERCTF_TEST_HELLO_LAB="$EXAMPLES/hello-stack"
  scenario docker-lab-segmented integration_tests::docker::segmented_runs_end_to_end CYBERCTF_TEST_SEGMENTED_LAB="$EXAMPLES/segmented"
  scenario docker-vm-lab integration_tests::docker_vm::a_container_lab_runs_in_a_local_vm_end_to_end \
    CYBERCTF_TEST_DOCKER_VM_LAB="$SOLO" CYBERCTF_TEST_VM_PROVIDER="$DOCKER_VM_PROVIDER"
  scenario vm-lab integration_tests::vm::a_vm_lab_runs_end_to_end_through_the_runner \
    CYBERCTF_TEST_VM_LAB="$SOLO" CYBERCTF_TEST_VM_PROVIDER="$VM_PROVIDER"
  scenario per-machine integration_tests::per_machine::a_dual_form_lab_runs_as_one_vm_per_machine_end_to_end \
    CYBERCTF_TEST_PER_MACHINE_LAB="$EXAMPLES/supplier-portal-api" CYBERCTF_TEST_VM_PROVIDER="$VM_PROVIDER" \
    CYBERCTF_TEST_VM_INPUTS=API_URL=http://localhost:3206,LAUNCH_TOKEN=test-token
  scenario launch-commands integration_tests::commands::a_local_lab_launches_and_tears_down_through_the_commands \
    CYBERCTF_TEST_LOCAL_LAB="$EXAMPLES/hello-stack"
fi

if tier aws; then
  # Signed in first: a run whose sign-in runs out halfway leaves the lab behind.
  if aws sts get-caller-identity >/dev/null 2>&1; then
    # Real tests ask for this machine's real public IP: the lab's firewall opens to it alone.
    scenario box-aws-docker "$AWS_BOX" CYBERCTF_TEST_BOX_LAB="$SOLO" CYBERCTF_TEST_BOX_FORM=cloud-docker \
      CYBERCTF_TEST_AWS_REGION="$AWS_REGION" CYBERCTF_TEST_REAL_PUBLIC_IP=1
    scenario box-aws-vm "$AWS_BOX" CYBERCTF_TEST_BOX_LAB="$SOLO" CYBERCTF_TEST_BOX_FORM=cloud-vm \
      CYBERCTF_TEST_AWS_REGION="$AWS_REGION" CYBERCTF_TEST_REAL_PUBLIC_IP=1
  else
    skip box-aws-docker "the AWS CLI isn't signed in (aws login)"
    skip box-aws-vm "the AWS CLI isn't signed in (aws login)"
  fi
fi

if tier server; then
  # A lab on a server through the runner, as the launcher drives it; each host by its own knobs
  # (infra/test-servers/README.md makes a throwaway one).
  if [ -n "${CYBERCTF_TEST_PVE_ENDPOINT:-}" ]; then
    scenario proxmox-lab integration_tests::servers::a_vm_lab_runs_on_proxmox_through_the_runner CYBERCTF_TEST_PVE_LAB="$SOLO"
  else
    skip proxmox-lab "no Proxmox host: set CYBERCTF_TEST_PVE_* (infra/test-servers/README.md)"
  fi
  if [ -n "${CYBERCTF_TEST_ESXI_HOST:-}" ]; then
    scenario esxi-lab integration_tests::servers::a_vm_lab_runs_on_esxi_through_the_runner CYBERCTF_TEST_ESXI_LAB="$SOLO"
  else
    skip esxi-lab "no ESXi host: set CYBERCTF_TEST_ESXI_* (infra/test-servers/README.md)"
  fi
fi

suite_end "integration tests ($TIERS), logs in $LOGS"
