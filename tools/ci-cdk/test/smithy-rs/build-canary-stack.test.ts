/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import { App } from "aws-cdk-lib";
import { Template } from "aws-cdk-lib/assertions";
import {
    BuildCanaryStack,
    METRIC_NAMESPACE,
    ORCHESTRATOR_ENVIRONMENT,
    RECORD_PREFIX,
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

test("it should create exactly three roles", () => {
    const roles = synth().findResources("AWS::IAM::Role");
    expect(Object.values(roles).map((r) => r.Properties.RoleName).sort()).toEqual([
        "smithy-rs-build-canary-alarms-github-oidc-role",
        "smithy-rs-build-canary-metrics-github-oidc-role",
        "smithy-rs-build-canary-records-github-oidc-role",
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

test("the record role should only ever add to the archive", () => {
    const statements = policyFor(synth(), "records");
    const actions = actionsOf(statements);

    // An archive should only grow. No delete, so history cannot be rewritten; no read or list, so
    // a token leaked from a run cannot enumerate it. A PutObject that succeeds is its own
    // confirmation, so nothing is lost by withholding the rest.
    expect(actions.filter((a) => a.includes("Delete"))).toEqual([]);
    expect(actions.filter((a) => a.includes("Get"))).toEqual([]);
    expect(actions.filter((a) => a.includes("List"))).toEqual([]);
    expect(actions.some((a) => a.startsWith("s3:PutObject"))).toBe(true);

    // And scoped to the prefix, so a later use of this bucket is not writable by default.
    for (const resource of statements
        .flatMap((s) => (Array.isArray(s.Resource) ? s.Resource : [s.Resource]))
        .map((r) => JSON.stringify(r))) {
        expect(resource).toContain(`${RECORD_PREFIX}/*`);
    }
});

test("the record bucket should outlive the stack, since the records are the only copy", () => {
    // Every other bucket in this app holds reproducible build inputs and is DESTROY. This one
    // holds the only remaining history once GitHub has deleted the runs.
    const buckets = synth().findResources("AWS::S3::Bucket");
    expect(Object.values(buckets)[0].DeletionPolicy).toBe("Retain");
});

test("the record bucket should be private, encrypted and TLS-only", () => {
    const template = synth();
    const buckets = Object.values(template.findResources("AWS::S3::Bucket"));
    expect(buckets).toHaveLength(1);

    // All four Block Public Access settings on, which is what makes "private" true rather than
    // merely "no public policy written yet".
    expect(buckets[0].Properties.PublicAccessBlockConfiguration).toEqual({
        BlockPublicAcls: true,
        BlockPublicPolicy: true,
        IgnorePublicAcls: true,
        RestrictPublicBuckets: true,
    });
    expect(
        JSON.stringify(buckets[0].Properties.BucketEncryption),
    ).toContain("AES256");

    // `enforceSSL` renders as a bucket policy denying anything not over TLS.
    const policies = Object.values(template.findResources("AWS::S3::BucketPolicy"));
    const policyJson = JSON.stringify(policies);
    expect(policyJson).toContain("aws:SecureTransport");
    expect(policyJson).toContain("Deny");

    // And nothing grants anonymous access.
    expect(policyJson).not.toContain('"Principal":"*"');
    expect(policyJson).not.toContain("AllUsers");
    expect(policyJson).not.toContain("AuthenticatedUsers");
});

test("the records should not expire, unlike every other bucket in this app", () => {
    // Outliving GitHub's 90-day retention is the entire purpose. A lifecycle rule here would
    // reintroduce the gap the bucket exists to close.
    const buckets = Object.values(synth().findResources("AWS::S3::Bucket"));
    expect(buckets[0].Properties.LifecycleConfiguration).toBeUndefined();
});

test("a leg's permissions should be disjoint from the orchestrator's", () => {
    // The property the split exists for. If an orchestrator role could write metrics it could fake
    // a verdict; if the metrics role could touch alarms or the bucket, then every leg -- and
    // anything else able to assume it -- could delete the alarm watching it or rewrite the
    // baseline that decides whether it runs at all.
    const template = synth();
    const metrics = new Set(actionsOf(policyFor(template, "metrics")));
    const orchestrator = new Set([
        ...actionsOf(policyFor(template, "alarms")),
        ...actionsOf(policyFor(template, "records")),
    ]);

    expect([...metrics].filter((a) => orchestrator.has(a))).toEqual([]);
    expect([...orchestrator].some((a) => a.includes("PutMetricData"))).toBe(false);
    expect([...metrics].some((a) => a.includes("Alarm"))).toBe(false);
    expect([...metrics].some((a) => a.startsWith("s3:"))).toBe(false);
});

test("the orchestrator's roles should be gated on the environment, and the legs' must not", () => {
    const template = synth();

    // The environment is the only thing distinguishing the orchestrator from a leg: both run in
    // this repository from the same release tag, so their tokens are otherwise identical.
    for (const role of ["alarms", "records"]) {
        expect(trustFor(template, role).StringEquals[`${CLAIM}:environment`]).toBe(
            ORCHESTRATOR_ENVIRONMENT,
        );
    }

    // The legs must not need an environment -- conditioning on the claim would *require* every leg
    // to declare one, and they run by hand during the report-only window too.
    expect(trustFor(template, "metrics").StringEquals).not.toHaveProperty(`${CLAIM}:environment`);
});

test("it should output everything that has to be copied into a secret by hand", () => {
    const outputs = synth().findOutputs("*");
    const descriptions = Object.values(outputs).map((o) => o.Description);

    expect(descriptions).toContain(
        "Set as the BUILD_CANARY_METRICS_ROLE_ARN repository secret",
    );
    expect(descriptions).toContain("Set as the BUILD_CANARY_ALARM_ROLE_ARN repository secret");
    expect(descriptions).toContain("Set as the BUILD_CANARY_RECORD_ROLE_ARN repository secret");
    expect(descriptions).toContain("Set as the BUILD_CANARY_RECORD_BUCKET repository secret");
});
