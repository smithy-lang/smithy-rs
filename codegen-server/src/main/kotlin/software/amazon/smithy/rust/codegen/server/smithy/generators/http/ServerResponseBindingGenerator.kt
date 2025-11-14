/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators.http

import software.amazon.smithy.model.shapes.ByteShape
import software.amazon.smithy.model.shapes.CollectionShape
import software.amazon.smithy.model.shapes.IntegerShape
import software.amazon.smithy.model.shapes.LongShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShortShape
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.generators.http.HttpBindingCustomization
import software.amazon.smithy.rust.codegen.core.smithy.generators.http.HttpBindingGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.http.HttpBindingSection
import software.amazon.smithy.rust.codegen.core.smithy.generators.http.HttpMessageType
import software.amazon.smithy.rust.codegen.core.smithy.protocols.Protocol
import software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize.ValueExpression
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.workingWithPublicConstrainedWrapperTupleType

class ServerResponseBindingGenerator(
    protocol: Protocol,
    private val codegenContext: ServerCodegenContext,
    operationShape: OperationShape,
) {
    private val httpBindingGenerator =
        HttpBindingGenerator(
            protocol,
            codegenContext,
            codegenContext.symbolProvider,
            operationShape,
            listOf(
                ServerResponseBeforeIteratingOverMapBoundWithHttpPrefixHeadersUnwrapConstrainedMapHttpBindingCustomization(
                    codegenContext,
                ),
                ServerResponseBeforeRenderingHeadersHttpBindingCustomization(
                    codegenContext,
                ),
            ),
        )

    fun generateAddHeadersFn(shape: Shape): RuntimeType? =
        httpBindingGenerator.generateAddHeadersFn(shape, HttpMessageType.RESPONSE)
}

/**
 * A customization to, just before we iterate over a _constrained_ map shape that is bound to HTTP response headers via
 * `@httpPrefixHeaders`, unwrap the wrapper newtype and take a shared reference to the actual `std::collections::HashMap`
 * within it.
 */
class ServerResponseBeforeIteratingOverMapBoundWithHttpPrefixHeadersUnwrapConstrainedMapHttpBindingCustomization(val codegenContext: ServerCodegenContext) :
    HttpBindingCustomization() {
    override fun section(section: HttpBindingSection): Writable =
        when (section) {
            is HttpBindingSection.BeforeIteratingOverMapShapeBoundWithHttpPrefixHeaders ->
                writable {
                    if (workingWithPublicConstrainedWrapperTupleType(
                            section.shape,
                            codegenContext.model,
                            codegenContext.settings.codegenConfig.publicConstrainedTypes,
                        )
                    ) {
                        rust("let ${section.variableName} = &${section.variableName}.0;")
                    }
                }

            is HttpBindingSection.BeforeRenderingHeaderValue,
            is HttpBindingSection.AfterDeserializingIntoAHashMapOfHttpPrefixHeaders,
            is HttpBindingSection.AfterDeserializingIntoADateTimeOfHttpHeaders,
            -> emptySection
        }
}

/**
 * A customization to, just before we render a _constrained_ member shape to an HTTP response header,
 * unwrap the wrapper newtype and take a shared reference to the actual inner type within it.
 */
class ServerResponseBeforeRenderingHeadersHttpBindingCustomization(val codegenContext: ServerCodegenContext) :
    HttpBindingCustomization() {
    override fun section(section: HttpBindingSection): Writable =
        when (section) {
            is HttpBindingSection.BeforeRenderingHeaderValue ->
                writable {
                    val isIntegral =
                        section.context.shape is ByteShape || section.context.shape is ShortShape || section.context.shape is IntegerShape || section.context.shape is LongShape
                    val isCollection = section.context.shape is CollectionShape

                    val workingWithPublicWrapper =
                        workingWithPublicConstrainedWrapperTupleType(
                            section.context.shape,
                            codegenContext.model,
                            codegenContext.settings.codegenConfig.publicConstrainedTypes,
                        )

                    if (workingWithPublicWrapper && (isIntegral || isCollection)) {
                        section.context.valueExpression =
                            ValueExpression.Reference("&${section.context.valueExpression.name.removePrefix("&")}.0")
                    }
                }

            is HttpBindingSection.BeforeIteratingOverMapShapeBoundWithHttpPrefixHeaders,
            is HttpBindingSection.AfterDeserializingIntoAHashMapOfHttpPrefixHeaders,
            is HttpBindingSection.AfterDeserializingIntoADateTimeOfHttpHeaders,
            -> emptySection
        }
}
