#!/bin/bash
# Comprehensive benchmark script comparing handle_connection vs handle_connection_strategy
#
# This script automates:
# - Code switching between implementations
# - Load testing with oha
# - Flamegraph generation
# - Performance profiling with samply
# - Side-by-side comparison reports
#
# Usage:
#   ./bench_comparison.sh [duration_in_seconds]
#
# Example:
#   ./bench_comparison.sh 120  # Run for 2 minutes
#   ./bench_comparison.sh 30   # Run for 30 seconds

set -e

# Configuration
DEFAULT_DURATION=120  # 2 minutes
DURATION=${1:-$DEFAULT_DURATION}
CONCURRENCY="100"
URL="http://127.0.0.1:3000/"
SERVE_FILE="src/serve/mod.rs"
BACKUP_FILE="src/serve/mod.rs.backup"
RESULTS_DIR="/tmp/bench_results_$(date +%Y%m%d_%H%M%S)"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m' # No Color

# Progress indicators
CHECKMARK="${GREEN}✓${NC}"
CROSSMARK="${RED}✗${NC}"
ARROW="${BLUE}→${NC}"

echo -e "${BLUE}╔════════════════════════════════════════════════════════════╗${NC}"
echo -e "${BLUE}║  Comprehensive Serve Implementation Benchmark             ║${NC}"
echo -e "${BLUE}╚════════════════════════════════════════════════════════════╝${NC}"
echo ""
echo -e "${CYAN}Configuration:${NC}"
echo -e "  Duration:     ${DURATION}s"
echo -e "  Concurrency:  ${CONCURRENCY}"
echo -e "  URL:          ${URL}"
echo -e "  Results:      ${RESULTS_DIR}"
echo ""

# Create results directory
mkdir -p "$RESULTS_DIR"

# ============================================================================
# Prerequisites Check
# ============================================================================

# Check for python3
if ! command -v python3 &> /dev/null; then
    echo -e "${RED}Error: python3 is required but not installed${NC}"
    echo "Please install python3 to continue"
    exit 1
fi

# ============================================================================
# Tool Installation & Checks
# ============================================================================

echo -e "${BLUE}[1/9] Checking required tools...${NC}"

# Check and install oha
if ! command -v oha &> /dev/null; then
    echo -e "${YELLOW}  oha not found, installing...${NC}"
    cargo install oha
    echo -e "  ${CHECKMARK} oha installed"
else
    echo -e "  ${CHECKMARK} oha found"
fi

# Check and install cargo-flamegraph
if ! command -v flamegraph &> /dev/null; then
    echo -e "${YELLOW}  cargo-flamegraph not found, installing...${NC}"
    cargo install flamegraph
    echo -e "  ${CHECKMARK} cargo-flamegraph installed"
else
    echo -e "  ${CHECKMARK} cargo-flamegraph found"
fi

# Check and install samply
if ! command -v samply &> /dev/null; then
    echo -e "${YELLOW}  samply not found, installing...${NC}"
    cargo install samply
    echo -e "  ${CHECKMARK} samply installed"
else
    echo -e "  ${CHECKMARK} samply found"
fi

echo ""

# ============================================================================
# Code Switching Functions
# ============================================================================

backup_code() {
    cp "$SERVE_FILE" "$BACKUP_FILE"
}

restore_code() {
    if [ -f "$BACKUP_FILE" ]; then
        mv "$BACKUP_FILE" "$SERVE_FILE"
    fi
}

switch_to_strategy() {
    echo -e "${BLUE}[2/9] Verifying handle_connection_strategy is active...${NC}"
    # The code should already be using handle_connection_strategy
    # Just verify it's there
    if grep -q "handle_connection_strategy::<L, M, S, B, \$connection_strategy" "$SERVE_FILE"; then
        echo -e "  ${CHECKMARK} Strategy pattern already active"
    else
        echo -e "  ${YELLOW}Warning: Expected pattern not found, restoring from backup${NC}"
        restore_code
    fi
    echo ""
}

