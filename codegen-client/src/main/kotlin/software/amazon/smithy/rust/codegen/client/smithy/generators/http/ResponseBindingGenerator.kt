/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators.http

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.generators.builderSymbol
import software.amazon.smithy.rust.codegen.core.smithy.generators.http.HttpBindingGenerator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.HttpBindingDescriptor
import software.amazon.smithy.rust.codegen.core.smithy.protocols.Protocol

class ResponseBindingGenerator(
    protocol: Protocol,
    private val codegenContext: CodegenContext,
    operationShape: OperationShape,
) {
    private fun builderSymbol(shape: StructureShape): Symbol = shape.builderSymbol(codegenContext.symbolProvider)

    private val httpBindingGenerator =
        HttpBindingGenerator(protocol, codegenContext, codegenContext.symbolProvider, operationShape, ::builderSymbol)

    fun generateDeserializeHeaderFn(binding: HttpBindingDescriptor): RuntimeType =
        httpBindingGenerator.generateDeserializeHeaderFn(binding)

    fun generateDeserializePrefixHeaderFn(binding: HttpBindingDescriptor): RuntimeType =
        httpBindingGenerator.generateDeserializePrefixHeaderFn(binding)

    fun generateDeserializePayloadFn(
        binding: HttpBindingDescriptor,
        errorT: RuntimeType,
        payloadParser: RustWriter.(String) -> Unit,
    ): RuntimeType = httpBindingGenerator.generateDeserializePayloadFn(
        binding,
        errorT,
        payloadParser,
    )
}
