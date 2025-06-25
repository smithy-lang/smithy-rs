#
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0
#

# The base image adds a non-root `build` user and creates a home directory for it,
# but this `build` user's user ID might not match the user ID that is executing the
# Docker image.
#
# This `add-local-user.dockerfile` image creates a `localuser` with a group ID that
# matches the group ID of the user that will execute it. That way, any build artifacts,
# Gradle caches, or Cargo/Rust caches will not require root access to manipulate after
# Docker execution is completed.

FROM smithy-rs-base-image:local AS bare_base_image

ARG USER_ID
USER root
RUN useradd -l -u ${USER_ID} -G build -o -s /bin/bash localbuild || \
    { exit_code=$?; [ $exit_code -eq 9 ] && echo "User localbuild already exists, continuing..." || \
    { echo "Failed to create user with error code: $exit_code"; exit $exit_code; }; }
USER localbuild
RUN /home/build/sanity-test
