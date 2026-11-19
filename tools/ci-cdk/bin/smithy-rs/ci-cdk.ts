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
import { BuildCanaryStack } from "../../lib/smithy-rs/build-canary-stack";
import { CanaryStack, OidcProps } from "../../lib/canary-stack";
import { OidcProviderStack } from "../../lib/oidc-provider-stack";
import { SMITHY_RS } from "../../lib/github-repositories";

const app = new App({});
const env = { region: "us-west-2" };

const oidcProviderStack = new OidcProviderStack(app, "oidc-provider-stack", {
    env,
});

const githubActionsOidcProvider = oidcProviderStack.githubActionsOidcProvider;

const oidcProps: OidcProps = {
    roleId: "smithy-rs",
    roleName: "smithy-rs-canary",
    roleGithubOrg: SMITHY_RS.githubOrg,
    roleGithubRepo: SMITHY_RS.githubRepo,
    roleGithubOrgId: SMITHY_RS.githubOrgId,
    roleGithubRepoId: SMITHY_RS.githubRepoId,
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

// The post-publish build canary's three roles and its record bucket. A separate stack so it can be
// deployed on its own, because it depends only on the OIDC provider.
//
// Deploy it as `deploy --exclusively smithy-rs-build-canary-stack`. Not `npm run deploy`, which
// passes `--all` and would also roll the pull-request CDN and the SDK canary; and not a bare
// `deploy <name>` either, because CDK includes a stack's dependencies and the `Fn::ImportValue` of
// the provider makes `oidc-provider-stack` one of them. `--exclusively` is what limits the change
// to this stack, and the provider is already deployed — the roles the existing workflows assume
// could not work otherwise.
//
// `smithy-rs-canary-stack` needs deploying too, also `--exclusively`, and that one is the risk.
// It carries the deny that keeps the SDK canary's role out of this canary's metric namespace —
// without it, a pull request's code running in `manual-canary.yml` holds that role's credentials
// and its `PutMetricData` allow has no namespace condition, so it could forge a verdict here. But
// the same deploy also rewrites a *live* role's trust policy, because `GitHubOidcRole` is shared:
// `smithy-rs-canary-github-oidc-role` gains exact `repository_owner_id` and `repository_id`
// conditions it has never had, and every pull request's canary job assumes it. Verify by
// dispatching `manual-canary.yml` on a pull request straight afterwards; that is the path the new
// conditions would break.
//
// Order: this stack, then the ruleset active, then `smithy-rs-canary-stack` verified, then the
// secrets. The roles are inert until the secrets exist and nothing pages until the alarm is armed,
// so the two deploys are independent — what must not happen is paging secrets being set while the
// deny is still missing. The pull-request CDN stack's identical trust change can go out whenever.
new BuildCanaryStack(app, "smithy-rs-build-canary-stack", {
    githubActionsOidcProvider: githubActionsOidcProvider,
    env,
});
