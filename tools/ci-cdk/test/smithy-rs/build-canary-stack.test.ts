/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import { App } from "aws-cdk-lib";
import { Template } from "aws-cdk-lib/assertions";
import {
    BuildCanaryStack,
    LEG_WORKFLOW,
    MAIN_REF,
    METRIC_NAMESPACE,
    ORCHESTRATOR_ENVIRONMENT,
    ORCHESTRATOR_WORKFLOWS,
    RELEASE_REFS,
    RELEASE_TAG_REFS,
} from "../../lib/smithy-rs/build-canary-stack";
import { OidcProviderStack } from "../../lib/oidc-provider-stack";
import { SMITHY_RS } from "../../lib/github-repositories";
import { CLAIM, Statement, admits, claimsFor, trustStatements } from "../oidc-claims";

function synth(): Template {
    const app = new App();
    const oidcStack = new OidcProviderStack(app, "oidc-provider-stack", {});
    const stack = new BuildCanaryStack(app, "build-canary-stack", {
        githubActionsOidcProvider: oidcStack.githubActionsOidcProvider,
    });
    return Template.fromStack(stack);
}

/** The inline policy statements attached to the role whose name ends in `suffix`. */
function policyFor(template: Template, suffix: string): Statement[] {
    const policies = Object.values(template.findResources("AWS::IAM::Policy")).filter((p) =>
        JSON.stringify(p.Properties.Roles).includes(suffix),
    );
    expect(policies).toHaveLength(1);
    return policies[0].Properties.PolicyDocument.Statement;
}

/** The trust condition of the role whose name ends in `suffix`. */
function trustFor(
    template: Template,
    suffix: string,
): Record<string, Record<string, string | string[]>> {
    // Role names are `smithy-rs-build-canary-<suffix>-github-oidc-role`, so match on containment.
    const roles = Object.values(template.findResources("AWS::IAM::Role")).filter((r) =>
        String(r.Properties.RoleName).includes(`-${suffix}-`),
    );
    expect(roles).toHaveLength(1);
    return roles[0].Properties.AssumeRolePolicyDocument.Statement[0].Condition;
}

const RELEASE_TAG = "refs/tags/release-2026-11-16";

/** A leg's claims, unless another workflow is named. */
const token = (ref: string, environment?: string, workflow: string = LEG_WORKFLOW) =>
    claimsFor(SMITHY_RS, ref, workflow, environment);

const actionsOf = (statements: Statement[]): string[] =>
    statements.flatMap((s) => (Array.isArray(s.Action) ? s.Action : [s.Action]));

test("it should create exactly two roles", () => {
    // Two, and not a third carved out of the orchestrator's permissions: any two roles split out
    // of them would accept identical tokens -- same repository, same environment, same refs --
    // which buys least privilege per step and no isolation. The leg role is separate because its
    // tokens are the numerous, least-trusted ones.
    const roles = synth().findResources("AWS::IAM::Role");
    expect(
        Object.values(roles)
            .map((r) => r.Properties.RoleName)
            .sort(),
    ).toEqual([
        "smithy-rs-build-canary-metrics-github-oidc-role",
        "smithy-rs-build-canary-orchestrator-github-oidc-role",
    ]);
});

test("the metrics role should only write metrics, and only to the canary's namespace", () => {
    const statements = policyFor(synth(), "metrics");

    expect(actionsOf(statements)).toEqual(["cloudwatch:PutMetricData"]);
    // `PutMetricData` has no resource-level permissions, so `*` is the only accepted value and the
    // namespace condition is the only bound available. Without it the role could write to any
    // namespace in the account.
    expect(statements[0].Resource).toBe("*");
    expect(statements[0].Condition).toEqual({
        StringEquals: { "cloudwatch:namespace": METRIC_NAMESPACE },
    });
});

