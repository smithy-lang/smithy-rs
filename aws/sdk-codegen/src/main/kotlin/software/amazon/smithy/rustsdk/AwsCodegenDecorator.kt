/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.rust.codegen.smithy.RetryConfigDecorator
import software.amazon.smithy.rust.codegen.smithy.customize.CombinedCodegenDecorator
import software.amazon.smithy.rustsdk.customize.apigateway.ApiGatewayDecorator
import software.amazon.smithy.rustsdk.customize.auth.DisabledAuthDecorator
import software.amazon.smithy.rustsdk.customize.ec2.Ec2Decorator
import software.amazon.smithy.rustsdk.customize.s3.S3Decorator

val DECORATORS = listOf(
    // General AWS Decorators
    CredentialsProviderDecorator(),
    RegionDecorator(),
    AwsEndpointDecorator(),
    UserAgentDecorator(),
    SigV4SigningDecorator(),
    RetryPolicyDecorator(),
    IntegrationTestDecorator(),
    AwsFluentClientDecorator(),
    CrateLicenseDecorator(),
    SharedConfigDecorator(),
    AwsPresigningDecorator(),

    // Smithy specific decorators
    RetryConfigDecorator(),

    // Service specific decorators
    DisabledAuthDecorator(),
    ApiGatewayDecorator(),
    S3Decorator(),
    Ec2Decorator()
)

class AwsCodegenDecorator : CombinedCodegenDecorator(DECORATORS) {
    override val name: String = "AwsSdkCodegenDecorator"
    override val order: Byte = -1
}
