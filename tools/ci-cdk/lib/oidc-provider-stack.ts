/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import { OpenIdConnectProvider } from "aws-cdk-lib/aws-iam";
import { StackProps, Stack, Tags } from "aws-cdk-lib";
import { Construct } from "constructs";

/// This thumbprint is used to validate GitHub's identity to AWS. This is
/// just a SHA-1 hash of the top intermediate certificate authority's certificate.
/// It may need to be updated when GitHub's certificate renews and this
/// thumbprint changes.
///
/// It was obtained by following instructions at:
/// https://docs.aws.amazon.com/IAM/latest/UserGuide/id_roles_providers_create_oidc_verify-thumbprint.html
///
/// This was done with the initial Idp URL of:
/// https://token.actions.githubusercontent.com/.well-known/openid-configuration
///
/// Note: as of June 27, 2023, there are now two possible thumbprints from GitHub:
/// https://github.blog/changelog/2023-06-27-github-actions-update-on-oidc-integration-with-aws/
export const GITHUB_CERTIFICATE_THUMBPRINTS = [
    "6938FD4D98BAB03FAADB97B34396831E3780AEA1",
    "1C58A3A8518E8759BF075B76B750D4F2DF264FCD",
];

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
            thumbprints: GITHUB_CERTIFICATE_THUMBPRINTS,
            clientIds: ["sts.amazonaws.com"],
        });
    }
}