switch_to_branching() {
    echo -e "${BLUE}[6/9] Switching to handle_connection (branching)...${NC}"

    # Restore original first to ensure clean state
    restore_code

    # Create a temporary Python script to do the replacement
    cat > /tmp/switch_impl.py << 'PYTHON_SCRIPT'
import sys
import re

with open(sys.argv[1], 'r') as f:
    content = f.read()

# Pattern 1: Replace handle_connection_strategy with handle_connection (without graceful shutdown)
# This appears in the accept_loop macro around line 661
pattern1 = r'(handle_connection_strategy::<L, M, S, B, \$connection_strategy, WithoutGracefulShutdown>\(\s+&mut \$make_service,\s+io,\s+remote_addr,\s+\$hyper_builder\.as_ref\(\),\s+)WithoutGracefulShutdown::default\(\),'

replacement1 = r'\1true,\n                None,'

content = re.sub(pattern1, replacement1, content)

# Also replace the function name
content = content.replace(
    'handle_connection_strategy::<L, M, S, B, $connection_strategy, WithoutGracefulShutdown>(',
    'handle_connection::<L, M, S, B>('
)

# Pattern 2: Replace handle_connection_strategy with handle_connection (with graceful shutdown)
# This appears in the accept_loop_with_shutdown macro around line 685
pattern2 = r'(handle_connection_strategy::<L, M, S, B, \$connection_strategy, WithGracefulShutdown>\(\s+&mut \$make_service,\s+io,\s+remote_addr,\s+\$hyper_builder\.as_ref\(\),\s+)WithGracefulShutdown::new\(\$graceful\.watcher\(\)\),'

replacement2 = r'\1true,\n                        Some(&$graceful),'

content = re.sub(pattern2, replacement2, content, flags=re.MULTILINE)

# Also replace the function name
content = content.replace(
    'handle_connection_strategy::<L, M, S, B, $connection_strategy, WithGracefulShutdown>(',
    'handle_connection::<L, M, S, B>('
)

with open(sys.argv[1], 'w') as f:
    f.write(content)
PYTHON_SCRIPT

    # Run the Python script
    python3 /tmp/switch_impl.py "$SERVE_FILE"
    rm /tmp/switch_impl.py

    echo -e "  ${CHECKMARK} Switched to branching pattern"
    echo ""
}

# ============================================================================
# Benchmark Functions
# ============================================================================

wait_for_server() {
    local max_attempts=30
    local attempt=0

    while [ $attempt -lt $max_attempts ]; do
        if curl -s "$URL" > /dev/null 2>&1; then
            return 0
        fi
        sleep 1
        attempt=$((attempt + 1))
    done

    return 1
}

run_benchmark() {
    local impl_name=$1
    local step=$2

    echo -e "${BLUE}[${step}/8] Benchmarking ${impl_name}...${NC}"

    local output_dir="${RESULTS_DIR}/${impl_name}"
    mkdir -p "$output_dir"

    # Build the example
    echo -e "  ${ARROW} Building with --release..."
    cargo build --example serve_comparison --release > "${output_dir}/build.log" 2>&1

    if [ $? -ne 0 ]; then
        echo -e "  ${CROSSMARK} Build failed, check ${output_dir}/build.log"
        return 1
    fi
    echo -e "  ${CHECKMARK} Build successful"

    # Start server in background
    echo -e "  ${ARROW} Starting server..."
    cargo run --example serve_comparison --release > "${output_dir}/server.log" 2>&1 &
    SERVER_PID=$!

    # Wait for server to be ready
    if ! wait_for_server; then
        echo -e "  ${CROSSMARK} Server failed to start"
        kill $SERVER_PID 2>/dev/null || true
        return 1
    fi
    echo -e "  ${CHECKMARK} Server started (PID: ${SERVER_PID})"

    # Run load test with oha
    echo -e "  ${ARROW} Running load test (${DURATION}s with ${CONCURRENCY} connections)..."
    oha -z "${DURATION}s" -c "$CONCURRENCY" --no-tui "$URL" > "${output_dir}/oha_results.txt" 2>&1

    echo -e "  ${CHECKMARK} Load test complete"

    # Stop server
    kill $SERVER_PID 2>/dev/null || true
    wait $SERVER_PID 2>/dev/null || true

    echo ""
}

