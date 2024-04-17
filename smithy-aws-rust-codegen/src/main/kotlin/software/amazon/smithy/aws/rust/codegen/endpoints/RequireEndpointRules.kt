/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.aws.rust.codegen.endpoints

import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.rust.codegen.client.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.endpoint.EndpointTypesGenerator
import software.amazon.smithy.rust.codegen.core.RustCrate
import software.amazon.smithy.aws.rust.codegen.sdkSettings

class RequireEndpointRules : ClientCodegenDecorator {
    override val name: String = "RequireEndpointRules"
    override val order: Byte = 100

    override fun extras(
        codegenContext: ClientCodegenContext,
        rustCrate: RustCrate,
    ) {
        if (!codegenContext.sdkSettings().requireEndpointResolver) {
            return
        }
        val epTypes = EndpointTypesGenerator.fromContext(codegenContext)
        if (epTypes.defaultResolver() == null) {
            throw CodegenException(
                "${codegenContext.serviceShape} did not provide endpoint rules. To explicitly allow this, set `awsSdk.requireEndpointResolver: false` in smithy-build.json.",
            )
        }
    }
}
