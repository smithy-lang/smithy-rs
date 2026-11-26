/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import { readFileSync } from "fs";
import { join } from "path";

import { App } from "aws-cdk-lib";
import { Template } from "aws-cdk-lib/assertions";
import { CanaryStack } from "../lib/canary-stack";
import { LEG_WORKFLOW, METRIC_NAMESPACE } from "../lib/smithy-rs/build-canary-stack";
import { CANARY_WORKFLOW, MANUAL_CANARY_WORKFLOW } from "../lib/smithy-rs/workflows";
import { OidcProviderStack } from "../lib/oidc-provider-stack";
import { SMITHY_RS } from "../lib/github-repositories";
import { Statement as TrustStatement, admits, claimsFor } from "./oidc-claims";

/** The SDK canary's own namespace, hardcoded in `canary-runner/src/run.rs`. */
const SDK_CANARY_NAMESPACE = "aws-sdk-rust-canary";

interface Statement {
    Action: string | string[];
    Effect: string;
    Resource?: string | string[];
    Condition?: Record<string, Record<string, string>>;
}

function synth(): Template {
    const app = new App();
    const oidcStack = new OidcProviderStack(app, "oidc-provider-stack", {});
    const stack = new CanaryStack(app, "smithy-rs-canary-stack", {
        lambdaExecutionRole: "smithy-rs-canary-lambda-exec-role",
        oidcProps: {
            roleId: "smithy-rs",
            roleName: OIDC_ROLE_NAME,
            roleGithubOrg: SMITHY_RS.githubOrg,
            roleGithubRepo: SMITHY_RS.githubRepo,
            roleGithubOrgId: SMITHY_RS.githubOrgId,
            roleGithubRepoId: SMITHY_RS.githubRepoId,
            provider: oidcStack.githubActionsOidcProvider,
            jobWorkflows: PINNED_WORKFLOWS,
        },
    });
    return Template.fromStack(stack);
}

/** What `bin/smithy-rs/ci-cdk.ts` deploys, mirrored so the synth here matches the real one. */
const OIDC_ROLE_NAME = "smithy-rs-canary";
const PINNED_WORKFLOWS = [CANARY_WORKFLOW, MANUAL_CANARY_WORKFLOW];

/**
 * The trust statements of the OIDC role, found by exact name.
 *
 * Not by containment: the Lambda execution role is `smithy-rs-canary-lambda-exec-role`, so a
 * `-canary-` match would find two roles.
 */
function oidcTrustStatements(): TrustStatement[] {
    const roles = Object.values(synth().findResources("AWS::IAM::Role")).filter(
        (r) => String(r.Properties.RoleName) === `${OIDC_ROLE_NAME}-github-oidc-role`,
    );
    expect(roles).toHaveLength(1);
    return roles[0].Properties.AssumeRolePolicyDocument.Statement;
}

/** This repository's claims for a job at `ref` defined in `workflow`. */
const token = (workflow: string, ref = "refs/heads/main") => claimsFor(SMITHY_RS, ref, workflow);

/** Every statement on the Lambda execution role, which is what the canaries themselves run as. */
function lambdaStatements(): Statement[] {
    const policies = Object.values(synth().findResources("AWS::IAM::Policy")).filter((p) =>
        JSON.stringify(p.Properties.Roles).includes("lambdaexecutionrole"),
    );
    expect(policies).toHaveLength(1);
    return policies[0].Properties.PolicyDocument.Statement as Statement[];
}
/** Every `PutMetricData` statement on the role this repository's CI assumes. */
function metricStatements(): Statement[] {
    const policies = Object.values(synth().findResources("AWS::IAM::Policy")).filter((p) =>
        JSON.stringify(p.Properties.Roles).includes("smithyrsoidcrole"),
    );
    expect(policies).toHaveLength(1);
    return (policies[0].Properties.PolicyDocument.Statement as Statement[]).filter((s) =>
        JSON.stringify(s.Action).includes("cloudwatch:PutMetricData"),
    );
}

