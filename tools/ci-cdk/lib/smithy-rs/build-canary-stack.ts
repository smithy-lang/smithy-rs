/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import { Effect, OpenIdConnectProvider, PolicyStatement } from "aws-cdk-lib/aws-iam";
import { ArnFormat, CfnOutput, Stack, StackProps, Tags } from "aws-cdk-lib";
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

/** The GitHub environment a job must run in to assume the alarm role. */
export const ALARM_ENVIRONMENT = "canary-alarms";

export interface Properties extends StackProps {
    githubActionsOidcProvider: OpenIdConnectProvider;
}

/**
 * The two roles the build canary assumes, kept separate on purpose.
 *
 * Every leg needs to write a metric, and legs run on whatever ref the release was cut from. The
 * alarm is created once per release by a single job. One role doing both would mean every leg — and
 * anything else in the repository able to assume that role — also held permission to delete the
 * alarm watching it, so a branch could quietly disable the thing that pages the oncall.
 *
 * The two permission sets are therefore disjoint, and neither is a superset of the other: the
 * metrics role cannot touch alarms, and the alarm role cannot write metrics. A `Red` datapoint and
 * the alarm evaluating it are separate authorities.
 *
 * Telling the two jobs apart is the difficult half. Both run in this repository from the same
 * release tag, so their tokens are identical in everything except the GitHub environment — which
 * is why the alarm role is gated on one, and why that gate is load-bearing rather than decorative.
 */
export class BuildCanaryStack extends Stack {
    /** Assumed by every leg, to record its verdict. */
    public readonly metricsRole: GitHubOidcRole;
    /** Assumed only from the alarm environment, to arm and disarm the alarm. */
    public readonly alarmRole: GitHubOidcRole;

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
            environment: ALARM_ENVIRONMENT,
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

        // The two ARNs have to be copied into repository secrets by hand, so emit them rather than
        // making someone dig through the console after deploying.
        new CfnOutput(this, "metrics-role-arn", {
            value: this.metricsRole.oidcRole.roleArn,
            description: "Set as the BUILD_CANARY_METRICS_ROLE_ARN repository secret",
        });
        new CfnOutput(this, "alarm-role-arn", {
            value: this.alarmRole.oidcRole.roleArn,
            description: "Set as the BUILD_CANARY_ALARM_ROLE_ARN repository secret",
        });
        new CfnOutput(this, "metric-namespace", {
            value: METRIC_NAMESPACE,
            description: "The namespace the metrics role is constrained to",
        });
    }
}
