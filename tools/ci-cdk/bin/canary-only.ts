#!/usr/bin/env node
/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

// This CDK app sets up the absolute minimum set of resources to succesfully
// execute the canary with.

import "source-map-support/register";
import { App } from "aws-cdk-lib";
import { CanaryStack } from "../lib/aws-sdk-rust/canary-stack";

const app = new App();

new CanaryStack(app, "aws-sdk-rust-canary-stack", {});
