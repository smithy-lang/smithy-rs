/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import { OpenIdConnectProvider } from "aws-cdk-lib/aws-iam";
import { StackProps, Stack, Tags } from "aws-cdk-lib";
import { Construct } from "constructs";

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
            clientIds: ["sts.amazonaws.com"],
        });
    }
}
