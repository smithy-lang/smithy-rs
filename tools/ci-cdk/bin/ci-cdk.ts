#!/usr/bin/env node
/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import "source-map-support/register";
import { App } from "aws-cdk-lib";
import { PullRequestCdnStack } from "../lib/smithy-rs/pull-request-cdn-stack";
import { CanaryStack } from "../lib/aws-sdk-rust/canary-stack";
import { OidcProviderStack } from "../lib/oidc-provider-stack";

const app = new App();

const oidcProviderStack = new OidcProviderStack(app, "oidc-provider-stack", {});

new PullRequestCdnStack(app, "smithy-rs-pull-request-cdn-stack", {
    githubActionsOidcProvider: oidcProviderStack.githubActionsOidcProvider,
});

new CanaryStack(app, "aws-sdk-rust-canary-stack", {
    githubActionsOidcProvider: oidcProviderStack.githubActionsOidcProvider,
});