run_flamegraph() {
    local impl_name=$1
    local step=$2

    echo -e "${BLUE}[${step}/8] Collecting flamegraph for ${impl_name}...${NC}"

    local output_dir="${RESULTS_DIR}/${impl_name}"

    # Start server with flamegraph in background
    echo -e "  ${ARROW} Starting server with flamegraph profiling..."

    # Use cargo flamegraph which will profile the entire process
    cd "$(dirname "$0")"
    CARGO_PROFILE_RELEASE_DEBUG=true cargo flamegraph --example serve_comparison -o "${output_dir}/flamegraph.svg" -- > "${output_dir}/flamegraph_server.log" 2>&1 &
    FLAMEGRAPH_PID=$!

    # Wait for server to be ready
    sleep 5
    if ! wait_for_server; then
        echo -e "  ${CROSSMARK} Server with flamegraph failed to start"
        kill $FLAMEGRAPH_PID 2>/dev/null || true
        return 1
    fi

    echo -e "  ${CHECKMARK} Server with flamegraph started"
    echo -e "  ${ARROW} Running load test for flamegraph data..."

    # Run shorter load test for flamegraph (30 seconds or 25% of duration, whichever is larger)
    local flamegraph_duration=$((DURATION / 4))
    if [ $flamegraph_duration -lt 30 ]; then
        flamegraph_duration=30
    fi

    oha -z "${flamegraph_duration}s" -c "$CONCURRENCY" --no-tui "$URL" > /dev/null 2>&1

    # Stop flamegraph
    kill -INT $FLAMEGRAPH_PID 2>/dev/null || true
    wait $FLAMEGRAPH_PID 2>/dev/null || true

    echo -e "  ${CHECKMARK} Flamegraph saved to ${output_dir}/flamegraph.svg"
    echo ""
}

run_samply() {
    local impl_name=$1
    local step=$2

    echo -e "${BLUE}[${step}/8] Profiling with samply for ${impl_name}...${NC}"

    local output_dir="${RESULTS_DIR}/${impl_name}"

    # Build the binary first
    cargo build --example serve_comparison --release > /dev/null 2>&1

    local binary="target/release/examples/serve_comparison"

    echo -e "  ${ARROW} Starting server with samply profiling..."

    # Start samply in background
    samply record -o "${output_dir}/samply.json" "$binary" > "${output_dir}/samply_server.log" 2>&1 &
    SAMPLY_PID=$!

    # Wait for server
    sleep 5
    if ! wait_for_server; then
        echo -e "  ${CROSSMARK} Server with samply failed to start"
        kill $SAMPLY_PID 2>/dev/null || true
        return 1
    fi

    echo -e "  ${CHECKMARK} Server with samply started"
    echo -e "  ${ARROW} Running load test for profiling data..."

    # Run shorter load test for profiling (30 seconds or 25% of duration)
    local profile_duration=$((DURATION / 4))
    if [ $profile_duration -lt 30 ]; then
        profile_duration=30
    fi

    oha -z "${profile_duration}s" -c "$CONCURRENCY" --no-tui "$URL" > /dev/null 2>&1

    # Stop samply gracefully
    kill -INT $SAMPLY_PID 2>/dev/null || true
    wait $SAMPLY_PID 2>/dev/null || true

    echo -e "  ${CHECKMARK} Samply profile saved to ${output_dir}/samply.json"
    echo -e "  ${CYAN}  View with: samply load ${output_dir}/samply.json${NC}"
    echo ""
}

# ============================================================================
# Comparison Report Generation
# ============================================================================