test("this role may write only to its own namespace", () => {
    // The property, not a particular mechanism for it. `manual-canary.yml` hands this role's
    // credentials to a pull request's checked-out code, so what has to hold is that the code
    // cannot write anywhere else -- the build canary's namespace above all, since a datapoint
    // there pages an oncall.
    const statements = metricStatements();
    expect(statements.filter((s) => s.Effect === "Deny")).toEqual([]);

    const allow = statements.filter((s) => s.Effect === "Allow");
    expect(allow).toHaveLength(1);
    expect(allow[0].Condition).toEqual({
        StringEquals: { "cloudwatch:namespace": SDK_CANARY_NAMESPACE },
    });

    // An allow conditioned on one namespace cannot reach another, so naming the build canary's is
    // belt and braces -- but it is the one that pages, and this is what would catch a later
    // widening of the condition.
    expect(allow[0].Condition?.StringEquals["cloudwatch:namespace"]).not.toBe(METRIC_NAMESPACE);
});

/**
 * Equal namespaces would both switch off this canary's own metrics and let a pull request's code,
 * which holds this role's credentials in `manual-canary.yml`, write the build canary's series.
 */
test("the two canaries must not share a namespace", () => {
    expect(METRIC_NAMESPACE).not.toEqual(SDK_CANARY_NAMESPACE);
});

/**
 * The EC2 grant is the union of two mutually exclusive builds, and has to stay that way.
 *
 * `DescribeRegions` belongs to the benchmark, behind `#[cfg(feature = "lambda-benchmark")]`.
 * `DescribeSpotPriceHistory` and `DescribeVpcs` belong to the paginator canary, which runs in the
 * default set. A change that narrows the list to whichever build it was written for leaves the
 * other with a role that cannot make its calls -- and because the deployed policy only changes when
 * the stack is deployed, the break lands on whoever next deploys rather than on whoever caused it.
 */
test("the lambda role keeps every EC2 action the canaries call", () => {
    const ec2 = lambdaStatements().filter((s) => JSON.stringify(s.Action).includes("ec2:"));
    const actions = ec2.flatMap((s) => (Array.isArray(s.Action) ? s.Action : [s.Action])).sort();
    expect(actions).toEqual([
        "ec2:DescribeRegions",
        "ec2:DescribeSpotPriceHistory",
        "ec2:DescribeVpcs",
    ]);
});

/**
 * This role grants Lambda create/invoke/delete, and the repository alone does not identify who may
 * take it: every job holding `id-token: write` presents the same `repository_id`. The build
 * canary's matrix is the case that motivated this -- forty jobs resolving and building crates from
 * the open internet -- but the condition is about any job added later just as much.
 */
test("only the canary's own workflows can assume the role", () => {
    const statements = oidcTrustStatements();
    for (const workflow of PINNED_WORKFLOWS) {
        expect(admits(statements, token(workflow))).toBe(true);
    }
});

test("no other workflow in this repository can assume the role", () => {
    const statements = oidcTrustStatements();
    for (const workflow of [
        LEG_WORKFLOW,
        "post-publish-watch.yml",
        "pull-request-bot.yml",
        "ci-pr.yml",
        "release.yml",
    ]) {
        expect(PINNED_WORKFLOWS).not.toContain(workflow);
        expect(admits(statements, token(workflow))).toBe(false);
    }
});

/**
 * A workflow pinned by the synth above but not by the app is a condition that exists only in this
 * file. The props are duplicated here rather than imported, because importing the app entry point
 * would run it.
 */
test("the deployed role is pinned to those same workflows", () => {
    const source = readFileSync(join(__dirname, "../bin/smithy-rs/ci-cdk.ts"), "utf8");
    expect(source).toContain("jobWorkflows: [CANARY_WORKFLOW, MANUAL_CANARY_WORKFLOW]");
});
