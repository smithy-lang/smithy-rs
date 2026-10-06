# Shared settings for the multi-protocol A/B fuzz scripts. Sourced by every script here.
# Override any variable by exporting it before running a script.

AB_FUZZ_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
: "${CURRENT_REPO:=$(cd "$AB_FUZZ_DIR/../.." && pwd)}"      # multi-protocol schema server
: "${BASELINE_REPO:=$HOME/smithy-rs-latest}"                # one legacy server per protocol
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
# string for a byte-for-byte comparison of responses only.
: "${SMITHY_FUZZ_SEMANTIC_COMPARE=1}"
# Accept the differences that follow from serving several protocols: any answer to a request the
# single-protocol server does not route to an operation (the multi-protocol server may route it
# through another protocol), and the multi-protocol server's protocol-neutral 404 for a request no
# protocol claims when the single-protocol server rejects that request too, with any 4xx, before a
# handler runs. A request whose handler the single-protocol server invokes must still reach the
# same handler. Set to the empty string to compare all of those as well.
: "${SMITHY_FUZZ_IGNORE_UNROUTED=}"
for v in SMITHY_FUZZ_SEMANTIC_COMPARE SMITHY_FUZZ_IGNORE_UNROUTED; do
  if [ -n "${!v}" ]; then export "$v"; else unset "$v"; fi
done

# AFL settings for a shared workstation.
export AFL_SKIP_CPUFREQ=1
export AFL_I_DONT_CARE_ABOUT_MISSING_CRASHES=1
export AFL_NO_UI=1
export AFL_FORKSRV_INIT_TMOUT=120000
export AFL_AUTORESUME=1
# Many fuzzers starting at once push some seeds past AFL's dry-run limit, which aborts the main
# fuzzer. Skip those seeds instead.
export AFL_IGNORE_SEED_PROBLEMS=1

# The protocol directory and harness directory of a case, relative to $FUZZ_ROOT/gen/<suite>.
case_harness() {
  case "$1" in
    rest-xml-nojson) echo "rest-xml/ab-harness-xml" ;;
    *) echo "$1/ab-harness" ;;
  esac
}
