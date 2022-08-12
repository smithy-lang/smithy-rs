#
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0
#

# This is the base Docker build image used by CI

ARG base_image=public.ecr.aws/amazonlinux/amazonlinux:2
FROM ${base_image} AS bare_base_image

#
# Node Installation Stage
#
FROM bare_base_image AS install_node
ARG node_version=v16.14.0
ENV DEST_PATH=/opt/nodejs \
    PATH=/opt/nodejs/bin:${PATH}
RUN yum -y updateinfo && \
    yum -y install \
        ca-certificates \
        curl \
        tar \
        xz && \
    yum clean all
WORKDIR /root
RUN set -eux; \
    ARCHITECTURE=""; \
    if [[ "$(uname -m)" == "aarch64" || "$(uname -m)" == "arm64" ]]; then \
        curl "https://nodejs.org/dist/${node_version}/node-${node_version}-linux-arm64.tar.xz" --output node.tar.xz; \
        echo "5a6e818c302527a4b1cdf61d3188408c8a3e4a1bbca1e3f836c93ea8469826ce  node.tar.xz" | sha256sum --check; \
        ARCHITECTURE="arm64"; \
    else \
        curl "https://nodejs.org/dist/${node_version}/node-${node_version}-linux-x64.tar.xz" --output node.tar.xz; \
        echo "0570b9354959f651b814e56a4ce98d4a067bf2385b9a0e6be075739bc65b0fae  node.tar.xz" | sha256sum --check; \
        ARCHITECTURE="x64"; \
    fi; \
    mkdir -p "${DEST_PATH}"; \
    tar -xJvf node.tar.xz -C "${DEST_PATH}"; \
    mv "${DEST_PATH}/node-${node_version}-linux-${ARCHITECTURE}/"* "${DEST_PATH}"; \
    rmdir "${DEST_PATH}"/node-${node_version}-linux-${ARCHITECTURE}; \
    rm node.tar.xz; \
    node --version

#
# Rust & Tools Installation Stage
#
FROM bare_base_image AS install_rust
ARG rust_stable_version=1.58.1
ARG rust_nightly_version=nightly-2022-07-25
ARG cargo_udeps_version=0.1.29
ARG cargo_hack_version=0.5.14
ARG cargo_minimal_versions_version=0.1.4
ENV RUSTUP_HOME=/opt/rustup \
    CARGO_HOME=/opt/cargo \
    PATH=/opt/cargo/bin/:${PATH} \
    CARGO_INCREMENTAL=0
WORKDIR /root
RUN yum -y updateinfo && \
    yum -y install \
        autoconf \
        automake \
        binutils \
        ca-certificates \
        curl \
        gcc \
        gcc-c++ \
        git \
        make \
        openssl-devel \
        pkgconfig && \
    yum clean all
RUN set -eux; \
    if [[ "$(uname -m)" == "aarch64" || "$(uname -m)" == "arm64" ]]; then \
        curl https://static.rust-lang.org/rustup/archive/1.24.3/aarch64-unknown-linux-gnu/rustup-init --output rustup-init; \
        echo "32a1532f7cef072a667bac53f1a5542c99666c4071af0c9549795bbdb2069ec1 rustup-init" | sha256sum --check; \
    else \
        curl https://static.rust-lang.org/rustup/archive/1.24.3/x86_64-unknown-linux-gnu/rustup-init --output rustup-init; \
        echo "3dc5ef50861ee18657f9db2eeb7392f9c2a6c95c90ab41e45ab4ca71476b4338 rustup-init" | sha256sum --check; \
    fi; \
    chmod +x rustup-init; \
    ./rustup-init -y --no-modify-path --profile minimal --default-toolchain ${rust_stable_version}; \
    rm rustup-init; \
    rustup --version; \
    rustup component add rustfmt; \
    rustup component add clippy; \
    rustup toolchain install ${rust_nightly_version} --component clippy; \
    cargo --version; \
    cargo +${rust_nightly_version} --version;
COPY . tools/
# when `checkout_smithy_rs_tools` is set to true, this commit will be checked out
ARG smithy_rs_commit_hash=main
# If the `checkout_smithy_rs_tools` arg is set to true, then the Dockerfile will acquire the tools
# source code by checking out awslabs/smithy-rs/main rather than copying them from the local directory.
ARG checkout_smithy_rs_tools=false
RUN set -eux; \
    cargo +${rust_nightly_version} install cargo-udeps --locked --version ${cargo_udeps_version}; \
    cargo install cargo-hack --locked --version ${cargo_hack_version}; \
    cargo install cargo-minimal-versions --version ${cargo_minimal_versions_version}; \
    if [[ "${checkout_smithy_rs_tools}" == "true" ]]; then \
        git clone https://github.com/awslabs/smithy-rs.git; \
        cd smithy-rs; \
        git checkout ${smithy_rs_commit_hash}; \
    fi; \
    cargo install --locked --path tools/publisher; \
    cargo install --locked --path tools/cargo-check-external-types; \
    cargo install --locked --path tools/changelogger; \
    cargo install --locked --path tools/crate-hasher; \
    cargo install --locked --path tools/sdk-lints; \
    cargo install --locked --path tools/sdk-sync; \
    cargo install --locked --path tools/sdk-versioner; \
    chmod g+rw -R /opt/cargo/registry

#
# Final image
#
FROM bare_base_image AS final_image
ARG rust_stable_version=1.58.1
ARG rust_nightly_version=nightly-2022-07-25
RUN set -eux; \
    yum -y updateinfo; \
    yum -y install \
        ca-certificates \
        gcc \
        git \
        java-11-amazon-corretto-headless \
        make \
        openssl-devel \
        pkgconfig \
        python3 \
        python3-devel \
        python3-pip \
        shadow-utils; \
    yum clean all; \
    rm -rf /var/cache/yum; \
    groupadd build; \
    useradd -m -g build build; \
    chmod 775 /home/build;
COPY --chown=build:build --from=install_node /opt/nodejs /opt/nodejs
COPY --chown=build:build --from=install_rust /opt/cargo /opt/cargo
COPY --chown=build:build --from=install_rust /opt/rustup /opt/rustup
ENV PATH=/opt/cargo/bin:/opt/nodejs/bin:$PATH \
    CARGO_HOME=/opt/cargo \
    RUSTUP_HOME=/opt/rustup \
    JAVA_HOME=/usr/lib/jvm/java-11-amazon-corretto.x86_64 \
    GRADLE_USER_HOME=/home/build/.gradle \
    RUST_STABLE_VERSION=${rust_stable_version} \
    RUST_NIGHTLY_VERSION=${rust_nightly_version} \
    CARGO_INCREMENTAL=0 \
    RUSTDOCFLAGS="-D warnings" \
    RUSTFLAGS="-D warnings" \
    LANG=en_US.UTF-8 \
    LC_ALL=en_US.UTF-8
# SMITHY_RS_DOCKER_BUILD_IMAGE indicates to build scripts that they are being built inside of the Docker build image.
# This is used primarily by the `build.gradle.kts` files in choosing how to execute build tools. If inside the image,
# they will assume the tools are on the PATH, but if outside of the image, they will `cargo run` the tools.
ENV SMITHY_RS_DOCKER_BUILD_IMAGE=1
RUN npm install -g diff2html-cli@5.1.11 && pip3 install --no-cache-dir uvloop==0.16.0
WORKDIR /home/build
COPY ci-build/scripts/sanity-test /home/build/sanity-test
RUN /home/build/sanity-test
