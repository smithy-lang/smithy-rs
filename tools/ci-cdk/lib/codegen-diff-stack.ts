/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

import * as cdk from "@aws-cdk/core";
import { GitHubOidcRole } from "./constructs/github-oidc-role";

export class CodeGenDiffStack extends cdk.Stack {
    public readonly smithyRsOidcRole: GitHubOidcRole;

    constructor(scope: cdk.Construct, id: string, props?: cdk.StackProps) {
        super(scope, id, props);

        this.smithyRsOidcRole = new GitHubOidcRole(this, "smithy-rs", {
            name: "codegen-diff-smithy-rs",
            githubOrg: "awslabs",
            githubRepo: "smithy-rs",
        });
    }
}
