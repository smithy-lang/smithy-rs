/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customizations.DocsRsMetadataDecorator
import software.amazon.smithy.rust.codegen.client.smithy.customizations.DocsRsMetadataSettings
import software.amazon.smithy.rust.codegen.client.smithy.customize.CombinedCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.protocol.ClientProtocolGenerator
import software.amazon.smithy.rustsdk.customize.apigateway.ApiGatewayDecorator
import software.amazon.smithy.rustsdk.customize.auth.DisabledAuthDecorator
import software.amazon.smithy.rustsdk.customize.ec2.Ec2Decorator
import software.amazon.smithy.rustsdk.customize.glacier.GlacierDecorator
import software.amazon.smithy.rustsdk.customize.route53.Route53Decorator
import software.amazon.smithy.rustsdk.customize.s3.S3Decorator
import software.amazon.smithy.rustsdk.customize.sts.STSDecorator

val DECORATORS = listOf(
    // General AWS Decorators
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

    // Service specific decorators
    ApiGatewayDecorator(),
    DisabledAuthDecorator(),
    Ec2Decorator(),
    GlacierDecorator(),
    Route53Decorator(),
    S3Decorator(),
    STSDecorator(),

    // Only build docs-rs for linux to reduce load on docs.rs
    DocsRsMetadataDecorator(DocsRsMetadataSettings(targets = listOf("x86_64-unknown-linux-gnu"), allFeatures = true)),
)

class AwsCodegenDecorator : CombinedCodegenDecorator<ClientProtocolGenerator, ClientCodegenContext>(DECORATORS) {
    override val name: String = "AwsSdkCodegenDecorator"
    override val order: Byte = -1
}
