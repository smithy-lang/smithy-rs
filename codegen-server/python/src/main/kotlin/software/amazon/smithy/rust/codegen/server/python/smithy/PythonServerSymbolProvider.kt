/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.python.smithy.generators

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.BlobShape
import software.amazon.smithy.model.shapes.ListShape
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.SetShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeVisitor
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.TimestampShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.model.traits.EnumDefinition
import software.amazon.smithy.rust.codegen.rustlang.RustType
import software.amazon.smithy.rust.codegen.server.python.smithy.PythonServerRuntimeType
import software.amazon.smithy.rust.codegen.smithy.MaybeRenamed
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.SymbolVisitorConfig
import software.amazon.smithy.rust.codegen.smithy.rustType
import software.amazon.smithy.rust.codegen.smithy.shape
import software.amazon.smithy.rust.codegen.util.toPascalCase
import software.amazon.smithy.rust.codegen.util.toSnakeCase

// Wrapping symbol visitor provider allowing to implement symbol providers that can recursively replace symbols in nested shapes.
open class PythonWrappingVisitingSymbolProvider(private val base: RustSymbolProvider, private val model: Model) : ShapeVisitor.Default<Symbol>(), RustSymbolProvider {
    override fun getDefault(shape: Shape): Symbol {
        return base.toSymbol(shape)
    }

    override fun config(): SymbolVisitorConfig {
        return base.config()
    }

    override fun toEnumVariantName(definition: EnumDefinition): MaybeRenamed? {
        return base.toEnumVariantName(definition)
    }

    override fun toMemberName(shape: MemberShape): String = when (val container = model.expectShape(shape.container)) {
        is StructureShape -> shape.memberName.toSnakeCase()
        is UnionShape -> shape.memberName.toPascalCase()
        else -> error("unexpected container shape: $container")
    }

    override fun listShape(shape: ListShape): Symbol {
        val inner = toSymbol(shape.member)
        return symbolBuilder(RustType.Vec(inner.rustType())).addReference(inner).build()
    }

    override fun mapShape(shape: MapShape): Symbol {
        val target = model.expectShape(shape.key.target)
        require(target.isStringShape) { "unexpected key shape: ${shape.key}: $target [keys must be strings]" }
        val key = toSymbol(shape.key)
        val value = toSymbol(shape.value)
        return symbolBuilder(RustType.HashMap(key.rustType(), value.rustType())).addReference(key)
            .addReference(value).build()
    }

    override fun setShape(shape: SetShape): Symbol {
        val inner = toSymbol(shape.member)
        val builder = if (model.expectShape(shape.member.target).isStringShape) {
            symbolBuilder(RustType.HashSet(inner.rustType()))
        } else {
            // only strings get put into actual sets because floats are unhashable
            symbolBuilder(RustType.Vec(inner.rustType()))
        }
        return builder.addReference(inner).build()
    }

    override fun memberShape(shape: MemberShape): Symbol {
        return toSymbol(model.expectShape(shape.target))
    }

    override fun toSymbol(shape: Shape): Symbol {
        return shape.accept(this)
    }

    private fun symbolBuilder(rustType: RustType): Symbol.Builder {
        return Symbol.builder().rustType(rustType)
            .definitionFile("python.rs")
    }
}

/**
 * Input / output / error structures can refer to complex types like the ones implemented inside
 * `aws_smithy_types` (a good example is `aws_smithy_types::Blob`).
 * `aws_smithy_http_server_python::types` wraps those types that do not implement directly the
 * `pyo3::PyClass` trait and cannot be shared safely with Python, providing an idiomatic Python / Rust API.
 *
 * This symbol provider ensures types not implementing `pyo3::PyClass` are swapped with their wrappers from
 * `aws_smithy_http_server_python::types`.
 */
class PythonServerSymbolProvider(private val base: RustSymbolProvider, private val model: Model) : PythonWrappingVisitingSymbolProvider(base, model) {
    private val runtimeConfig = config().runtimeConfig

    override fun timestampShape(shape: TimestampShape?): Symbol {
        return PythonServerRuntimeType.DateTime(runtimeConfig).toSymbol()
    }

    override fun blobShape(shape: BlobShape?): Symbol {
        return PythonServerRuntimeType.Blob(runtimeConfig).toSymbol()
    }
}
