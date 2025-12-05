#!/bin/bash

set -e

echo "Running comprehensive Rust crate tests..."

# Core tests
echo "=== Core tests ==="
cargo test
cargo test --all-features
cargo test --doc
cargo test --doc --all-features

# Compilation checks
echo "=== Compilation checks ==="
cargo check --all-targets
cargo check --all-targets --all-features
cargo clippy --all-targets --all-features
cargo doc --no-deps --all-features

# No default features
echo "=== No default features ==="
cargo test --no-default-features


# Individual features
echo "=== Individual features ==="
echo "Testing feature: event-stream"
cargo check --no-default-features --features event-stream
cargo test --no-default-features --features event-stream
echo "Testing feature: rt-tokio"
cargo check --no-default-features --features rt-tokio
cargo test --no-default-features --features rt-tokio

# Cargo hack tests (requires: cargo install cargo-hack)
echo "=== Cargo hack tests (exhaustive) ==="
if command -v cargo-hack &> /dev/null; then
    echo "Running cargo hack --each-feature..."
    cargo hack check --each-feature --no-dev-deps
    cargo hack test --each-feature

    # Optional: test all feature combinations (can be slow!)
    # Uncomment the following lines if you want exhaustive testing:
    # echo "Running cargo hack --feature-powerset (this may take a while)..."
    # cargo hack test --feature-powerset --depth 2
else
    echo "⚠️  cargo-hack not found. Install with: cargo install cargo-hack"
    echo "   Skipping exhaustive feature combination testing."
fi

echo "All tests completed successfully!"
