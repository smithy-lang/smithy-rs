/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

import { OpenIdConnectProvider } from "@aws-cdk/aws-iam";
import { Construct, StackProps, Stack, Tags } from "@aws-cdk/core";

/// This thumbprint is used to validate GitHub's identity to AWS.
///
/// It was obtained by following instructions at:
/// https://docs.aws.amazon.com/IAM/latest/UserGuide/id_roles_providers_create_oidc_verify-thumbprint.html
///
/// This was done with the initial Idp URL of:
/// https://token.actions.githubusercontent.com/.well-known/openid-configuration
const GITHUB_CERTIFICATE_THUMBPRINT = "A031C46782E6E6C662C2C87C76DA9AA62CCABD8E";

// There can only be one OIDC provider for a given URL per AWS account,
// so put these in their own stack to be shared with other stacks.
export class OidcProviderStack extends Stack {
    public readonly githubActionsOidcProvider: OpenIdConnectProvider;

    constructor(scope: Construct, id: string, props?: StackProps) {
        super(scope, id, props);

        // Tag the resources created by this stack to make identifying resources easier
        Tags.of(this).add("stack", id);

        this.githubActionsOidcProvider = new OpenIdConnectProvider(this, "oidc-provider", {
            url: "https://token.actions.githubusercontent.com",
            thumbprints: [GITHUB_CERTIFICATE_THUMBPRINT],
            clientIds: ["sts.amazonaws.com"],
        });
    }
}
