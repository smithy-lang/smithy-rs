/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.rust.codegen.client.smithy.customizations.DocsRsMetadataDecorator
import software.amazon.smithy.rust.codegen.client.smithy.customizations.DocsRsMetadataSettings
import software.amazon.smithy.rust.codegen.client.smithy.customizations.ManifestHintsDecorator
import software.amazon.smithy.rust.codegen.client.smithy.customizations.ManifestHintsSettings
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.customize.CombinedClientCodegenDecorator
import software.amazon.smithy.rustsdk.customize.AwsDisableStalledStreamProtection
import software.amazon.smithy.rustsdk.customize.DisabledAuthDecorator
import software.amazon.smithy.rustsdk.customize.EnvironmentTokenProviderDecorator
import software.amazon.smithy.rustsdk.customize.IsTruncatedPaginatorDecorator
import software.amazon.smithy.rustsdk.customize.RemoveDefaultsDecorator
import software.amazon.smithy.rustsdk.customize.Sigv4aAuthTraitBackfillDecorator
import software.amazon.smithy.rustsdk.customize.apigateway.ApiGatewayDecorator
import software.amazon.smithy.rustsdk.customize.applyDecorators
import software.amazon.smithy.rustsdk.customize.applyExceptFor
import software.amazon.smithy.rustsdk.customize.dsql.DsqlDecorator
import software.amazon.smithy.rustsdk.customize.ec2.Ec2Decorator
import software.amazon.smithy.rustsdk.customize.glacier.GlacierDecorator
import software.amazon.smithy.rustsdk.customize.onlyApplyTo
import software.amazon.smithy.rustsdk.customize.onlyApplyToList
import software.amazon.smithy.rustsdk.customize.rds.RdsDecorator
import software.amazon.smithy.rustsdk.customize.route53.Route53Decorator
import software.amazon.smithy.rustsdk.customize.s3.S3Decorator
import software.amazon.smithy.rustsdk.customize.s3.S3ExpiresDecorator
import software.amazon.smithy.rustsdk.customize.s3.S3ExpressDecorator
import software.amazon.smithy.rustsdk.customize.s3.S3ExtendedRequestIdDecorator
import software.amazon.smithy.rustsdk.customize.s3control.S3ControlDecorator
import software.amazon.smithy.rustsdk.customize.sso.SSODecorator
import software.amazon.smithy.rustsdk.customize.sts.STSDecorator
import software.amazon.smithy.rustsdk.customize.timestream.TimestreamDecorator
import software.amazon.smithy.rustsdk.endpoints.AwsEndpointsStdLib
import software.amazon.smithy.rustsdk.endpoints.OperationInputTestDecorator
import software.amazon.smithy.rustsdk.endpoints.RequireEndpointRules

