/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import { App } from "aws-cdk-lib";
import { Template } from "aws-cdk-lib/assertions";
import {
    ALARM_ENVIRONMENT,
    BuildCanaryStack,
    METRIC_NAMESPACE,
} from "../../lib/smithy-rs/build-canary-stack";
import { OidcProviderStack } from "../../lib/oidc-provider-stack";

const CLAIM = "token.actions.githubusercontent.com";

interface Statement {
    Action: string | string[];
    Resource?: string | string[];
    Condition?: Record<string, Record<string, string | string[]>>;
}

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

const actionsOf = (statements: Statement[]): string[] =>
    statements.flatMap((s) => (Array.isArray(s.Action) ? s.Action : [s.Action]));

test("it should create exactly two roles", () => {
    const roles = synth().findResources("AWS::IAM::Role");
    expect(Object.values(roles).map((r) => r.Properties.RoleName).sort()).toEqual([
        "smithy-rs-build-canary-alarms-github-oidc-role",
        "smithy-rs-build-canary-metrics-github-oidc-role",
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

test("the alarm role should only manage the canary's own alarms", () => {
    const statements = policyFor(synth(), "alarms");

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

test("the two permission sets should be disjoint, neither a superset of the other", () => {
    // The property the split exists for. If the alarm role could write metrics it could fake a
    // verdict; if the metrics role could touch alarms, then every leg -- and anything else able to
    // assume it -- could delete the alarm watching it.
    const template = synth();
    const metrics = new Set(actionsOf(policyFor(template, "metrics")));
    const alarms = new Set(actionsOf(policyFor(template, "alarms")));

    const overlap = [...metrics].filter((a) => alarms.has(a));
    expect(overlap).toEqual([]);
    expect([...alarms].some((a) => a.includes("PutMetricData"))).toBe(false);
    expect([...metrics].some((a) => a.includes("Alarm"))).toBe(false);
});

test("only the alarm role should be gated on the GitHub environment", () => {
    const template = synth();

    // The environment is the only thing distinguishing the two jobs: both run in this repository
    // from the same release tag, so their tokens are otherwise identical.
    expect(trustFor(template, "alarms").StringEquals[`${CLAIM}:environment`]).toBe(
        ALARM_ENVIRONMENT,
    );

    // The legs must not need an environment -- conditioning on the claim would *require* every leg
    // to declare one, and they run by hand during the report-only window too.
    expect(trustFor(template, "metrics").StringEquals).not.toHaveProperty(`${CLAIM}:environment`);
});

test("it should output both role ARNs, which have to be copied into secrets by hand", () => {
    const outputs = synth().findOutputs("*");
    const descriptions = Object.values(outputs).map((o) => o.Description);

    expect(descriptions).toContain(
        "Set as the BUILD_CANARY_METRICS_ROLE_ARN repository secret",
    );
    expect(descriptions).toContain("Set as the BUILD_CANARY_ALARM_ROLE_ARN repository secret");
});
