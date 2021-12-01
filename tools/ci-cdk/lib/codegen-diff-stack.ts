/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

import * as cdk from "@aws-cdk/core";
import { Duration, RemovalPolicy, Tags } from "@aws-cdk/core";
import { CloudFrontS3Cdn } from "./constructs/cloudfront-s3-cdn";
import { GitHubOidcRole } from "./constructs/github-oidc-role";

export class CodeGenDiffStack extends cdk.Stack {
    public readonly smithyRsOidcRole: GitHubOidcRole;
    public readonly diffCdn: CloudFrontS3Cdn;

    constructor(scope: cdk.Construct, id: string, props?: cdk.StackProps) {
        super(scope, id, props);

        // Tag the resources created by this stack to make identifying resources easier
        Tags.of(this).add("stack", id);

        this.smithyRsOidcRole = new GitHubOidcRole(this, "smithy-rs", {
            name: "codegen-diff-smithy-rs",
            githubOrg: "awslabs",
            githubRepo: "smithy-rs",
        });

        this.diffCdn = new CloudFrontS3Cdn(this, "diff-cdn", {
            name: "codegen-diff-smithy-rs",
            lifecycleRules: [
                {
                    id: "delete-old-diffs",
                    enabled: true,
                    expiration: Duration.days(90),
                },
            ],
            // Delete the bucket and all its files if deleting the stack
            removalPolicy: RemovalPolicy.DESTROY,
        });

        // Grant the OIDC role permission to write to the CDN bucket
        this.diffCdn.bucket.grantWrite(this.smithyRsOidcRole.oidcRole);
    }
}
