/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.rust.codegen.client.smithy.customizations.DocsRsMetadataDecorator
import software.amazon.smithy.rust.codegen.client.smithy.customizations.DocsRsMetadataSettings
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.customize.CombinedClientCodegenDecorator
import software.amazon.smithy.rustsdk.customize.apigateway.ApiGatewayDecorator
import software.amazon.smithy.rustsdk.customize.auth.DisabledAuthDecorator
import software.amazon.smithy.rustsdk.customize.ec2.Ec2Decorator
import software.amazon.smithy.rustsdk.customize.glacier.GlacierDecorator
import software.amazon.smithy.rustsdk.customize.route53.Route53Decorator
import software.amazon.smithy.rustsdk.customize.s3.S3Decorator
import software.amazon.smithy.rustsdk.customize.s3control.S3ControlDecorator
import software.amazon.smithy.rustsdk.customize.sts.STSDecorator
import software.amazon.smithy.rustsdk.endpoints.AwsEndpointDecorator
import software.amazon.smithy.rustsdk.endpoints.AwsEndpointsStdLib
import software.amazon.smithy.rustsdk.endpoints.OperationInputTestDecorator

val DECORATORS: List<ClientCodegenDecorator> = listOf(
    // General AWS Decorators
    CredentialsCacheDecorator(),
    CredentialsProviderDecorator(),
    RegionDecorator(),
    AwsEndpointDecorator(),
    UserAgentDecorator(),
    SigV4SigningDecorator(),
    HttpRequestChecksumDecorator(),
    HttpResponseChecksumDecorator(),
    RetryClassifierDecorator(),
    IntegrationTestDecorator(),
    AwsFluentClientDecorator(),
    CrateLicenseDecorator(),
    SdkConfigDecorator(),
    ServiceConfigDecorator(),
    AwsPresigningDecorator(),
    AwsReadmeDecorator(),
    HttpConnectorDecorator(),
    AwsEndpointsStdLib(),
    *PromotedBuiltInsDecorators,
    GenericSmithySdkConfigSettings(),
    OperationInputTestDecorator(),

    // Service specific decorators
    ApiGatewayDecorator(),
    DisabledAuthDecorator(),
    Ec2Decorator(),
    GlacierDecorator(),
    Route53Decorator(),
    S3Decorator(),
    S3ControlDecorator(),
    STSDecorator(),

    // Only build docs-rs for linux to reduce load on docs.rs
    DocsRsMetadataDecorator(DocsRsMetadataSettings(targets = listOf("x86_64-unknown-linux-gnu"), allFeatures = true)),
)

class AwsCodegenDecorator : CombinedClientCodegenDecorator(DECORATORS) {
    override val name: String = "AwsSdkCodegenDecorator"
    override val order: Byte = -1
}
