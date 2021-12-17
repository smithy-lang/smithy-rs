/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

import {
    Effect,
    OpenIdConnectProvider,
    PolicyStatement,
    Role,
    ServicePrincipal,
} from "@aws-cdk/aws-iam";
import { BlockPublicAccess, Bucket, BucketEncryption } from "@aws-cdk/aws-s3";
import { Construct, StackProps, Stack, Tags, RemovalPolicy, Duration } from "@aws-cdk/core";
import { GitHubOidcRole } from "../constructs/github-oidc-role";

export interface Properties extends StackProps {
    githubActionsOidcProvider: OpenIdConnectProvider;
}

export class CanaryStack extends Stack {
    public readonly awsSdkRustOidcRole: GitHubOidcRole;
    public readonly lambdaExecutionRole: Role;
    public readonly canaryCodeBucket: Bucket;
    public readonly canaryTestBucket: Bucket;

    constructor(scope: Construct, id: string, props: Properties) {
        super(scope, id, props);

        // Tag the resources created by this stack to make identifying resources easier
        Tags.of(this).add("stack", id);

        this.awsSdkRustOidcRole = new GitHubOidcRole(this, "aws-sdk-rust", {
            name: "aws-sdk-rust-canary",
            githubOrg: "awslabs",
            githubRepo: "aws-sdk-rust",
            oidcProvider: props.githubActionsOidcProvider,
        });

        // Grant permission to create/invoke/delete a canary Lambda
        this.awsSdkRustOidcRole.oidcRole.addToPolicy(
            new PolicyStatement({
                actions: [
                    "lambda:CreateFunction",
                    "lambda:DeleteFunction",
                    "lambda:InvokeFunction",
                    "lambda:GetFunctionConfiguration",
                ],
                effect: Effect.ALLOW,
                // Only allow this for functions starting with prefix `canary-`
                resources: ["arn:aws:lambda:*:*:function:canary-*"],
            }),
        );

        // Create S3 bucket to upload canary Lambda code into
        this.canaryCodeBucket = new Bucket(this, "canary-code-bucket", {
            blockPublicAccess: BlockPublicAccess.BLOCK_ALL,
            encryption: BucketEncryption.S3_MANAGED,
            lifecycleRules: [
                {
                    id: "delete-old-code-files",
                    enabled: true,
                    expiration: Duration.days(7),
                },
            ],
            versioned: false,
            removalPolicy: RemovalPolicy.DESTROY,
        });

        // Allow the OIDC role to GetObject and PutObject to the code bucket
        this.canaryCodeBucket.grantRead(this.awsSdkRustOidcRole.oidcRole);
        this.canaryCodeBucket.grantWrite(this.awsSdkRustOidcRole.oidcRole);

        // Create S3 bucket for the canaries to talk to
        this.canaryTestBucket = new Bucket(this, "canary-test-bucket", {
            blockPublicAccess: BlockPublicAccess.BLOCK_ALL,
            encryption: BucketEncryption.S3_MANAGED,
            lifecycleRules: [
                {
                    id: "delete-old-test-files",
                    enabled: true,
                    expiration: Duration.days(7),
                },
            ],
            versioned: false,
            removalPolicy: RemovalPolicy.DESTROY,
        });

        // Create a role for the canary Lambdas to assume
        this.lambdaExecutionRole = new Role(this, "lambda-execution-role", {
            roleName: "aws-sdk-rust-canary-lambda-exec-role",
            assumedBy: new ServicePrincipal("lambda.amazonaws.com"),
        });

        // Allow canaries to write logs to CloudWatch
        this.lambdaExecutionRole.addToPolicy(
            new PolicyStatement({
                actions: ["logs:CreateLogGroup", "logs:CreateLogStream", "logs:PutLogEvents"],
                effect: Effect.ALLOW,
                resources: ["arn:aws:logs:*:*:/aws/lambda/canary-lambda-*:*"],
            }),
        );

        // Allow canaries to talk to their test bucket
        this.canaryTestBucket.grantReadWrite(this.lambdaExecutionRole);

        // Allow canaries to call Transcribe's StartStreamTranscription
        this.lambdaExecutionRole.addToPolicy(
            new PolicyStatement({
                actions: ["transcribe:StartStreamTranscription"],
                effect: Effect.ALLOW,
                resources: ["*"],
            }),
        );

        // Allow the OIDC role to pass the Lambda execution role to Lambda
        this.awsSdkRustOidcRole.oidcRole.addToPolicy(
            new PolicyStatement({
                actions: ["iam:PassRole"],
                effect: Effect.ALLOW,
                // Security: only allow the Lambda execution role to be passed
                resources: [this.lambdaExecutionRole.roleArn],
                // Security: only allow the role to be passed to Lambda
                conditions: {
                    StringEquals: {
                        "iam:PassedToService": "lambda.amazonaws.com",
                    },
                },
            }),
        );
    }
}
