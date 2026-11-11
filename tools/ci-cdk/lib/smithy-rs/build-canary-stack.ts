/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import { Effect, OpenIdConnectProvider, PolicyStatement } from "aws-cdk-lib/aws-iam";
import { ArnFormat, CfnOutput, RemovalPolicy, Stack, StackProps, Tags } from "aws-cdk-lib";
import { BlockPublicAccess, Bucket, BucketEncryption } from "aws-cdk-lib/aws-s3";
import { GitHubOidcRole } from "../constructs/github-oidc-role";
import { Construct } from "constructs";

/**
 * The namespace the build canary's verdicts are written to.
 *
 * Its own, deliberately not shared with the `aws-sdk-rust-canary` namespace. Two canaries writing
 * to one namespace could not be alarmed on independently, and the metrics role below is constrained
 * to this namespace — which is only meaningful if nothing else writes here.
 *
 * **The leg must emit to exactly this namespace.** Nothing checks the two agree, because the
 * emitting code is Rust and this is TypeScript. A mismatch fails quietly in the worst way: the
 * `PutMetricData` call is denied by the namespace condition, so a verdict is simply never recorded.
 */
export const METRIC_NAMESPACE = "smithy-rs-build-canary";

/**
 * Alarm name prefixes the alarm role may manage: the canary's own, and the paging self-test that
 * exercises alarm to ticket without the canary.
 */
export const ALARM_PREFIXES = ["build-canary", "paging-selftest"];

/**
 * The GitHub environment a job must run in to assume either of the orchestrator's roles.
 *
 * Named for the job rather than for one of its permissions, because it gates both arming the alarm
 * and writing the resolved-graph baselines. A GitHub job declares exactly one environment, so the
 * orchestrator's roles have to share it; see the class docs for what that does and does not buy.
 */
export const ORCHESTRATOR_ENVIRONMENT = "canary-orchestrator";

/**
 * The key prefix under which the baselines live, as `baselines/<manifest-id>/Cargo.lock`.
 *
 * **The Rust side must use the same prefix.** Unlike {@link METRIC_NAMESPACE}, this one is checked:
 * a test in the canary crate reads this file and compares, because a disagreement here would make
 * every read miss and silently turn the dispatch gate into "dispatch on every tick".
 */
export const BASELINE_PREFIX = "baselines";

export interface Properties extends StackProps {
    githubActionsOidcProvider: OpenIdConnectProvider;
}

/**
 * The three roles the build canary assumes, and the bucket holding its baselines.
 *
 * The split that matters is leg versus orchestrator. Every leg needs to write a metric, and legs
 * run on whatever ref the release was cut from, sixty at a time. The orchestrator is one job per
 * release: it arms the alarm, polls publish status, and decides which legs to dispatch. One role
 * doing both would mean every leg — and anything else in the repository able to assume that role —
 * also held permission to delete the alarm watching it and to rewrite the baselines the dispatch
 * gate reads, so a branch could quietly disable the thing that pages the oncall.
 *
 * The leg's permission set is therefore disjoint from the orchestrator's, and neither is a superset
 * of the other: the metrics role cannot touch alarms or the bucket, and neither orchestrator role
 * can write metrics. A `Red` datapoint and the alarm evaluating it are separate authorities.
 *
 * Telling a leg from the orchestrator is the difficult half. Both run in this repository from the
 * same release tag, so their tokens are identical in everything except the GitHub environment —
 * which is why the orchestrator's roles are gated on one, and why that gate is load-bearing rather
 * than decorative.
 *
 * The orchestrator's two roles share that environment, because a GitHub job declares exactly one.
 * So the boundary between *them* is not isolation — anything able to assume one can assume the
 * other — it is least privilege per step: the step that writes a baseline holds no alarm
 * permission, so a bug in it cannot delete an alarm. Said plainly so nobody reads more into the
 * split than is there. If the orchestrator is ever divided into separate jobs, the two roles can
 * take separate environments without any change to their grants.
 */
export class BuildCanaryStack extends Stack {
    /** Assumed by every leg, to record its verdict. */
    public readonly metricsRole: GitHubOidcRole;
    /** Assumed only from the orchestrator environment, to arm and disarm the alarm. */
    public readonly alarmRole: GitHubOidcRole;
    /** Assumed only from the orchestrator environment, to read and replace the baselines. */
    public readonly baselineRole: GitHubOidcRole;
    /** The last resolved graph per manifest, which the dispatch gate diffs against. */
    public readonly baselineBucket: Bucket;

