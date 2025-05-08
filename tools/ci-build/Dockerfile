#
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0
#

# This is the base Docker build image used by CI

ARG base_image=public.ecr.aws/amazonlinux/amazonlinux:2023
ARG rust_stable_version=1.82.0
ARG rust_nightly_version=nightly-2024-10-17

FROM ${base_image} AS bare_base_image
RUN yum -y updateinfo

FROM bare_base_image as musl_toolchain
RUN yum -y install --allowerasing tar gzip gcc make
RUN curl https://musl.libc.org/releases/musl-1.2.5.tar.gz -o musl-1.2.5.tar.gz \
    && ls \
    && tar xvzf musl-1.2.5.tar.gz \
    && (cd musl-1.2.5 && ./configure && make install)

#
# Rust & Tools Installation Stage
#
FROM bare_base_image AS install_rust
ARG rust_stable_version
ARG rust_nightly_version
ENV RUSTUP_HOME=/opt/rustup \
    CARGO_HOME=/opt/cargo \
    PATH=/opt/cargo/bin/:${PATH} \
    CARGO_INCREMENTAL=0 \
    JAVA_HOME=/usr/lib/jvm/java-17-amazon-corretto.x86_64 \
    GRADLE_USER_HOME=/home/build/.gradle

WORKDIR /root
RUN yum -y install --allowerasing \
        autoconf \
        automake \
        binutils \
        ca-certificates \
        cmake \
        curl \
        gcc \
        gcc-c++ \
        git \
        java-17-amazon-corretto-headless \
        make \
        openssl-devel \
        perl \
        pkgconfig \
        tar \
        xz && \
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
    rustup target add x86_64-unknown-linux-musl; \
    rustup target add wasm32-unknown-unknown; \
    # NOTE: wasm32-wasi target is still valid in MSRV 1.81, when we update to 1.84 or above we _should_ be able to remove this target in favor of just wasip1
    rustup target add wasm32-wasi; \
    rustup target add wasm32-wasip1; \
    cargo --version; \
    cargo +${rust_nightly_version} --version;

FROM install_rust AS local_tools
ARG rust_nightly_version
COPY . tools/ci-build
# when `checkout_smithy_rs_tools` is set to true, this commit will be checked out
ARG smithy_rs_commit_hash=main
# If the `checkout_smithy_rs_tools` arg is set to true, then the Dockerfile will acquire the tools
# source code by checking out smithy-lang/smithy-rs/main rather than copying them from the local directory.
ARG checkout_smithy_rs_tools=false
RUN set -eux; \
    if [[ "${checkout_smithy_rs_tools}" == "true" ]]; then \
        git clone https://github.com/smithy-lang/smithy-rs.git; \
        cd smithy-rs; \
        git checkout ${smithy_rs_commit_hash}; \
        # Run the gradle wrapper with no args to download the gradle binary and cache it in the image
        ./gradlew; \
    fi; \
    cargo install --locked --path tools/ci-build/changelogger; \
    cargo install --locked --path tools/ci-build/crate-hasher; \
    cargo install --locked --path tools/ci-build/difftags; \
    cargo install --locked --path tools/ci-build/publisher; \
    cargo install --locked --path tools/ci-build/runtime-versioner; \
    cargo install --locked --path tools/ci-build/sdk-lints; \
    cargo install --locked --path tools/ci-build/sdk-lockfiles; \
    cargo install --locked --path tools/ci-build/sdk-versioner; \
    chmod g+rw -R /opt/cargo/registry

FROM install_rust AS cargo_deny
ARG cargo_deny_version=0.16.4
RUN cargo install cargo-deny --locked --version ${cargo_deny_version}

FROM install_rust AS cargo_udeps
ARG cargo_udeps_version=0.1.52
ARG rust_nightly_version
RUN cargo +${rust_nightly_version} install cargo-udeps --locked --version ${cargo_udeps_version}

FROM install_rust AS cargo_hack
ARG cargo_hack_version=0.6.27
RUN cargo install cargo-hack --locked --version ${cargo_hack_version}

FROM install_rust AS cargo_minimal_versions
ARG cargo_minimal_versions_version=0.1.27
RUN cargo install cargo-minimal-versions --locked --version ${cargo_minimal_versions_version}

FROM install_rust AS cargo_check_external_types
ARG cargo_check_external_types_version=0.1.13
RUN cargo install cargo-check-external-types --locked --version ${cargo_check_external_types_version}

FROM install_rust AS maturin
ARG maturin_version=1.5.1
RUN cargo install maturin --locked --version ${maturin_version}

FROM install_rust AS wasm_pack
ARG wasm_pack_version=0.13.1
RUN cargo install wasm-pack --locked --version ${wasm_pack_version}

