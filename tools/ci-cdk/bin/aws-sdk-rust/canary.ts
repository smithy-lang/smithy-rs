#!/usr/bin/env node
/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

// This CDK app, in addition to provisioning necessary resources for the canary,
// deploys a GihHub OIDC role to execute it from a CI in the `aws-sdk-rust` repository.

import "source-map-support/register";
import { App } from "aws-cdk-lib";
import { CanaryStack, OidcProps } from "../../lib/canary-stack";
import { OidcProviderStack } from "../../lib/oidc-provider-stack";

const app = new App();
const env = { region: "us-west-2" };

const oidcProviderStack = new OidcProviderStack(app, "oidc-provider-stack", {
    env,
});

const oidcProps: OidcProps = {
    roleId: "aws-sdk-rust",
    roleName: "aws-sdk-rust-canary",
    roleGithubOrg: "awslabs",
    roleGithubRepo: "aws-sdk-rust",
    provider: oidcProviderStack.githubActionsOidcProvider,
};

new CanaryStack(app, "aws-sdk-rust-canary-stack", {
    lambdaExecutionRole: "aws-sdk-rust-canary-lambda-exec-role",
    oidcProps,
    env,
});