val DECORATORS: List<ClientCodegenDecorator> =
    listOf(
        // General AWS Decorators
        listOf(
            CredentialsProviderDecorator(),
            RegionDecorator(),
            RequireEndpointRules(),
            UserAgentDecorator(),
            SigV4AuthDecorator(),
            HttpRequestChecksumDecorator(),
            HttpResponseChecksumDecorator(),
            IntegrationTestDecorator(),
            AwsFluentClientDecorator(),
            CrateLicenseDecorator(),
            SdkConfigDecorator(),
            ServiceConfigDecorator(),
            AwsPresigningDecorator(),
            AwsChunkedContentEncodingDecorator(),
            AwsCrateDocsDecorator(),
            AwsEndpointsStdLib(),
            *PromotedBuiltInsDecorators,
            GenericSmithySdkConfigSettings(),
            OperationInputTestDecorator(),
            AwsRequestIdDecorator(),
            DisabledAuthDecorator(),
            RecursionDetectionDecorator(),
            EndpointOverrideDecorator(),
            ObservabilityDetectionDecorator(),
            InvocationIdDecorator(),
            RetryInformationHeaderDecorator(),
            RemoveDefaultsDecorator(),
            TokenProvidersDecorator(),
            ServiceEnvConfigDecorator(),
            HttpRequestCompressionDecorator(),
            DisablePayloadSigningDecorator(),
            AwsDisableStalledStreamProtection(),
            Sigv4aAuthTraitBackfillDecorator(),
            EndpointBasedAuthSchemeDecorator(),
            SpanDecorator(),
            // TODO(https://github.com/smithy-lang/smithy-rs/issues/3863): Comment in once the issue has been resolved
            // SmokeTestsDecorator(),
        ),
        // S3 needs `AwsErrorCodeClassifier` to handle an `InternalError` as a transient error. We need to customize
        // that behavior for S3 in a way that does not conflict with the globally applied `RetryClassifierDecorator`.
        // Therefore, that decorator is applied to all but S3, and S3 customizes the creation of `AwsErrorCodeClassifier`
        // accordingly (see https://github.com/smithy-lang/smithy-rs/pull/3699).
        RetryClassifierDecorator().applyExceptFor("com.amazonaws.s3#AmazonS3"),
        // Service specific decorators
        ApiGatewayDecorator().onlyApplyTo("com.amazonaws.apigateway#BackplaneControlService"),
        DsqlDecorator().onlyApplyTo("com.amazonaws.dsql#DSQL"),
        Ec2Decorator().onlyApplyTo("com.amazonaws.ec2#AmazonEC2"),
        GlacierDecorator().onlyApplyTo("com.amazonaws.glacier#Glacier"),
        RdsDecorator().onlyApplyTo("com.amazonaws.rds#AmazonRDSv19"),
        Route53Decorator().onlyApplyTo("com.amazonaws.route53#AWSDnsV20130401"),
        "com.amazonaws.s3#AmazonS3".applyDecorators(
            S3Decorator(),
            S3ExpressDecorator(),
            S3ExtendedRequestIdDecorator(),
            IsTruncatedPaginatorDecorator(),
            S3ExpiresDecorator(),
        ),
        S3ControlDecorator().onlyApplyTo("com.amazonaws.s3control#AWSS3ControlServiceV20180820"),
        STSDecorator().onlyApplyTo("com.amazonaws.sts#AWSSecurityTokenServiceV20110615"),
        SSODecorator().onlyApplyTo("com.amazonaws.sso#SWBPortalService"),
        TimestreamDecorator().onlyApplyTo("com.amazonaws.timestreamwrite#Timestream_20181101"),
        TimestreamDecorator().onlyApplyTo("com.amazonaws.timestreamquery#Timestream_20181101"),
        listOf("bedrock").map { EnvironmentTokenProviderDecorator(it) },
        // Only build docs-rs for linux to reduce load on docs.rs
        listOf(
            DocsRsMetadataDecorator(
                DocsRsMetadataSettings(
                    targets = listOf("x86_64-unknown-linux-gnu"),
                    allFeatures = true,
                ),
            ),
        ),
        ManifestHintsDecorator(ManifestHintsSettings(mostlyUnused = true)).onlyApplyToList(
            listOf(
                "com.amazonaws.cloudformation#CloudFormation",
                "com.amazonaws.dynamodb#DynamoDB_20120810",
                "com.amazonaws.ec2#AmazonEC2",
                "com.amazonaws.lambda#AWSGirApiService",
                "com.amazonaws.rds#AmazonRDSv19",
                "com.amazonaws.s3#AmazonS3",
                "com.amazonaws.sns#AmazonSimpleNotificationService",
                "com.amazonaws.sqs#AmazonSQS",
                "com.amazonaws.ssm#AmazonSSM",
                "com.amazonaws.sts#AWSSecurityTokenServiceV20110615",
            ),
        ),
    ).flatten()

class AwsCodegenDecorator : CombinedClientCodegenDecorator(DECORATORS) {
    override val name: String = "AwsSdkCodegenDecorator"
    override val order: Byte = -1
}