    constructor(scope: Construct, id: string, props: Properties) {
        super(scope, id, props);

        // Tag the resources created by this stack to make identifying resources easier
        Tags.of(this).add("stack", id);

        const github = {
            githubOrg: "smithy-lang",
            githubRepo: "smithy-rs",
            oidcProvider: props.githubActionsOidcProvider,
        };

        this.metricsRole = new GitHubOidcRole(this, "metrics", {
            name: "smithy-rs-build-canary-metrics",
            ...github,
            // Deliberately not narrowed by ref or environment. A leg runs for whatever tag the
            // release was cut from, and also by hand during the report-only window, so pinning the
            // ref here would break the second case for no gain: the namespace condition below is
            // what bounds the damage, not the ref.
        });
        this.metricsRole.oidcRole.addToPolicy(
            new PolicyStatement({
                actions: ["cloudwatch:PutMetricData"],
                effect: Effect.ALLOW,
                // `PutMetricData` has no resource-level permissions, so `*` is the only accepted
                // value and `cloudwatch:namespace` is the sole lever available. Without the
                // condition this role could write to any namespace in the account, including one
                // another team alarms on.
                resources: ["*"],
                conditions: {
                    StringEquals: { "cloudwatch:namespace": METRIC_NAMESPACE },
                },
            }),
        );

        this.alarmRole = new GitHubOidcRole(this, "alarms", {
            name: "smithy-rs-build-canary-alarms",
            ...github,
            environment: ORCHESTRATOR_ENVIRONMENT,
        });
        this.alarmRole.oidcRole.addToPolicy(
            new PolicyStatement({
                // Create, delete and read back. The readback is what makes "the arming job is
                // green" mean something, since it is the one job whose failure the paging
                // mechanism cannot report — it *is* the paging mechanism.
                actions: [
                    "cloudwatch:PutMetricAlarm",
                    "cloudwatch:DeleteAlarms",
                    "cloudwatch:DescribeAlarms",
                ],
                effect: Effect.ALLOW,
                // Scoped by alarm name, which these three actions do support, unlike
                // `PutMetricData`. Narrow scope on `DescribeAlarms` excludes composite alarms,
                // which need `*`; the canary creates metric alarms only.
                resources: ALARM_PREFIXES.map((prefix) =>
                    this.formatArn({
                        service: "cloudwatch",
                        resource: "alarm",
                        resourceName: `${prefix}-*`,
                        arnFormat: ArnFormat.COLON_RESOURCE_NAME,
                    }),
                ),
            }),
        );

        // Holds one object per manifest: the resolved graph as of the last time the orchestrator
        // looked. The dispatch tier diffs a fresh resolution against it and dispatches only the
        // manifests whose graph moved, which is what keeps a 15-minute tick from running 60 legs
        // every time regardless of whether anything was published.
        //
        // Private, like every bucket in this app. Nothing outside the orchestrator reads it: a
        // responder investigating a red leg gets the same lockfile from the leg's own uploaded
        // evidence. The contents are derived entirely from crates.io and so are not themselves
        // sensitive, which bounds the cost of a mistake but is not a reason to relax the default.
        this.baselineBucket = new Bucket(this, "baseline-bucket", {
            blockPublicAccess: BlockPublicAccess.BLOCK_ALL,
            encryption: BucketEncryption.S3_MANAGED,
            // Not in the other buckets in this app, which predate it. Adds a bucket policy denying
            // any request not over TLS; the AWS CLI always uses TLS, so there is nothing to break.
            enforceSSL: true,
            // Deliberately no lifecycle expiration, unlike every other bucket here. The baseline is
            // current state, not a build artifact: expiring it during a quiet fortnight would make
            // the next tick see no baseline and dispatch everything. Thirteen objects of about
            // 70 KB is not worth a rule that can only misfire.
            versioned: false,
            removalPolicy: RemovalPolicy.DESTROY,
        });

        this.baselineRole = new GitHubOidcRole(this, "baselines", {
            name: "smithy-rs-build-canary-baselines",
            ...github,
            environment: ORCHESTRATOR_ENVIRONMENT,
        });
        // Read and replace, never delete. Overwriting a baseline is a `PutObject`, so withholding
        // `DeleteObject` costs the orchestrator nothing and means a bug cannot leave a manifest
        // with no baseline at all — which would be silent, since an absent baseline reads as "never
        // seen" and simply dispatches.
        //
        // Scoped to the prefix even though the bucket holds nothing else, so that a later use of
        // this bucket is not readable by the orchestrator by default.
        const baselineObjects = `${BASELINE_PREFIX}/*`;
        this.baselineBucket.grantRead(this.baselineRole.oidcRole, baselineObjects);
        this.baselineBucket.grantPut(this.baselineRole.oidcRole, baselineObjects);

        // The ARNs and the bucket name have to be copied into repository secrets by hand, so emit
        // them rather than making someone dig through the console after deploying.
        new CfnOutput(this, "metrics-role-arn", {
            value: this.metricsRole.oidcRole.roleArn,
            description: "Set as the BUILD_CANARY_METRICS_ROLE_ARN repository secret",
        });
        new CfnOutput(this, "alarm-role-arn", {
            value: this.alarmRole.oidcRole.roleArn,
            description: "Set as the BUILD_CANARY_ALARM_ROLE_ARN repository secret",
        });
        new CfnOutput(this, "baseline-role-arn", {
            value: this.baselineRole.oidcRole.roleArn,
            description: "Set as the BUILD_CANARY_BASELINE_ROLE_ARN repository secret",
        });
        new CfnOutput(this, "baseline-bucket-name", {
            value: this.baselineBucket.bucketName,
            description: "Set as the BUILD_CANARY_BASELINE_BUCKET repository secret",
        });
        new CfnOutput(this, "metric-namespace", {
            value: METRIC_NAMESPACE,
            description: "The namespace the metrics role is constrained to",
        });
    }
}
