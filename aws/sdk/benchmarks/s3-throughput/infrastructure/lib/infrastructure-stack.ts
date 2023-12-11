/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import * as cdk from "aws-cdk-lib";
import { Construct } from "constructs";
import * as ec2 from "aws-cdk-lib/aws-ec2";
import * as s3 from "aws-cdk-lib/aws-s3";
import * as iam from "aws-cdk-lib/aws-iam";
import * as assets from "aws-cdk-lib/aws-s3-assets";
import * as path from "path";

export class InfrastructureStack extends cdk.Stack {
    constructor(scope: Construct, id: string, props?: cdk.StackProps) {
        super(scope, id, props);

        const assetInitInstance = new assets.Asset(this, "assetInitInstance", {
            path: path.join(__dirname, "../assets/init_instance.sh"),
        });
        const assetRunBenchmark = new assets.Asset(this, "assetRunBenchmark", {
            path: path.join(__dirname, "../assets/run_benchmark.sh"),
        });
        const assetBenchmark = new assets.Asset(this, "assetBenchmark", {
            path: path.join(__dirname, "../../benchmark"),
        });
        const assetBucket = s3.Bucket.fromBucketName(
            this,
            "assetBucket",
            assetInitInstance.s3BucketName,
        );

        const benchmarkBucket = new s3.Bucket(this, "benchmarkBucket", {
            blockPublicAccess: s3.BlockPublicAccess.BLOCK_ALL,
            encryption: s3.BucketEncryption.S3_MANAGED,
            enforceSSL: true,
            versioned: false,
            removalPolicy: cdk.RemovalPolicy.DESTROY,
        });

        const instanceUserData = ec2.UserData.forLinux();
        const initInstancePath = instanceUserData.addS3DownloadCommand({
            bucket: assetBucket,
            bucketKey: assetInitInstance.s3ObjectKey,
        });
        const runBenchmarkPath = instanceUserData.addS3DownloadCommand({
            bucket: assetBucket,
            bucketKey: assetRunBenchmark.s3ObjectKey,
        });
        const benchmarkPath = instanceUserData.addS3DownloadCommand({
            bucket: assetBucket,
            bucketKey: assetBenchmark.s3ObjectKey,
        });
        instanceUserData.addExecuteFileCommand({
            filePath: initInstancePath,
            arguments: `${benchmarkPath}`,
        });
        instanceUserData.addExecuteFileCommand({
            filePath: runBenchmarkPath,
            arguments: `${benchmarkBucket.bucketName}`,
        });

        const vpc = new ec2.Vpc(this, "VPC", {});
        const securityGroup = new ec2.SecurityGroup(this, "securityGroup", {
            vpc,
            description: "Allow outbound and SSH inbound",
            allowAllOutbound: true,
        });
        securityGroup.addIngressRule(ec2.Peer.anyIpv4(), ec2.Port.tcp(22), "SSH");

        const executionRole = new iam.Role(this, "executionRole", {
            assumedBy: new iam.ServicePrincipal("ec2.amazonaws.com"),
        });
        assetBucket.grantRead(executionRole);
        benchmarkBucket.grantReadWrite(executionRole);

        new ec2.Instance(this, `S3Benchmark_${this.stackName}`, {
            instanceType: ec2.InstanceType.of(ec2.InstanceClass.C5N, ec2.InstanceSize.XLARGE18),
            vpc,
            machineImage: ec2.MachineImage.latestAmazonLinux2023(),
            userData: instanceUserData,
            role: executionRole,
            keyName: "S3BenchmarkKeyPair",
            securityGroup,
            vpcSubnets: { subnets: vpc.publicSubnets },
            requireImdsv2: true,
        });
    }
}
