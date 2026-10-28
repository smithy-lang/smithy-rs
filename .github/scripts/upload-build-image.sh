#!/bin/bash
#
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0
set -eux

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
ECR_IMAGE="${AWS_ACCOUNT_ID}.dkr.ecr.${AWS_REGION}.amazonaws.com/${ECR_REPOSITORY}:${TAG_NAME}"

echo "Logging in to Amazon ECR..."
aws ecr get-login-password --region ${AWS_REGION} | ${OCI_EXE} login --username AWS --password-stdin ${AWS_ACCOUNT_ID}.dkr.ecr.${AWS_REGION}.amazonaws.com

if [ $? -ne 0 ]; then
    echo "Error: Failed to login to ECR"
    exit 1
fi

echo "Tagging image as: ${ECR_IMAGE}"
# Publish the base image before acquire-build-image adds the runner-specific user.
# Publishing the derived build image would accumulate layers on every CI run.
${OCI_EXE} tag "smithy-rs-base-image:${TAG_NAME}" "${ECR_IMAGE}"

if [ $? -ne 0 ]; then
    echo "Error: Failed to tag the image"
    exit 1
fi

if [[ "${DRY_RUN}" == "true" ]]; then
  echo "Dry run enabled - skipping push to ECR"
  exit 0
else
  echo "Pushing image to ECR..."
  ${OCI_EXE} push ${ECR_IMAGE}

  if [ $? -ne 0 ]; then
      echo "Error: Failed to push the image"
      exit 1
  fi

  echo "Successfully uploaded image to ECR: ${ECR_IMAGE}"
fi
