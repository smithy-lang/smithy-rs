/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import { Effect, OpenIdConnectProvider, PolicyStatement } from "aws-cdk-lib/aws-iam";
import { ArnFormat, CfnOutput, RemovalPolicy, Stack, StackProps, Tags } from "aws-cdk-lib";
import { BlockPublicAccess, Bucket, BucketEncryption } from "aws-cdk-lib/aws-s3";
import { GitHubOidcRole } from "../constructs/github-oidc-role";
import { SMITHY_RS } from "../github-repositories";
import { Construct } from "constructs";

/**
 * The namespace the build canary's verdicts are written to.
 *
 * Its own, deliberately not shared with the `aws-sdk-rust-canary` namespace: two canaries on one
 * namespace could not be alarmed on independently. The metrics role is constrained to this
 * namespace, and the SDK canary's role is confined to its own — the latter in
 * `smithy-rs-canary-stack`, so that half holds only once *that* stack is deployed. See the deploy
 * notes in `bin/smithy-rs`.
 *
 * **The leg must emit to exactly this namespace**, or `PutMetricData` is denied by that condition
 * and the verdict is never recorded. The canary crate has a test that reads this file and compares.
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
 * and filing the release's record. A GitHub job declares exactly one environment, so the
 * orchestrator's roles have to share it; see the class docs for what that does and does not buy.
 */
export const ORCHESTRATOR_ENVIRONMENT = "canary-orchestrator";

/**
 * The refs a build canary job may run from: a real release tag, or `main` for a by-hand dispatch.
 *
 * `?` matches exactly one character in an IAM `StringLike`, so a date-shaped tag matches and a stray
 * `release-test` gets no role. This is what bounds who can write the canary's series — a namespace
 * has no owner, so the metrics role's namespace condition does not.
 *
 * **It bounds the shape of the tag, not who created it**, and the two halves are a pair. Checking
 * the author is a repository ruleset's job, and the ruleset has to be *enforcing* for this
 * condition to mean anything: while it only evaluates, anyone with write access can tag an
 * arbitrary commit `release-YYYY-MM-DD`, and code at that tag can assume the metrics role — or, if
 * its workflow declares the orchestrator's environment, the alarm and record roles too, since the
 * environment restricts which refs may use it and not who may push them. Enough to forge a page,
 * delete the alarms, and write to the archive. So the ruleset being active is a prerequisite for
 * deploying these roles, not a later hardening step.
 *
 * The gate's only bypass is the release automation, so no human account can create a tag these
 * roles will trust — not an admin either. That is the strong form, and it is only correct while the
 * release is actually cut by that automation: a release tagged by a person would be refused, and
 * the symptom is a rejected push at release time rather than anything visible here.
 */
export const RELEASE_REFS = [
    "refs/tags/release-????-??-??",
    "refs/tags/release-????-??-??.*",
    "refs/heads/main",
];

/**
 * The branch a by-hand dispatch runs from, split out because it is not trusted on the same terms.
 *
 * A release tag is created only by the release automation, so a job running from one is the
 * canary's. `main` is also where maintainer-triggered workflows run a pull request's code, so jobs
 * there are trusted only inside {@link ORCHESTRATOR_ENVIRONMENT}.
 */
export const MAIN_REF = "refs/heads/main";

/** {@link RELEASE_REFS} without {@link MAIN_REF}: what the leg role accepts, and all it accepts. */
export const RELEASE_TAG_REFS = RELEASE_REFS.filter((ref) => ref !== MAIN_REF);

/**
 * The key prefix the durable records live under, as `runs/<release>/<run-id>-<attempt>/`.
 *
 * **The Rust side must use the same prefix.** A test in the canary crate reads this file and
 * compares, as one does for {@link METRIC_NAMESPACE}, because a disagreement would make every write
 * denied and the archive silently empty.
 */
export const RECORD_PREFIX = "runs";

export interface Properties extends StackProps {
    githubActionsOidcProvider: OpenIdConnectProvider;
}

/**
 * The two roles the build canary assumes, and the bucket holding one record per broken release.
 *
 * The resolved-graph baselines are not here: they live in a directory in the repository, for the
 * reasons the canary crate's `baseline` module gives.
 *
 * The split is leg versus orchestrator, and it is a split by *how far the token travels*. A leg's
 * token is minted forty-five times per dispatch, in jobs that check out and build published crates
 * from the open internet; it can write one metric in one namespace and nothing else. The
 * orchestrator's is minted once per release, in a job that declares a GitHub environment a pull
 * request's code cannot declare, and it arms the alarms, records its own verdicts about itself, and
 * appends to the archive.
 *
 * Two roles rather than three. An earlier shape split the orchestrator's alarm and archive
 * permissions, but both accepted *identical* tokens -- same repository, same environment, same
 * refs -- so anything able to assume one could assume the other, and the division bought
 * least-privilege per step rather than isolation. It cost a role, a repository secret, and a second
 * credential switch inside one job. The leg role stays separate because its tokens are the
 * numerous, least-trusted ones, and those must not be able to delete the alarm watching them.
 */
