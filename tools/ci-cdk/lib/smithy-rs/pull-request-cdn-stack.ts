/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

import * as cdk from "@aws-cdk/core";
import { Duration, RemovalPolicy, Tags } from "@aws-cdk/core";
import { CloudFrontS3Cdn } from "../constructs/cloudfront-s3-cdn";
import { GitHubOidcRole } from "../constructs/github-oidc-role";

export class PullRequestCdnStack extends cdk.Stack {
    public readonly smithyRsOidcRole: GitHubOidcRole;
    public readonly pullRequestCdn: CloudFrontS3Cdn;

    constructor(scope: cdk.Construct, id: string, props?: cdk.StackProps) {
        super(scope, id, props);

        // Tag the resources created by this stack to make identifying resources easier
        Tags.of(this).add("stack", id);

        this.smithyRsOidcRole = new GitHubOidcRole(this, "smithy-rs", {
            name: "smithy-rs-pull-request",
            githubOrg: "awslabs",
            githubRepo: "smithy-rs",
        });

        this.pullRequestCdn = new CloudFrontS3Cdn(this, "pull-request-cdn", {
            name: "smithy-rs-pull-request",
            lifecycleRules: [
                {
                    id: "delete-old-codegen-diffs",
                    enabled: true,
                    expiration: Duration.days(90),
                    prefix: "codegen-diff/",
                },
                {
                    id: "delete-old-docs",
                    enabled: true,
                    // The docs are huge, so keep them for a much shorter period of time
                    expiration: Duration.days(14),
                    prefix: "docs/",
                },
            ],
            // Delete the bucket and all its files if deleting the stack
            removalPolicy: RemovalPolicy.DESTROY,
        });

        // Grant the OIDC role permission to write to the CDN bucket
        this.pullRequestCdn.bucket.grantWrite(this.smithyRsOidcRole.oidcRole);
    }
}
