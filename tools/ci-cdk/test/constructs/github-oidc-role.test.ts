/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import { Match, Template } from "aws-cdk-lib/assertions";
import { App, Stack } from "aws-cdk-lib";
import { GitHubOidcRole } from "../../lib/constructs/github-oidc-role";
import { OidcProviderStack } from "../../lib/oidc-provider-stack";

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
                            StringLike: {
                                "token.actions.githubusercontent.com:sub":
                                    "repo:some-org/some-repo:*",
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
