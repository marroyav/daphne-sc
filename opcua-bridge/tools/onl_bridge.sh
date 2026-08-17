#!/usr/bin/env bash
set -euo pipefail

JUMP_TARGET="${JUMP_TARGET:-marroyav@lxtunnel.CERN.CH}"
ONL_TARGET="${ONL_TARGET:-marroyav@np04-onl-004.CERN.CH}"
REMOTE_DIR="${REMOTE_DIR:-daphne-sc-opcua-stage}"
CONFIG_PATH="${CONFIG_PATH:-opcua-bridge/config/np04-onl-004.example.conf}"
CONTRACT_DIR="${CONTRACT_DIR:-../Interface2/interface-data/daphne/exports}"
OPEN62541_PREFIX="${OPEN62541_PREFIX:-../daphne-sc-opcua/deps/open62541-install}"
SSH_CONTROL_PATH="${SSH_CONTROL_PATH:-}"

usage() {
  cat <<'EOF'
usage: opcua-bridge/tools/onl_bridge.sh [stage|probe|build|fake|real|v8|tunnel]

Environment overrides:
  JUMP_TARGET=marroyav@lxtunnel.CERN.CH
  ONL_TARGET=marroyav@np04-onl-004.CERN.CH
  REMOTE_DIR=daphne-sc-opcua-stage
  CONFIG_PATH=opcua-bridge/config/np04-onl-004.example.conf
  CONTRACT_DIR=../Interface2/interface-data/daphne/exports
  OPEN62541_PREFIX=../daphne-sc-opcua/deps/open62541-install
  SSH_CONTROL_PATH=~/.ssh/cm-np04-onl-004

Commands:
  stage   copy bridge sources, proto files, and v8 contract exports
  probe   run the hardware/network probe on the ONL host
  build   configure and build the C++ bridge on the ONL host
  fake    stage, build, and run the bridge with fake DAPHNE/power endpoints
  real    stage, build, and run the bridge against configured real endpoints
  v8      stage, build, and run the read-only DAPHNE-015 v8 test profile
  tunnel  open an SSH tunnel from localhost:4840 to ONL localhost:4840
EOF
}

validate_remote_dir() {
  if [[ ! "${REMOTE_DIR}" =~ ^daphne-sc-opcua[-A-Za-z0-9._]*$ ]]; then
    echo "REMOTE_DIR must be a relative staging name beginning with daphne-sc-opcua" >&2
    return 2
  fi
}

remote_dir_q() {
  printf '%q' "${REMOTE_DIR}"
}

ssh_control_opts() {
  if [[ -n "${SSH_CONTROL_PATH}" ]]; then
    printf '%s\n' -o "ControlPath=${SSH_CONTROL_PATH}" -o ControlMaster=auto -o ControlPersist=5h
  fi
}

ssh_exec() {
  mapfile -t opts < <(ssh_control_opts)
  ssh "${opts[@]}" -J "${JUMP_TARGET}" "${ONL_TARGET}" "$@"
}

ssh_tty() {
  mapfile -t opts < <(ssh_control_opts)
  ssh "${opts[@]}" -tt -J "${JUMP_TARGET}" "${ONL_TARGET}" "$@"
}

stage() {
  local require_contracts="${1:-false}"
  local remote_dir
  validate_remote_dir
  remote_dir="$(remote_dir_q)"
  local contract_files=(
    tag_list.csv
    opc_ua_control_policy.csv
    coverage_summary.csv
    instance_sets.csv
  )
  local contract_file
  local have_contracts=true
  for contract_file in "${contract_files[@]}"; do
    if [[ ! -f "${CONTRACT_DIR}/${contract_file}" ]]; then
      have_contracts=false
    fi
  done

  if [[ "${have_contracts}" != true && "${require_contracts}" == true ]]; then
    echo "v8 contract exports are incomplete under ${CONTRACT_DIR}" >&2
    return 2
  fi

  ssh_exec "mkdir -p ${remote_dir}"
  tar --exclude=build --exclude=target --exclude=.git -czf - \
    opcua-bridge proto README.md .gitignore |
    ssh_exec "tar -xzf - -C ${remote_dir}"
  if [[ "${have_contracts}" == true ]]; then
    ssh_exec "mkdir -p ${remote_dir}/contract"
    tar -C "${CONTRACT_DIR}" -czf - "${contract_files[@]}" |
      ssh_exec "tar -xzf - -C ${remote_dir}/contract"
  else
    echo "warning: v8 contract exports not staged; set CONTRACT_DIR before using the v8 profile" >&2
  fi
}

probe() {
  local remote_dir
  validate_remote_dir
  remote_dir="$(remote_dir_q)"
  ssh_exec "cd ${remote_dir} && bash opcua-bridge/tools/np04_probe.sh"
}

build() {
  local remote_dir
  validate_remote_dir
  remote_dir="$(remote_dir_q)"
  local open62541_prefix
  printf -v open62541_prefix '%q' "${OPEN62541_PREFIX}"
  ssh_exec "cd ${remote_dir} && CMAKE_PREFIX_PATH=${open62541_prefix} cmake -S opcua-bridge -B build/opcua-bridge && cmake --build build/opcua-bridge -j\$(nproc)"
}

run_fake() {
  local remote_dir
  validate_remote_dir
  remote_dir="$(remote_dir_q)"
  local config_path
  printf -v config_path '%q' "${CONFIG_PATH}"
  ssh_tty "cd ${remote_dir} && build/opcua-bridge/pds-opcua-bridge --config ${config_path} --fake"
}

run_real() {
  local remote_dir
  validate_remote_dir
  remote_dir="$(remote_dir_q)"
  local config_path
  printf -v config_path '%q' "${CONFIG_PATH}"
  ssh_tty "cd ${remote_dir} && build/opcua-bridge/pds-opcua-bridge --config ${config_path}"
}

tunnel() {
  mapfile -t opts < <(ssh_control_opts)
  ssh "${opts[@]}" -J "${JUMP_TARGET}" -L 4840:localhost:4840 "${ONL_TARGET}"
}

cmd="${1:-fake}"
case "${cmd}" in
  stage)
    stage
    ;;
  probe)
    probe
    ;;
  build)
    build
    ;;
  fake)
    stage
    build
    run_fake
    ;;
  real)
    stage
    build
    run_real
    ;;
  v8)
    stage true
    build
    CONFIG_PATH=opcua-bridge/config/np04-daphne-015-v8-test.example.conf
    run_real
    ;;
  tunnel)
    tunnel
    ;;
  -h|--help|help)
    usage
    ;;
  *)
    usage >&2
    exit 2
    ;;
esac
