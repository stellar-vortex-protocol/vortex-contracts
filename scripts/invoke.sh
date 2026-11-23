#!/bin/bash

# Vortex Contract CLI Helper
# Wraps stellar contract invoke commands for every contract in the workspace.
# Reduces boilerplate by reading common parameters from environment variables
# or a .env.local config file.
#
# Usage:
#   ./scripts/invoke.sh <contract> <function> [options]
#
# The network is not a positional argument. Select it with VORTEX_NETWORK
# (testnet or local; default: testnet). For the local network started by
# ./setup-local-network.sh, run:
#   VORTEX_NETWORK=local ./scripts/invoke.sh intent_settlement get_stats
#
# Examples — intent_settlement:
#   ./scripts/invoke.sh intent_settlement submit_intent --user <USER> --src_chain ethereum --src_token <TOKEN> --src_amount <AMOUNT> --dst_token <USDC_SAC> --min_dst_amount <MIN>
#   ./scripts/invoke.sh intent_settlement register_solver --solver <SOLVER> --bond_amount <AMOUNT>
#   ./scripts/invoke.sh intent_settlement accept_intent --solver <SOLVER> --intent_id <INTENT_ID>
#   ./scripts/invoke.sh intent_settlement fill_intent --solver <SOLVER> --intent_id <INTENT_ID> --fill_amount <AMOUNT>
#   ./scripts/invoke.sh intent_settlement slash_solver --intent_id <INTENT_ID>
#   ./scripts/invoke.sh intent_settlement get_stats
#
# Examples — solver_registry:
#   ./scripts/invoke.sh solver_registry stake --solver <SOLVER> --amount <AMOUNT>
#   ./scripts/invoke.sh solver_registry deregister_solver --solver <SOLVER>
#   ./scripts/invoke.sh solver_registry record_fill --solver <SOLVER> --amount <AMOUNT>
#   ./scripts/invoke.sh solver_registry record_failure --solver <SOLVER>
#
# Examples — proof_registry:
#   ./scripts/invoke.sh proof_registry receive_proof --intent_id <INTENT_ID> --src_chain_id <CHAIN_ID> --src_amount <AMOUNT>
#   ./scripts/invoke.sh proof_registry authorize_emitter --chain_id <CHAIN_ID> --emitter <EMITTER>
#
# Examples — reputation_badge:
#   ./scripts/invoke.sh reputation_badge mint --solver <SOLVER>
#   ./scripts/invoke.sh reputation_badge get_badge --solver <SOLVER>
#
# Examples — governor:
#   ./scripts/invoke.sh governor propose --proposer <PROPOSER> --target <TARGET> --function <FN> --calldata <HEX>
#   ./scripts/invoke.sh governor cast_vote --voter <VOTER> --proposal_id <ID> --support <0|1|2>
#   ./scripts/invoke.sh governor queue --proposal_id <ID>
#   ./scripts/invoke.sh governor execute --proposal_id <ID>
#   ./scripts/invoke.sh governor state --proposal_id <ID>
#
# Examples — backstop_vault:
#   ./scripts/invoke.sh backstop_vault deposit --lp <LP> --amount <AMOUNT>
#   ./scripts/invoke.sh backstop_vault withdraw --lp <LP> --shares <SHARES>
#   ./scripts/invoke.sh backstop_vault get_vault_state
#   ./scripts/invoke.sh backstop_vault get_lp_position --lp <LP>
#
# Examples — treasury_streams:
#   ./scripts/invoke.sh treasury_streams create_stream --sender <SENDER> --recipient <RECIPIENT> --token <TOKEN> --total <TOTAL> --start <START> --cliff <CLIFF> --end <END>
#   ./scripts/invoke.sh treasury_streams withdraw --stream_id <ID>
#   ./scripts/invoke.sh treasury_streams cancel --stream_id <ID>
#   ./scripts/invoke.sh treasury_streams balance_of --stream_id <ID>
#   ./scripts/invoke.sh treasury_streams get_stream --stream_id <ID>
#
# Examples — intent_periphery:
#   ./scripts/invoke.sh intent_periphery get_stats
#   ./scripts/invoke.sh intent_periphery list_solvers --start 0 --limit 20
#   ./scripts/invoke.sh intent_periphery list_open_intents --start 0 --limit 20
#   ./scripts/invoke.sh intent_periphery get_fee_schedule --solver <SOLVER>
#   ./scripts/invoke.sh intent_periphery set_settlement --settlement <CONTRACT_ID>
#
# Configuration:
#   Set these via environment variables or in .env.local:
#     VORTEX_<CONTRACT>_ID   - Contract ID for a specific contract (e.g. VORTEX_INTENT_SETTLEMENT_ID)
#     VORTEX_CONTRACT_ID     - Fallback contract ID used when the contract-specific var is unset
#     VORTEX_SECRET_KEY      - Secret key for signing (required)
#     VORTEX_NETWORK         - Network: testnet or local (default: testnet)
#     VORTEX_CONFIG          - Config file path (default: .env.local)

