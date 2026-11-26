/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import { Effect, OpenIdConnectProvider, PolicyStatement } from "aws-cdk-lib/aws-iam";
import { ArnFormat, CfnOutput, Stack, StackProps, Tags } from "aws-cdk-lib";
import { GitHubOidcRole, jobWorkflowRef } from "../constructs/github-oidc-role";
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
 * Alarm name prefixes the orchestrator role may manage.
 *
 * `build-canary-*` is the canary's own, one alarm per release. `paging-selftest-*` belongs to a
 * hand-dispatched workflow that exercises datapoint to ticket on its own, without a release and
 * without building anything; it arrives with the canary. Both are granted here so that bringing
 * this stack up is a single deploy rather than two.
 */
export const ALARM_PREFIXES = ["build-canary", "paging-selftest"];

/**
 * The GitHub environment a job must run in to assume the orchestrator's role.
 *
 * Named for the job rather than for one of its permissions, because that job both arms the alarms
 * and writes its own verdicts about the release. A GitHub job declares exactly one environment, so
 * naming it after a single permission would mislead whoever adds the next one.
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
 * its workflow declares the orchestrator's environment, the orchestrator's role too, since the
 * environment restricts which refs may use it and not who may push them. Enough to forge a page
 * and delete the alarms. So the ruleset being active is a prerequisite for deploying these roles,
 * not a later hardening step.
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
 * The workflow each role's jobs are defined by, pinned as a `job_workflow_ref` condition.
 *
 * {@link RELEASE_REFS} bounds *where* a job ran, which is not the same as *which workflow* it
 * belongs to: several workflows can run from one ref. `manual-canary.yml` and
 * `manual-pull-request-bot.yml` are `workflow_dispatch`, check out a pull request's code, and hold
 * `id-token: write` -- and a dispatch can be launched from a release tag. Without this condition a
 * token minted by one of those jobs satisfies everything the metrics role checks, so a pull
 * request's code could write a verdict.
 *
 * These files arrive with the canary itself, like the `paging-selftest-*` alarm prefix above.
 */
export const LEG_WORKFLOW = "build-canary.yml";
export const ORCHESTRATOR_WORKFLOWS = ["post-publish-watch.yml", "paging-selftest.yml"];

/** One of this repository's workflows, as a `job_workflow_ref` pattern. */
const workflowRef = (workflow: string): string =>
    jobWorkflowRef(SMITHY_RS.githubOrg, SMITHY_RS.githubRepo, workflow);

export interface Properties extends StackProps {
    githubActionsOidcProvider: OpenIdConnectProvider;
}

/**
 * The two roles the build canary assumes.
 *
 * Nothing durable is stored here. A release's record is a 14-day artifact, and the resolved-graph
 * baselines are a directory on the runner rather than a bucket, for the reasons the canary crate's
 * `baseline` module gives.
 *
 * The split is leg versus orchestrator, and it is a split by *how far the token travels*. A leg's
 * token is minted once per leg, forty times in a release's matrix, in jobs that check out and
 * build published crates from the open internet; it can write one metric in one namespace and
 * nothing else. The orchestrator's is minted once per release, in a job that declares a GitHub
 * environment a pull request's code cannot declare, and it arms the alarms and writes its own
 * verdicts about itself.
 *
 * The leg role is separate because its tokens are the numerous, least-trusted ones, and those must
 * not be able to delete the alarm watching them. The orchestrator's permissions are not split
 * further than that: any two roles carved out of them would accept *identical* tokens -- same
 * repository, same environment, same refs -- so anything able to assume one could assume the
 * other, which buys least privilege per step and no isolation.
 */
export class BuildCanaryStack extends Stack {
    /** Assumed by every leg, from a release tag, to record its verdict and nothing else. */
    public readonly metricsRole: GitHubOidcRole;
    /** Assumed only from the orchestrator environment: the alarms, and its own verdicts. */
    public readonly orchestratorRole: GitHubOidcRole;

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
            // ref is enough on its own. The workflow is pinned separately, because the ref alone
            // does not say which workflow a job belongs to.
            //
            // `main` is deliberately absent. It is where maintainer-triggered workflows run a pull
            // request's checked-out code in jobs holding `id-token: write` --
            // `manual-canary.yml`, called by `fork-canary.yml` from `pull_request_target`, and
            // `manual-pull-request-bot.yml`. Admitting `main` here would let that code mint a
            // token this role accepts and write a `Red` datapoint, tripping the release's alarm
            // and paging the oncall over a pull request. Anything of the canary's that legitimately
            // runs from `main` uses the orchestrator role below, which is environment-gated.
            refs: RELEASE_TAG_REFS,
            jobWorkflowRefs: [workflowRef(LEG_WORKFLOW)],
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
            // Two workflows define the orchestrator's jobs, so both are listed. `build-canary.yml`
            // is deliberately not among them: a leg must never reach the role that deletes alarms.
            jobWorkflowRefs: ORCHESTRATOR_WORKFLOWS.map(workflowRef),
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

        // Its own verdicts about itself: a failed dispatch, an unchecked release, a rigged
        // break during the self-test. One job per release writes these, where a leg's are written
        // forty at a time, which is why the leg role exists separately at all.
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

        // Both ARNs have to be copied into repository secrets by hand, so emit them rather than
        // making someone dig through the console after deploying.
        new CfnOutput(this, "metrics-role-arn", {
            value: this.metricsRole.oidcRole.roleArn,
            description: "Set as the BUILD_CANARY_METRICS_ROLE_ARN repository secret",
        });
        new CfnOutput(this, "orchestrator-role-arn", {
            value: this.orchestratorRole.oidcRole.roleArn,
            description: "Set as the BUILD_CANARY_ORCHESTRATOR_ROLE_ARN repository secret",
        });
        new CfnOutput(this, "metric-namespace", {
            value: METRIC_NAMESPACE,
            description: "The namespace the metrics role is constrained to",
        });
    }
}