generate_comparison() {
    echo -e "${BLUE}[9/9] Generating comparison report...${NC}"

    local report="${RESULTS_DIR}/comparison_report.txt"
    local strategy_results="${RESULTS_DIR}/strategy/oha_results.txt"
    local branching_results="${RESULTS_DIR}/branching/oha_results.txt"

    cat > "$report" << EOF
╔════════════════════════════════════════════════════════════════════════════╗
║                    BENCHMARK COMPARISON REPORT                             ║
╚════════════════════════════════════════════════════════════════════════════╝

Test Configuration:
  Duration:     ${DURATION}s
  Concurrency:  ${CONCURRENCY}
  URL:          ${URL}
  Date:         $(date)

════════════════════════════════════════════════════════════════════════════

STRATEGY PATTERN (handle_connection_strategy)
────────────────────────────────────────────────────────────────────────────
EOF

    if [ -f "$strategy_results" ]; then
        grep -E "Success rate:|Requests/sec:|Average:|Slowest:|Fastest:" "$strategy_results" >> "$report" || true
    else
        echo "Results not available" >> "$report"
    fi

    cat >> "$report" << EOF

════════════════════════════════════════════════════════════════════════════

BRANCHING PATTERN (handle_connection)
────────────────────────────────────────────────────────────────────────────
EOF

    if [ -f "$branching_results" ]; then
        grep -E "Success rate:|Requests/sec:|Average:|Slowest:|Fastest:" "$branching_results" >> "$report" || true
    else
        echo "Results not available" >> "$report"
    fi

    cat >> "$report" << EOF

════════════════════════════════════════════════════════════════════════════

GENERATED ARTIFACTS
────────────────────────────────────────────────────────────────────────────

Strategy Pattern:
  - Load test results:  ${RESULTS_DIR}/strategy/oha_results.txt
  - Flamegraph:         ${RESULTS_DIR}/strategy/flamegraph.svg
  - Samply profile:     ${RESULTS_DIR}/strategy/samply.json
  - Server logs:        ${RESULTS_DIR}/strategy/server.log

Branching Pattern:
  - Load test results:  ${RESULTS_DIR}/branching/oha_results.txt
  - Flamegraph:         ${RESULTS_DIR}/branching/flamegraph.svg
  - Samply profile:     ${RESULTS_DIR}/branching/samply.json
  - Server logs:        ${RESULTS_DIR}/branching/server.log

════════════════════════════════════════════════════════════════════════════

To view flamegraphs:
  open ${RESULTS_DIR}/strategy/flamegraph.svg
  open ${RESULTS_DIR}/branching/flamegraph.svg

To view samply profiles:
  samply load ${RESULTS_DIR}/strategy/samply.json
  samply load ${RESULTS_DIR}/branching/samply.json

════════════════════════════════════════════════════════════════════════════
EOF

    echo -e "  ${CHECKMARK} Comparison report generated"
    echo ""

    # Display the report
    cat "$report"
}

# ============================================================================
# Cleanup Handler
# ============================================================================

cleanup() {
    echo ""
    echo -e "${YELLOW}Cleaning up...${NC}"

    # Kill any remaining processes
    pkill -f "serve_comparison" 2>/dev/null || true

    # Restore original code
    restore_code

    echo -e "${CHECKMARK} Cleanup complete"
}

trap cleanup EXIT INT TERM

# ============================================================================
# Main Execution
# ============================================================================

# Backup original code
backup_code

# Test 1: Strategy Pattern
switch_to_strategy
run_benchmark "strategy" "3"
run_flamegraph "strategy" "4"
run_samply "strategy" "5"

# Test 2: Branching Pattern
switch_to_branching
run_benchmark "branching" "6"
run_flamegraph "branching" "7"
run_samply "branching" "8"

# Generate comparison report
generate_comparison

echo -e "${GREEN}╔════════════════════════════════════════════════════════════╗${NC}"
echo -e "${GREEN}║  Benchmark Complete!                                       ║${NC}"
echo -e "${GREEN}╚════════════════════════════════════════════════════════════╝${NC}"
echo ""
echo -e "${CYAN}Results directory: ${RESULTS_DIR}${NC}"
echo ""
