/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import {
    FederatedPrincipal,
    OpenIdConnectProvider,
    PolicyStatement,
    Role,
} from "aws-cdk-lib/aws-iam";
import { Construct } from "constructs";
import { Tags } from "aws-cdk-lib";

/** The OIDC provider's claims appear as IAM condition keys under this prefix. */
const CLAIM = "token.actions.githubusercontent.com";

export interface Properties {
    name: string;
    githubOrg: string;
    githubRepo: string;
    /**
     * The organisation's numeric id, from `gh api orgs/ORG --jq .id`.
     *
     * Required rather than optional because this and {@link githubRepoId} are what actually
     * authorise the role; see the class docs. A wrong value produces a role nobody can assume.
     */
    githubOrgId: string;
    /** The repository's numeric id, from `gh api repos/ORG/REPO --jq .id`. */
    githubRepoId: string;
    oidcProvider: OpenIdConnectProvider;

    /**
     * Only let jobs that declare this GitHub environment assume the role.
     *
     * Conditioning on `environment` also *requires* a job to declare one, which is what makes an
     * environment's protection rules — required reviewers, branch and tag restrictions — part of
     * the trust decision rather than a convention.
     */
    environment?: string;

    /**
     * Only let jobs triggered by a matching git ref assume the role, for example
     * `refs/tags/release-*` or `refs/heads/main`. Matched with `StringLike`, so `*` is a wildcard.
     */
    refs?: string[];

    /**
     * Only let jobs defined by a matching workflow assume the role, for example
     * `smithy-lang/smithy-rs/.github/workflows/build-canary.yml@refs/tags/release-*`. Matched with
     * `StringLike`.
     *
     * This is the claim that distinguishes *which workflow* a job belongs to. {@link refs} cannot:
     * a ref says where a job ran, and several workflows can run from the same ref. A
     * `workflow_dispatch` workflow that checks out a pull request's code can be launched from a
     * release tag, so a role trusting release tags alone will accept a token minted by that job.
     *
     * Note the asymmetry with the id conditions: this claim carries the repository *name*, so a
     * rename stops it matching and the role becomes unassumable. That is a fail-closed break --
     * CI stops rather than letting the wrong caller in -- but it is a second place to update,
     * alongside `github-repositories.ts`.
     */
    jobWorkflowRefs?: string[];
    /**
     * Further jobs that may assume the role, each as a trust statement of its own.
     *
     * A single statement ANDs its conditions, so one alternative's narrowing applies to all of
     * them: a role that must accept any job on a release tag *and* only orchestrator jobs on
     * `main` cannot say so in one block without requiring the environment on the tag too. Separate
     * statements keep each alternative's conditions to itself.
     */
    alsoTrust?: { environment?: string; refs?: string[]; jobWorkflowRefs?: string[] }[];
}

/**
 * A role assumable by GitHub Actions through OIDC.
 *
 * ## Why the trust policy looks the way it does
 *
 * The traditional condition is on the `sub` claim alone, which looks like
 * `repo:ORG/REPO:ref:refs/heads/main`, matched with a `repo:ORG/REPO:*` wildcard. That has two
 * problems, and they pull in opposite directions.
 *
 * **`sub` alone is too coarse.** Everything — repository, ref, environment, trigger type — is
 * collapsed into one colon-delimited string whose shape changes with the trigger, so a wildcard is
 * the only practical match and it admits any branch, environment and workflow in the repository.
 * Compound conditions are impossible. Since February 2026, AWS STS exposes GitHub's individual
 * claims as condition keys — `repository`, `repository_id`, `ref`, `environment`,
 * `job_workflow_ref` and others — which take exact `StringEquals` and can be combined.
 *
 * **But `sub` cannot simply be dropped.** IAM recommends that any role trusting GitHub's provider
 * evaluate `sub`, and a role without such a condition is exactly the shape AWS's identity-provider
 * controls exist to catch.
 *
 * So both are present, with the precise work done by the exact keys and `sub` left coarse.
 *
 * ## Identifiers, not names
 *
 * The exact keys are `repository_id` and `repository_owner_id`, not `repository`. Organisation and
 * repository names are mutable, and a name freed by a rename can be claimed by another account, so
 * a condition on `repository` would refuse this repository after a rename *and* admit whoever took
 * the old name. The ids are immutable across a rename.
 *
 * The same reasoning shapes the `sub` patterns, one per format:
 *
 * ```text
 * previous:   repo:octo-org/octo-repo:ref:refs/heads/main
 * immutable:  repo:octo-org@123456/octo-repo@456789:ref:refs/heads/main
 * ```
 *
 * Repositories created after 15 July 2026 use the immutable format, and a rename or transfer moves
 * an existing one to it — carrying the *new* names. So the immutable pattern wildcards the name
 * positions and pins the ids. Wildcarding the ids instead would reinstate the recycled-namespace
 * risk the format exists to remove, leaving trust name-based again.
 *
 * Every condition in the block must hold, so the coarse `sub` match cannot widen what the exact
 * keys allow.
 *
 * A **transfer** is not covered: neither GitHub nor AWS documents whether a repository id survives
 * one, so the ids in `github-repositories.ts` have to be re-read after one.
 *
 * References: GitHub's OpenID Connect reference, "Immutable subject claims"; the IAM User Guide,
 * "Available keys for AWS OIDC federation", GitHub tab, "Use immutable identifiers, not names".
 */
