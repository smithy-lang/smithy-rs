/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.rust.codegen.smithy.customize.CombinedCodegenDecorator
import software.amazon.smithy.rustsdk.customize.apigateway.ApiGatewayCustomizationDecorator

val DECORATORS = listOf(
    AllowClippyLintsDecorator(),
    CredentialsProviderDecorator(),
    RegionDecorator(),
    AwsEndpointDecorator(),
    UserAgentDecorator(),
    SigV4SigningDecorator(),
    RetryPolicyDecorator(),
    IntegrationTestDecorator(),
    FluentClientDecorator(),
    ApiGatewayCustomizationDecorator(),
    CrateLicenseDecorator()
)

class AwsCodegenDecorator : CombinedCodegenDecorator(DECORATORS) {
    override val name: String = "AwsSdkCodegenDecorator"
    override val order: Byte = -1
}
