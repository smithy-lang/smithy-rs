/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import { FederatedPrincipal, OpenIdConnectProvider, Role } from "aws-cdk-lib/aws-iam";
import { Construct } from "constructs";
import { Tags } from "aws-cdk-lib";

export interface Properties {
    name: string;
    githubOrg: string;
    githubRepo: string;
    oidcProvider: OpenIdConnectProvider;
}

export class GitHubOidcRole extends Construct {
    public readonly oidcRole: Role;

    constructor(scope: Construct, id: string, properties: Properties) {
        super(scope, id);

        // Tag the resources created by this construct to make identifying resources easier
        Tags.of(this).add("construct-name", properties.name);
        Tags.of(this).add("construct-type", "GitHubOidcRole");

        this.oidcRole = new Role(this, "oidc-role", {
            roleName: `${properties.name}-github-oidc-role`,
            assumedBy: new FederatedPrincipal(
                properties.oidcProvider.openIdConnectProviderArn,
                {
                    StringLike: {
                        "token.actions.githubusercontent.com:sub": `repo:${properties.githubOrg}/${properties.githubRepo}:*`,
                    },
                },
                "sts:AssumeRoleWithWebIdentity",
            ),
        });
    }
}