test("the orchestrator role should only manage the canary's own alarms", () => {
    const statements = policyFor(synth(), "orchestrator").filter((s) =>
        actionsOf([s]).some((a) => a.includes("Alarm")),
    );

    expect(actionsOf(statements).sort()).toEqual([
        "cloudwatch:DeleteAlarms",
        "cloudwatch:DescribeAlarms",
        "cloudwatch:PutMetricAlarm",
    ]);

    // Scoped by alarm name. Unlike PutMetricData these actions do take resource-level permissions,
    // so there is no excuse for a `*` here.
    const resources = JSON.stringify(statements[0].Resource);
    expect(resources).not.toBe('"*"');
    expect(resources).toContain(":alarm:build-canary-*");
    expect(resources).toContain(":alarm:paging-selftest-*");
});

test("a leg must not be able to touch the alarms", () => {
    // The direction that matters. A leg's token is minted forty times in a release's matrix, in
    // jobs that build published crates off the open internet, so it must not be able to delete the
    // alarm watching it.
    //
    // The reverse is not a property: the orchestrator writes its own verdicts -- a failed
    // dispatch, an unchecked release -- so a leg's authority is a subset of its, deliberately.
    const template = synth();
    const metrics = actionsOf(policyFor(template, "metrics"));
    const orchestrator = actionsOf(policyFor(template, "orchestrator"));

    expect(metrics.some((a) => a.includes("Alarm"))).toBe(false);
    expect(metrics).toEqual(["cloudwatch:PutMetricData"]);

    for (const needed of ["cloudwatch:PutMetricAlarm", "cloudwatch:PutMetricData"]) {
        expect(orchestrator).toContain(needed);
    }
});

test("the orchestrator should be gated on the environment, and a leg must not be", () => {
    const template = synth();
    // The environment is the only thing a pull request's code running on `main` cannot declare,
    // which is why it and not the ref is what separates the orchestrator from everything else.
    expect(trustFor(template, "orchestrator").StringEquals[`${CLAIM}:environment`]).toBe(
        ORCHESTRATOR_ENVIRONMENT,
    );
    // Conditioning the leg role on an environment would require every leg in the matrix to declare
    // one. The release tag is what bounds it instead.
    expect(trustFor(template, "metrics").StringEquals).not.toHaveProperty(`${CLAIM}:environment`);
});

/**
 * The ref is what bounds who can write the canary's series. A namespace has no owner, so the
 * metrics role's namespace condition does not.
 */
test("the orchestrator may run from main, and a leg only from a release tag", () => {
    const template = synth();

    // The orchestrator needs `main` for the paging self-test and for a hand-dispatched watch, and
    // the environment is what makes that safe.
    expect(trustFor(template, "orchestrator").StringLike[`${CLAIM}:ref`]).toEqual(RELEASE_REFS);
    expect(RELEASE_REFS).toContain(MAIN_REF);

    // The leg role has no `main` at all, in one statement, so there is nothing to qualify.
    const metrics = trustStatements(template, "metrics");
    expect(metrics).toHaveLength(1);
    expect(metrics[0].Condition?.StringLike[`${CLAIM}:ref`]).toEqual(RELEASE_TAG_REFS);
    expect(RELEASE_TAG_REFS).not.toContain(MAIN_REF);

    // A date-shaped tag matches; a stray `release-test` must not.
    expect(RELEASE_REFS).toContain("refs/tags/release-????-??-??");
    expect(RELEASE_REFS.some((r) => r.includes("release-*"))).toBe(false);
});

test("a leg on a release tag can assume the metrics role", () => {
    expect(admits(trustStatements(synth(), "metrics"), token(RELEASE_TAG))).toBe(true);
});

test("the orchestrator on main, inside its environment, can assume its role", () => {
    const claims = token(MAIN_REF, ORCHESTRATOR_ENVIRONMENT, ORCHESTRATOR_WORKFLOWS[0]);
    expect(admits(trustStatements(synth(), "orchestrator"), claims)).toBe(true);
});

test("nothing on main can assume the leg role, environment or not", () => {
    const statements = trustStatements(synth(), "metrics");
    expect(admits(statements, token(MAIN_REF))).toBe(false);
    expect(admits(statements, token(MAIN_REF, ORCHESTRATOR_ENVIRONMENT))).toBe(false);
});

