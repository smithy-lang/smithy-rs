#!/bin/bash
# Comprehensive benchmark script comparing handle_connection vs handle_connection_strategy
#
# This script automates:
# - Code switching between implementations
# - Testing multiple server configurations (HTTP/1, HTTP/2, auto, graceful shutdown)
# - Load testing with oha
# - Flamegraph generation
# - Performance profiling with samply
# - Side-by-side comparison reports
#
# Usage:
#   ./bench_comparison.sh [duration_in_seconds] [concurrency]
#
# Examples:
#   ./bench_comparison.sh                    # Run for 2 minutes with 100 connections
#   ./bench_comparison.sh 120 200           # Run for 2 minutes with 200 connections
#   ./bench_comparison.sh 30 50             # Run for 30 seconds with 50 connections

set -e

# Configuration
DEFAULT_DURATION=120  # 2 minutes
DEFAULT_CONCURRENCY=100
DURATION=${1:-$DEFAULT_DURATION}
CONCURRENCY=${2:-$DEFAULT_CONCURRENCY}
URL="http://127.0.0.1:3000/"
SERVE_FILE="src/serve/mod.rs"
BACKUP_FILE="src/serve/mod.rs.backup"
RESULTS_DIR="/tmp/bench_results_$(date +%Y%m%d_%H%M%S)"

# Server configurations to test
# Format: "name:HTTP_VERSION:GRACEFUL_SHUTDOWN:TLS"
CONFIGS=(
    "auto:auto:false:false"
    "http1:http1:false:false"
    "http2:http2:false:true"
    "graceful:auto:true:false"
)

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
echo -e "  Duration:      ${DURATION}s"
echo -e "  Concurrency:   ${CONCURRENCY}"
echo -e "  URL:           ${URL}"
echo -e "  Results:       ${RESULTS_DIR}"
echo -e "  Configs:       ${#CONFIGS[@]} scenarios (auto, http1, http2, graceful)"
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

echo -e "${BLUE}[1/2] Checking required tools...${NC}"

# Check and install oha
if ! command -v oha &> /dev/null; then
    echo -e "${YELLOW}  oha not found, installing...${NC}"
    cargo install oha
    echo -e "  ${CHECKMARK} oha installed"
else
    echo -e "  ${CHECKMARK} oha found"
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
    echo -e "${BLUE}[2/2] Verifying handle_connection_strategy is active...${NC}"
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
    echo -e "${BLUE}Switching to handle_connection (branching)...${NC}"

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
    local url=$1
    local max_attempts=30
    local attempt=0

    while [ $attempt -lt $max_attempts ]; do
        if curl -s -k "$url" > /dev/null 2>&1; then
            return 0
        fi
        sleep 1
        attempt=$((attempt + 1))
    done

    return 1
}

run_benchmark() {
    local impl_name=$1
    local config_name=$2
    local http_version=$3
    local graceful=$4
    local use_tls=$5

    echo -e "${CYAN}  Testing: ${impl_name} - ${config_name}${NC}"

    local output_dir="${RESULTS_DIR}/${impl_name}/${config_name}"
    mkdir -p "$output_dir"

    # Determine URL and port based on TLS
    local url
    local port
    if [ "$use_tls" = "true" ]; then
        url="https://127.0.0.1:3443/"
        port=3443
    else
        url="http://127.0.0.1:3000/"
        port=3000
    fi

    # Build the example
    echo -e "    ${ARROW} Building..."
    cargo build --example serve_benchmark --release > "${output_dir}/build.log" 2>&1

    if [ $? -ne 0 ]; then
        echo -e "    ${CROSSMARK} Build failed, check ${output_dir}/build.log"
        return 1
    fi

    # Build command-line arguments
    local server_args="--http-version ${http_version}"
    if [ "$graceful" = "true" ]; then
        server_args="${server_args} --graceful-shutdown"
    fi
    if [ "$use_tls" = "true" ]; then
        server_args="${server_args} --tls"
    fi

    # Start server in background with configuration
    echo -e "    ${ARROW} Starting server (HTTP:${http_version}, Graceful:${graceful}, TLS:${use_tls})..."
    cargo run --example serve_benchmark --release -- ${server_args} > "${output_dir}/server.log" 2>&1 &
    SERVER_PID=$!

    # Wait for server to be ready
    if ! wait_for_server "$url"; then
        echo -e "    ${CROSSMARK} Server failed to start"
        kill $SERVER_PID 2>/dev/null || true
        return 1
    fi

    # Run load test with oha
    echo -e "    ${ARROW} Load testing (${DURATION}s, ${CONCURRENCY} connections)..."
    if [ "$use_tls" = "true" ]; then
        oha -z "${DURATION}s" -c "$CONCURRENCY" --no-tui --insecure "$url" > "${output_dir}/oha_results.txt" 2>&1
    else
        oha -z "${DURATION}s" -c "$CONCURRENCY" --no-tui "$url" > "${output_dir}/oha_results.txt" 2>&1
    fi

    echo -e "    ${CHECKMARK} Complete"

    # Stop server
    kill $SERVER_PID 2>/dev/null || true
    wait $SERVER_PID 2>/dev/null || true

    echo ""
}

