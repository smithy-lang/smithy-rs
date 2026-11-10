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
 * ## Why this does not condition on `sub`
 *
 * The obvious trust condition is the `sub` claim, which looks like
 * `repo:ORG/REPO:ref:refs/heads/main`, and that is what this construct used to do with a
 * `repo:ORG/REPO:*` wildcard. It is the wrong claim to key on.
 *
 * Repositories created after 15 July 2026 use an *immutable* `sub` that embeds the owner and
 * repository IDs, and renames or transfers after that date move an existing repository to it too:
 *
 * ```text
 * previous:   repo:octo-org/octo-repo:ref:refs/heads/main
 * immutable:  repo:octo-org@123456/octo-repo@456789:ref:refs/heads/main
 * ```
 *
 * The IDs are inserted into the `repo` segment rather than appended, so a `repo:ORG/REPO:*` pattern
 * silently stops matching — every role built by this construct would become unassumable the moment
 * the repository is renamed or opts in. Tolerating both shapes through `sub` would need wildcards
 * inside the org and repo segments, which then also match a *neighbouring* org or repository name.
 *
 * So condition on the dedicated claims instead. `repository`, `environment` and `ref` are separate
 * claims, unaffected by the `sub` format, and they allow exact `StringEquals` rather than a
 * wildcard. For jobs in a given repository, `repository == "ORG/REPO"` admits exactly the set that
 * `sub LIKE "repo:ORG/REPO:*"` used to, so this is equivalent today and survives the change.
 *
 * Reference: GitHub's OpenID Connect reference, "Immutable subject claims".
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

        const stringEquals: Record<string, string> = {
            [`${CLAIM}:repository`]: `${properties.githubOrg}/${properties.githubRepo}`,
        };
        if (properties.environment !== undefined) {
            stringEquals[`${CLAIM}:environment`] = properties.environment;
        }

        const conditions: Record<string, Record<string, string | string[]>> = {
            StringEquals: stringEquals,
        };
        if (properties.refs !== undefined) {
            conditions.StringLike = { [`${CLAIM}:ref`]: properties.refs };
        }

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
