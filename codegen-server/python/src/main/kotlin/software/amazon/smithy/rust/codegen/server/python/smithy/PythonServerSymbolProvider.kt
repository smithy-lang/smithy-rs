/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.python.smithy

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.WrappingSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.rustType

/**
 * Input / output / error structures can refer to complex types like the ones implemented inside
 * `aws_smithy_types` (a good example is `aws_smithy_types::Blob`).
 * `aws_smithy_http_server_python::types` wraps those types that do not implement directly the
 * `pyo3::PyClass` trait and cannot be shared safely with Python, providing an idiomatic Python / Rust API.
 *
 * This symbol provider ensures types not implementing `pyo3::PyClass` are swapped with their wrappers from
 * `aws_smithy_http_server_python::types`.
 */
class PythonServerSymbolProvider(private val base: RustSymbolProvider) :
    WrappingSymbolProvider(base) {

    private val runtimeConfig = config().runtimeConfig

    /**
     * Convert a shape to a Symbol.
     *
     * Swap the shape's symbol if its associated type does not implement `pyo3::PyClass`.
     */
    override fun toSymbol(shape: Shape): Symbol {
        return when (base.toSymbol(shape).rustType()) {
            RuntimeType.Blob(runtimeConfig).toSymbol().rustType() -> {
                PythonServerRuntimeType.Blob(runtimeConfig).toSymbol()
            }
            else -> {
                base.toSymbol(shape)
            }
        }
    }
}
