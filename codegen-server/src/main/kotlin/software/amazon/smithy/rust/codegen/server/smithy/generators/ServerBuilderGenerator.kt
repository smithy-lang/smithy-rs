/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.rust.codegen.rustlang.Attribute
import software.amazon.smithy.rust.codegen.rustlang.RustType
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.conditionalBlock
import software.amazon.smithy.rust.codegen.rustlang.docs
import software.amazon.smithy.rust.codegen.rustlang.documentShape
import software.amazon.smithy.rust.codegen.rustlang.implInto
import software.amazon.smithy.rust.codegen.rustlang.render
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.rustlang.stripOuter
import software.amazon.smithy.rust.codegen.rustlang.withBlock
import software.amazon.smithy.rust.codegen.server.smithy.ConstraintViolationSymbolProvider
import software.amazon.smithy.rust.codegen.server.smithy.ServerRuntimeType
import software.amazon.smithy.rust.codegen.smithy.PubCrateConstrainedShapeSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.RustBoxTrait
import software.amazon.smithy.rust.codegen.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.smithy.expectRustMetadata
import software.amazon.smithy.rust.codegen.smithy.generators.StructureGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.builderSymbol
import software.amazon.smithy.rust.codegen.smithy.hasConstraintTraitOrTargetHasConstraintTrait
import software.amazon.smithy.rust.codegen.smithy.hasPublicConstrainedWrapperTupleType
import software.amazon.smithy.rust.codegen.smithy.isOptional
import software.amazon.smithy.rust.codegen.smithy.isRustBoxed
import software.amazon.smithy.rust.codegen.smithy.isTransitivelyConstrained
import software.amazon.smithy.rust.codegen.smithy.letIf
import software.amazon.smithy.rust.codegen.smithy.makeMaybeConstrained
import software.amazon.smithy.rust.codegen.smithy.makeOptional
import software.amazon.smithy.rust.codegen.smithy.makeRustBoxed
import software.amazon.smithy.rust.codegen.smithy.mapRustType
import software.amazon.smithy.rust.codegen.smithy.rustType
import software.amazon.smithy.rust.codegen.smithy.targetCanReachConstrainedShape
import software.amazon.smithy.rust.codegen.smithy.traits.SyntheticInputTrait
import software.amazon.smithy.rust.codegen.util.hasTrait
import software.amazon.smithy.rust.codegen.util.toPascalCase
import software.amazon.smithy.rust.codegen.util.toSnakeCase

