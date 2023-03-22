/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.knowledge.NullableIndex
import software.amazon.smithy.model.shapes.BlobShape
import software.amazon.smithy.model.shapes.ByteShape
import software.amazon.smithy.model.shapes.CollectionShape
import software.amazon.smithy.model.shapes.IntegerShape
import software.amazon.smithy.model.shapes.LongShape
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShortShape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.traits.LengthTrait
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.RustReservedWords
import software.amazon.smithy.rust.codegen.core.rustlang.RustType
import software.amazon.smithy.rust.codegen.core.rustlang.Visibility
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.WrappingSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.contextName
import software.amazon.smithy.rust.codegen.core.smithy.handleOptionality
import software.amazon.smithy.rust.codegen.core.smithy.handleRustBoxing
import software.amazon.smithy.rust.codegen.core.smithy.locatedIn
import software.amazon.smithy.rust.codegen.core.smithy.rustType
import software.amazon.smithy.rust.codegen.core.smithy.symbolBuilder
import software.amazon.smithy.rust.codegen.core.util.getTrait
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.core.util.orNull
import software.amazon.smithy.rust.codegen.core.util.toPascalCase
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase
import software.amazon.smithy.rust.codegen.server.smithy.generators.serverBuilderModule
import software.amazon.smithy.rust.codegen.server.smithy.traits.SyntheticStructureFromConstrainedMemberTrait

/**
 * The [ConstrainedShapeSymbolProvider] returns, for a given _directly_
 * constrained shape, a symbol whose Rust type can hold the constrained values.
 *
 * For all shapes with supported traits directly attached to them, this type is
 * a [RustType.Opaque] wrapper tuple newtype holding the inner constrained
 * type.
 *
 * The symbols this symbol provider returns are always public and exposed to
 * the end user.
 *
 * This symbol provider is meant to be used "deep" within the wrapped symbol
 * providers chain, just above the core base symbol provider, `SymbolVisitor`.
 *
 * If the shape is _transitively but not directly_ constrained, use
 * [PubCrateConstrainedShapeSymbolProvider] instead, which returns symbols
 * whose associated types are `pub(crate)` and thus not exposed to the end
 * user.
 */
