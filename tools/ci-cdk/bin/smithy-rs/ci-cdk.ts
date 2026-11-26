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
import { CANARY_WORKFLOW, MANUAL_CANARY_WORKFLOW } from "../../lib/smithy-rs/workflows";

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
    // The two workflows that assume this role: `ci.yml` runs the canary on a pull request, and
    // `manual-canary.yml` is the maintainer-triggered and fork entry point. Both hold
    // `id-token: write` while a pull request's code is checked out, which is this role's purpose
    // and not what the condition is for -- it is for every *other* job in the repository that
    // holds `id-token: write`, the build canary's matrix included.
    jobWorkflows: [CANARY_WORKFLOW, MANUAL_CANARY_WORKFLOW],
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

// The post-publish build canary's two roles. A separate stack so it can be deployed on its own,
// because it depends only on the OIDC provider.
//
// Deploy it as `deploy --exclusively smithy-rs-build-canary-stack`. Not `npm run deploy`, which
// passes `--all` and would also roll the pull-request CDN and the SDK canary; and not a bare
// `deploy <name>` either, because CDK includes a stack's dependencies and the `Fn::ImportValue` of
// the provider makes `oidc-provider-stack` one of them. `--exclusively` is what limits the change
// to this stack, and the provider is already deployed — the roles the existing workflows assume
// could not work otherwise.
//
// Every role these three stacks deploy conditions on `job_workflow_ref`, and no local check can
// exercise it: if GitHub does not set that claim for these jobs, or sets it in a shape the patterns
// do not match, the role becomes unassumable. That fails closed -- nothing is admitted wrongly --
// and it surfaces as a credentials error on the first run that needs the role, not at deploy time.
// So the first dispatch of `build-canary.yml` is part of the verification rather than a formality,
// and so is each check below.
//
// `smithy-rs-canary-stack` needs deploying too, also `--exclusively`, and that one is the risk.
// It confines the SDK canary's role to the SDK canary's own metric namespace — until it is
// deployed that role's `PutMetricData` allow has no namespace condition, and a pull request's code
// running in `manual-canary.yml` holds its credentials, so it could forge a verdict here. That
// deploy also restores the EC2 actions the paginator canary calls, which the policy in
// `canary-stack.ts` had narrowed away. But it rewrites a *live* role's trust policy too, because
// `GitHubOidcRole` is shared: `smithy-rs-canary-github-oidc-role` gains exact
// `repository_owner_id` and `repository_id` conditions it has never had, and a `job_workflow_ref`
// pin to the two workflows that assume it. Every pull request's canary job assumes that role.
// Dispatching `manual-canary.yml` on a pull request straight afterwards exercises one of the two;
// `ci.yml` is the other, and the next pull request covers it. Both are paths the new conditions
// would break.
//
// `smithy-rs-pull-request-cdn-stack` carries the *same* trust rewrite on
// `smithy-rs-pull-request-github-oidc-role`, down to the `job_workflow_ref` pin, so it carries the
// same risk and needs the same check.
// Its check is cheaper, though: the role is assumed in `pull-request-bot.yml`, which is a
// `workflow_call` reachable from `manual-pull-request-bot.yml` *and* from `ci-pr.yml` on every
// pull request. So the next pull request after that deploy exercises it without anyone dispatching
// anything — and if the conditions are wrong, the doc preview and codegen diff fail on every pull
// request in the repository, not just on a bot run somebody asked for. Watch the next one.
//
// Order: the release-tag ruleset enforcing, which is a prerequisite rather than a step and is why
// `RELEASE_REFS` means anything; then this stack; then `smithy-rs-canary-stack` verified; then
// `smithy-rs-pull-request-cdn-stack`, verified by the next pull request; then the secrets. The
// roles are inert until the secrets exist and nothing pages until the alarm is armed, so the
// deploys are independent — what must not happen is paging secrets being set while the SDK
// canary's role can still write the build canary's namespace.
new BuildCanaryStack(app, "smithy-rs-build-canary-stack", {
    githubActionsOidcProvider: githubActionsOidcProvider,
    env,
});
