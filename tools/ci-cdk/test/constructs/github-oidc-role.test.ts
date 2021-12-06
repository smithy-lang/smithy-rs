/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

import { Match, Template } from "@aws-cdk/assertions";
import * as cdk from "@aws-cdk/core";
import { Stack } from "@aws-cdk/core";
import { GitHubOidcRole } from "../../lib/constructs/github-oidc-role";

test("it should have an OIDC provider and access role", () => {
    const app = new cdk.App();
    const stack = new Stack(app, "test-stack");

    new GitHubOidcRole(stack, "test", {
        name: "some-name",
        githubOrg: "some-org",
        githubRepo: "some-repo",
    });
    const template = Template.fromStack(stack);

    // Verify the OIDC provider
    template.hasResourceProperties("Custom::AWSCDKOpenIdConnectProvider", {
        ClientIDList: ["sts.amazonaws.com"],
        ThumbprintList: ["A031C46782E6E6C662C2C87C76DA9AA62CCABD8E"],
        Url: "https://token.actions.githubusercontent.com",
    });

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
                                Ref: Match.anyValue(),
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
