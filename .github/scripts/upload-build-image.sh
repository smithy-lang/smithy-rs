#!/bin/bash
#
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0
set -euo pipefail
set -x

if [ $# -ne 1 ]; then
    echo "Error: Tag name is required"
    echo "Usage: $0 <tag-name>"
    exit 1
fi

# Set OCI executor - default to docker if not set
: "${OCI_EXE:=docker}"

DRY_RUN=${DRY_RUN:-false}
TAG_NAME=$1
AWS_REGION="us-west-2"
AWS_ACCOUNT_ID="686190543447"
ECR_REPOSITORY="smithy-rs-build-image"
SOURCE_IMAGE="smithy-rs-base-image:${TAG_NAME}"
ECR_IMAGE="${AWS_ACCOUNT_ID}.dkr.ecr.${AWS_REGION}.amazonaws.com/${ECR_REPOSITORY}:${TAG_NAME}"
IMAGE_ROLE_LABEL="org.smithy-rs.image-role"
EXPECTED_IMAGE_ROLE="base"

# The remotely cached ci-<hash> image is the reusable base built from
# tools/ci-build/Dockerfile. The runner-specific smithy-rs-build-image:latest
# adds a local UID and must never become the base of the next build.
if ! ${OCI_EXE} image inspect "${SOURCE_IMAGE}" >/dev/null; then
    echo "Error: Base image does not exist locally: ${SOURCE_IMAGE}"
    exit 1
fi

IMAGE_ROLE="$(${OCI_EXE} image inspect --format "{{ index .Config.Labels \"${IMAGE_ROLE_LABEL}\" }}" "${SOURCE_IMAGE}")"
if [[ "${IMAGE_ROLE}" != "${EXPECTED_IMAGE_ROLE}" ]]; then
    echo "Error: Refusing to publish ${SOURCE_IMAGE}: expected ${IMAGE_ROLE_LABEL}=${EXPECTED_IMAGE_ROLE}, got ${IMAGE_ROLE:-<unset>}"
    exit 1
fi

echo "Logging in to Amazon ECR..."
aws ecr get-login-password --region "${AWS_REGION}" | ${OCI_EXE} login --username AWS --password-stdin "${AWS_ACCOUNT_ID}.dkr.ecr.${AWS_REGION}.amazonaws.com"

echo "Tagging base image ${SOURCE_IMAGE} as ${ECR_IMAGE}"
${OCI_EXE} tag "${SOURCE_IMAGE}" "${ECR_IMAGE}"

if [[ "${DRY_RUN}" == "true" ]]; then
    echo "Dry run enabled - skipping push to ECR"
else
    echo "Pushing base image to ECR..."
    ${OCI_EXE} push "${ECR_IMAGE}"
    echo "Successfully uploaded base image to ECR: ${ECR_IMAGE}"
fi