set -euo pipefail

# Configuration: read from environment or .env.local file
CONFIG_FILE="${VORTEX_CONFIG:-${PWD}/.env.local}"

# Load configuration from file if it exists
if [[ -f "$CONFIG_FILE" ]]; then
  # shellcheck disable=SC1090
  source "$CONFIG_FILE"
fi

# Use environment variables with fallback to defaults
SECRET_KEY="${VORTEX_SECRET_KEY:-}"
NETWORK="${VORTEX_NETWORK:-testnet}"

# ─── Resolve contract ID ────────────────────────────────────────────────────
# Each contract has its own env var (VORTEX_<UPPER_NAME>_ID); falls back to
# the generic VORTEX_CONTRACT_ID when the specific var is absent.

resolve_contract_id() {
  local contract="$1"
  local upper_name
  upper_name="$(echo "$contract" | tr '[:lower:]' '[:upper:]')"
  local specific_var="VORTEX_${upper_name}_ID"
  # indirect variable expansion (bash 4+)
  local id="${!specific_var:-}"
  if [[ -z "$id" ]]; then
    id="${VORTEX_CONTRACT_ID:-}"
  fi
  if [[ -z "$id" ]]; then
    echo "Error: Neither ${specific_var} nor VORTEX_CONTRACT_ID is set." >&2
    echo "Set it via environment variable or in $CONFIG_FILE" >&2
    exit 1
  fi
  echo "$id"
}

# ─── Print usage ────────────────────────────────────────────────────────────

print_usage() {
  cat << 'EOF'
Vortex Contract CLI Helper

Usage:
  ./scripts/invoke.sh <contract> <function> [options]

Example (local network, after ./setup-local-network.sh start):
  VORTEX_NETWORK=local VORTEX_SECRET_KEY=<SECRET> VORTEX_INTENT_SETTLEMENT_ID=<CONTRACT_ID> \
    ./scripts/invoke.sh intent_settlement get_stats

Contracts:
  intent_settlement   Core swap-intent lifecycle
  solver_registry     Solver bond management and reputation
  proof_registry      Cross-chain proof ingestion
  reputation_badge    Solver reputation badge (SBT)
  governor            On-chain governance (propose, vote, execute)
  backstop_vault      LP vault for backstop coverage
  treasury_streams    Token vesting streams
  intent_periphery    Heavy view/governance proxy for intent_settlement

Run ./scripts/invoke.sh <contract> for per-contract help.

Configuration:
  Set these via environment variables or in .env.local:
    VORTEX_<CONTRACT>_ID   e.g. VORTEX_INTENT_SETTLEMENT_ID (per-contract)
    VORTEX_CONTRACT_ID     Fallback contract ID
    VORTEX_SECRET_KEY      Secret key for signing (required)
    VORTEX_NETWORK         Network: testnet or local (default: testnet)
    VORTEX_CONFIG          Config file path (default: .env.local)
EOF
  exit 1
}