FROM install_rust AS wasmtime
ARG cargo_wasmtime_version=28.0.0
ARG rust_nightly_version
RUN cargo install wasmtime-cli --features="component-model" --locked --version ${cargo_wasmtime_version}

FROM install_rust AS cargo_component
ARG cargo_component_version=0.20.0
ARG rust_nightly_version
RUN cargo +${rust_nightly_version} install cargo-component --locked --version ${cargo_component_version}

FROM install_rust AS cargo_semver_checks
ARG cargo_semver_checks_version=0.36.0
RUN cargo install cargo-semver-checks --locked --version ${cargo_semver_checks_version}

FROM install_rust AS cargo_mdbook
ARG cargo_mdbook_version=0.4.37
RUN cargo install mdbook --locked --version ${cargo_mdbook_version}

FROM install_rust AS cargo_mdbook_mermaid
ARG cargo_mdbook_mermaid_version=0.13.0
RUN cargo install mdbook-mermaid --locked --version ${cargo_mdbook_mermaid_version}

#
# Final image
#
# `clang` needed by SDK examples for `libxlsxwriter-sys`
FROM bare_base_image AS final_image
ARG rust_stable_version
ARG rust_nightly_version
RUN set -eux; \
    yum -y install --allowerasing \
        bc \
        ca-certificates \
        clang \
        gcc \
        git \
        glibc-langpack-en \
        go \
        java-17-amazon-corretto-headless \
        make \
        openssl-devel \
        pkgconfig \
        python3 \
        python3-devel \
        python3-pip \
        shadow-utils \
        cmake \
        tar \
        unzip; \
    yum clean all; \
    rm -rf /var/cache/yum; \
    groupadd build; \
    useradd -m -g build build; \
    chmod 775 /home/build;
RUN set -eux; cd /tmp; curl 'https://awscli.amazonaws.com/awscli-exe-linux-x86_64.zip' -o awscliv2.zip && unzip awscliv2.zip && ./aws/install
COPY --chown=build:build --from=local_tools /opt/cargo /opt/cargo
COPY --chown=build:build --from=cargo_deny /opt/cargo/bin/cargo-deny /opt/cargo/bin/cargo-deny
COPY --chown=build:build --from=cargo_udeps /opt/cargo/bin/cargo-udeps /opt/cargo/bin/cargo-udeps
COPY --chown=build:build --from=cargo_hack /opt/cargo/bin/cargo-hack /opt/cargo/bin/cargo-hack
COPY --chown=build:build --from=cargo_minimal_versions /opt/cargo/bin/cargo-minimal-versions /opt/cargo/bin/cargo-minimal-versions
COPY --chown=build:build --from=cargo_check_external_types /opt/cargo/bin/cargo-check-external-types /opt/cargo/bin/cargo-check-external-types
COPY --chown=build:build --from=maturin /opt/cargo/bin/maturin /opt/cargo/bin/maturin
COPY --chown=build:build --from=wasm_pack /opt/cargo/bin/wasm-pack /opt/cargo/bin/wasm-pack
COPY --chown=build:build --from=wasmtime /opt/cargo/bin/wasmtime /opt/cargo/bin/wasmtime
COPY --chown=build:build --from=cargo_component /opt/cargo/bin/cargo-component /opt/cargo/bin/cargo-component
COPY --chown=build:build --from=install_rust /opt/rustup /opt/rustup
COPY --chown=build:build --from=cargo_semver_checks /opt/cargo/bin/cargo-semver-checks /opt/cargo/bin/cargo-semver-checks
COPY --chown=build:build --from=cargo_mdbook /opt/cargo/bin/mdbook /opt/cargo/bin/mdbook
COPY --chown=build:build --from=cargo_mdbook_mermaid /opt/cargo/bin/mdbook-mermaid /opt/cargo/bin/mdbook-mermaid
COPY --chown=build:build --from=musl_toolchain /usr/local/musl/ /usr/local/musl/
ENV PATH=$PATH:/usr/local/musl/bin/
ENV PATH=/opt/cargo/bin:$PATH \
    CARGO_HOME=/opt/cargo \
    RUSTUP_HOME=/opt/rustup \
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
RUN pip3 install --no-cache-dir mypy==0.991
WORKDIR /home/build
# RUSTUP_TOOLCHAIN takes precedence over everything except `+<toolchain>` args. This will allow us to ignore the toolchain
# file during CI, avoiding issues during Rust version upgrades.
ENV RUSTUP_TOOLCHAIN=${rust_stable_version}
COPY sanity-test /home/build/sanity-test
RUN /home/build/sanity-test
