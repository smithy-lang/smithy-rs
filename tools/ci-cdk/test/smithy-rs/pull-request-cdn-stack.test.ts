/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import { App } from "aws-cdk-lib";
import { Template } from "aws-cdk-lib/assertions";

import { PullRequestCdnStack } from "../../lib/smithy-rs/pull-request-cdn-stack";
import { LEG_WORKFLOW } from "../../lib/smithy-rs/build-canary-stack";
import { PULL_REQUEST_BOT_WORKFLOW } from "../../lib/smithy-rs/workflows";
import { OidcProviderStack } from "../../lib/oidc-provider-stack";
import { SMITHY_RS } from "../../lib/github-repositories";
import { Statement, admits, claimsFor } from "../oidc-claims";

const ROLE_NAME = "smithy-rs-pull-request-github-oidc-role";

function synth(): Template {
    const app = new App();
    const oidcStack = new OidcProviderStack(app, "oidc-provider-stack", {});
    const stack = new PullRequestCdnStack(app, "smithy-rs-pull-request-cdn-stack", {
        githubActionsOidcProvider: oidcStack.githubActionsOidcProvider,
    });
    return Template.fromStack(stack);
}

/** The trust statements of the OIDC role, by exact name: the CDN creates roles of its own. */
function trust(): Statement[] {
    const roles = Object.values(synth().findResources("AWS::IAM::Role")).filter(
        (r) => String(r.Properties.RoleName) === ROLE_NAME,
    );
    expect(roles).toHaveLength(1);
    return roles[0].Properties.AssumeRolePolicyDocument.Statement;
}

/** This repository's claims for a job defined in `workflow`. */
const token = (workflow: string, ref = "refs/heads/main") => claimsFor(SMITHY_RS, ref, workflow);

/**
 * The pull request bot writes the codegen diff and doc preview a reviewer reads, so a job that can
 * assume this role can publish content under this repository's name. The role is conditioned on the
 * repository, which every job in it satisfies, so the workflow condition is what keeps the set to
 * the one workflow that needs it.
 */
test("the pull request bot can assume the role", () => {
    expect(admits(trust(), token(PULL_REQUEST_BOT_WORKFLOW))).toBe(true);
});

/**
 * A reusable workflow called from `ci-pr.yml` and `manual-pull-request-bot.yml`. The claim names the
 * file a job is defined in rather than the caller, so the callers must not need pinning -- and a
 * caller presenting its own name must not be admitted either.
 */
test("the role is pinned to the called workflow, not to its callers", () => {
    const statements = trust();
    expect(admits(statements, token(PULL_REQUEST_BOT_WORKFLOW, "refs/pull/5021/merge"))).toBe(true);
    for (const caller of ["ci-pr.yml", "manual-pull-request-bot.yml"]) {
        expect(admits(statements, token(caller))).toBe(false);
    }
});

/**
 * The case that motivated the condition: the build canary's legs resolve and build crates from the
 * open internet, in this same repository.
 */
test("no other workflow in this repository can assume the role", () => {
    const statements = trust();
    for (const workflow of [LEG_WORKFLOW, "post-publish-watch.yml", "ci.yml", "release.yml"]) {
        expect(workflow).not.toEqual(PULL_REQUEST_BOT_WORKFLOW);
        expect(admits(statements, token(workflow))).toBe(false);
    }
});
