/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk.endpoints

import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.EndpointTypesGenerator
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate

class AwsEndpointDecorator : ClientCodegenDecorator {
    override val name: String = "AwsEndpoint"
    override val order: Byte = 100
    override fun extras(codegenContext: ClientCodegenContext, rustCrate: RustCrate) {
        val epTypes = EndpointTypesGenerator.fromContext(codegenContext)
        if (epTypes.defaultResolver() == null) {
            throw CodegenException(
                "${codegenContext.serviceShape} did not provide endpoint rules. " +
                    "This is a bug and the generated client will not work. All AWS services MUST define endpoint rules.",
            )
        }
    }
}
