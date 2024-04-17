/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.generators.http

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.CodegenContext
import software.amazon.smithy.rust.codegen.core.RuntimeType
import software.amazon.smithy.rust.codegen.core.generators.http.HttpBindingGenerator
import software.amazon.smithy.rust.codegen.core.protocols.HttpBindingDescriptor
import software.amazon.smithy.rust.codegen.core.protocols.Protocol

class ResponseBindingGenerator(
    protocol: Protocol,
    codegenContext: CodegenContext,
    operationShape: OperationShape,
) {
    private val httpBindingGenerator =
        HttpBindingGenerator(protocol, codegenContext, codegenContext.symbolProvider, operationShape)

    fun generateDeserializeHeaderFn(binding: HttpBindingDescriptor): RuntimeType =
        httpBindingGenerator.generateDeserializeHeaderFn(binding)

    fun generateDeserializePrefixHeaderFn(binding: HttpBindingDescriptor): RuntimeType =
        httpBindingGenerator.generateDeserializePrefixHeaderFn(binding)

    fun generateDeserializePayloadFn(
        binding: HttpBindingDescriptor,
        errorSymbol: Symbol,
        payloadParser: RustWriter.(String) -> Unit,
    ): RuntimeType = httpBindingGenerator.generateDeserializePayloadFn(binding, errorSymbol, payloadParser)
}
