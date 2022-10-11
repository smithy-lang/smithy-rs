/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators.http

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.generators.http.HttpBindingGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.http.HttpMessageType
import software.amazon.smithy.rust.codegen.core.smithy.protocols.HttpBindingDescriptor
import software.amazon.smithy.rust.codegen.core.smithy.protocols.Protocol
import software.amazon.smithy.rust.codegen.core.smithy.serverBuilderSymbol
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext

class ServerRequestBindingGenerator(
    protocol: Protocol,
    private val codegenContext: ServerCodegenContext,
    operationShape: OperationShape,
) {
    private fun serverBuilderSymbol(shape: StructureShape): Symbol = shape.serverBuilderSymbol(
        codegenContext.symbolProvider,
        !codegenContext.settings.codegenConfig.publicConstrainedTypes,
    )
    private val httpBindingGenerator =
        HttpBindingGenerator(protocol, codegenContext, codegenContext.unconstrainedShapeSymbolProvider, operationShape, ::serverBuilderSymbol)

    fun generateDeserializeHeaderFn(binding: HttpBindingDescriptor): RuntimeType =
        httpBindingGenerator.generateDeserializeHeaderFn(binding)

    fun generateDeserializePayloadFn(
        binding: HttpBindingDescriptor,
        errorT: RuntimeType,
        structuredHandler: RustWriter.(String) -> Unit,
    ): RuntimeType = httpBindingGenerator.generateDeserializePayloadFn(
        binding,
        errorT,
        structuredHandler,
        HttpMessageType.REQUEST,
    )

    fun generateDeserializePrefixHeadersFn(
        binding: HttpBindingDescriptor,
    ): RuntimeType = httpBindingGenerator.generateDeserializePrefixHeaderFn(binding)
}
