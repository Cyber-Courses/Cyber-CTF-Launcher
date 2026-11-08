#!/usr/bin/env bash
# Real labs, end to end, on the real tools: the regression suite for what only real Docker,
# Vagrant, hypervisors and clouds show. Each scenario is an opt-in Rust test (`#[ignore]`) run
# with the knobs it needs; one verdict per scenario at the end, and a failure doesn't stop the
# run. docs/real-labs.md says what each scenario guards against (most come from a field bug).
#
#   scripts/real-labs.sh                      # the local tier: Docker, docker-vm, VMs on this machine
#   TIERS="local aws" scripts/real-labs.sh    # and the AWS tier (billed while it runs)
#   ONLY="box-docker box-vm" scripts/real-labs.sh
#   SKIP="box-vm" scripts/real-labs.sh
#
# Knobs: ISOLOOM (an Isoloom checkout, for its example labs; default ../isoloom), VM_PROVIDER
# (default qemu), DOCKER_VM_PROVIDER (default utm on macOS, else VM_PROVIDER), AWS_REGION
# (default eu-west-3), LOGS (default .real-labs).
set -uo pipefail

HERE=$(cd "$(dirname "$0")/.." && pwd)
cd "$HERE/src-tauri" || exit 2
export PATH="$PATH:/usr/local/bin:/opt/homebrew/bin:$HOME/.cargo/bin"
ISOLOOM=$(cd "${ISOLOOM:-$HERE/../isoloom}" 2>/dev/null && pwd) || { echo "no Isoloom checkout: set ISOLOOM" >&2; exit 2; }
EXAMPLES="$ISOLOOM/examples"
TIERS=${TIERS:-local}
VM_PROVIDER=${VM_PROVIDER:-qemu}
if [ "$(uname)" = Darwin ]; then DOCKER_VM_PROVIDER=${DOCKER_VM_PROVIDER:-utm}; else DOCKER_VM_PROVIDER=${DOCKER_VM_PROVIDER:-$VM_PROVIDER}; fi
AWS_REGION=${AWS_REGION:-eu-west-3}
LOGS=${LOGS:-$HERE/.real-labs}
mkdir -p "$LOGS"

RESULTS=()
FAILED=0
wanted() {
  if [ -n "${ONLY:-}" ] && [[ " $ONLY " != *" $1 "* ]]; then return 1; fi
  if [ -n "${SKIP:-}" ] && [[ " $SKIP " == *" $1 "* ]]; then return 1; fi
  return 0
}
record() { RESULTS+=("$(printf '%-18s %-6s %6ss  %s' "$1" "$2" "$3" "$4")"); }
# scenario <name> <test name> [VAR=value...]: the opt-in test, alone, with its knobs; its output
# goes to $LOGS/<name>.log, its checks (`ok:` / `FAILED:`) to the screen.
scenario() {
  local name=$1 test=$2
  shift 2
  wanted "$name" || return 0
  printf '\n==== %s (%s)\n' "$name" "$test"
  local start=$SECONDS log="$LOGS/$name.log"
  if env "$@" cargo test -q --lib -- --ignored --nocapture --test-threads=1 --exact "$test" >"$log" 2>&1 && grep -q "test result: ok. 1 passed" "$log"; then
    grep -E '^(ok|FAILED):' "$log"
    record "$name" ok $((SECONDS - start)) ""
  else
    grep -E '^(ok|FAILED):|panicked' "$log" | tail -20
    FAILED=$((FAILED + 1))
    record "$name" FAIL $((SECONDS - start)) "see $log"
  fi
}
tier() { [[ " $TIERS " == *" $1 "* ]]; }