export class GitHubOidcRole extends Construct {
    public readonly oidcRole: Role;

    constructor(scope: Construct, id: string, properties: Properties) {
        super(scope, id);

        // Tag the resources created by this construct to make identifying resources easier
        Tags.of(this).add("construct-name", properties.name);
        Tags.of(this).add("construct-type", "GitHubOidcRole");

        const {
            githubOrg: org,
            githubRepo: repo,
            githubOrgId: orgId,
            githubRepoId: repoId,
        } = properties;

        // A `sub` condition as well, because IAM recommends that every role trusting GitHub's
        // provider evaluate it and a role without one is the shape AWS's identity-provider
        // controls are designed to catch. Deliberately the coarse condition: the exact ids below
        // do the real authorising, and both have to match, so this cannot widen anything.
        const subPatterns = [
            // Previous format, which carries no ids. Only reachable while this repository has
            // neither been renamed nor opted in to immutable subjects.
            `repo:${org}/${repo}:*`,
            // Immutable format. Wildcards sit where the *names* go, so a rename still matches; the
            // ids are pinned, because wildcarding those is what would make this name-based trust.
            `repo:*@${orgId}/*@${repoId}:*`,
        ];

        /**
         * One alternative's conditions. Each gets a statement of its own, so that widening one
         * never widens another: a single block ANDs its conditions together, so an environment
         * required for `main` would also be required for a release tag.
         */
        const conditionsFor = (
            environment?: string,
            refs?: string[],
            jobWorkflowRefs?: string[],
        ): Record<string, Record<string, string | string[]>> => {
            // Reject the shapes that would produce an alternative nobody can match. A trust policy
            // that matches nothing fails closed, which is safe but silent, and a role that cannot
            // be assumed surfaces much later as an opaque permissions error in CI.
            if (environment !== undefined && environment.length === 0) {
                throw new Error(
                    `${properties.name}: environment is an empty string, which no job can match. ` +
                        "Omit it to allow any job in the repository.",
                );
            }
            if (refs !== undefined && refs.length === 0) {
                throw new Error(
                    `${properties.name}: refs is an empty array, which no job can match. ` +
                        "Omit it to allow any ref.",
                );
            }
            if (jobWorkflowRefs !== undefined && jobWorkflowRefs.length === 0) {
                throw new Error(
                    `${properties.name}: jobWorkflowRefs is an empty array, which no job can ` +
                        "match. Omit it to allow any workflow.",
                );
            }

            const stringEquals: Record<string, string> = {
                // Enforced by the provider's client ID list too, but IAM's own examples all carry
                // it and it costs one line.
                [`${CLAIM}:aud`]: "sts.amazonaws.com",
                // The ids, not `repository`: see the class docs on identifiers versus names.
                [`${CLAIM}:repository_owner_id`]: orgId,
                [`${CLAIM}:repository_id`]: repoId,
            };
            if (environment !== undefined) {
                stringEquals[`${CLAIM}:environment`] = environment;
            }

            const stringLike: Record<string, string | string[]> = {
                [`${CLAIM}:sub`]: subPatterns,
            };
            if (refs !== undefined) {
                stringLike[`${CLAIM}:ref`] = refs;
            }
            if (jobWorkflowRefs !== undefined) {
                stringLike[`${CLAIM}:job_workflow_ref`] = jobWorkflowRefs;
            }

            return { StringEquals: stringEquals, StringLike: stringLike };
        };

        const conditions = conditionsFor(
            properties.environment,
            properties.refs,
            properties.jobWorkflowRefs,
        );

        this.oidcRole = new Role(this, "oidc-role", {
            roleName: `${properties.name}-github-oidc-role`,
            assumedBy: new FederatedPrincipal(
                properties.oidcProvider.openIdConnectProviderArn,
                conditions,
                "sts:AssumeRoleWithWebIdentity",
            ),
        });

        for (const trust of properties.alsoTrust ?? []) {
            this.oidcRole.assumeRolePolicy?.addStatements(
                new PolicyStatement({
                    actions: ["sts:AssumeRoleWithWebIdentity"],
                    principals: [
                        new FederatedPrincipal(
                            properties.oidcProvider.openIdConnectProviderArn,
                            conditionsFor(trust.environment, trust.refs, trust.jobWorkflowRefs),
                        ),
                    ],
                }),
            );
        }
    }
}
