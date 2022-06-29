/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.smithy.generators.http

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.smithy.CoreCodegenContext
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.protocols.HttpBindingDescriptor
import software.amazon.smithy.rust.codegen.smithy.protocols.Protocol

class ResponseBindingGenerator(
    protocol: Protocol,
    coreCodegenContext: CoreCodegenContext,
    operationShape: OperationShape
) {
    private val httpBindingGenerator = HttpBindingGenerator(protocol, coreCodegenContext, operationShape)

    fun generateDeserializeHeaderFn(binding: HttpBindingDescriptor): RuntimeType =
        httpBindingGenerator.generateDeserializeHeaderFn(binding)

    fun generateDeserializePrefixHeaderFn(binding: HttpBindingDescriptor): RuntimeType =
        httpBindingGenerator.generateDeserializePrefixHeaderFn(binding)

    fun generateDeserializePayloadFn(
        binding: HttpBindingDescriptor,
        errorT: RuntimeType,
        payloadParser: RustWriter.(String) -> Unit
    ): RuntimeType = httpBindingGenerator.generateDeserializePayloadFn(
        binding,
        errorT,
        payloadParser
    )
}
