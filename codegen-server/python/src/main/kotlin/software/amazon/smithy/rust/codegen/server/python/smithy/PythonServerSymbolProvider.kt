/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.python.smithy

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.BlobShape
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.TimestampShape
import software.amazon.smithy.rust.codegen.rustlang.RustType
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
class PythonServerSymbolProvider(private val base: RustSymbolProvider, private val model: Model) :
    WrappingSymbolProvider(base) {

    private val runtimeConfig = config().runtimeConfig

    /**
     * Convert a shape to a Symbol.
     *
     * Swap the shape's symbol if its associated type does not implement `pyo3::PyClass`.
     */
    override fun toSymbol(shape: Shape): Symbol {
        var originalSymbol = base.toSymbol(shape)
        if (shape is MemberShape) {
            val target = model.expectShape(shape.target)
            originalSymbol = base.toSymbol(target)
            return when (target) {
                is MapShape -> {
                    val key = base.toSymbol(target.key)
                    val value = shapeToSymbol(model.expectShape(target.value.target), originalSymbol)
                    println("AAAAAAAAAAAAAAAAA: $key, $value")
                    symbolBuilder(target, RustType.HashMap(key.rustType(), value.rustType())).addReference(key).addReference(value).build()
                }
                // is ListShape -> {
                //     val innerTarget = model.expectShape(target.member.target)
                //     val inner = shapeToSymbol(target.member, originalSymbol)
                //     println("AAAAAAAAAAAAAAAAA: ${target.member} $inner")
                //     symbolBuilder(innerTarget, RustType.Vec(inner.rustType())).addReference(inner).build()
                // }
                // is SetShape -> {
                //     val inner = shapeToSymbol(target.member, originalSymbol)
                //     val builder = if (model.expectShape(target.member.target).isStringShape) {
                //         symbolBuilder(target, RustType.HashSet(inner.rustType()))
                //     } else {
                //         // only strings get put into actual sets because floats are unhashable
                //         symbolBuilder(target, RustType.Vec(inner.rustType()))
                //     }
                //     builder.addReference(inner).build()
                // }
                is BlobShape -> {
                    PythonServerRuntimeType.Blob(runtimeConfig).toSymbol()
                }
                is TimestampShape -> PythonServerRuntimeType.DateTime(runtimeConfig).toSymbol()
                else -> originalSymbol
            }
        } else {
            return shapeToSymbol(shape, originalSymbol)
        }
    }

    private fun symbolBuilder(shape: Shape?, rustType: RustType): Symbol.Builder {
        val builder = Symbol.builder().putProperty("shape", shape)
        return builder.rustType(rustType)
            .name(rustType.name)
            .definitionFile("python.rs")
    }

    private fun shapeToSymbol(shape: Shape, originalSymbol: Symbol): Symbol =
        when (shape) {
            is BlobShape -> PythonServerRuntimeType.Blob(runtimeConfig).toSymbol()
            is TimestampShape -> PythonServerRuntimeType.DateTime(runtimeConfig).toSymbol()
            else -> originalSymbol
        }
}