open class ConstrainedShapeSymbolProvider(
    private val base: RustSymbolProvider,
    private val serviceShape: ServiceShape,
    private val publicConstrainedTypes: Boolean,
) : WrappingSymbolProvider(base) {
    private val nullableIndex = NullableIndex.of(model)

    private fun publicConstrainedSymbolForMapOrCollectionShape(shape: Shape): Symbol {
        check(shape is MapShape || shape is CollectionShape)

        val (name, module) = getMemberNameAndModule(shape, serviceShape, ServerRustModule.Model, !publicConstrainedTypes)
        val rustType = RustType.Opaque(name)
        return symbolBuilder(shape, rustType).locatedIn(module).build()
    }

    override fun toSymbol(shape: Shape): Symbol {
        return when (shape) {
            is MemberShape -> {
                // TODO(https://github.com/awslabs/smithy-rs/issues/1401) Member shapes can have constraint traits
                //  (constraint trait precedence).
                val target = model.expectShape(shape.target)
                val targetSymbol = this.toSymbol(target)
                // Handle boxing first, so we end up with `Option<Box<_>>`, not `Box<Option<_>>`.
                handleOptionality(
                    handleRustBoxing(targetSymbol, shape),
                    shape,
                    nullableIndex,
                    base.config.nullabilityCheckMode,
                )
            }

            is MapShape -> {
                if (shape.isDirectlyConstrained(base)) {
                    check(shape.hasTrait<LengthTrait>()) {
                        "Only the `length` constraint trait can be applied to map shapes"
                    }
                    publicConstrainedSymbolForMapOrCollectionShape(shape)
                } else {
                    val keySymbol = this.toSymbol(shape.key)
                    val valueSymbol = this.toSymbol(shape.value)
                    symbolBuilder(shape, RustType.HashMap(keySymbol.rustType(), valueSymbol.rustType()))
                        .addReference(keySymbol)
                        .addReference(valueSymbol)
                        .build()
                }
            }

            is CollectionShape -> {
                if (shape.isDirectlyConstrained(base)) {
                    check(constrainedCollectionCheck(shape)) {
                        "Only the `length` and `uniqueItems` constraint traits can be applied to list shapes"
                    }
                    publicConstrainedSymbolForMapOrCollectionShape(shape)
                } else {
                    val inner = this.toSymbol(shape.member)
                    symbolBuilder(shape, RustType.Vec(inner.rustType())).addReference(inner).build()
                }
            }

            is StringShape, is IntegerShape, is ShortShape, is LongShape, is ByteShape, is BlobShape -> {
                if (shape.isDirectlyConstrained(base)) {
                    // A standalone constrained shape goes into `ModelsModule`, but one
                    // arising from a constrained member shape goes into a module for the container.
                    val (name, module) = getMemberNameAndModule(shape, serviceShape, ServerRustModule.Model, !publicConstrainedTypes)
                    val rustType = RustType.Opaque(name)
                    symbolBuilder(shape, rustType).locatedIn(module).build()
                } else {
                    base.toSymbol(shape)
                }
            }

            else -> base.toSymbol(shape)
        }
    }

    /**
     * Checks that the collection:
     *  - Has at least 1 supported constraint applied to it, and
     *  - That it has no unsupported constraints applied.
     */
    private fun constrainedCollectionCheck(shape: CollectionShape): Boolean {
        val supportedConstraintTraits =
            supportedCollectionConstraintTraits.mapNotNull { shape.getTrait(it).orNull() }.toSet()
        val allConstraintTraits = allConstraintTraits.mapNotNull { shape.getTrait(it).orNull() }.toSet()

        return supportedConstraintTraits.isNotEmpty() && allConstraintTraits.subtract(supportedConstraintTraits)
            .isEmpty()
    }

    /**
     * Returns the pair (Rust Symbol Name, Inline Module) for the shape. At the time of model transformation all
     * constrained member shapes are extracted and are given a model-wide unique name. However, the generated code
     * for the new shapes is in a module that is named after the containing shape (structure, list, map or union).
     * The new shape's Rust Symbol is renamed from `{structureName}{memberName}` to  `{structure_name}::{member_name}`
     */
    private fun getMemberNameAndModule(
        shape: Shape,
        serviceShape: ServiceShape,
        defaultModule: RustModule.LeafModule,
        pubCrateServerBuilder: Boolean,
    ): Pair<String, RustModule.LeafModule> {
        val syntheticMemberTrait = shape.getTrait<SyntheticStructureFromConstrainedMemberTrait>()
            ?: return Pair(shape.contextName(serviceShape), defaultModule)

        return if (syntheticMemberTrait.container is StructureShape) {
            val builderModule = syntheticMemberTrait.container.serverBuilderModule(base, pubCrateServerBuilder)
            val renameTo = syntheticMemberTrait.member.memberName ?: syntheticMemberTrait.member.id.name
            Pair(renameTo.toPascalCase(), builderModule)
        } else {
            // For non-structure shapes, the new shape defined for a constrained member shape
            // needs to be placed in an inline module named `pub {container_name_in_snake_case}`.
            val moduleName = RustReservedWords.escapeIfNeeded(syntheticMemberTrait.container.id.name.toSnakeCase())
            val innerModuleName = moduleName + if (pubCrateServerBuilder) {
                "_internal"
            } else {
                ""
            }

            val innerModule = RustModule.new(
                innerModuleName,
                visibility = Visibility.publicIf(!pubCrateServerBuilder, Visibility.PUBCRATE),
                parent = defaultModule,
                inline = true,
                documentationOverride = "",
            )
            val renameTo = syntheticMemberTrait.member.memberName ?: syntheticMemberTrait.member.id.name
            Pair(renameTo.toPascalCase(), innerModule)
        }
    }
}
