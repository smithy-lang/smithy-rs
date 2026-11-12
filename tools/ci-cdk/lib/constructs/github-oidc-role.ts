/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import { FederatedPrincipal, OpenIdConnectProvider, Role } from "aws-cdk-lib/aws-iam";
import { Construct } from "constructs";
import { Tags } from "aws-cdk-lib";

/** The OIDC provider's claims appear as IAM condition keys under this prefix. */
const CLAIM = "token.actions.githubusercontent.com";

export interface Properties {
    name: string;
    githubOrg: string;
    githubRepo: string;
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
 * So both are present, with the precise work done by the exact keys and `sub` left coarse. The
 * `sub` condition carries one pattern per format, because repositories created after 15 July 2026
 * use an *immutable* `sub` embedding the owner and repository IDs, and a rename or transfer moves
 * an existing repository to it:
 *
 * ```text
 * previous:   repo:octo-org/octo-repo:ref:refs/heads/main
 * immutable:  repo:octo-org@123456/octo-repo@456789:ref:refs/heads/main
 * ```
 *
 * The IDs land inside the `repo` segment rather than at the end, so adding a trailing wildcard
 * achieves nothing and a single `repo:ORG/REPO:*` pattern silently stops matching — every role here
 * would become unassumable the moment this repository were renamed. Listing both patterns avoids
 * that. The wildcards in the immutable pattern sit only where an ID goes, after an `@`, rather than
 * standing in for the org and repository names, so neither pattern can match a *neighbouring* org
 * or a repository whose name merely starts the same way.
 *
 * Every condition in the block must hold, so the coarse `sub` match cannot widen what the exact
 * `repository` match allows.
 *
 * References: GitHub's OpenID Connect reference, "Immutable subject claims"; the IAM User Guide,
 * "Available keys for AWS OIDC federation", GitHub tab.
 */
export class GitHubOidcRole extends Construct {
    public readonly oidcRole: Role;

    constructor(scope: Construct, id: string, properties: Properties) {
        super(scope, id);

        // Tag the resources created by this construct to make identifying resources easier
        Tags.of(this).add("construct-name", properties.name);
        Tags.of(this).add("construct-type", "GitHubOidcRole");

        // Reject the shapes that would produce a role nobody can assume. A trust policy that
        // matches nothing fails closed, which is safe but silent, and a role that cannot be assumed
        // surfaces much later as an opaque permissions error in CI.
        if (properties.environment !== undefined && properties.environment.length === 0) {
            throw new Error(
                `${properties.name}: environment is an empty string, which no job can match. ` +
                    "Omit it to allow any job in the repository.",
            );
        }
        if (properties.refs !== undefined && properties.refs.length === 0) {
            throw new Error(
                `${properties.name}: refs is an empty array, which no job can match. ` +
                    "Omit it to allow any ref.",
            );
        }

        const { githubOrg: org, githubRepo: repo } = properties;

        const stringEquals: Record<string, string> = {
            // Enforced by the provider's client ID list too, but IAM's own examples all carry it
            // and it costs one line.
            [`${CLAIM}:aud`]: "sts.amazonaws.com",
            [`${CLAIM}:repository`]: `${org}/${repo}`,
        };
        if (properties.environment !== undefined) {
            stringEquals[`${CLAIM}:environment`] = properties.environment;
        }

        // A `sub` condition as well, for two reasons. IAM recommends that every role trusting
        // GitHub's provider evaluate `sub`, and a role without one is the shape AWS's own
        // identity-provider controls are designed to catch. It is deliberately the coarse
        // condition here: one pattern per `sub` format, so neither a rename nor an opt-in to
        // immutable subjects can make the role unassumable, while the exact `repository` match
        // above does the real authorising. Both have to match, so the coarse one cannot widen
        // anything.
        const subPatterns = [
            `repo:${org}/${repo}:*`, // previous format
            `repo:${org}@*/${repo}@*:*`, // immutable format, any owner and repository IDs
        ];
        const stringLike: Record<string, string | string[]> = {
            [`${CLAIM}:sub`]: subPatterns,
        };
        if (properties.refs !== undefined) {
            stringLike[`${CLAIM}:ref`] = properties.refs;
        }

        const conditions: Record<string, Record<string, string | string[]>> = {
            StringEquals: stringEquals,
            StringLike: stringLike,
        };

        this.oidcRole = new Role(this, "oidc-role", {
            roleName: `${properties.name}-github-oidc-role`,
            assumedBy: new FederatedPrincipal(
                properties.oidcProvider.openIdConnectProviderArn,
                conditions,
                "sts:AssumeRoleWithWebIdentity",
            ),
        });
    }
}
