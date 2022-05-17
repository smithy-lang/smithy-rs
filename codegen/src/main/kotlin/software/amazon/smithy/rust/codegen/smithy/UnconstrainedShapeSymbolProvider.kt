/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.knowledge.NullableIndex
import software.amazon.smithy.model.shapes.CollectionShape
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.rustlang.RustReservedWords
import software.amazon.smithy.rust.codegen.rustlang.RustType
import software.amazon.smithy.rust.codegen.smithy.generators.builderSymbol
import software.amazon.smithy.rust.codegen.util.hasTrait
import software.amazon.smithy.rust.codegen.util.toPascalCase
import software.amazon.smithy.rust.codegen.util.toSnakeCase

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
    private val nullableIndex = NullableIndex.of(model)

    private fun unconstrainedSymbolForCollectionOrMapShape(shape: Shape): Symbol {
        check(shape is CollectionShape || shape is MapShape)

        val name = unconstrainedTypeNameForCollectionOrMapShape(shape, serviceShape)
        val namespace = "crate::${Unconstrained.namespace}::${RustReservedWords.escapeIfNeeded(name.toSnakeCase())}"
        val rustType = RustType.Opaque(name, namespace)
        return Symbol.builder()
            .rustType(rustType)
            .name(rustType.name)
            .namespace(rustType.namespace, "::")
            .definitionFile(Unconstrained.filename)
            .build()
    }

    // TODO The following two methods have been copied from `SymbolVisitor.kt`.
    private fun handleOptionality(symbol: Symbol, member: MemberShape): Symbol =
        if (member.isRequired) {
            symbol
        } else if (nullableIndex.isNullable(member)) {
            symbol.makeOptional()
        } else {
            symbol
        }

    /**
     * Boxes and returns [symbol], the symbol for the target of the member shape [shape], if [shape] is annotated with
     * [RustBoxTrait]; otherwise returns [symbol] unchanged.
     *
     * See `RecursiveShapeBoxer.kt` for the model transformation pass that annotates model shapes with [RustBoxTrait].
     */
    private fun handleRustBoxing(symbol: Symbol, shape: MemberShape): Symbol =
        if (shape.hasTrait<RustBoxTrait>()) {
            symbol.makeRustBoxed()
        } else symbol

    override fun toSymbol(shape: Shape): Symbol =
        when (shape) {
            is CollectionShape -> {
                if (shape.canReachConstrainedShape(model, base)) {
                    unconstrainedSymbolForCollectionOrMapShape(shape)
                } else {
                    base.toSymbol(shape)
                }
            }
            is MapShape -> {
                if (shape.canReachConstrainedShape(model, base)) {
                    unconstrainedSymbolForCollectionOrMapShape(shape)
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
            is MemberShape -> {
                // The only case where we use this symbol provider on a member shape is when generating deserializers
                // for HTTP-bound member shapes. See how e.g. [HttpBindingGenerator] generates deserializers for a member
                // shape with the `httpPrefixHeaders` trait targeting a map shape of string keys and values.
                if (model.expectShape(shape.container).isStructureShape && shape.targetCanReachConstrainedShape(model, base)) {
                    val targetShape = model.expectShape(shape.target)
                    val targetSymbol = this.toSymbol(targetShape)
                    // Handle boxing first so we end up with `Option<Box<_>>`, not `Box<Option<_>>`.
                    handleOptionality(handleRustBoxing(targetSymbol, shape), shape)
                } else {
                    base.toSymbol(shape)
                }
                // TODO Constraint traits on member shapes are not implemented yet.
            }
            else -> base.toSymbol(shape)
        }
}

fun unconstrainedTypeNameForCollectionOrMapShape(shape: Shape, serviceShape: ServiceShape): String {
    check(shape is CollectionShape || shape is MapShape)
    return "${shape.id.getName(serviceShape).toPascalCase()}Unconstrained"
}
