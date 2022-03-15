#
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0.
#
ARG base_image=smithy-rs-build-image-original
FROM ${base_image} AS bare_base_image

ARG USER_ID
RUN useradd -l -u ${USER_ID} -G build -o -s /bin/bash localbuild;
USER localbuild
RUN /home/build/scripts/sanity-test
