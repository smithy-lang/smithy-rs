/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import { Match, Template } from "aws-cdk-lib/assertions";
import { App, Stack } from "aws-cdk-lib";
import { GitHubOidcRole, Properties } from "../../lib/constructs/github-oidc-role";
import { OidcProviderStack } from "../../lib/oidc-provider-stack";

const CLAIM = "token.actions.githubusercontent.com";

/** The fixture repository: names, plus the ids the trust policy actually authorises on. */
const REPO = {
    githubOrg: "some-org",
    githubRepo: "some-repo",
    githubOrgId: "123456",
    githubRepoId: "456789",
} as const;

/** The coarse `sub` match, one pattern per subject format. */
const SUB_PATTERNS = ["repo:some-org/some-repo:*", "repo:*@123456/*@456789:*"];

/** The exact claims every role carries, whatever else it narrows on. */
const EXACT_CLAIMS = {
    [`${CLAIM}:aud`]: "sts.amazonaws.com",
    [`${CLAIM}:repository_owner_id`]: "123456",
    [`${CLAIM}:repository_id`]: "456789",
};

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
        ...REPO,
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
                            // The precise work is done by the exact keys, on the ids rather than
                            // the names: a name is mutable and a freed one can be claimed by
                            // another account, so `repository` would both lock this repository out
                            // after a rename and let its successor in.
                            StringEquals: EXACT_CLAIMS,
                            // `sub` stays, coarse, because IAM recommends every role trusting
                            // GitHub's provider evaluate it. One pattern per format.
                            StringLike: {
                                [`${CLAIM}:sub`]: SUB_PATTERNS,
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

test("it should authorise on the ids and never on the repository name", () => {
    // A condition on `repository` would change on a rename, and the name it frees can be taken by
    // another account, so it would refuse the right caller and admit the wrong one.
    const condition = trustConditionFor(REPO) as {
        StringEquals: Record<string, string>;
    };

    expect(condition.StringEquals).toEqual(EXACT_CLAIMS);
    expect(condition.StringEquals).not.toHaveProperty(`${CLAIM}:repository`);
});

test("it should keep a sub condition that survives both sub formats", () => {
    // IAM recommends every role trusting GitHub's provider evaluate `sub`, so it cannot simply be
    // dropped in favour of the exact claims. It carries one pattern per format instead: a single
    // `repo:ORG/REPO:*` pattern would stop matching the moment this repository were renamed or
    // opted in to immutable subjects, which would make the role unassumable.
    const condition = trustConditionFor(REPO) as { StringLike: Record<string, string[]> };

    expect(condition.StringLike[`${CLAIM}:sub`]).toEqual(SUB_PATTERNS);
});

test("the sub patterns should follow a rename and refuse another repository", () => {
    const [previous, immutable] = (
        trustConditionFor(REPO) as { StringLike: Record<string, string[]> }
    ).StringLike[`${CLAIM}:sub`];

    // IAM's StringLike treats `*` as any sequence of characters.
    const matches = (pattern: string, value: string): boolean =>
        new RegExp(`^${pattern.split("*").map(escapeRegExp).join(".*")}$`).test(value);

    expect(matches(previous, "repo:some-org/some-repo:ref:refs/heads/main")).toBe(true);
    expect(matches(immutable, "repo:some-org@123456/some-repo@456789:ref:refs/heads/main")).toBe(
        true,
    );

    // The point of pinning ids and wildcarding names: a rename moves a repository to the immutable
    // format carrying its *new* name, and the role stays assumable.
    expect(matches(immutable, "repo:some-org@123456/renamed-repo@456789:ref:refs/heads/main")).toBe(
        true,
    );

    // And the reverse of wildcarding the ids, which would leave trust purely name-based: a
    // repository or owner that merely shares a name gets no match, because its ids differ.
    for (const impostor of [
        "repo:some-org@999999/some-repo@456789:ref:refs/heads/main",
        "repo:some-org@123456/some-repo@999999:ref:refs/heads/main",
    ]) {
        expect(matches(immutable, impostor)).toBe(false);
    }

    // The previous-format pattern has no ids to pin, so it carries the whole names rather than a
    // prefix: `repo:some-org*/some-repo*:*` would also admit these.
    for (const impostor of [
        "repo:some-org-evil/some-repo:ref:refs/heads/main",
        "repo:some-org/some-repo-evil:ref:refs/heads/main",
    ]) {
        expect(matches(previous, impostor)).toBe(false);
    }
});

function escapeRegExp(literal: string): string {
    return literal.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

test("it should narrow to a GitHub environment when asked", () => {
    const condition = trustConditionFor({
        ...REPO,
        environment: "canary-alarms",
    });

    expect(condition).toEqual({
        StringEquals: {
            ...EXACT_CLAIMS,
            [`${CLAIM}:environment`]: "canary-alarms",
        },
        StringLike: { [`${CLAIM}:sub`]: SUB_PATTERNS },
    });
});

test("it should narrow to git refs when asked", () => {
    const condition = trustConditionFor({
        ...REPO,
        refs: ["refs/tags/release-*", "refs/heads/main"],
    });

    // Refs use StringLike because a pattern like `refs/tags/release-*` needs the wildcard, and the
    // ids stay exact matches.
    expect(condition).toEqual({
        StringEquals: EXACT_CLAIMS,
        StringLike: {
            [`${CLAIM}:sub`]: SUB_PATTERNS,
            [`${CLAIM}:ref`]: ["refs/tags/release-*", "refs/heads/main"],
        },
    });
});

test("it should combine an environment and refs", () => {
    const condition = trustConditionFor({
        ...REPO,
        environment: "canary-alarms",
        refs: ["refs/tags/release-*"],
    });

    expect(condition).toEqual({
        StringEquals: {
            ...EXACT_CLAIMS,
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
    expect(() => trustConditionFor({ ...REPO, refs: [] })).toThrow(/refs is an empty array/);

    expect(() => trustConditionFor({ ...REPO, environment: "" })).toThrow(
        /environment is an empty string/,
    );
});
