/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.generators

import software.amazon.smithy.codegen.core.SymbolProvider
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.lang.RustWriter
import software.amazon.smithy.rust.codegen.lang.rustBlock
import software.amazon.smithy.rust.codegen.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.smithy.RuntimeType

interface ProtocolGeneratorFactory<out T : HttpProtocolGenerator> {

    fun build(
        model: Model,
        symbolProvider: SymbolProvider,
        runtimeConfig: RuntimeConfig,
        writer: RustWriter,
        operationShape: OperationShape,
        inputShape: StructureShape
    ): T
}

abstract class HttpProtocolGenerator(
    private val symbolProvider: SymbolProvider,
    private val writer: RustWriter,
    private val inputShape: StructureShape
) {
    fun render() {
        writer.rustBlock("impl ${symbolProvider.toSymbol(inputShape).name}") {
            toHttpRequestImpl(this)
        }
    }

    protected fun httpBuilderFun(implBlockWriter: RustWriter, f: RustWriter.() -> Unit) {
        implBlockWriter.rustBlock(
            "pub fn build_http_request(&self) -> \$T",
            RuntimeType.HttpRequestBuilder
        ) {
            f(this)
        }
    }

    /**
     * Add necessary methods to the impl block for the input shape.
     *
     * Your implementation MUST call `httpBuilderFun` to create the public method.
     */
    abstract fun toHttpRequestImpl(implBlockWriter: RustWriter)
}
