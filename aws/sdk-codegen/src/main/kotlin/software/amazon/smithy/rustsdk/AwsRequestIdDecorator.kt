/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.customize.OperationCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.OperationSection
import software.amazon.smithy.rust.codegen.core.smithy.generators.error.ErrorCustomization
import software.amazon.smithy.rust.codegen.core.smithy.generators.error.ErrorSection

/**
 * Customizes response parsing logic to add AWS request IDs to error metadata and outputs
 */
class AwsRequestIdDecorator : ClientCodegenDecorator {
    override val name: String = "AwsRequestIdDecorator"
    override val order: Byte = 0

    override fun operationCustomizations(
        codegenContext: ClientCodegenContext,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>,
    ): List<OperationCustomization> = baseCustomizations + listOf(AwsRequestIdOperationCustomization(codegenContext))

    override fun errorCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ErrorCustomization>,
    ): List<ErrorCustomization> = baseCustomizations + listOf(AwsRequestIdErrorCustomization(codegenContext))

    override fun extras(codegenContext: ClientCodegenContext, rustCrate: RustCrate) {
        rustCrate.withModule(RustModule.Types) {
            // Re-export RequestId in generated crate
            rust("pub use #T;", codegenContext.requestIdTrait())
        }
    }
}

private class AwsRequestIdOperationCustomization(
    private val codegenContext: ClientCodegenContext,
) : OperationCustomization() {
    override fun section(section: OperationSection): Writable = writable {
        when (section) {
            is OperationSection.PopulateGenericErrorExtras -> {
                rustTemplate(
                    "${section.builderName} = #{apply_request_id}(${section.builderName}, ${section.responseName}.headers());",
                    "apply_request_id" to codegenContext.requestIdModule().resolve("apply_request_id"),
                )
            }
            else -> {}
        }
    }
}

private class AwsRequestIdErrorCustomization(private val codegenContext: ClientCodegenContext) : ErrorCustomization() {
    override fun section(section: ErrorSection): Writable = writable {
        when (section) {
            is ErrorSection.OperationErrorAdditionalTraitImpls -> {
                rustTemplate(
                    """
                    impl #{RequestId} for #{error} {
                        fn request_id(&self) -> Option<&str> {
                            self.meta.request_id()
                        }
                    }
                    """,
                    "RequestId" to codegenContext.requestIdTrait(),
                    "error" to section.errorType,
                )
            }
            else -> {}
        }
    }
}

private fun ClientCodegenContext.requestIdModule(): RuntimeType =
    AwsRuntimeType.awsHttp(runtimeConfig).resolve("request_id")

private fun ClientCodegenContext.requestIdTrait(): RuntimeType = requestIdModule().resolve("RequestId")
