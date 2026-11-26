/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Shared by the tests of every stack that creates a `GitHubOidcRole`.
//!
//! What these stacks' security turns on is whether a given token can assume a given role, and a
//! condition block read side by side does not answer that. So the tests assert the outcome through
//! a small evaluator rather than against rendered JSON, and the evaluator lives here because three
//! stacks need the same one.

import { Template } from "aws-cdk-lib/assertions";

/** The OIDC provider's claims appear as IAM condition keys under this prefix. */
export const CLAIM = "token.actions.githubusercontent.com";

export interface Statement {
    Action: string | string[];
    Effect?: string;
    Resource?: string | string[];
    Condition?: Record<string, Record<string, string | string[]>>;
}

/** A job's OIDC claims, as the provider presents them. */
export type Claims = Record<string, string>;

/** A repository as `lib/github-repositories` describes it. */
export interface Repository {
    githubOrg: string;
    githubRepo: string;
    githubOrgId: string;
    githubRepoId: string;
}

/** Every trust statement on the role whose name contains `-${suffix}-`, in order. */
export function trustStatements(template: Template, suffix: string): Statement[] {
    const roles = Object.values(template.findResources("AWS::IAM::Role")).filter((r) =>
        String(r.Properties.RoleName).includes(`-${suffix}-`),
    );
    expect(roles).toHaveLength(1);
    return roles[0].Properties.AssumeRolePolicyDocument.Statement;
}

/**
 * The claims GitHub puts in a token for a job at `ref`, defined in `workflow`.
 *
 * Every token carries `job_workflow_ref` -- GitHub sets it for ordinary jobs, not only for jobs in
 * reusable workflows -- so a test that omitted it would be modelling a token that does not exist,
 * and would pass against a role it should not be able to assume.
 */
export function claimsFor(
    repository: Repository,
    ref: string,
    workflow: string,
    environment?: string,
): Claims {
    const { githubOrg, githubRepo, githubOrgId, githubRepoId } = repository;
    const claims: Claims = {
        [`${CLAIM}:aud`]: "sts.amazonaws.com",
        [`${CLAIM}:repository_owner_id`]: githubOrgId,
        [`${CLAIM}:repository_id`]: githubRepoId,
        [`${CLAIM}:sub`]: `repo:${githubOrg}/${githubRepo}:ref:${ref}`,
        [`${CLAIM}:ref`]: ref,
        [`${CLAIM}:job_workflow_ref`]: `${githubOrg}/${githubRepo}/.github/workflows/${workflow}@${ref}`,
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
 * A condition on a claim the token does not carry matches nothing, which is how IAM behaves and is
 * the direction that matters.
 */
export function admits(statements: Statement[], claims: Claims): boolean {
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
