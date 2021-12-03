#!/usr/bin/env node
/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

import "source-map-support/register";
import * as cdk from "@aws-cdk/core";
import { PullRequestCdnStack } from "../lib/smithy-rs/pull-request-cdn-stack";

const app = new cdk.App();

new PullRequestCdnStack(app, "smithy-rs-pull-request-cdn-stack", {});
