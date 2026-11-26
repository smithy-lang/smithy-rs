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
import { AWS_SDK_RUST } from "../../lib/github-repositories";

const app = new App();
const env = { region: "us-west-2" };

const oidcProviderStack = new OidcProviderStack(app, "oidc-provider-stack", {
    env,
});

// No `jobWorkflows`, where the `smithy-rs-canary` role in `bin/smithy-rs` pins two. The workflows
// that assume this role are in the SDK's repository, so a list here could not be checked against
// the files it names and a rename there would leave a role nobody can assume -- surfacing on
// whoever next deploys this stack. Not a gap: the role is conditioned on the SDK repository's id,
// so no token minted in `smithy-rs` satisfies it whatever workflow asks.
const oidcProps: OidcProps = {
    roleId: "aws-sdk-rust",
    roleName: "aws-sdk-rust-canary",
    roleGithubOrg: AWS_SDK_RUST.githubOrg,
    roleGithubRepo: AWS_SDK_RUST.githubRepo,
    roleGithubOrgId: AWS_SDK_RUST.githubOrgId,
    roleGithubRepoId: AWS_SDK_RUST.githubRepoId,
    provider: oidcProviderStack.githubActionsOidcProvider,
};

new CanaryStack(app, "aws-sdk-rust-canary-stack", {
    lambdaExecutionRole: "aws-sdk-rust-canary-lambda-exec-role",
    oidcProps,
    env,
});