export class BuildCanaryStack extends Stack {
    /** Assumed by every leg, from a release tag, to record its verdict and nothing else. */
    public readonly metricsRole: GitHubOidcRole;
    /** Assumed only from the orchestrator environment: alarms, its own verdicts, the archive. */
    public readonly orchestratorRole: GitHubOidcRole;
    /** One record per release that broke, kept after the workflow run has been deleted. */
    public readonly recordBucket: Bucket;

    constructor(scope: Construct, id: string, props: Properties) {
        super(scope, id, props);

        // Tag the resources created by this stack to make identifying resources easier
        Tags.of(this).add("stack", id);

        const github = {
            ...SMITHY_RS,
            oidcProvider: props.githubActionsOidcProvider,
        };

        this.metricsRole = new GitHubOidcRole(this, "metrics", {
            name: "smithy-rs-build-canary-metrics",
            ...github,
            // Release tags only, and no environment. Requiring one would mean every leg
            // declaring it, and a release tag is created only by the release automation, so the
            // ref is enough on its own.
            //
            // `main` is deliberately absent. It is where maintainer-triggered workflows run a pull
            // request's checked-out code in jobs holding `id-token: write` --
            // `manual-canary.yml`, called by `fork-canary.yml` from `pull_request_target`, and
            // `manual-pull-request-bot.yml`. Admitting `main` here would let that code mint a
            // token this role accepts and write a `Red` datapoint, tripping the release's alarm
            // and paging the oncall over a pull request. Anything of the canary's that legitimately
            // runs from `main` uses the orchestrator role below, which is environment-gated.
            refs: RELEASE_TAG_REFS,
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

        this.orchestratorRole = new GitHubOidcRole(this, "orchestrator", {
            name: "smithy-rs-build-canary-orchestrator",
            ...github,
            environment: ORCHESTRATOR_ENVIRONMENT,
            // The environment already restricts the refs; this narrows `release-*` to real tags.
            // `main` is here, where it is absent from the leg role, because the environment is
            // what a pull request's code cannot declare.
            refs: RELEASE_REFS,
        });
        this.orchestratorRole.oidcRole.addToPolicy(
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
            // An overwrite keeps the old record as a version, and the record role has no delete of
            // any kind. The key carries the run id and attempt, so an overwrite should not happen
            // at all; this is for when one does anyway.
            versioned: true,
            // No lifecycle expiration, unlike every other bucket here: outliving GitHub's retention
            // is the entire purpose, a record is a few hundred kilobytes, and only a release that
            // actually broke gets one.
            //
            // The bucket outlives the stack too, because once the runs are deleted these records
            // are the only copy. The others here hold reproducible build inputs and are `DESTROY`.
            removalPolicy: RemovalPolicy.RETAIN,
        });

        // Its own verdicts about itself: a failed dispatch, an unchecked release, a rigged
        // break during the self-test. One job per release writes these, where a leg's are written
        // forty-five at a time, which is why the leg role exists separately at all.
        this.orchestratorRole.oidcRole.addToPolicy(
            new PolicyStatement({
                actions: ["cloudwatch:PutMetricData"],
                effect: Effect.ALLOW,
                resources: ["*"],
                conditions: {
                    StringEquals: { "cloudwatch:namespace": METRIC_NAMESPACE },
                },
            }),
        );
        // Write only: no read, no delete, no list. An archive should only ever grow, and the
        // workflow needs nothing else — a `PutObject` that succeeds is its own confirmation, and
        // the keys carry the release, the run id and the attempt, so a write cannot land on an
        // existing record. Withholding read also means a token leaked from a run cannot enumerate
        // the history.
        //
        // Scoped to the prefix even though the bucket holds nothing else, so a later use of this
        // bucket is not writable by the orchestrator by default.
        this.recordBucket.grantPut(this.orchestratorRole.oidcRole, `${RECORD_PREFIX}/*`);

        // The ARNs and the bucket name have to be copied into repository secrets by hand, so emit
        // them rather than making someone dig through the console after deploying.
        new CfnOutput(this, "metrics-role-arn", {
            value: this.metricsRole.oidcRole.roleArn,
            description: "Set as the BUILD_CANARY_METRICS_ROLE_ARN repository secret",
        });
        new CfnOutput(this, "orchestrator-role-arn", {
            value: this.orchestratorRole.oidcRole.roleArn,
            description: "Set as the BUILD_CANARY_ORCHESTRATOR_ROLE_ARN repository secret",
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
