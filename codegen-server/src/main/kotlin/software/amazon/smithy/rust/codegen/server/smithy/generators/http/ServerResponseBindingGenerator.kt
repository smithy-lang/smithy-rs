/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators.http


import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.generators.http.HttpBindingGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.http.HttpMessageType
import software.amazon.smithy.rust.codegen.smithy.protocols.HttpBindingDescriptor
import software.amazon.smithy.rust.codegen.smithy.protocols.Protocol

class ServerResponseBindingGenerator(
    protocol: Protocol,
    codegenContext: CodegenContext,
    operationShape: OperationShape
) {
    private val httpBindingGenerator = HttpBindingGenerator(protocol, codegenContext, operationShape)

    fun generateAddHeadersFn(bindings: List<HttpBindingDescriptor>, shape: StructureShape): RuntimeType =
        httpBindingGenerator.generateAddHeadersFn(bindings, shape, HttpMessageType.RESPONSE)
}
