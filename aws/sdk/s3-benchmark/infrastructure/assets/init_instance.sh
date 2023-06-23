#!/bin/bash
#
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0
#

echo "init_instance.sh starting"
set -eux

BENCHMARK_ZIP_PATH="${1}"
echo "benchmark zip path: ${BENCHMARK_ZIP_PATH}"

sudo yum -y install \
    autoconf \
    automake \
    gcc \
    gcc-c++ \
    git \
    make \
    openssl-devel

# Install Rustup and Rust
curl https://static.rust-lang.org/rustup/archive/1.24.3/x86_64-unknown-linux-gnu/rustup-init --output rustup-init
echo "3dc5ef50861ee18657f9db2eeb7392f9c2a6c95c90ab41e45ab4ca71476b4338 rustup-init" | sha256sum --check
chmod +x rustup-init
./rustup-init -y --no-modify-path --profile minimal --default-toolchain 1.67.1
rm rustup-init

# Verify install
source "${HOME}/.cargo/env"
rustc --version
cargo --version

# Compile the benchmark
unzip -d benchmark "${BENCHMARK_ZIP_PATH}"
cd benchmark
cargo build --release
