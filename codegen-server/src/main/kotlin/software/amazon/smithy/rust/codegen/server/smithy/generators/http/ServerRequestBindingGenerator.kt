/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators.http

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.client.smithy.UnconstrainedShapeSymbolProvider
import software.amazon.smithy.rust.codegen.client.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.client.smithy.CoreCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.client.smithy.generators.http.HttpBindingGenerator
import software.amazon.smithy.rust.codegen.client.smithy.generators.http.HttpMessageType
import software.amazon.smithy.rust.codegen.client.smithy.protocols.HttpBindingDescriptor
import software.amazon.smithy.rust.codegen.client.smithy.protocols.Protocol

class ServerRequestBindingGenerator(
    protocol: Protocol,
    coreCodegenContext: CoreCodegenContext,
    unconstrainedShapeSymbolProvider: UnconstrainedShapeSymbolProvider,
    operationShape: OperationShape,
) {
    private val httpBindingGenerator =
        HttpBindingGenerator(protocol, coreCodegenContext, unconstrainedShapeSymbolProvider, operationShape)

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
