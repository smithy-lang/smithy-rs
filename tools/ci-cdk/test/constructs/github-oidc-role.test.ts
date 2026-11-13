/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import { Match, Template } from "aws-cdk-lib/assertions";
import { App, Stack } from "aws-cdk-lib";
import { GitHubOidcRole, Properties } from "../../lib/constructs/github-oidc-role";
import { OidcProviderStack } from "../../lib/oidc-provider-stack";

const CLAIM = "token.actions.githubusercontent.com";

/** The coarse `sub` match, one pattern per subject format, for `some-org/some-repo`. */
const SUB_PATTERNS = ["repo:some-org/some-repo:*", "repo:some-org@*/some-repo@*:*"];

/** Build a role and return the one trust-policy condition block it produced. */
function trustConditionFor(properties: Omit<Properties, "oidcProvider" | "name">): unknown {
    const app = new App();
    const oidcStack = new OidcProviderStack(app, "oidc-provider-stack", {});
    const stack = new Stack(app, "test-stack");

    new GitHubOidcRole(stack, "test", {
        name: "some-name",
        oidcProvider: oidcStack.githubActionsOidcProvider,
        ...properties,
    });

    const roles = Template.fromStack(stack).findResources("AWS::IAM::Role");
    const statements = Object.values(roles)[0].Properties.AssumeRolePolicyDocument.Statement;
    expect(statements).toHaveLength(1);
    return statements[0].Condition;
}

test("it should have an OIDC access role", () => {
    const app = new App();
    const oidcStack = new OidcProviderStack(app, "oidc-provider-stack", {});
    const stack = new Stack(app, "test-stack");

    new GitHubOidcRole(stack, "test", {
        name: "some-name",
        githubOrg: "some-org",
        githubRepo: "some-repo",
        oidcProvider: oidcStack.githubActionsOidcProvider,
    });
    const template = Template.fromStack(stack);

    // Verify the OIDC role to be assumed
    template.hasResourceProperties(
        "AWS::IAM::Role",
        Match.objectEquals({
            AssumeRolePolicyDocument: {
                Statement: [
                    {
                        Action: "sts:AssumeRoleWithWebIdentity",
                        Condition: {
                            // The precise work is done by the exact keys. `repository` admits the
                            // same set the old `sub LIKE "repo:ORG/REPO:*"` wildcard did, and
                            // unlike `sub` it is unaffected by the immutable subject format.
                            StringEquals: {
                                [`${CLAIM}:aud`]: "sts.amazonaws.com",
                                [`${CLAIM}:repository`]: "some-org/some-repo",
                            },
                            // `sub` stays, coarse, because IAM recommends every role trusting
                            // GitHub's provider evaluate it. One pattern per format, so a rename
                            // cannot make the role unassumable.
                            StringLike: {
                                [`${CLAIM}:sub`]: [
                                    "repo:some-org/some-repo:*",
                                    "repo:some-org@*/some-repo@*:*",
                                ],
                            },
                        },
                        Principal: {
                            Federated: {
                                "Fn::ImportValue": Match.anyValue(),
                            },
                        },
                        Effect: "Allow",
                    },
                ],
                Version: "2012-10-17",
            },
            RoleName: "some-name-github-oidc-role",
            Tags: [
                {
                    Key: "construct-name",
                    Value: "some-name",
                },
                {
                    Key: "construct-type",
                    Value: "GitHubOidcRole",
                },
            ],
        }),
    );
});

test("it should keep a sub condition that survives both sub formats", () => {
    // IAM recommends every role trusting GitHub's provider evaluate `sub`, so it cannot simply be
    // dropped in favour of the exact claims. It carries one pattern per format instead: a single
    // `repo:ORG/REPO:*` pattern would stop matching the moment this repository were renamed or
    // opted in to immutable subjects, which would make the role unassumable.
    const condition = trustConditionFor({
        githubOrg: "some-org",
        githubRepo: "some-repo",
    }) as { StringLike: Record<string, string[]> };

    expect(condition.StringLike[`${CLAIM}:sub`]).toEqual([
        "repo:some-org/some-repo:*",
        "repo:some-org@*/some-repo@*:*",
    ]);
});

