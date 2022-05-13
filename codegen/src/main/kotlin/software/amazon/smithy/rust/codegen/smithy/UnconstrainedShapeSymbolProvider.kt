/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.ListShape
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.SetShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.rustlang.RustReservedWords
import software.amazon.smithy.rust.codegen.rustlang.RustType
import software.amazon.smithy.rust.codegen.smithy.generators.builderSymbol
import software.amazon.smithy.rust.codegen.util.toPascalCase
import software.amazon.smithy.rust.codegen.util.toSnakeCase

fun unconstrainedTypeNameForListOrMapShape(shape: Shape, serviceShape: ServiceShape): String {
    check(shape.isListShape || shape.isMapShape)
    return "${shape.id.getName(serviceShape).toPascalCase()}Unconstrained"
}

/**
 * The [UnconstrainedShapeSymbolProvider] returns, _for a given constrained
 * shape_, a symbol whose Rust type can hold the corresponding unconstrained
 * values.
 *
 * For collection and map shapes, this type is a [RustType.Opaque] wrapper
 * tuple newtype holding a container over the inner unconstrained type. For
 * structure shapes, it's their builder type. For simple shapes, it's whatever
 * the regular base symbol provider returns.
 *
 * So, for example, given the following model:
 *
 * ```smithy
 * list ListA {
 *     member: ListB
 * }
 *
 * list ListB {
 *     member: Structure
 * }
 *
 * structure Structure {
 *     @required
 *     string: String
 * }
 * ```
 *
 * `ListB` is not _directly_ constrained, but it is constrained, because it
 * holds `Structure`s, that are constrained. So the corresponding unconstrained
 * symbol has Rust type `struct
 * ListBUnconstrained(std::vec::Vec<crate::model::structure::Builder>)`.
 * Likewise, `ListA` is also constrained. Its unconstrained symbol has Rust
 * type `struct ListAUnconstrained(std::vec::Vec<ListBUnconstrained>)`.
 *
 * For an _unconstrained_ shape and for simple shapes, this symbol provider
 * delegates to the base symbol provider. It is therefore important that this
 * symbol provider _not_ wrap [PublicConstrainedShapeSymbolProvider] (from the
 * `codegen-server` subproject), because that symbol provider will return a
 * constrained type for shapes that have constraint traits attached.
 *
 * While this symbol provider is only used by the server, it needs to be in the
 * `codegen` subproject because the (common to client and server) parsers use
 * it.
 */
class UnconstrainedShapeSymbolProvider(
    private val base: RustSymbolProvider,
    private val model: Model,
    private val serviceShape: ServiceShape,
) : WrappingSymbolProvider(base) {
    private fun unconstrainedSymbolForListOrMapShape(shape: Shape): Symbol {
        check(shape is ListShape || shape is MapShape)

        val name = unconstrainedTypeNameForListOrMapShape(shape, serviceShape)
        val namespace = "crate::${Unconstrained.namespace}::${RustReservedWords.escapeIfNeeded(name.toSnakeCase())}"
        val rustType = RustType.Opaque(name, namespace)
        return Symbol.builder()
            .rustType(rustType)
            .name(rustType.name)
            .namespace(rustType.namespace, "::")
            .definitionFile(Unconstrained.filename)
            .build()
    }

    override fun toSymbol(shape: Shape): Symbol =
        when (shape) {
            is SetShape -> {
//                TODO("Set shapes can only contain some simple shapes, but constraint traits on simple shapes are not implemented")
                base.toSymbol(shape)
            }
            is ListShape -> {
                if (shape.canReachConstrainedShape(model, base)) {
                    unconstrainedSymbolForListOrMapShape(shape)
                } else {
                    base.toSymbol(shape)
                }
            }
            is MapShape -> {
                if (shape.canReachConstrainedShape(model, base)) {
                    unconstrainedSymbolForListOrMapShape(shape)
                } else {
                    base.toSymbol(shape)
                }
            }
            is StructureShape -> {
                if (shape.canReachConstrainedShape(model, base)) {
                    shape.builderSymbol(base)
                } else {
                    base.toSymbol(shape)
                }
            }
            // TODO(https://github.com/awslabs/smithy-rs/pull/1199) Simple shapes can have constraint traits.
            else -> base.toSymbol(shape)
        }
}