# ─── Per-contract usage helpers ─────────────────────────────────────────────

usage_intent_settlement() {
  cat << 'EOF'
intent_settlement functions:

  submit_intent         --user <ADDR> --src_chain <STR> --src_token <STR>
                        --src_amount <INT> --dst_token <ADDR> --min_dst_amount <INT>
  register_solver       --solver <ADDR> --bond_amount <INT>
  deregister_solver     --solver <ADDR>
  withdraw_bond         --solver <ADDR> --amount <INT>
  accept_intent         --solver <ADDR> --intent_id <HEX>
  fill_intent           --solver <ADDR> --intent_id <HEX> --fill_amount <INT>
  slash_solver          --intent_id <HEX>
  cancel_intent         --intent_id <HEX>
  expire_intent         --intent_id <HEX>
  is_intent_fillable    --intent_id <HEX> --solver <ADDR>
  get_stats
  get_intent            --intent_id <HEX>
  get_solver            --solver <ADDR>
  pause
  unpause
EOF
  exit 1
}

usage_solver_registry() {
  cat << 'EOF'
solver_registry functions:

  stake                 --solver <ADDR> --amount <INT>
  deregister_solver     --solver <ADDR>
  record_fill           --solver <ADDR> --amount <INT>
  record_failure        --solver <ADDR>
  slash                 --solver <ADDR> --slash_amount <INT>
  lock_obligation       --solver <ADDR> --intent_id <HEX>
  release_obligation    --solver <ADDR> --intent_id <HEX>
  set_tier_threshold    --tier <INT> --min_bond <INT> --min_score_bps <INT>
  get_solver            --solver <ADDR>
EOF
  exit 1
}

usage_proof_registry() {
  cat << 'EOF'
proof_registry functions:

  authorize_emitter     --chain_id <INT> --emitter <ADDR>
  authorize_axelar      --chain_name <STR> --source_address <STR>
  receive_proof         --intent_id <HEX> --src_chain_id <INT> --src_amount <INT>
  receive_proof_axelar  --intent_id <HEX> --src_chain_id <INT> --src_amount <INT>
  get_proof             --intent_id <HEX>
EOF
  exit 1
}

usage_reputation_badge() {
  cat << 'EOF'
reputation_badge functions:

  mint                  --solver <ADDR>
  upgrade_badge         --solver <ADDR>
  get_badge             --solver <ADDR>
EOF
  exit 1
}

usage_governor() {
  cat << 'EOF'
governor functions:

  initialize            --registry <ADDR> --timelock <ADDR> --voting_delay <INT>
                        --voting_period <INT> --proposal_threshold <INT> --quorum <INT>
  propose               --proposer <ADDR> --target <ADDR> --function <SYM> --calldata <HEX>
  cast_vote             --voter <ADDR> --proposal_id <INT> --support <0|1|2>
                          (0=Against, 1=For, 2=Abstain)
  queue                 --proposal_id <INT>
  execute               --proposal_id <INT>
  state                 --proposal_id <INT>
  get_proposal          --proposal_id <INT>
  voting_power          --account <ADDR> --ledger <INT>
EOF
  exit 1
}

usage_backstop_vault() {
  cat << 'EOF'
backstop_vault functions:

  initialize            --admin <ADDR> --bond_token <ADDR> --settlement <ADDR>
  deposit               --lp <ADDR> --amount <INT>
  withdraw              --lp <ADDR> --shares <INT>
  process_queued_withdrawal --lp <ADDR>
  cover                 --intent_id <HEX> --amount <INT> --recipient <ADDR>
  receive_income        --amount <INT>
  set_emergency_mode    --enabled <true|false>
  get_vault_state
  get_lp_position       --lp <ADDR>
EOF
  exit 1
}

