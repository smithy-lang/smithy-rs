# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0

# Shared settings for the multi-protocol A/B fuzz scripts. Sourced by every script here.
# Override any variable by exporting it before running a script.

AB_FUZZ_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
: "${CURRENT_REPO:=$(cd "$AB_FUZZ_DIR/../.." && pwd)}"      # multi-protocol schema server
# realpath: the generated server crates embed the canonical runtime path, and a second
# spelling of the same path (e.g. /home vs /local/home) is a cargo lockfile package collision.
: "${BASELINE_REPO:=$(realpath "$HOME/smithy-rs-latest")}"  # one legacy server per protocol
: "${FUZZ_ROOT:=$AB_FUZZ_DIR/artifacts}"                    # generated code, driver, AFL state, replays
: "${SUITES:=multiprotocol pokemon}"
# Each after server serves exactly one protocol, with schema serde enabled.
: "${CASES:=aws-json-10 aws-json-11 rest-json1 rest-xml rpcv2-cbor}"
export AB_FUZZ_DIR CURRENT_REPO BASELINE_REPO FUZZ_ROOT SUITES CASES

# The driver is built from the current checkout into FUZZ_ROOT, leaving ~/.cargo/bin alone. cargo-afl
# keeps its AFL++ build in the XDG data directory; generate.sh builds one under FUZZ_ROOT, so the
# campaign does not depend on (or change) the one in ~/.local/share.
export XDG_DATA_HOME="$FUZZ_ROOT/xdg"
export PATH="$FUZZ_ROOT/driver/bin:$HOME/.cargo/bin:$PATH"
export RUSTUP_TOOLCHAIN="$(sed -n 's/channel = "\(.*\)"/\1/p' "$CURRENT_REPO/rust-toolchain.toml")"

# Compare the deserialized input and the response, ignoring differences a client cannot observe
# (`content-length`, JSON member order, CBOR encoding choices, XML member order). Set to the empty
# string, 0, false, no, or off for a byte-for-byte comparison of responses only.
: "${SMITHY_FUZZ_SEMANTIC_COMPARE=1}"
# Retired suppression flags cannot turn a failing seed or routing difference into agreement.
unset SMITHY_FUZZ_IGNORE_UNROUTED AFL_IGNORE_SEED_PROBLEMS
for v in SMITHY_FUZZ_SEMANTIC_COMPARE; do
  ab_fuzz_flag_value="${!v}"
  ab_fuzz_flag_value="${ab_fuzz_flag_value#"${ab_fuzz_flag_value%%[![:space:]]*}"}"
  ab_fuzz_flag_value="${ab_fuzz_flag_value%"${ab_fuzz_flag_value##*[![:space:]]}"}"
  ab_fuzz_flag_value="$(printf '%s' "$ab_fuzz_flag_value" | tr '[:upper:]' '[:lower:]')"
  case "$ab_fuzz_flag_value" in
    ""|0|false|no|off) unset "$v" ;;
    *) export "$v=1" ;;
  esac
done
unset ab_fuzz_flag_value

# AFL settings for a shared workstation.
export AFL_SKIP_CPUFREQ=1
export AFL_I_DONT_CARE_ABOUT_MISSING_CRASHES=1
export AFL_NO_UI=1
export AFL_FORKSRV_INIT_TMOUT=120000
export AFL_AUTORESUME=1

# The protocol directory and harness directory of a case, relative to $FUZZ_ROOT/gen/<suite>.
case_harness() {
  case "$1" in
    *@*)
      # `<protocol>@<variant>` compares the single-protocol before server against the
      # `<variant>` after server (bunch-json, bunch-xml, full, full-xml), generated with
      # MP_FUZZ_VARIANTS under the protocol's directory as `ab-harness-<variant>`.
      local proto="${1%%@*}"
      case "$proto" in rest-xml*) proto=rest-xml ;; esac
      echo "$proto/ab-harness-${1##*@}" ;;
    rest-xml-nojson) echo "rest-xml/ab-harness-xml" ;;
    *) echo "$1/ab-harness" ;;
  esac
}
