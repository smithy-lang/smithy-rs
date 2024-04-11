#!/usr/bin/env node
/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

// This CDK app, in addition to provisioning necessary resources for CI checks
// and the canary, deploys a GihHub OIDC role to execute the canary from a CI
// in the `smithy-rs` repository.

import "source-map-support/register";
import { App } from "aws-cdk-lib";
import { PullRequestCdnStack } from "../../lib/smithy-rs/pull-request-cdn-stack";
import { CanaryStack, OidcProps } from "../../lib/canary-stack";
import { OidcProviderStack } from "../../lib/oidc-provider-stack";

const app = new App({});
const env = { region: "us-west-2" };

const oidcProviderStack = new OidcProviderStack(app, "oidc-provider-stack", {
    env,
});

const githubActionsOidcProvider = oidcProviderStack.githubActionsOidcProvider;

const oidcProps: OidcProps = {
    roleId: "smithy-rs",
    roleName: "smithy-rs-canary",
    roleGithubOrg: "smithy-lang",
    roleGithubRepo: "smithy-rs",
    provider: githubActionsOidcProvider,
};

new PullRequestCdnStack(app, "smithy-rs-pull-request-cdn-stack", {
    githubActionsOidcProvider: githubActionsOidcProvider,
    env,
});

new CanaryStack(app, "smithy-rs-canary-stack", {
    lambdaExecutionRole: "smithy-rs-canary-lambda-exec-role",
    oidcProps,
    env,
});