usage_treasury_streams() {
  cat << 'EOF'
treasury_streams functions:

  initialize            --governor <ADDR>
  create_stream         --sender <ADDR> --recipient <ADDR> --token <ADDR>
                        --total <INT> --start <INT> --cliff <INT> --end <INT>
  withdraw              --stream_id <INT>
  cancel                --stream_id <INT>
  balance_of            --stream_id <INT>
  get_stream            --stream_id <INT>
EOF
  exit 1
}

usage_intent_periphery() {
  cat << 'EOF'
intent_periphery functions:

  initialize            --admin <ADDR> --settlement <ADDR>
  set_settlement        --settlement <ADDR>
  get_settlement
  get_stats
  list_solvers          --start <INT> --limit <INT>
  list_open_intents     --start <INT> --limit <INT>
  list_intents_by_user  --user <ADDR> --start <INT> --limit <INT>
  get_fee_schedule      --solver <ADDR>
  get_solver            --solver <ADDR>
  get_intent            --intent_id <HEX>
  get_protocol_health
EOF
  exit 1
}

# ─── Argument parsing helpers ────────────────────────────────────────────────

# Append a flag and its value to STELLAR_CMD array.
# Strings are quoted for Soroban; integers and addresses are passed plain.
append_string() { STELLAR_CMD+=("--$1" "\"$2\""); }
append_value()  { STELLAR_CMD+=("--$1" "$2"); }

# Generic two-arg consumer: reads --key value pairs and appends them.
# Each contract section builds STELLAR_CMD using append_string / append_value.

# ─── Entry point ─────────────────────────────────────────────────────────────

if [[ $# -lt 1 || "$1" == "-h" || "$1" == "--help" ]]; then
  print_usage
fi

CONTRACT="${1:-}"
shift

# Build the base stellar CLI command using the resolved contract ID.
CONTRACT_ID="$(resolve_contract_id "$CONTRACT")"

if [[ $# -lt 1 ]]; then
  case "$CONTRACT" in
    intent_settlement) usage_intent_settlement ;;
    solver_registry)   usage_solver_registry ;;
    proof_registry)    usage_proof_registry ;;
    reputation_badge)  usage_reputation_badge ;;
    governor)          usage_governor ;;
    backstop_vault)    usage_backstop_vault ;;
    treasury_streams)  usage_treasury_streams ;;
    intent_periphery)  usage_intent_periphery ;;
    *)
      echo "Unknown contract: $CONTRACT"
      echo "Run ./scripts/invoke.sh for the list of contracts."
      exit 1
      ;;
  esac
fi

COMMAND="$1"
shift

if [[ -z "$SECRET_KEY" ]]; then
  echo "Error: VORTEX_SECRET_KEY not set. Set it via environment or in $CONFIG_FILE"
  exit 1
fi

STELLAR_CMD=(
  stellar contract invoke
  --id "$CONTRACT_ID"
  --source "$SECRET_KEY"
  --network "$NETWORK"
  --
  "$COMMAND"
)

# ─── Per-contract dispatch ───────────────────────────────────────────────────

