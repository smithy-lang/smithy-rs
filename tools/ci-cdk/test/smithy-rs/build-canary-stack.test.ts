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
    MAIN_REF,
    RECORD_PREFIX,
    RELEASE_REFS,
} from "../../lib/smithy-rs/build-canary-stack";
import { OidcProviderStack } from "../../lib/oidc-provider-stack";
import { SMITHY_RS } from "../../lib/github-repositories";

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

/** Every trust statement on the role whose name ends in `suffix`, in order. */
function trustStatements(template: Template, suffix: string): Statement[] {
    const roles = Object.values(template.findResources("AWS::IAM::Role")).filter((r) =>
        String(r.Properties.RoleName).includes(`-${suffix}-`),
    );
    expect(roles).toHaveLength(1);
    return roles[0].Properties.AssumeRolePolicyDocument.Statement;
}

/** A job's OIDC claims, as the provider presents them. */
type Claims = Record<string, string>;

const RELEASE_TAG = "refs/tags/release-2026-11-16";

/** The claims a job in this repository presents, optionally declaring an environment. */
function token(ref: string, environment?: string): Claims {
    const claims: Claims = {
        [`${CLAIM}:aud`]: "sts.amazonaws.com",
        [`${CLAIM}:repository_owner_id`]: SMITHY_RS.githubOrgId,
        [`${CLAIM}:repository_id`]: SMITHY_RS.githubRepoId,
        [`${CLAIM}:sub`]: `repo:${SMITHY_RS.githubOrg}/${SMITHY_RS.githubRepo}:ref:${ref}`,
        [`${CLAIM}:ref`]: ref,
    };
    if (environment !== undefined) {
        claims[`${CLAIM}:environment`] = environment;
    }
    return claims;
}

const escapeRegExp = (literal: string): string => literal.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

/**
 * Whether any of a role's trust statements admits these claims.
 *
 * A small evaluator rather than assertions on the rendered JSON, because what this stack's security
 * turns on is whether a given token can assume a given role — and with two alternatives per role
 * that is no longer legible by reading condition blocks side by side. A condition on a claim the
 * token does not carry matches nothing, which is how IAM behaves and is the direction that matters.
 */
function admits(statements: Statement[], claims: Claims): boolean {
    // IAM's StringLike: `*` is any sequence of characters, `?` is exactly one. The `?` matters
    // here -- it is what makes `release-????-??-??` admit a date and refuse `release-test`.
    const like = (pattern: string, value: string): boolean => {
        const expression = [...pattern]
            .map((c) => (c === "*" ? ".*" : c === "?" ? "." : escapeRegExp(c)))
            .join("");
        return new RegExp(`^${expression}$`).test(value);
    };

    return statements.some((statement) => {
        const condition = statement.Condition ?? {};
        for (const [key, want] of Object.entries(condition.StringEquals ?? {})) {
            if (claims[key] !== want) return false;
        }
        for (const [key, want] of Object.entries(condition.StringLike ?? {})) {
            const value = claims[key];
            if (value === undefined) return false;
            const patterns = Array.isArray(want) ? want : [want];
            if (!patterns.some((pattern) => like(pattern, value))) return false;
        }
        return true;
    });
}

const actionsOf = (statements: Statement[]): string[] =>
    statements.flatMap((s) => (Array.isArray(s.Action) ? s.Action : [s.Action]));

test("it should create exactly three roles", () => {
    const roles = synth().findResources("AWS::IAM::Role");
    expect(
        Object.values(roles)
            .map((r) => r.Properties.RoleName)
            .sort(),
    ).toEqual([
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
    expect(JSON.stringify(buckets[0].Properties.BucketEncryption)).toContain("AES256");

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

test("the orchestrator's roles should be gated on the environment, and a leg on a tag must not be", () => {
    const template = synth();
    // The environment is the only thing distinguishing the orchestrator from a leg: both run in
    // this repository from the same release tag, so their tokens are otherwise identical.
    for (const role of ["alarms", "records"]) {
        expect(trustFor(template, role).StringEquals[`${CLAIM}:environment`]).toBe(
            ORCHESTRATOR_ENVIRONMENT,
        );
    }
    // Conditioning the metrics role's *tag* alternative on an environment would require every leg
    // to declare one. Its `main` alternative does require it, which the tests below cover.
    const [onATag] = trustStatements(template, "metrics");
    expect(onATag.Condition?.StringEquals).not.toHaveProperty(`${CLAIM}:environment`);
});

/**
 * The ref is what bounds who can write the canary's series. A namespace has no owner, so the
 * metrics role's namespace condition does not.
 */
test("every role should only be assumable from a release tag or main", () => {
    const template = synth();
    for (const role of ["alarms", "records"]) {
        expect(trustFor(template, role).StringLike[`${CLAIM}:ref`]).toEqual(RELEASE_REFS);
    }

    // The legs' role splits the two into separate statements, because `main` is not trusted on the
    // same terms as a release tag.
    const metrics = trustStatements(template, "metrics");
    expect(metrics).toHaveLength(2);
    expect(metrics[0].Condition?.StringLike[`${CLAIM}:ref`]).toEqual(
        RELEASE_REFS.filter((ref) => ref.startsWith("refs/tags/")),
    );
    expect(metrics[1].Condition?.StringLike[`${CLAIM}:ref`]).toEqual([MAIN_REF]);

    // A date-shaped tag matches; a stray `release-test` must not.
    expect(RELEASE_REFS).toContain("refs/tags/release-????-??-??");
    expect(RELEASE_REFS.some((r) => r.includes("release-*"))).toBe(false);
});

test("a leg on a release tag can assume the metrics role", () => {
    expect(admits(trustStatements(synth(), "metrics"), token(RELEASE_TAG))).toBe(true);
});

test("the orchestrator on main, inside its environment, can assume the metrics role", () => {
    const claims = token(MAIN_REF, ORCHESTRATOR_ENVIRONMENT);
    expect(admits(trustStatements(synth(), "metrics"), claims)).toBe(true);
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
test("a job on main running a pull request's code cannot assume the metrics role", () => {
    expect(admits(trustStatements(synth(), "metrics"), token(MAIN_REF))).toBe(false);
});

test("no role should be assumable from any other ref, with or without the environment", () => {
    const template = synth();
    for (const role of ["metrics", "alarms", "records"]) {
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
    expect(descriptions).toContain("Set as the BUILD_CANARY_ALARM_ROLE_ARN repository secret");
    expect(descriptions).toContain("Set as the BUILD_CANARY_RECORD_ROLE_ARN repository secret");
    expect(descriptions).toContain("Set as the BUILD_CANARY_RECORD_BUCKET repository secret");
});
