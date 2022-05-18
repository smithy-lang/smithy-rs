/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import {
    AllowedMethods,
    CachedMethods,
    Distribution,
    OriginAccessIdentity,
    PriceClass,
    ViewerProtocolPolicy,
} from "aws-cdk-lib/aws-cloudfront";
import { S3Origin } from "aws-cdk-lib/aws-cloudfront-origins";
import { BlockPublicAccess, Bucket, BucketEncryption, LifecycleRule } from "aws-cdk-lib/aws-s3";
import { RemovalPolicy, Tags } from "aws-cdk-lib";
import { Construct } from "constructs";

export interface Properties {
    name: string;
    lifecycleRules?: LifecycleRule[];
    removalPolicy: RemovalPolicy;
}

export class CloudFrontS3Cdn extends Construct {
    public readonly bucket: Bucket;
    public readonly distribution: Distribution;
    private readonly originAccessIdentity: OriginAccessIdentity;

    constructor(scope: Construct, id: string, properties: Properties) {
        super(scope, id);

        // Tag the resources created by this construct to make identifying resources easier
        Tags.of(this).add("construct-name", properties.name);
        Tags.of(this).add("construct-type", "CloudFrontS3Cdn");

        this.bucket = new Bucket(this, "bucket", {
            blockPublicAccess: BlockPublicAccess.BLOCK_ALL,
            encryption: BucketEncryption.S3_MANAGED,
            lifecycleRules: properties.lifecycleRules,
            versioned: false,
            removalPolicy: properties.removalPolicy,
        });

        this.originAccessIdentity = new OriginAccessIdentity(this, "bucket-access-identity");
        this.bucket.grantRead(this.originAccessIdentity);

        this.distribution = new Distribution(this, "distribution", {
            enabled: true,
            defaultBehavior: {
                compress: true,
                origin: new S3Origin(this.bucket, {
                    originAccessIdentity: this.originAccessIdentity,
                }),
                allowedMethods: AllowedMethods.ALLOW_GET_HEAD_OPTIONS,
                cachedMethods: CachedMethods.CACHE_GET_HEAD_OPTIONS,
                viewerProtocolPolicy: ViewerProtocolPolicy.REDIRECT_TO_HTTPS,
            },
            priceClass: PriceClass.PRICE_CLASS_100,
        });
    }
}