// TODO Document differences:
//     - This one takes in codegenContext.
//     - Unlike in `BuilderGenerator.kt`, we don't add helper methods to add items to vectors and hash maps.
//     - This builder is not `PartialEq`.
//     - Always implements either From<Builder> for Structure or TryFrom<Builder> for Structure.
//     - `pubCrateConstrainedShapeSymbolProvider` only needed if we want the builder to take in unconstrained types.
class ServerBuilderGenerator(
    private val codegenContext: ServerCodegenContext,
    private val shape: StructureShape,
    private val pubCrateConstrainedShapeSymbolProvider: PubCrateConstrainedShapeSymbolProvider? = null,
) {
    private val takeInUnconstrainedTypes = pubCrateConstrainedShapeSymbolProvider != null
    private val model = codegenContext.model
    private val symbolProvider = codegenContext.symbolProvider
    private val constrainedShapeSymbolProvider = codegenContext.constrainedShapeSymbolProvider
    private val constraintViolationSymbolProvider =
        ConstraintViolationSymbolProvider(codegenContext.symbolProvider, model, codegenContext.serviceShape)
    private val members: List<MemberShape> = shape.allMembers.values.toList()
    private val structureSymbol = symbolProvider.toSymbol(shape)
    private val moduleName = shape.builderSymbol(symbolProvider).namespace.split("::").last()
    private val isBuilderFallible = StructureGenerator.serverHasFallibleBuilder(shape, model, symbolProvider, takeInUnconstrainedTypes)
    private val publicConstrainedTypes = codegenContext.settings.codegenConfig.publicConstrainedTypes

    private val codegenScope = arrayOf(
        "RequestRejection" to ServerRuntimeType.RequestRejection(codegenContext.runtimeConfig),
        "Structure" to structureSymbol,
        "From" to RuntimeType.From,
        "TryFrom" to RuntimeType.TryFrom,
        "MaybeConstrained" to RuntimeType.MaybeConstrained()
    )

    fun render(writer: RustWriter) {
        writer.docs("See #D.", structureSymbol)
        writer.withModule(moduleName) {
            renderBuilder(this)
        }
    }

    private fun renderBuilder(writer: RustWriter) {
        if (isBuilderFallible) {
            Attribute.Derives(setOf(RuntimeType.Debug, RuntimeType.PartialEq)).render(writer)
            writer.docs("Holds one variant for each of the ways the builder can fail.")
            Attribute.NonExhaustive.render(writer)
            val constraintViolationSymbolName = constraintViolationSymbolProvider.toSymbol(shape).name
            writer.rustBlock("pub enum $constraintViolationSymbolName") {
                constraintViolations().forEach { renderConstraintViolation(this, it) }
            }

            renderImplDisplayConstraintViolation(writer)
            writer.rust("impl #T for ConstraintViolation { }", RuntimeType.StdError)

            // Only generate converter from `ConstraintViolation` into `RequestRejection` if the structure shape is
            // an operation input shape.
            if (shape.hasTrait<SyntheticInputTrait>()) {
                renderImplFromConstraintViolationForRequestRejection(writer)
            }

            if (takeInUnconstrainedTypes) {
                renderImplFromBuilderForMaybeConstrained(writer)
            }

            renderTryFromBuilderImpl(writer)
        } else {
            renderFromBuilderImpl(writer)
        }

        writer.docs("A builder for #D.", structureSymbol)
        // Matching derives to the main structure, - `PartialEq`, + `Default` since we are a builder and everything is optional.
        // TODO Manually implement `Default` so that we can add custom docs.
        val baseDerives = structureSymbol.expectRustMetadata().derives
        val derives = baseDerives.derives.intersect(setOf(RuntimeType.Debug, RuntimeType.Clone)) + RuntimeType.Default
        baseDerives.copy(derives = derives).render(writer)
        writer.rustBlock("pub struct Builder") {
            members.forEach { renderBuilderMember(this, it) }
        }

        writer.rustBlock("impl Builder") {
            for (member in members) {
                renderBuilderMemberFn(this, member)

                if (takeInUnconstrainedTypes && member.targetCanReachConstrainedShape(model, symbolProvider)) {
                    renderBuilderMemberSetterFn(this, member)
                }
            }
            renderBuildFn(this)
        }
    }

    // TODO This impl does not take into account sensitive trait.
    private fun renderImplDisplayConstraintViolation(writer: RustWriter) {
        writer.rustBlock("impl #T for ConstraintViolation", RuntimeType.Display) {
            rustBlock("fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result") {
                rustBlock("match self") {
                    constraintViolations().forEach {
                        val arm = if (it.hasInner()) {
                            "ConstraintViolation::${it.name()}(_)"
                        } else {
                            "ConstraintViolation::${it.name()}"
                        }
                        rust("""$arm => write!(f, "${constraintViolationMessage(it)}"),""")
                    }
                }
            }
        }
    }

    private fun renderImplFromConstraintViolationForRequestRejection(writer: RustWriter) {
        writer.rustTemplate(
            """
            impl #{From}<ConstraintViolation> for #{RequestRejection} {
                fn from(value: ConstraintViolation) -> Self {
                    Self::Build(value.into())
                }
            }
            """,
            *codegenScope
        )
    }

    private fun renderImplFromBuilderForMaybeConstrained(writer: RustWriter) {
        writer.rustTemplate(
            """
            impl #{From}<Builder> for #{StructureMaybeConstrained} {
                fn from(builder: Builder) -> Self {
                    Self::Unconstrained(builder)
                }
            }
            """,
            *codegenScope,
            "StructureMaybeConstrained" to structureSymbol.makeMaybeConstrained()
        )
    }

    private fun renderBuildFn(implBlockWriter: RustWriter) {
        val returnType = when (isBuilderFallible) {
            true -> "Result<${implBlockWriter.format(structureSymbol)}, ConstraintViolation>"
            false -> implBlockWriter.format(structureSymbol)
        }
        implBlockWriter.docs("""Consumes the builder and constructs a #D.""", structureSymbol)
        if (isBuilderFallible) {
            implBlockWriter.docs(
                """
                The builder fails to construct a #D if you do not provide a value for all non-`Option`al members.
                """,
                structureSymbol
            )

            if (constraintViolations().size > 1) {
                implBlockWriter.docs("If the builder fails, it will return the _first_ encountered [`ConstraintViolation`].")
            }
        }
        implBlockWriter.rustBlock("pub fn build(self) -> $returnType") {
            conditionalBlock("Ok(", ")", conditional = isBuilderFallible) {
                coreBuilder(this)
            }
        }
    }

    fun renderConvenienceMethod(implBlock: RustWriter) {
        val builderSymbol = shape.builderSymbol(symbolProvider)
        implBlock.docs("Creates a new builder-style object to manufacture #D.", structureSymbol)
        implBlock.rustBlock("pub fn builder() -> #T", builderSymbol) {
            write("#T::default()", builderSymbol)
        }
    }

    private fun renderBuilderMember(writer: RustWriter, member: MemberShape) {
        val memberSymbol = builderMemberSymbol(member)
        val memberName = symbolProvider.toMemberName(member)
        // Builder members are crate-public to enable using them directly in serializers/deserializers.
        // During XML deserialization, `builder.<field>.take` is used to append to lists and maps.
        writer.write("pub(crate) $memberName: #T,", memberSymbol)
    }

    /**
     * Render a `foo` method to set shape member `foo`. The caller must provide a value with the exact same type
     * as the shape member's type.
     *
     * This method is meant for use by the user; it is not used by the generated crate's (de)serializers.
     */
    private fun renderBuilderMemberFn(
        writer: RustWriter,
        member: MemberShape,
    ) {
        val symbol = symbolProvider.toSymbol(member)
        val memberName = symbolProvider.toMemberName(member)

        val hasBox = symbol.mapRustType { it.stripOuter<RustType.Option>() }.isRustBoxed()
        val wrapInMaybeConstrained = takeInUnconstrainedTypes && member.targetCanReachConstrainedShape(model, symbolProvider)

        writer.documentShape(member, model)

        if (hasBox && wrapInMaybeConstrained) {
            // In the case of recursive shapes, the member might be boxed. If so, and the member is also constrained, the
            // implementation of this function needs to immediately unbox the value to wrap it in `MaybeConstrained`,
            // and then re-box. Clippy warns us that we could have just taken in an unboxed value to avoid this round-trip
            // to the heap. However, that will make the builder take in a value whose type does not exactly match the
            // shape member's type.
            // We don't want to introduce API asymmetry just for this particular case, so we disable the lint.
            Attribute.Custom("allow(clippy::boxed_local)").render(writer)
        }
        writer.rustBlock("pub fn $memberName(mut self, input: ${symbol.rustType().render()}) -> Self") {
            rust("self.$memberName = ")
            conditionalBlock("Some(", ")", conditional = !symbol.isOptional()) {
                if (wrapInMaybeConstrained) {
                    val maybeConstrainedVariant = if (!publicConstrainedTypes &&
                        member.hasPublicConstrainedWrapperTupleType(model))
                        // If constrained types are not public and this member shape is one that would generate a
                        // public constrained type were the setting to be enabled, the user is sending us an unconstrained
                        // type, and we will have to check the constraints when `build()` is called.
                        "${symbol.makeMaybeConstrained().rustType().namespace}::MaybeConstrained::Unconstrained"
                    else {
                        "${symbol.makeMaybeConstrained().rustType().namespace}::MaybeConstrained::Constrained"
                    }
                    // TODO Add a protocol testing the branch (`symbol.isOptional() == false`, `hasBox == true`).
                    var varExpr = if (symbol.isOptional()) "v" else "input"
                    if (hasBox) varExpr = "*$varExpr"
                    if (publicConstrainedTypes && !constrainedTypeHoldsFinalType(member)) varExpr = "($varExpr).into()"

                    // If constrained types are not public, the user is sending us a fully unconstrained type.
                    // If the shape is transitively but not directly constrained, we need to constrain the
                    // fully unconstrained inner types into their corresponding unconstrained types first.
                    val targetShape = model.expectShape(member.target)
                    if (!publicConstrainedTypes &&
                        member.isTransitivelyConstrained(model, symbolProvider) &&
                        !targetShape.isStructureShape &&
                        !targetShape.isUnionShape) {
                        varExpr = "($varExpr).into()"
                    }

                    conditionalBlock("input.map(##[allow(clippy::redundant_closure)] |v| ", ")", conditional = symbol.isOptional()) {
                        conditionalBlock("Box::new(", ")", conditional = hasBox) {
                            rust("$maybeConstrainedVariant($varExpr)")
                        }
                    }
                } else {
                    write("input")
                }
            }
            rust(";")
            rust("self")
        }
    }

    /**
     * Returns whether the constrained builder member type (the type on which the `Constrained` trait is implemented)
     * is the final type the user sees when receiving the built struct. This is true when the corresponding constrained
     * type is public and not `pub(crate)`, which happens when the target is a structure shape, a union shape, or is
     * directly constrained.
     *
     * An example where this returns false is when the member shape targets a list whose members are lists of structures
     * having at least one `required` member. In this case the member shape is transitively but not directly constrained,
     * so the generated constrained type is `pub(crate)` and needs converting into the final type the user will be
     * exposed to.
     *
     * See [PubCrateConstrainedShapeSymbolProvider] too.
     */
    private fun constrainedTypeHoldsFinalType(member: MemberShape): Boolean {
        val targetShape = model.expectShape(member.target)
        return targetShape is StructureShape ||
                targetShape is UnionShape ||
                member.hasConstraintTraitOrTargetHasConstraintTrait(model, symbolProvider)
    }

    /**
     * Render a `set_foo` method.
     * This method is able to take in unconstrained types for constrained shapes, like builders of structs in the case
     * of structure shapes.
     *
     * This method is only used by deserializers at the moment and is therefore `pub(crate)`.
     */
    private fun renderBuilderMemberSetterFn(
        writer: RustWriter,
        member: MemberShape,
    ) {
        val builderMemberSymbol = builderMemberSymbol(member)
        val inputType = builderMemberSymbol.rustType().stripOuter<RustType.Option>().implInto().letIf(member.isOptional) { "Option<$it>" }
        val memberName = symbolProvider.toMemberName(member)

        writer.documentShape(member, model)
        // Setter names will never hit a reserved word and therefore never need escaping.
        writer.rustBlock("pub(crate) fn set_${memberName.toSnakeCase()}(mut self, input: $inputType) -> Self") {
            rust(
                """
                self.$memberName = ${
                if (member.isOptional) {
                    "input.map(|v| v.into())"
                } else {
                    "Some(input.into())"
                }
                };
                self
                """
            )
        }
    }

    /**
     * The kinds of constraint violations that can occur when building the builder.
     */
    enum class ConstraintViolationKind {
        // A field is required but was not provided.
        MISSING_MEMBER,
        // An unconstrained type was provided for a field targeting a constrained shape, but it failed to convert into the constrained type.
        CONSTRAINED_SHAPE_FAILURE,
    }

    data class ConstraintViolation(val forMember: MemberShape, val kind: ConstraintViolationKind) {
        fun name() = when (kind) {
            ConstraintViolationKind.MISSING_MEMBER -> "Missing${forMember.memberName.toPascalCase()}"
            ConstraintViolationKind.CONSTRAINED_SHAPE_FAILURE -> "${forMember.memberName.toPascalCase()}ConstraintViolation"
        }

        /**
         * Whether the constraint violation is a Rust tuple struct with one element.
         */
        fun hasInner() = kind == ConstraintViolationKind.CONSTRAINED_SHAPE_FAILURE
    }

    private fun renderConstraintViolation(writer: RustWriter, constraintViolation: ConstraintViolation) =
        when (constraintViolation.kind) {
            ConstraintViolationKind.MISSING_MEMBER -> {
                writer.docs("${constraintViolationMessage(constraintViolation).replaceFirstChar { it.uppercase() }}.")
                writer.rust("${constraintViolation.name()},")
            }
            ConstraintViolationKind.CONSTRAINED_SHAPE_FAILURE -> {
                val targetShape = model.expectShape(constraintViolation.forMember.target)

                val constraintViolationSymbol =
                    constraintViolationSymbolProvider.toSymbol(targetShape)
                        // If the corresponding structure's member is boxed, box this constraint violation symbol too.
                        .letIf(constraintViolation.forMember.hasTrait<RustBoxTrait>()) {
                            it.makeRustBoxed()
                        }

                // Note we cannot express the inner constraint violation as `<T as TryFrom<T>>::Error`, because `T` might
                // be `pub(crate)` and that would leak `T` in a public interface.
                writer.docs("${constraintViolationMessage(constraintViolation)}.")
                Attribute.DocHidden.render(writer)
                writer.rust("${constraintViolation.name()}(#T),", constraintViolationSymbol)
            }
        }

    /**
     * A message for a `ConstraintViolation` variant. This is used in both Rust documentation and the `Display` trait implementation.
     */
    private fun constraintViolationMessage(constraintViolation: ConstraintViolation): String {
        val memberName = symbolProvider.toMemberName(constraintViolation.forMember)
        return when (constraintViolation.kind) {
            ConstraintViolationKind.MISSING_MEMBER -> "`$memberName` was not provided but it is required when building `${structureSymbol.name}`"
            // TODO Nest errors.
            ConstraintViolationKind.CONSTRAINED_SHAPE_FAILURE -> "constraint violation occurred building member `$memberName` when building `${structureSymbol.name}`"
        }
    }

    private fun constraintViolations() = members.flatMap { member ->
        listOfNotNull(
            builderMissingFieldForMember(member),
            builderConstraintViolationForMember(member),
        )
    }

    /**
     * Returns the builder failure associated with the `member` field if its target is constrained.
     */
    private fun builderConstraintViolationForMember(member: MemberShape) =
        if (takeInUnconstrainedTypes && member.targetCanReachConstrainedShape(model, symbolProvider)) {
            ConstraintViolation(member, ConstraintViolationKind.CONSTRAINED_SHAPE_FAILURE)
        } else {
            null
        }

    /**
     * Returns the builder failure associated with the `member` field if it is `required`.
     */
    private fun builderMissingFieldForMember(member: MemberShape) =
        // TODO(https://github.com/awslabs/smithy-rs/issues/1302): We go through the symbol provider because
        //  non-`required` blob streaming members are interpreted as `required`, so we can't use `member.isOptional`.
        if (symbolProvider.toSymbol(member).isOptional()) {
            null
        } else {
            ConstraintViolation(member, ConstraintViolationKind.MISSING_MEMBER)
        }

    private fun renderTryFromBuilderImpl(writer: RustWriter) {
        writer.rustTemplate(
            """
            impl #{TryFrom}<Builder> for #{Structure} {
                type Error = ConstraintViolation;
                
                fn try_from(builder: Builder) -> Result<Self, Self::Error> {
                    builder.build()
                }
            }
            """,
            *codegenScope
        )
    }

    private fun renderFromBuilderImpl(writer: RustWriter) {
        writer.rustTemplate(
            """
            impl #{From}<Builder> for #{Structure} {
                fn from(builder: Builder) -> Self {
                    builder.build()
                }
            }
            """,
            *codegenScope
        )
    }

    /**
     * Returns the symbol for a builder's member.
     * All builder members are optional, but only some are `Option<T>`s where `T` needs to be constrained.
     */
    private fun builderMemberSymbol(member: MemberShape): Symbol =
        if (takeInUnconstrainedTypes && member.targetCanReachConstrainedShape(model, symbolProvider)) {
            val strippedOption = if (member.hasConstraintTraitOrTargetHasConstraintTrait(model, symbolProvider)) {
                constrainedShapeSymbolProvider.toSymbol(member)
            } else {
                pubCrateConstrainedShapeSymbolProvider!!.toSymbol(member)
            }
            // Strip the `Option` in case the member is not `required`.
            .mapRustType { it.stripOuter<RustType.Option>() }

            val hadBox = strippedOption.isRustBoxed()
            strippedOption
                // Strip the `Box` in case the member can reach itself recursively.
                .mapRustType { it.stripOuter<RustType.Box>() }
                // Wrap it in the Cow-like `constrained::MaybeConstrained` type, since we know the target member shape can
                // reach a constrained shape.
                .makeMaybeConstrained()
                // Box it in case the member can reach itself recursively.
                .letIf(hadBox) { it.makeRustBoxed() }
                // Ensure we always end up with an `Option`.
                .makeOptional()
        } else {
            symbolProvider.toSymbol(member).makeOptional()
        }

    /**
     * Writes the code to instantiate the struct the builder builds.
     *
     * Builder member types are either:
     *     1. `Option<MaybeConstrained<U>>`; or
     *     2. `Option<U>`.
     *
     * Where `U` is a constrained type.
     *
     * The structs they build have member types:
     *     a) `Option<T>`; or
     *     b) `T`.
     *
     * `U` is equal to `T` when the shape for `U` has a constraint trait or the member shape is a structure shape.
     * Otherwise, `U` is always a `pub(crate)` tuple newtype holding `T`.
     *
     * For each member, this function first safely unwraps case 1. into 2., then converts `U` into `T` if necessary,
     * and then converts into b) if necessary.
     */
    private fun coreBuilder(writer: RustWriter) {
        writer.rustBlock("#T", structureSymbol) {
            for (member in members) {
                val memberName = symbolProvider.toMemberName(member)

                withBlock("$memberName: self.$memberName", ",") {
                    // Write the modifier(s).
                    builderConstraintViolationForMember(member)?.also { constraintViolation ->
                        val hasBox = builderMemberSymbol(member)
                            .mapRustType { it.stripOuter<RustType.Option>() }
                            .isRustBoxed()
                        if (hasBox) {
                            rustTemplate(
                                """
                                .map(|v| match *v {
                                    #{MaybeConstrained}::Constrained(x) => Ok(Box::new(x)),
                                    #{MaybeConstrained}::Unconstrained(x) => Ok(Box::new(x.try_into()?)),
                                })
                                .map(|res| 
                                    res${ if (constrainedTypeHoldsFinalType(member)) "" else ".map(|v| v.into())" }
                                       .map_err(|err| ConstraintViolation::${constraintViolation.name()}(Box::new(err)))
                                )
                                .transpose()?
                                """,
                                *codegenScope
                            )
                        } else {
                            rustTemplate(
                                """
                                .map(|v| match v {
                                    #{MaybeConstrained}::Constrained(x) => Ok(x),
                                    #{MaybeConstrained}::Unconstrained(x) => x.try_into(),
                                })
                                .map(|res| 
                                    res${ if (constrainedTypeHoldsFinalType(member)) "" else ".map(|v| v.into())" }
                                       .map_err(ConstraintViolation::${constraintViolation.name()})
                                )
                                .transpose()?
                                """,
                                *codegenScope
                            )
                        }
                        // If constrained types are not public and this is a member shape that would have generated a
                        // public constrained type, were the setting to be enabled, it means the user sent us an
                        // unconstrained type. We've just checked the constraints hold by going through the non-public
                        // constrained type, but the user wants to work with the unconstrained type, so we have to
                        // unwrap it.
                        if (!codegenContext.settings.codegenConfig.publicConstrainedTypes &&
                            member.hasPublicConstrainedWrapperTupleType(model)) {
                            rust(
                                ".map(|v: #T| v.into())",
                                constrainedShapeSymbolProvider.toSymbol(model.expectShape(member.target)),
                            )
                        }
                    }
                    builderMissingFieldForMember(member)?.also {
                        rust(".ok_or(ConstraintViolation::${it.name()})?")
                    }
                }
            }
        }
    }
}
