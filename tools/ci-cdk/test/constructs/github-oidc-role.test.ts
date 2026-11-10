/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import { Match, Template } from "aws-cdk-lib/assertions";
import { App, Stack } from "aws-cdk-lib";
import { GitHubOidcRole, Properties } from "../../lib/constructs/github-oidc-role";
import { OidcProviderStack } from "../../lib/oidc-provider-stack";

const CLAIM = "token.actions.githubusercontent.com";

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
                            // Keyed on `repository`, not on `sub`. For jobs in a given repository
                            // this admits exactly the set that `sub LIKE "repo:ORG/REPO:*"` used
                            // to, and unlike `sub` it is unaffected by the immutable subject
                            // format that a rename or transfer now switches a repository to.
                            StringEquals: {
                                [`${CLAIM}:repository`]: "some-org/some-repo",
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

test("it should not condition on the sub claim at all", () => {
    // The whole point of the change: `sub` is the claim whose format moves under us, so no role
    // built by this construct may depend on it.
    const condition = trustConditionFor({
        githubOrg: "some-org",
        githubRepo: "some-repo",
    });
    expect(JSON.stringify(condition)).not.toContain(":sub");
});

test("it should narrow to a GitHub environment when asked", () => {
    const condition = trustConditionFor({
        githubOrg: "some-org",
        githubRepo: "some-repo",
        environment: "canary-alarms",
    });

    expect(condition).toEqual({
        StringEquals: {
            [`${CLAIM}:repository`]: "some-org/some-repo",
            [`${CLAIM}:environment`]: "canary-alarms",
        },
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
            [`${CLAIM}:repository`]: "some-org/some-repo",
        },
        StringLike: {
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
            [`${CLAIM}:repository`]: "some-org/some-repo",
            [`${CLAIM}:environment`]: "canary-alarms",
        },
        StringLike: {
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
