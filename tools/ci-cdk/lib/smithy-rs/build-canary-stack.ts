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
 * Alarm name prefixes the alarm role may manage.
 *
 * `build-canary-*` is the canary's own, one alarm per release. `paging-selftest-*` belongs to a
 * hand-dispatched workflow that exercises datapoint to ticket on its own, without a release and
 * without building anything; it arrives with the canary. Both are granted here so that bringing
 * this stack up is a single deploy rather than two.
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
 * The key prefix the durable records live under, as `runs/<release>/<run-id>-<attempt>/`.
 *
 * **The Rust side must use the same prefix.** Unlike {@link METRIC_NAMESPACE}, this one is checked:
 * a test in the canary crate reads this file and compares, because a disagreement would make every
 * write denied and the archive silently empty.
 */
export const RECORD_PREFIX = "runs";

export interface Properties extends StackProps {
    githubActionsOidcProvider: OpenIdConnectProvider;
}

/**
 * The three roles the build canary assumes, and the bucket holding its baselines.
 *
 * The split that matters is leg versus orchestrator. Every leg needs to write a metric, and legs
 * run on whatever ref the release was cut from, forty at a time. The orchestrator is one job per
 * release: it arms the alarm, polls publish status, decides which legs to dispatch, and files the
 * record at the end. One role doing both would mean every leg — and anything else in the repository
 * able to assume that role — also held permission to delete the alarm watching it and to write into
 * the archive, so a branch could quietly disable the thing that pages the oncall.
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
 * other — it is least privilege per step: the step that files a record holds no alarm permission,
 * so a bug in it cannot delete an alarm. Said plainly so nobody reads more into the split than is
 * there. If the orchestrator is ever divided into separate jobs, the two roles can take separate
 * environments without any change to their grants.
 */
export class BuildCanaryStack extends Stack {
    /** Assumed by every leg, to record its verdict. */
    public readonly metricsRole: GitHubOidcRole;
    /** Assumed only from the orchestrator environment, to arm and disarm the alarm. */
    public readonly alarmRole: GitHubOidcRole;
    /** Assumed only from the orchestrator environment, to add a record to the archive. */
    public readonly recordRole: GitHubOidcRole;
    /** One record per release that broke, kept after the workflow run has been deleted. */
    public readonly recordBucket: Bucket;

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

        // One object per release that broke, written when the run ends and kept indefinitely.
        //
        // This exists because the evidence in GitHub expires. Artifacts and logs are kept 90 days
        // by default, and since October 2026 that retention covers the workflow runs themselves, so
        // three months after a release broke there is no log, no artifact and no run. The red-leg
        // evidence this repository uploads is kept 14 days. The question asked after an incident is
        // "how often does this happen, and to what", and it is asked long after every trace has
        // been deleted.
        //
        // Not about privacy: the job log is public and carries the same diagnosis for as long as it
        // exists. Private anyway, because that is the default and there is no reason to relax it.
        this.recordBucket = new Bucket(this, "record-bucket", {
            blockPublicAccess: BlockPublicAccess.BLOCK_ALL,
            encryption: BucketEncryption.S3_MANAGED,
            // Not in the other buckets in this app, which predate it. Adds a bucket policy denying
            // any request not over TLS; the AWS CLI always uses TLS, so there is nothing to break.
            enforceSSL: true,
            // Deliberately no lifecycle expiration, unlike every other bucket here, because
            // outliving GitHub's retention is the entire purpose. A record is a few hundred
            // kilobytes and only a release that actually broke gets one.
            versioned: false,
            // The records are the only copy of this history once the runs are deleted, so the
            // bucket outlives the stack. The other buckets here hold reproducible build inputs and
            // are `DESTROY`; this one is not reproducible.
            removalPolicy: RemovalPolicy.RETAIN,
        });

        this.recordRole = new GitHubOidcRole(this, "records", {
            name: "smithy-rs-build-canary-records",
            ...github,
            environment: ORCHESTRATOR_ENVIRONMENT,
        });
        // Write only: no read, no delete, no list. An archive should only ever grow, and the
        // workflow needs nothing else — a `PutObject` that succeeds is its own confirmation, and
        // the keys carry the release, the run id and the attempt, so a write cannot land on an
        // existing record. Withholding read also means a token leaked from a run cannot enumerate
        // the history.
        //
        // Scoped to the prefix even though the bucket holds nothing else, so a later use of this
        // bucket is not writable by the orchestrator by default.
        this.recordBucket.grantPut(this.recordRole.oidcRole, `${RECORD_PREFIX}/*`);

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
        new CfnOutput(this, "record-role-arn", {
            value: this.recordRole.oidcRole.roleArn,
            description: "Set as the BUILD_CANARY_RECORD_ROLE_ARN repository secret",
        });
        new CfnOutput(this, "record-bucket-name", {
            value: this.recordBucket.bucketName,
            description: "Set as the BUILD_CANARY_RECORD_BUCKET repository secret",
        });
        new CfnOutput(this, "metric-namespace", {
            value: METRIC_NAMESPACE,
            description: "The namespace the metrics role is constrained to",
        });
    }
}
