/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.shapes.BlobShape
import software.amazon.smithy.model.shapes.ByteShape
import software.amazon.smithy.model.shapes.CollectionShape
import software.amazon.smithy.model.shapes.IntegerShape
import software.amazon.smithy.model.shapes.LongShape
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShortShape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.RustReservedWords
import software.amazon.smithy.rust.codegen.core.rustlang.RustType
import software.amazon.smithy.rust.codegen.core.rustlang.Visibility
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.WrappingSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.contextName
import software.amazon.smithy.rust.codegen.core.smithy.locatedIn
import software.amazon.smithy.rust.codegen.core.smithy.module
import software.amazon.smithy.rust.codegen.core.smithy.rustType
import software.amazon.smithy.rust.codegen.core.util.getTrait
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase
import software.amazon.smithy.rust.codegen.server.smithy.generators.serverBuilderSymbol
import software.amazon.smithy.rust.codegen.server.smithy.traits.SyntheticStructureFromConstrainedMemberTrait

/**
 * The [ConstraintViolationSymbolProvider] returns, for a given constrained
 * shape, a symbol whose Rust type can hold information about constraint
 * violations that may occur when building the shape from unconstrained values.
 *
 * So, for example, given the model:
 *
 * ```smithy
 * @pattern("\\w+")
 * @length(min: 1, max: 69)
 * string NiceString
 *
 * structure Structure {
 *     @required
 *     niceString: NiceString
 * }
 * ```
 *
 * A `NiceString` built from an arbitrary Rust `String` may give rise to at
 * most two constraint trait violations: one for `pattern`, one for `length`.
 * Similarly, the shape `Structure` can fail to be built when a value for
 * `niceString` is not provided.
 *
 * Said type is always called `ConstraintViolation`, and resides in a bespoke
 * module inside the same module as the _public_ constrained type the user is
 * exposed to. When the user is _not_ exposed to the constrained type, the
 * constraint violation type's module is a child of the `model` module.
 *
 * It is the responsibility of the caller to ensure that the shape is
 * constrained (either directly or transitively) before using this symbol
 * provider. This symbol provider intentionally crashes if the shape is not
 * constrained.
 */
class ConstraintViolationSymbolProvider(
    private val base: RustSymbolProvider,
    private val publicConstrainedTypes: Boolean,
    private val serviceShape: ServiceShape,
) : WrappingSymbolProvider(base) {
    private val constraintViolationName = "ConstraintViolation"
    private val visibility = when (publicConstrainedTypes) {
        true -> Visibility.PUBLIC
        false -> Visibility.PUBCRATE
    }

    private fun Shape.shapeModule(): RustModule.LeafModule {
        val documentation = if (publicConstrainedTypes && this.isDirectlyConstrained(base)) {
            val symbol = base.toSymbol(this)
            "See [`${this.contextName(serviceShape)}`]($symbol)."
        } else {
            ""
        }

        val syntheticTrait = getTrait<SyntheticStructureFromConstrainedMemberTrait>()

        val (module, name) = if (syntheticTrait != null) {
            // For constrained member shapes, the ConstraintViolation code needs to go in an inline rust module
            // that is a descendant of the module that contains the extracted shape itself.
            val overriddenMemberModule = this.getParentAndInlineModuleForConstrainedMember(base, publicConstrainedTypes)!!
            val name = syntheticTrait.member.memberName
            Pair(overriddenMemberModule.second, RustReservedWords.escapeIfNeeded(name).toSnakeCase())
        } else {
            // Need to use the context name so we get the correct name for maps.
            Pair(ServerRustModule.Model, RustReservedWords.escapeIfNeeded(this.contextName(serviceShape)).toSnakeCase())
        }

        return RustModule.new(
            name = name,
            visibility = visibility,
            parent = module,
            inline = true,
            documentationOverride = documentation,
        )
    }

    private fun constraintViolationSymbolForCollectionOrMapOrUnionShape(shape: Shape): Symbol {
        check(shape is CollectionShape || shape is MapShape || shape is UnionShape)

        val module = shape.shapeModule()
        val rustType = RustType.Opaque(constraintViolationName, module.fullyQualifiedPath())
        return Symbol.builder()
            .rustType(rustType)
            .name(rustType.name)
            .locatedIn(module)
            .build()
    }

    override fun toSymbol(shape: Shape): Symbol {
        check(shape.canReachConstrainedShape(model, base)) {
            "`ConstraintViolationSymbolProvider` was called on shape that does not reach a constrained shape: $shape"
        }

        return when (shape) {
            is MapShape, is CollectionShape, is UnionShape -> {
                constraintViolationSymbolForCollectionOrMapOrUnionShape(shape)
            }

            is StructureShape -> {
                val builderSymbol = shape.serverBuilderSymbol(base, pubCrate = !publicConstrainedTypes)

                val module = builderSymbol.module()
                val rustType = RustType.Opaque(constraintViolationName, module.fullyQualifiedPath())
                Symbol.builder()
                    .rustType(rustType)
                    .name(rustType.name)
                    .locatedIn(module)
                    .build()
            }

            is StringShape, is IntegerShape, is ShortShape, is LongShape, is ByteShape, is BlobShape -> {
                val module = shape.shapeModule()
                val rustType = RustType.Opaque(constraintViolationName, module.fullyQualifiedPath())
                Symbol.builder()
                    .rustType(rustType)
                    .name(rustType.name)
                    .locatedIn(module)
                    .build()
            }

            else -> TODO("Constraint traits on other shapes not implemented yet: $shape")
        }
    }
}
