/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.python.smithy

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.rust.codegen.rustlang.RustMetadata
import software.amazon.smithy.rust.codegen.rustlang.RustType
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.WrappingSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.meta
import software.amazon.smithy.rust.codegen.smithy.rustType
import java.util.logging.Logger

class PythonServerSymbolProvider(private val base: RustSymbolProvider, private val model: Model) :
    WrappingSymbolProvider(base) {

    private val logger = Logger.getLogger(javaClass.name)

    /**
     * Convert shape to a Symbol
     *
     * If this symbol provider renamed the symbol, a `renamedFrom` field will be set on the symbol,
     * enabling code generators to generate special docs.
     */
    override fun toSymbol(shape: Shape): Symbol {
        return when (base.toSymbol(shape).fullName) {
            "aws_smithy_types::Blob" -> {
                buildSymbol("Blob", "aws_smithy_http_server_python::types")
            }
            else -> {
                base.toSymbol(shape)
            }
        }
    }

    private fun buildSymbol(name: String, namespace: String): Symbol =
        Symbol.builder()
            .name(name)
            .namespace(namespace, "::")
            .meta(RustMetadata(public = false))
            .rustType(RustType.Opaque(name ?: "", namespace = namespace)).build()
}
