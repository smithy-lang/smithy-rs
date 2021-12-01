/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

import { FederatedPrincipal, OpenIdConnectProvider, Role } from "@aws-cdk/aws-iam";
import { Construct } from "@aws-cdk/core";

/// This thumbprint is used to validate GitHub's identity to AWS.
///
/// It was obtained by following instructions at:
/// https://docs.aws.amazon.com/IAM/latest/UserGuide/id_roles_providers_create_oidc_verify-thumbprint.html
///
/// This was done with the initial Idp URL of:
/// https://token.actions.githubusercontent.com/.well-known/openid-configuration
const GITHUB_CERTIFICATE_THUMBPRINT = "A031C46782E6E6C662C2C87C76DA9AA62CCABD8E";

export interface Properties {
    name: string;
    githubOrg: string;
    githubRepo: string;
}

export class GitHubOidcRole extends Construct {
    public readonly oidcProvider: OpenIdConnectProvider;
    public readonly oidcRole: Role;

    constructor(scope: Construct, id: string, properties: Properties) {
        super(scope, id);

        this.oidcProvider = new OpenIdConnectProvider(this, "oidc-provider", {
            url: "https://token.actions.githubusercontent.com",
            thumbprints: [GITHUB_CERTIFICATE_THUMBPRINT],
            clientIds: ["sts.amazonaws.com"],
        });

        this.oidcRole = new Role(this, "oidc-role", {
            roleName: `${properties.name}-github-oidc-role`,
            assumedBy: new FederatedPrincipal(
                this.oidcProvider.openIdConnectProviderArn,
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