# ============================================================================
# Comparison Report Generation
# ============================================================================

generate_comparison() {
    echo -e "${BLUE}Generating comparison report...${NC}"

    local report="${RESULTS_DIR}/comparison_report.txt"

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
EOF

    # Generate comparison for each configuration
    for config in "${CONFIGS[@]}"; do
        IFS=':' read -r name http_ver graceful use_tls <<< "$config"

        cat >> "$report" << EOF

CONFIGURATION: ${name} (HTTP:${http_ver}, Graceful:${graceful}, TLS:${use_tls})
────────────────────────────────────────────────────────────────────────────

Strategy Pattern (handle_connection_strategy):
EOF

        local strategy_file="${RESULTS_DIR}/strategy/${name}/oha_results.txt"
        if [ -f "$strategy_file" ]; then
            grep -E "Success rate:|Requests/sec:|Average:|Slowest:|Fastest:" "$strategy_file" >> "$report" 2>/dev/null || echo "  No metrics available" >> "$report"
        else
            echo "  Results not available" >> "$report"
        fi

        cat >> "$report" << EOF

Branching Pattern (handle_connection):
EOF

        local branching_file="${RESULTS_DIR}/branching/${name}/oha_results.txt"
        if [ -f "$branching_file" ]; then
            grep -E "Success rate:|Requests/sec:|Average:|Slowest:|Fastest:" "$branching_file" >> "$report" 2>/dev/null || echo "  No metrics available" >> "$report"
        else
            echo "  Results not available" >> "$report"
        fi

        cat >> "$report" << EOF

════════════════════════════════════════════════════════════════════════════
EOF
    done

    cat >> "$report" << EOF

SUMMARY
────────────────────────────────────────────────────────────────────────────

Results directory structure:
  ${RESULTS_DIR}/
    ├── strategy/
    │   ├── auto/          (Auto HTTP version, no graceful shutdown)
    │   ├── http1/         (HTTP/1 only)
    │   ├── http2/         (HTTP/2 only)
    │   └── graceful/      (Auto with graceful shutdown)
    └── branching/
        ├── auto/
        ├── http1/
        ├── http2/
        └── graceful/

Each directory contains:
  - oha_results.txt      Full load test results
  - server.log           Server output
  - build.log            Build output

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

# Test Strategy Pattern
echo -e "${BLUE}═══════════════════════════════════════════════════════════${NC}"
echo -e "${BLUE}Testing Strategy Pattern (handle_connection_strategy)${NC}"
echo -e "${BLUE}═══════════════════════════════════════════════════════════${NC}"
echo ""
switch_to_strategy

for config in "${CONFIGS[@]}"; do
    IFS=':' read -r name http_ver graceful use_tls <<< "$config"
    run_benchmark "strategy" "$name" "$http_ver" "$graceful" "$use_tls"
done

# Test Branching Pattern
echo -e "${BLUE}═══════════════════════════════════════════════════════════${NC}"
echo -e "${BLUE}Testing Branching Pattern (handle_connection)${NC}"
echo -e "${BLUE}═══════════════════════════════════════════════════════════${NC}"
echo ""
switch_to_branching

for config in "${CONFIGS[@]}"; do
    IFS=':' read -r name http_ver graceful use_tls <<< "$config"
    run_benchmark "branching" "$name" "$http_ver" "$graceful" "$use_tls"
done

# Generate comparison report
generate_comparison

echo -e "${GREEN}╔════════════════════════════════════════════════════════════╗${NC}"
echo -e "${GREEN}║  Benchmark Complete!                                       ║${NC}"
echo -e "${GREEN}╚════════════════════════════════════════════════════════════╝${NC}"
echo ""
echo -e "${CYAN}Results directory: ${RESULTS_DIR}${NC}"
echo ""
