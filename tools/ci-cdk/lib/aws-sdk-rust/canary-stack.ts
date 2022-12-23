/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import {
    Effect,
    OpenIdConnectProvider,
    PolicyStatement,
    Role,
    ServicePrincipal,
} from "aws-cdk-lib/aws-iam";
import { BlockPublicAccess, Bucket, BucketEncryption } from "aws-cdk-lib/aws-s3";
import { StackProps, Stack, Tags, RemovalPolicy, Duration, CfnOutput } from "aws-cdk-lib";
import { Construct } from "constructs";
import { GitHubOidcRole } from "../constructs/github-oidc-role";

export interface Properties extends StackProps {
    githubActionsOidcProvider?: OpenIdConnectProvider;
}

export class CanaryStack extends Stack {
    public readonly awsSdkRustOidcRole?: GitHubOidcRole;
    public readonly lambdaExecutionRole: Role;
    public readonly canaryCodeBucket: Bucket;
    public readonly canaryTestBucket: Bucket;

    public readonly lambdaExecutionRoleArn: CfnOutput;
    public readonly canaryCodeBucketName: CfnOutput;
    public readonly canaryTestBucketName: CfnOutput;

    constructor(scope: Construct, id: string, props: Properties) {
        super(scope, id, props);

        // Tag the resources created by this stack to make identifying resources easier
        Tags.of(this).add("stack", id);

        if (props.githubActionsOidcProvider) {
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

            // Grant permission to put metric data to CloudWatch
            this.awsSdkRustOidcRole.oidcRole.addToPolicy(
                new PolicyStatement({
                    actions: ["cloudwatch:PutMetricData"],
                    effect: Effect.ALLOW,
                    resources: ["*"],
                }),
            );
        }

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

        // Output the bucket name to make it easier to invoke the canary runner
        this.canaryCodeBucketName = new CfnOutput(this, "canary-code-bucket-name", {
            value: this.canaryCodeBucket.bucketName,
            description: "Name of the canary code bucket",
            exportName: "canaryCodeBucket",
        });

        // Allow the OIDC role to GetObject and PutObject to the code bucket
        if (this.awsSdkRustOidcRole) {
            this.canaryCodeBucket.grantRead(this.awsSdkRustOidcRole.oidcRole);
            this.canaryCodeBucket.grantWrite(this.awsSdkRustOidcRole.oidcRole);
        }

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

        // Output the bucket name to make it easier to invoke the canary runner
        this.canaryTestBucketName = new CfnOutput(this, "canary-test-bucket-name", {
            value: this.canaryTestBucket.bucketName,
            description: "Name of the canary test bucket",
            exportName: "canaryTestBucket",
        });

        // Create a role for the canary Lambdas to assume
        this.lambdaExecutionRole = new Role(this, "lambda-execution-role", {
            roleName: "aws-sdk-rust-canary-lambda-exec-role",
            assumedBy: new ServicePrincipal("lambda.amazonaws.com"),
        });

        // Output the Lambda execution role ARN to make it easier to invoke the canary runner
        this.lambdaExecutionRoleArn = new CfnOutput(this, "lambda-execution-role-arn", {
            value: this.lambdaExecutionRole.roleArn,
            description: "Canary Lambda execution role ARN",
            exportName: "canaryLambdaExecutionRoleArn",
        });

        // Allow canaries to write logs to CloudWatch
        this.lambdaExecutionRole.addToPolicy(
            new PolicyStatement({
                actions: ["logs:CreateLogGroup", "logs:CreateLogStream", "logs:PutLogEvents"],
                effect: Effect.ALLOW,
                resources: ["arn:aws:logs:*:*:/aws/lambda/canary-*:*"],
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
        if (this.awsSdkRustOidcRole) {
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
}
