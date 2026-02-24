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
import {
    BlockPublicAccess,
    Bucket,
    BucketEncryption,
    CfnMultiRegionAccessPoint,
} from "aws-cdk-lib/aws-s3";
import { CfnDirectoryBucket } from "aws-cdk-lib/aws-s3express";
import { StackProps, Stack, Tags, RemovalPolicy, Duration, CfnOutput } from "aws-cdk-lib";
import { Construct } from "constructs";
import { GitHubOidcRole } from "./constructs/github-oidc-role";

export interface OidcProps {
    roleId: string;
    roleName: string;
    roleGithubOrg: string;
    roleGithubRepo: string;
    provider: OpenIdConnectProvider;
}

export interface Properties extends StackProps {
    lambdaExecutionRole: string;
    oidcProps?: OidcProps;
}

export class CanaryStack extends Stack {
    public readonly githubOidcRole?: GitHubOidcRole;
    public readonly lambdaExecutionRole: Role;
    public readonly canaryCodeBucket: Bucket;
    public readonly canaryTestBucket: Bucket;
    public readonly canaryTestMrap: CfnMultiRegionAccessPoint;
    public readonly canaryTestExpressBucket: CfnDirectoryBucket;
    public readonly canaryCdkOutputsBucket: Bucket;

    public readonly lambdaExecutionRoleArn: CfnOutput;
    public readonly canaryCodeBucketName: CfnOutput;
    public readonly canaryTestBucketName: CfnOutput;
    public readonly canaryTestMrapBucketArn: CfnOutput;
    public readonly canaryTestExpressBucketName: CfnOutput;

    constructor(scope: Construct, id: string, props: Properties) {
        super(scope, id, props);

        // Tag the resources created by this stack to make identifying resources easier
        Tags.of(this).add("stack", id);

        if (props.oidcProps) {
            this.githubOidcRole = new GitHubOidcRole(this, props.oidcProps.roleId, {
                name: props.oidcProps.roleName,
                githubOrg: props.oidcProps.roleGithubOrg,
                githubRepo: props.oidcProps.roleGithubRepo,
                oidcProvider: props.oidcProps.provider,
            });

            // Grant permission to create/invoke/delete a canary Lambda
            this.githubOidcRole.oidcRole.addToPolicy(
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
            this.githubOidcRole.oidcRole.addToPolicy(
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
        if (this.githubOidcRole) {
            this.canaryCodeBucket.grantRead(this.githubOidcRole.oidcRole);
            this.canaryCodeBucket.grantWrite(this.githubOidcRole.oidcRole);
        }

        this.canaryCdkOutputsBucket = new Bucket(this, "canary-cdk-outputs-bucket", {
            blockPublicAccess: BlockPublicAccess.BLOCK_ALL,
            encryption: BucketEncryption.S3_MANAGED,
            versioned: false,
            removalPolicy: RemovalPolicy.DESTROY,
        });

        // Allow the OIDC role to GetObject from the cdk outputs bucket
        if (this.githubOidcRole) {
            this.canaryCdkOutputsBucket.grantRead(this.githubOidcRole.oidcRole);
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

        // Create a MultiRegionAccessPoint for the canary test bucket
        this.canaryTestMrap = new CfnMultiRegionAccessPoint(this, "canary-test-mrap-bucket", {
            regions: [{ bucket: this.canaryTestBucket.bucketName }],
            name: "canary-test-mrap-bucket",
        });

        const accountId = this.canaryTestMrap.stack.account;
        const alias = this.canaryTestMrap.attrAlias;
        const canaryTestMrapBucketArn = `arn:aws:s3::${accountId}:accesspoint/${alias}`;
        if (canaryTestMrapBucketArn) {
            // Output the bucket name to make it easier to invoke the canary runner
            this.canaryTestMrapBucketArn = new CfnOutput(this, "canary-test-mrap-bucket-arn", {
                value: canaryTestMrapBucketArn,
                description: "ARN of the canary test MRAP bucket",
                exportName: "canaryTestMrapBucketArn",
            });
        }

        this.canaryTestExpressBucket = new CfnDirectoryBucket(this, "canary-test-express-bucket", {
            dataRedundancy: "SingleAvailabilityZone",
            locationName: "usw2-az1",
        });

        // Output the bucket name to make it easier to invoke the canary runner
        this.canaryTestExpressBucketName = new CfnOutput(this, "canary-test-express-bucket-name", {
            value: this.canaryTestExpressBucket.ref,
            description: "Name of the canary express test bucket",
            exportName: "canaryExpressTestBucket",
        });

        // Create a role for the canary Lambdas to assume
        this.lambdaExecutionRole = new Role(this, "lambda-execution-role", {
            roleName: props.lambdaExecutionRole,
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

        this.lambdaExecutionRole.addToPolicy(
            new PolicyStatement({
                actions: ["s3:GetObject", "s3:PutObject", "s3:DeleteObject", "s3:ListAllMyBuckets"],
                effect: Effect.ALLOW,
                resources: [`${canaryTestMrapBucketArn}`, `${canaryTestMrapBucketArn}/object/*`],
            }),
        );

        // Allow canaries to perform operations on test express bucket
        // Unlike S3, no need to grant separate permissions for GetObject, PutObject, and so on because
        // the session token enables access instead:
        // https://docs.aws.amazon.com/AmazonS3/latest/userguide/s3-express-security-iam.html#s3-express-security-iam-actions
        this.lambdaExecutionRole.addToPolicy(
            new PolicyStatement({
                actions: ["s3express:CreateSession"],
                effect: Effect.ALLOW,
                resources: [`${this.canaryTestExpressBucket.attrArn}`],
            }),
        );

        // Allow canaries to list directory buckets
        this.lambdaExecutionRole.addToPolicy(
            new PolicyStatement({
                actions: ["s3express:ListAllMyDirectoryBuckets"],
                effect: Effect.ALLOW,
                resources: ["*"],
            }),
        );

        // Allow canaries to call Transcribe's StartStreamTranscription
        this.lambdaExecutionRole.addToPolicy(
            new PolicyStatement({
                actions: ["transcribe:StartStreamTranscription"],
                effect: Effect.ALLOW,
                resources: ["*"],
            }),
        );

        // Allow canaries to call STS
        this.lambdaExecutionRole.addToPolicy(
            new PolicyStatement({
                actions: ["sts:GetCallerIdentity"],
                effect: Effect.ALLOW,
                resources: ["*"],
            }),
        );

        // Allow canaries to call EC2
        this.lambdaExecutionRole.addToPolicy(
            new PolicyStatement({
                actions: ["ec2:DescribeRegions"],
                effect: Effect.ALLOW,
                resources: ["*"],
            }),
        );

        // Allow the OIDC role to pass the Lambda execution role to Lambda
        if (this.githubOidcRole) {
            this.githubOidcRole.oidcRole.addToPolicy(
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