case "$CONTRACT" in

  # ── intent_settlement ────────────────────────────────────────────────────
  intent_settlement)
    case "$COMMAND" in
      submit_intent)
        while [[ $# -gt 0 ]]; do
          case "$1" in
            --user)            append_value user "$2";             shift 2 ;;
            --src_chain)       append_string src_chain "$2";       shift 2 ;;
            --src_token)       append_string src_token "$2";       shift 2 ;;
            --src_amount)      append_value src_amount "$2";       shift 2 ;;
            --dst_token)       append_value dst_token "$2";        shift 2 ;;
            --min_dst_amount)  append_value min_dst_amount "$2";   shift 2 ;;
            *) echo "Unknown option: $1"; exit 1 ;;
          esac
        done ;;
      register_solver)
        while [[ $# -gt 0 ]]; do
          case "$1" in
            --solver)       append_value solver "$2";       shift 2 ;;
            --bond_amount)  append_value bond_amount "$2";  shift 2 ;;
            *) echo "Unknown option: $1"; exit 1 ;;
          esac
        done ;;
      deregister_solver)
        while [[ $# -gt 0 ]]; do
          case "$1" in
            --solver) append_value solver "$2"; shift 2 ;;
            *) echo "Unknown option: $1"; exit 1 ;;
          esac
        done ;;
      withdraw_bond)
        while [[ $# -gt 0 ]]; do
          case "$1" in
            --solver) append_value solver "$2"; shift 2 ;;
            --amount) append_value amount "$2"; shift 2 ;;
            *) echo "Unknown option: $1"; exit 1 ;;
          esac
        done ;;
      accept_intent)
        while [[ $# -gt 0 ]]; do
          case "$1" in
            --solver)     append_value solver "$2";     shift 2 ;;
            --intent_id)  append_value intent_id "$2";  shift 2 ;;
            *) echo "Unknown option: $1"; exit 1 ;;
          esac
        done ;;
      fill_intent)
        while [[ $# -gt 0 ]]; do
          case "$1" in
            --solver)       append_value solver "$2";       shift 2 ;;
            --intent_id)    append_value intent_id "$2";    shift 2 ;;
            --fill_amount)  append_value fill_amount "$2";  shift 2 ;;
            *) echo "Unknown option: $1"; exit 1 ;;
          esac
        done ;;
      slash_solver | cancel_intent | expire_intent)
        while [[ $# -gt 0 ]]; do
          case "$1" in
            --intent_id) append_value intent_id "$2"; shift 2 ;;
            *) echo "Unknown option: $1"; exit 1 ;;
          esac
        done ;;
      is_intent_fillable)
        while [[ $# -gt 0 ]]; do
          case "$1" in
            --intent_id) append_value intent_id "$2"; shift 2 ;;
            --solver)    append_value solver "$2";    shift 2 ;;
            *) echo "Unknown option: $1"; exit 1 ;;
          esac
        done ;;
      get_intent)
        while [[ $# -gt 0 ]]; do
          case "$1" in
            --intent_id) append_value intent_id "$2"; shift 2 ;;
            *) echo "Unknown option: $1"; exit 1 ;;
          esac
        done ;;
      get_solver)
        while [[ $# -gt 0 ]]; do
          case "$1" in
            --solver) append_value solver "$2"; shift 2 ;;
            *) echo "Unknown option: $1"; exit 1 ;;
          esac
        done ;;
      get_stats | pause | unpause) ;;   # no extra arguments
      *) echo "Unknown intent_settlement function: $COMMAND"; usage_intent_settlement ;;
    esac ;;

  # ── solver_registry ──────────────────────────────────────────────────────
  solver_registry)
    case "$COMMAND" in
      stake)
        while [[ $# -gt 0 ]]; do
          case "$1" in
            --solver) append_value solver "$2"; shift 2 ;;
            --amount) append_value amount "$2"; shift 2 ;;
            *) echo "Unknown option: $1"; exit 1 ;;
          esac
        done ;;
      deregister_solver)
        while [[ $# -gt 0 ]]; do
          case "$1" in
            --solver) append_value solver "$2"; shift 2 ;;
            *) echo "Unknown option: $1"; exit 1 ;;
          esac
        done ;;
      record_fill)
        while [[ $# -gt 0 ]]; do
          case "$1" in
            --solver) append_value solver "$2"; shift 2 ;;
            --amount) append_value amount "$2"; shift 2 ;;
            *) echo "Unknown option: $1"; exit 1 ;;
          esac
        done ;;
      record_failure)
        while [[ $# -gt 0 ]]; do
          case "$1" in
            --solver) append_value solver "$2"; shift 2 ;;
            *) echo "Unknown option: $1"; exit 1 ;;
          esac
        done ;;
      slash)
        while [[ $# -gt 0 ]]; do
          case "$1" in
            --solver)        append_value solver "$2";        shift 2 ;;
            --slash_amount)  append_value slash_amount "$2";  shift 2 ;;
            *) echo "Unknown option: $1"; exit 1 ;;
          esac
        done ;;
      lock_obligation | release_obligation)
        while [[ $# -gt 0 ]]; do
          case "$1" in
            --solver)    append_value solver "$2";    shift 2 ;;
            --intent_id) append_value intent_id "$2"; shift 2 ;;
            *) echo "Unknown option: $1"; exit 1 ;;
          esac
        done ;;
      set_tier_threshold)
        while [[ $# -gt 0 ]]; do
          case "$1" in
            --tier)           append_value tier "$2";           shift 2 ;;
            --min_bond)       append_value min_bond "$2";       shift 2 ;;
            --min_score_bps)  append_value min_score_bps "$2";  shift 2 ;;
            *) echo "Unknown option: $1"; exit 1 ;;
          esac
        done ;;
      get_solver)
        while [[ $# -gt 0 ]]; do
          case "$1" in
            --solver) append_value solver "$2"; shift 2 ;;
            *) echo "Unknown option: $1"; exit 1 ;;
          esac
        done ;;
      *) echo "Unknown solver_registry function: $COMMAND"; usage_solver_registry ;;
    esac ;;

  # ── proof_registry ───────────────────────────────────────────────────────
  proof_registry)
    case "$COMMAND" in
      authorize_emitter)
        while [[ $# -gt 0 ]]; do
          case "$1" in
            --chain_id) append_value chain_id "$2"; shift 2 ;;
            --emitter)  append_value emitter "$2";  shift 2 ;;
            *) echo "Unknown option: $1"; exit 1 ;;
          esac
        done ;;
      authorize_axelar)
        while [[ $# -gt 0 ]]; do
          case "$1" in
            --chain_name)     append_string chain_name "$2";     shift 2 ;;
            --source_address) append_string source_address "$2"; shift 2 ;;
            *) echo "Unknown option: $1"; exit 1 ;;
          esac
        done ;;
      receive_proof | receive_proof_axelar)
        while [[ $# -gt 0 ]]; do
          case "$1" in
            --intent_id)   append_value intent_id "$2";   shift 2 ;;
            --src_chain_id) append_value src_chain_id "$2"; shift 2 ;;
            --src_amount)  append_value src_amount "$2";  shift 2 ;;
            *) echo "Unknown option: $1"; exit 1 ;;
          esac
        done ;;
      get_proof)
        while [[ $# -gt 0 ]]; do
          case "$1" in
            --intent_id) append_value intent_id "$2"; shift 2 ;;
            *) echo "Unknown option: $1"; exit 1 ;;
          esac
        done ;;
      *) echo "Unknown proof_registry function: $COMMAND"; usage_proof_registry ;;
    esac ;;

  # ── reputation_badge ─────────────────────────────────────────────────────
  reputation_badge)
    case "$COMMAND" in
      mint | upgrade_badge | get_badge)
        while [[ $# -gt 0 ]]; do
          case "$1" in
            --solver) append_value solver "$2"; shift 2 ;;
            *) echo "Unknown option: $1"; exit 1 ;;
          esac
        done ;;
      *) echo "Unknown reputation_badge function: $COMMAND"; usage_reputation_badge ;;
    esac ;;

  # ── governor ─────────────────────────────────────────────────────────────
  governor)
    case "$COMMAND" in
      initialize)
        while [[ $# -gt 0 ]]; do
          case "$1" in
            --registry)           append_value registry "$2";           shift 2 ;;
            --timelock)           append_value timelock "$2";           shift 2 ;;
            --voting_delay)       append_value voting_delay "$2";       shift 2 ;;
            --voting_period)      append_value voting_period "$2";      shift 2 ;;
            --proposal_threshold) append_value proposal_threshold "$2"; shift 2 ;;
            --quorum)             append_value quorum "$2";             shift 2 ;;
            *) echo "Unknown option: $1"; exit 1 ;;
          esac
        done ;;
      propose)
        while [[ $# -gt 0 ]]; do
          case "$1" in
            --proposer)  append_value proposer "$2";  shift 2 ;;
            --target)    append_value target "$2";    shift 2 ;;
            --function)  append_value function "$2";  shift 2 ;;
            --calldata)  append_value calldata "$2";  shift 2 ;;
            *) echo "Unknown option: $1"; exit 1 ;;
          esac
        done ;;
      cast_vote)
        while [[ $# -gt 0 ]]; do
          case "$1" in
            --voter)       append_value voter "$2";       shift 2 ;;
            --proposal_id) append_value proposal_id "$2"; shift 2 ;;
            --support)     append_value support "$2";     shift 2 ;;
            *) echo "Unknown option: $1"; exit 1 ;;
          esac
        done ;;
      queue | execute | state | get_proposal)
        while [[ $# -gt 0 ]]; do
          case "$1" in
            --proposal_id) append_value proposal_id "$2"; shift 2 ;;
            *) echo "Unknown option: $1"; exit 1 ;;
          esac
        done ;;
      voting_power)
        while [[ $# -gt 0 ]]; do
          case "$1" in
            --account) append_value account "$2"; shift 2 ;;
            --ledger)  append_value ledger "$2";  shift 2 ;;
            *) echo "Unknown option: $1"; exit 1 ;;
          esac
        done ;;
      *) echo "Unknown governor function: $COMMAND"; usage_governor ;;
    esac ;;

  # ── backstop_vault ───────────────────────────────────────────────────────
  backstop_vault)
    case "$COMMAND" in
      initialize)
        while [[ $# -gt 0 ]]; do
          case "$1" in
            --admin)      append_value admin "$2";      shift 2 ;;
            --bond_token) append_value bond_token "$2"; shift 2 ;;
            --settlement) append_value settlement "$2"; shift 2 ;;
            *) echo "Unknown option: $1"; exit 1 ;;
          esac
        done ;;
      deposit)
        while [[ $# -gt 0 ]]; do
          case "$1" in
            --lp)     append_value lp "$2";     shift 2 ;;
            --amount) append_value amount "$2"; shift 2 ;;
            *) echo "Unknown option: $1"; exit 1 ;;
          esac
        done ;;
      withdraw)
        while [[ $# -gt 0 ]]; do
          case "$1" in
            --lp)     append_value lp "$2";     shift 2 ;;
            --shares) append_value shares "$2"; shift 2 ;;
            *) echo "Unknown option: $1"; exit 1 ;;
          esac
        done ;;
      process_queued_withdrawal | get_lp_position)
        while [[ $# -gt 0 ]]; do
          case "$1" in
            --lp) append_value lp "$2"; shift 2 ;;
            *) echo "Unknown option: $1"; exit 1 ;;
          esac
        done ;;
      cover)
        while [[ $# -gt 0 ]]; do
          case "$1" in
            --intent_id) append_value intent_id "$2"; shift 2 ;;
            --amount)    append_value amount "$2";    shift 2 ;;
            --recipient) append_value recipient "$2"; shift 2 ;;
            *) echo "Unknown option: $1"; exit 1 ;;
          esac
        done ;;
      receive_income)
        while [[ $# -gt 0 ]]; do
          case "$1" in
            --amount) append_value amount "$2"; shift 2 ;;
            *) echo "Unknown option: $1"; exit 1 ;;
          esac
        done ;;
      set_emergency_mode)
        while [[ $# -gt 0 ]]; do
          case "$1" in
            --enabled) append_value enabled "$2"; shift 2 ;;
            *) echo "Unknown option: $1"; exit 1 ;;
          esac
        done ;;
      get_vault_state) ;;   # no arguments
      *) echo "Unknown backstop_vault function: $COMMAND"; usage_backstop_vault ;;
    esac ;;

  # ── treasury_streams ─────────────────────────────────────────────────────
  treasury_streams)
    case "$COMMAND" in
      initialize)
        while [[ $# -gt 0 ]]; do
          case "$1" in
            --governor) append_value governor "$2"; shift 2 ;;
            *) echo "Unknown option: $1"; exit 1 ;;
          esac
        done ;;
      create_stream)
        while [[ $# -gt 0 ]]; do
          case "$1" in
            --sender)    append_value sender "$2";    shift 2 ;;
            --recipient) append_value recipient "$2"; shift 2 ;;
            --token)     append_value token "$2";     shift 2 ;;
            --total)     append_value total "$2";     shift 2 ;;
            --start)     append_value start "$2";     shift 2 ;;
            --cliff)     append_value cliff "$2";     shift 2 ;;
            --end)       append_value end "$2";       shift 2 ;;
            *) echo "Unknown option: $1"; exit 1 ;;
          esac
        done ;;
      withdraw | cancel | balance_of | get_stream)
        while [[ $# -gt 0 ]]; do
          case "$1" in
            --stream_id) append_value stream_id "$2"; shift 2 ;;
            *) echo "Unknown option: $1"; exit 1 ;;
          esac
        done ;;
      *) echo "Unknown treasury_streams function: $COMMAND"; usage_treasury_streams ;;
    esac ;;

  # ── intent_periphery ─────────────────────────────────────────────────────
  intent_periphery)
    case "$COMMAND" in
      initialize)
        while [[ $# -gt 0 ]]; do
          case "$1" in
            --admin)      append_value admin "$2";      shift 2 ;;
            --settlement) append_value settlement "$2"; shift 2 ;;
            *) echo "Unknown option: $1"; exit 1 ;;
          esac
        done ;;
      set_settlement)
        while [[ $# -gt 0 ]]; do
          case "$1" in
            --settlement) append_value settlement "$2"; shift 2 ;;
            *) echo "Unknown option: $1"; exit 1 ;;
          esac
        done ;;
      list_solvers | list_open_intents)
        while [[ $# -gt 0 ]]; do
          case "$1" in
            --start) append_value start "$2"; shift 2 ;;
            --limit) append_value limit "$2"; shift 2 ;;
            *) echo "Unknown option: $1"; exit 1 ;;
          esac
        done ;;
      list_intents_by_user)
        while [[ $# -gt 0 ]]; do
          case "$1" in
            --user)  append_value user "$2";  shift 2 ;;
            --start) append_value start "$2"; shift 2 ;;
            --limit) append_value limit "$2"; shift 2 ;;
            *) echo "Unknown option: $1"; exit 1 ;;
          esac
        done ;;
      get_fee_schedule | get_solver)
        while [[ $# -gt 0 ]]; do
          case "$1" in
            --solver) append_value solver "$2"; shift 2 ;;
            *) echo "Unknown option: $1"; exit 1 ;;
          esac
        done ;;
      get_intent)
        while [[ $# -gt 0 ]]; do
          case "$1" in
            --intent_id) append_value intent_id "$2"; shift 2 ;;
            *) echo "Unknown option: $1"; exit 1 ;;
          esac
        done ;;
      get_settlement | get_stats | get_protocol_health) ;;   # no arguments
      *) echo "Unknown intent_periphery function: $COMMAND"; usage_intent_periphery ;;
    esac ;;

  *)
    echo "Unknown contract: $CONTRACT"
    echo ""
    print_usage
    ;;
esac

# Execute the stellar CLI command
echo "Executing: ${STELLAR_CMD[*]}" >&2
exec "${STELLAR_CMD[@]}"