test("the sub patterns should match both formats and not a neighbouring repository", () => {
    const [previous, immutable] = (
        trustConditionFor({ githubOrg: "some-org", githubRepo: "some-repo" }) as {
            StringLike: Record<string, string[]>;
        }
    ).StringLike[`${CLAIM}:sub`];

    // IAM's StringLike treats `*` as any sequence of characters.
    const matches = (pattern: string, value: string): boolean =>
        new RegExp(`^${pattern.split("*").map(escapeRegExp).join(".*")}$`).test(value);

    expect(matches(previous, "repo:some-org/some-repo:ref:refs/heads/main")).toBe(true);
    expect(matches(immutable, "repo:some-org@123456/some-repo@456789:ref:refs/heads/main")).toBe(
        true,
    );

    // The reason for two explicit patterns rather than wildcards inside the segments: a pattern
    // like `repo:some-org*/some-repo*:*` would also admit these.
    for (const impostor of [
        "repo:some-org-evil/some-repo:ref:refs/heads/main",
        "repo:some-org/some-repo-evil:ref:refs/heads/main",
    ]) {
        expect(matches(previous, impostor)).toBe(false);
        expect(matches(immutable, impostor)).toBe(false);
    }
});

function escapeRegExp(literal: string): string {
    return literal.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

test("it should narrow to a GitHub environment when asked", () => {
    const condition = trustConditionFor({
        githubOrg: "some-org",
        githubRepo: "some-repo",
        environment: "canary-alarms",
    });

    expect(condition).toEqual({
        StringEquals: {
            [`${CLAIM}:aud`]: "sts.amazonaws.com",
            [`${CLAIM}:repository`]: "some-org/some-repo",
            [`${CLAIM}:environment`]: "canary-alarms",
        },
        StringLike: { [`${CLAIM}:sub`]: SUB_PATTERNS },
    });
});

test("it should narrow to git refs when asked", () => {
    const condition = trustConditionFor({
        githubOrg: "some-org",
        githubRepo: "some-repo",
        refs: ["refs/tags/release-*", "refs/heads/main"],
    });

    // Refs use StringLike because a pattern like `refs/tags/release-*` needs the wildcard, and the
    // repository stays an exact match.
    expect(condition).toEqual({
        StringEquals: {
            [`${CLAIM}:aud`]: "sts.amazonaws.com",
            [`${CLAIM}:repository`]: "some-org/some-repo",
        },
        StringLike: {
            [`${CLAIM}:sub`]: SUB_PATTERNS,
            [`${CLAIM}:ref`]: ["refs/tags/release-*", "refs/heads/main"],
        },
    });
});

test("it should combine an environment and refs", () => {
    const condition = trustConditionFor({
        githubOrg: "some-org",
        githubRepo: "some-repo",
        environment: "canary-alarms",
        refs: ["refs/tags/release-*"],
    });

    expect(condition).toEqual({
        StringEquals: {
            [`${CLAIM}:aud`]: "sts.amazonaws.com",
            [`${CLAIM}:repository`]: "some-org/some-repo",
            [`${CLAIM}:environment`]: "canary-alarms",
        },
        StringLike: {
            [`${CLAIM}:sub`]: SUB_PATTERNS,
            [`${CLAIM}:ref`]: ["refs/tags/release-*"],
        },
    });
});

test("it should reject narrowing that nothing could ever match", () => {
    // Both of these would synthesize a role that fails closed -- safe, but silently unassumable,
    // which surfaces much later as an opaque permissions error in CI.
    expect(() =>
        trustConditionFor({
            githubOrg: "some-org",
            githubRepo: "some-repo",
            refs: [],
        }),
    ).toThrow(/refs is an empty array/);

    expect(() =>
        trustConditionFor({
            githubOrg: "some-org",
            githubRepo: "some-repo",
            environment: "",
        }),
    ).toThrow(/environment is an empty string/);
});
