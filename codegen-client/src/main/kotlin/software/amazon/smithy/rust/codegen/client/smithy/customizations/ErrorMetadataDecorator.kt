/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.customizations

import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.genericError
import software.amazon.smithy.rust.codegen.core.smithy.generators.error.ErrorCustomization
import software.amazon.smithy.rust.codegen.core.smithy.generators.error.ErrorSection
import software.amazon.smithy.rust.codegen.core.smithy.generators.error.delegateToVariants

class ErrorMetadataDecorator : ClientCodegenDecorator {
    override val name: String = "ErrorMetadataDecorator"
    override val order: Byte = 0

    override fun errorCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ErrorCustomization>,
    ): List<ErrorCustomization> =
        baseCustomizations + listOf(
            object : ErrorCustomization() {
                override fun section(section: ErrorSection): Writable = writable {
                    when (section) {
                        is ErrorSection.ServiceErrorAdditionalUnhandledErrorBuildFields -> {
                            rust(".meta(#T::meta(&err).clone())", RuntimeType.errorMetadataTrait(codegenContext.runtimeConfig))
                        }

                        is ErrorSection.OperationErrorAdditionalTraitImpls -> {
                            rustBlock(
                                "impl #T for ${section.errorType.name}",
                                RuntimeType.errorMetadataTrait(codegenContext.runtimeConfig),
                            ) {
                                rustBlock("fn meta(&self) -> &#T", genericError(codegenContext.runtimeConfig)) {
                                    delegateToVariants(codegenContext.symbolProvider, section.allErrors) {
                                        writable { rust("_inner.meta()") }
                                    }
                                }
                            }
                        }
                    }
                }
            },
        )
}