/**
 * `manual-canary.yml` -- dispatched by a maintainer, or called by `fork-canary.yml` from
 * `pull_request_target`, whose ref is the default branch -- and `manual-pull-request-bot.yml` run
 * on `main` and execute a pull request's checked-out code in jobs holding `id-token: write`. That
 * code can mint a token with exactly these claims.
 *
 * Admitted, one `Red` datapoint carrying the current release's `Tag` would trip that release's
 * alarm and page the oncall over a pull request. The code cannot declare an environment to get
 * around this: under `pull_request_target` and `workflow_call` the workflow definition comes from
 * the base ref, not from the pull request.
 */
/**
 * A release tag says *where* a job ran, not *which workflow* it was.
 *
 * `manual-canary.yml` and `manual-pull-request-bot.yml` are `workflow_dispatch`, hold
 * `id-token: write`, and check out a pull request's commit -- and a dispatch can be launched from a
 * release tag. Without `job_workflow_ref` the token such a job mints satisfies every other
 * condition on the metrics role, so a pull request's code could write a `Red` datapoint and page
 * the oncall. The ref condition alone cannot tell the two apart.
 */
test("another workflow at a release tag cannot assume either role", () => {
    const template = synth();
    for (const workflow of ["manual-canary.yml", "manual-pull-request-bot.yml", "ci-pr.yml"]) {
        const claims = token(RELEASE_TAG, undefined, workflow);
        expect(admits(trustStatements(template, "metrics"), claims)).toBe(false);
        expect(
            admits(
                trustStatements(template, "orchestrator"),
                token(RELEASE_TAG, ORCHESTRATOR_ENVIRONMENT, workflow),
            ),
        ).toBe(false);
    }
    // And the workflow that is supposed to reach it still does.
    expect(
        admits(trustStatements(template, "metrics"), token(RELEASE_TAG, undefined, LEG_WORKFLOW)),
    ).toBe(true);
});

/**
 * The role that deletes alarms must be out of reach of the forty tokens minted per release.
 *
 * Two conditions keep it there and either alone would do, which is why both are asserted: the
 * orchestrator requires its environment, which a leg does not declare, and it requires its own
 * workflows, which a leg is not one of. Asserting the outcome rather than the mechanism is what
 * makes this survive a change to either -- an `alsoTrust` alternative granting release tags, for
 * instance, drops the environment requirement for that alternative and would otherwise pass
 * unnoticed.
 */
test("a leg's token cannot assume the orchestrator role", () => {
    const statements = trustStatements(synth(), "orchestrator");
    for (const ref of [RELEASE_TAG, MAIN_REF]) {
        for (const environment of [undefined, ORCHESTRATOR_ENVIRONMENT]) {
            expect(admits(statements, token(ref, environment, LEG_WORKFLOW))).toBe(false);
        }
    }
});

test("a job on main running a pull request's code cannot assume the metrics role", () => {
    expect(admits(trustStatements(synth(), "metrics"), token(MAIN_REF))).toBe(false);
});

test("no role should be assumable from any other ref, with or without the environment", () => {
    const template = synth();
    for (const role of ["metrics", "orchestrator"]) {
        const statements = trustStatements(template, role);
        for (const ref of [
            "refs/heads/some/feature-branch",
            "refs/pull/5021/merge",
            // Release-shaped but not date-shaped, which is the stray tag the patterns exclude.
            "refs/tags/release-test",
        ]) {
            expect(admits(statements, token(ref))).toBe(false);
            expect(admits(statements, token(ref, ORCHESTRATOR_ENVIRONMENT))).toBe(false);
        }
    }
});

test("it should output everything that has to be copied into a secret by hand", () => {
    const outputs = synth().findOutputs("*");
    const descriptions = Object.values(outputs).map((o) => o.Description);

    expect(descriptions).toContain("Set as the BUILD_CANARY_METRICS_ROLE_ARN repository secret");
    expect(descriptions).toContain(
        "Set as the BUILD_CANARY_ORCHESTRATOR_ROLE_ARN repository secret",
    );
});
