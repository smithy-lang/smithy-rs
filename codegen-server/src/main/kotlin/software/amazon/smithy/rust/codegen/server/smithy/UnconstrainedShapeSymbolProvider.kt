/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.knowledge.NullableIndex
import software.amazon.smithy.model.shapes.CollectionShape
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.RustReservedWords
import software.amazon.smithy.rust.codegen.core.rustlang.RustType
import software.amazon.smithy.rust.codegen.core.rustlang.Visibility
import software.amazon.smithy.rust.codegen.core.smithy.Default
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.WrappingSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.contextName
import software.amazon.smithy.rust.codegen.core.smithy.handleOptionality
import software.amazon.smithy.rust.codegen.core.smithy.handleRustBoxing
import software.amazon.smithy.rust.codegen.core.smithy.locatedIn
import software.amazon.smithy.rust.codegen.core.smithy.rustType
import software.amazon.smithy.rust.codegen.core.smithy.setDefault
import software.amazon.smithy.rust.codegen.core.smithy.symbolBuilder
import software.amazon.smithy.rust.codegen.core.util.toPascalCase
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase
import software.amazon.smithy.rust.codegen.server.smithy.generators.serverBuilderSymbol

/**
 * The [UnconstrainedShapeSymbolProvider] returns, _for a given constrained
 * shape_, a symbol whose Rust type can hold the corresponding unconstrained
 * values.
 *
 * For collection and map shapes, this type is a [RustType.Opaque] wrapper
 * tuple newtype holding a container over the inner unconstrained type. For
 * structure shapes, it's their builder type. For union shapes, it's an enum
 * whose variants are the corresponding unconstrained variants. For simple
 * shapes, it's whatever the regular base symbol provider returns.
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
 */
class UnconstrainedShapeSymbolProvider(
    private val base: RustSymbolProvider,
    private val publicConstrainedTypes: Boolean,
    private val serviceShape: ServiceShape,
) : WrappingSymbolProvider(base) {
    private val nullableIndex = NullableIndex.of(model)

    /**
     * Unconstrained type names are always suffixed with `Unconstrained` for clarity, even though we could dispense with it
     * given that they all live inside the `unconstrained` module, so they don't collide with the constrained types.
     */
    private fun unconstrainedTypeNameForCollectionOrMapOrUnionShape(shape: Shape): String {
        check(shape is CollectionShape || shape is MapShape || shape is UnionShape)
        // Normally, one could use the base symbol provider's name. However, in this case, the name will be `Vec` or
        // `HashMap` because the symbol provider _does not_ newtype the shapes. However, for unconstrained shapes,
        // we need to introduce a newtype that preserves the original name of the shape from smithy. To handle that,
        // we load the name of the shape directly from the model prior to add `Unconstrained`.
        return RustReservedWords.escapeIfNeeded(shape.contextName(serviceShape).toPascalCase() + "Unconstrained")
    }

    private fun unconstrainedSymbolForCollectionOrMapOrUnionShape(shape: Shape): Symbol {
        check(shape is CollectionShape || shape is MapShape || shape is UnionShape)

        val name = unconstrainedTypeNameForCollectionOrMapOrUnionShape(shape)
        val parent = shape.getParentAndInlineModuleForConstrainedMember(this, publicConstrainedTypes)?.second ?: ServerRustModule.UnconstrainedModule

        val module = RustModule.new(
            RustReservedWords.escapeIfNeeded(name.toSnakeCase()),
            visibility = Visibility.PUBCRATE,
            parent = parent,
            inline = true,
        )
        val rustType = RustType.Opaque(name, module.fullyQualifiedPath())
        return Symbol.builder()
            .rustType(rustType)
            .name(rustType.name)
            .locatedIn(module)
            .build()
    }

    override fun toSymbol(shape: Shape): Symbol =
        when (shape) {
            is CollectionShape -> {
                if (shape.canReachConstrainedShape(model, base)) {
                    unconstrainedSymbolForCollectionOrMapOrUnionShape(shape)
                } else {
                    base.toSymbol(shape)
                }
            }

            is MapShape -> {
                if (shape.canReachConstrainedShape(model, base)) {
                    unconstrainedSymbolForCollectionOrMapOrUnionShape(shape)
                } else {
                    base.toSymbol(shape)
                }
            }

            is StructureShape -> {
                if (shape.canReachConstrainedShape(model, base)) {
                    shape.serverBuilderSymbol(base, !publicConstrainedTypes)
                } else {
                    base.toSymbol(shape)
                }
            }

            is UnionShape -> {
                if (shape.canReachConstrainedShape(model, base)) {
                    unconstrainedSymbolForCollectionOrMapOrUnionShape(shape)
                } else {
                    base.toSymbol(shape)
                }
            }

            is MemberShape -> {
                // There are only two cases where we use this symbol provider on a member shape.
                //
                // 1. When generating deserializers for HTTP-bound member shapes. See, for example:
                //     * how [HttpBindingGenerator] generates deserializers for a member shape with the `httpPrefixHeaders`
                //       trait targeting a map shape of string keys and values; or
                //     * how [ServerHttpBoundProtocolGenerator] deserializes for a member shape with the `httpQuery`
                //       trait targeting a collection shape that can reach a constrained shape.
                //
                // 2. When generating members for unconstrained unions. See [UnconstrainedUnionGenerator].
                if (shape.targetCanReachConstrainedShape(model, base)) {
                    val targetShape = model.expectShape(shape.target)
                    val targetSymbol = this.toSymbol(targetShape)
                    // Handle boxing first, so we end up with `Option<Box<_>>`, not `Box<Option<_>>`.
                    handleOptionality(
                        handleRustBoxing(targetSymbol, shape),
                        shape,
                        nullableIndex,
                        base.config.nullabilityCheckMode,
                    )
                } else {
                    base.toSymbol(shape)
                }
                // TODO(https://github.com/awslabs/smithy-rs/issues/1401) Constraint traits on member shapes are not
                //  implemented yet.
            }

            is StringShape -> {
                if (shape.canReachConstrainedShape(model, base)) {
                    symbolBuilder(shape, RustType.String).setDefault(Default.RustDefault).build()
                } else {
                    base.toSymbol(shape)
                }
            }

            else -> base.toSymbol(shape)
        }
}
