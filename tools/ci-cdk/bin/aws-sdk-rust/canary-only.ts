#!/usr/bin/env node
/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

// This CDK app sets up the absolute minimum set of resources to successfully
// execute the canary with. However, this one is used by our internal CI only.
// Use canary-only.ts in the sibling smithy-rs directory instead.

import "source-map-support/register";
import { App } from "aws-cdk-lib";
import { CanaryStack } from "../../lib/canary-stack";

const app = new App();
const env = { region: "us-west-2" };

new CanaryStack(app, "aws-sdk-rust-canary-stack", {
    lambdaExecutionRole: "aws-sdk-rust-canary-lambda-exec-role",
    env,
});
