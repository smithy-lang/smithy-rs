#
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0
#

# This is the base Docker build image used by CI

ARG base_image=public.ecr.aws/amazonlinux/amazonlinux:2
ARG rust_stable_version=1.62.1

FROM ${base_image} AS bare_base_image
RUN yum -y updateinfo

# Get Ubuntu packages
RUN yum -y install build-essential curl
RUN yum -y install \
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
    shadow-utils;

RUN yum -y update
# Get Rust
RUN curl https://sh.rustup.rs -sSf | bash -s -- -y
RUN echo 'source $HOME/.cargo/env' >> $HOME/.bashrc

CMD bash main.sh