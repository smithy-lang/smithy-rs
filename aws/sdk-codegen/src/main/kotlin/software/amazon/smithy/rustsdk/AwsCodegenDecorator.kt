/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.rust.codegen.smithy.customize.CombinedCodegenDecorator

val DECORATORS = listOf(
    CredentialsProviderDecorator(),
    RegionDecorator(),
    AwsEndpointDecorator(),
    UserAgentDecorator(),
    SigV4SigningDecorator()
)

class AwsCodegenDecorator : CombinedCodegenDecorator(DECORATORS) {
    override val name: String = "AwsSdkCodegenDecorator"
    override val order: Byte = -1
}