BOX=runtime::real_attack_machine
if tier local; then
  # The attack box, a machine of the lab, on each local form: added, reached from, taken out.
  scenario box-docker "$BOX::the_attack_box_is_a_machine_of_the_lab_on_every_local_form" \
    CYBERCTF_TEST_BOX_LAB="$EXAMPLES/solo-web" CYBERCTF_TEST_BOX_FORM=docker
  scenario box-docker-vm "$BOX::the_attack_box_is_a_machine_of_the_lab_on_every_local_form" \
    CYBERCTF_TEST_BOX_LAB="$EXAMPLES/solo-web" CYBERCTF_TEST_BOX_FORM=docker-vm CYBERCTF_TEST_VM_PROVIDER="$DOCKER_VM_PROVIDER"
  scenario box-vm "$BOX::the_attack_box_is_a_machine_of_the_lab_on_every_local_form" \
    CYBERCTF_TEST_BOX_LAB="$EXAMPLES/solo-web" CYBERCTF_TEST_BOX_FORM=vm CYBERCTF_TEST_VM_PROVIDER="$VM_PROVIDER"
  # A lab's whole life on each local form: up, status, park and resume, checks, down, nothing left.
  scenario docker-lab runtime::real_labs::real_labs_hello_stack_on_the_runner CYBERCTF_TEST_HELLO_LAB="$EXAMPLES/hello-stack"
  scenario docker-lab-segmented runtime::real_labs::real_labs_segmented_on_the_runner CYBERCTF_TEST_SEGMENTED_LAB="$EXAMPLES/segmented"
  scenario docker-vm-lab runtime::real_docker_vm_labs::a_container_lab_runs_in_a_local_vm_end_to_end \
    CYBERCTF_TEST_DOCKER_VM_LAB="$EXAMPLES/solo-web" CYBERCTF_TEST_VM_PROVIDER="$DOCKER_VM_PROVIDER"
  scenario vm-lab runtime::real_vm_labs::a_vm_lab_runs_end_to_end_through_the_runner \
    CYBERCTF_TEST_VM_LAB="$EXAMPLES/solo-web" CYBERCTF_TEST_VM_PROVIDER="$VM_PROVIDER"
  scenario per-machine runtime::real_per_machine_labs::a_dual_form_lab_runs_as_one_vm_per_machine_end_to_end \
    CYBERCTF_TEST_PER_MACHINE_LAB="$EXAMPLES/supplier-portal-api" CYBERCTF_TEST_VM_PROVIDER="$VM_PROVIDER" \
    CYBERCTF_TEST_VM_INPUTS=API_URL=http://localhost:3206,LAUNCH_TOKEN=test-token
  scenario launch-commands commands::real_launch_labs::a_local_lab_launches_and_tears_down_through_the_commands \
    CYBERCTF_TEST_LOCAL_LAB="$EXAMPLES/hello-stack"
fi

if tier aws; then
  # Signed in first: a run started with a sign-in that runs out halfway leaves the lab behind.
  if aws sts get-caller-identity >/dev/null 2>&1; then
    scenario box-aws-docker "$BOX::the_attack_box_is_a_machine_of_an_aws_lab" \
      CYBERCTF_TEST_BOX_LAB="$EXAMPLES/solo-web" CYBERCTF_TEST_BOX_FORM=cloud-docker CYBERCTF_TEST_AWS_REGION="$AWS_REGION" CYBERCTF_TEST_REAL_PUBLIC_IP=1
    scenario box-aws-vm "$BOX::the_attack_box_is_a_machine_of_an_aws_lab" \
      CYBERCTF_TEST_BOX_LAB="$EXAMPLES/solo-web" CYBERCTF_TEST_BOX_FORM=cloud-vm CYBERCTF_TEST_AWS_REGION="$AWS_REGION" CYBERCTF_TEST_REAL_PUBLIC_IP=1
  else
    record aws skip 0 "the AWS CLI isn't signed in (aws login)"
  fi
fi

printf '\n==== real labs (%s)\n' "$TIERS"
printf '%s\n' "${RESULTS[@]}"
[ "$FAILED" -eq 0 ]
