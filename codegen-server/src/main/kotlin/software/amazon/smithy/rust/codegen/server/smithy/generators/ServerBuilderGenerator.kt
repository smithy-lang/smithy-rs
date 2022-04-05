/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.rustlang.Attribute
import software.amazon.smithy.rust.codegen.rustlang.RustReservedWords
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
import software.amazon.smithy.rust.codegen.server.smithy.ServerRuntimeType
import software.amazon.smithy.rust.codegen.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.expectRustMetadata
import software.amazon.smithy.rust.codegen.smithy.generators.StructureGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.targetNeedsValidation
import software.amazon.smithy.rust.codegen.smithy.isOptional
import software.amazon.smithy.rust.codegen.smithy.makeOptional
import software.amazon.smithy.rust.codegen.smithy.mapRustType
import software.amazon.smithy.rust.codegen.smithy.rustType
import software.amazon.smithy.rust.codegen.smithy.wrapValidated
import software.amazon.smithy.rust.codegen.util.dq
import software.amazon.smithy.rust.codegen.util.toPascalCase
import software.amazon.smithy.rust.codegen.util.toSnakeCase
import java.util.*

// TODO This function is the same as `BuilderGenerator.kt.`
fun StructureShape.builderSymbol(symbolProvider: RustSymbolProvider): RuntimeType {
    val symbol = symbolProvider.toSymbol(this)
    val builderNamespace = RustReservedWords.escapeIfNeeded(symbol.name.toSnakeCase())
    return RuntimeType("Builder", null, "${symbol.namespace}::$builderNamespace")
}

fun RuntimeConfig.operationBuildError() = RuntimeType.operationModule(this).member("BuildError")
fun RuntimeConfig.serializationError() = RuntimeType.operationModule(this).member("SerializationError")

class OperationBuildError(private val runtimeConfig: RuntimeConfig) {
    fun missingField(w: RustWriter, field: String, details: String) = "${w.format(runtimeConfig.operationBuildError())}::MissingField { field: ${field.dq()}, details: ${details.dq()} }"
    fun invalidField(w: RustWriter, field: String, details: String) = "${w.format(runtimeConfig.operationBuildError())}::InvalidField { field: ${field.dq()}, details: ${details.dq()}.to_string() }"
    fun serializationError(w: RustWriter, error: String) = "${w.format(runtimeConfig.operationBuildError())}::SerializationError($error.into())"
}

// TODO Document differences:
//     - This one takes in codegenContext.
class ServerBuilderGenerator(
    private val codegenContext: CodegenContext,
    private val shape: StructureShape
) {
    private val model = codegenContext.model
    private val symbolProvider = codegenContext.symbolProvider
    private val members: List<MemberShape> = shape.allMembers.values.toList()
    // TODO Ensure everyone uses this instead of recomputing
    private val structureSymbol = symbolProvider.toSymbol(shape)
    private val moduleName = shape.builderSymbol(symbolProvider).namespace.split("::").last()

    fun render(writer: RustWriter) {
        writer.docs("See #D.", structureSymbol)
        writer.withModule(moduleName) {
            renderBuilder(this)
        }

        // TODO Can't we move these into the builder module?
        if (StructureGenerator.serverHasFallibleBuilder(shape, model, symbolProvider)) {
            renderTryFromBuilderImpl(writer)
        } else {
            renderFromBuilderImpl(writer)
        }
    }

    // TODO This impl does not take into account sensitive trait.
    private fun renderImplDisplayValidationFailure(writer: RustWriter) {
        writer.rustBlock("impl std::fmt::Display for ValidationFailure") {
            rustBlock("fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result") {
                rustBlock("match self") {
                    validationFailures().forEach {
                        val arm = if (it.hasInner()) {
                           "ValidationFailure::${it.name()}(_)"
                        } else {
                            "ValidationFailure::${it.name()}"
                        }
                        rust("""$arm => write!(f, "${validationFailureErrorMessage(it)}"),""")
                    }
                }
            }
        }
    }

    // TODO This only needs to be generated for operation input shapes.
    private fun renderImplFromValidationFailureForRequestRejection(writer: RustWriter) {
        // TODO No need for rustBlock
        writer.rustBlock("impl From<ValidationFailure> for #T", ServerRuntimeType.RequestRejection(codegenContext.runtimeConfig)) {
            rustBlock("fn from(value: ValidationFailure) -> Self") {
                rust("Self::BuildV2(value.into())")
            }
        }
    }

    private fun renderImplFromBuilderForValidated(writer: RustWriter) {
        // TODO No need for rustBlock
        writer.rustBlock("impl From<Builder> for #T", structureSymbol.wrapValidated()) {
            rust(
                """
                fn from(value: Builder) -> Self {
                    Self::Unvalidated(value)
                }
                """
            )
        }
    }

    private fun renderBuilder(writer: RustWriter) {
        val builderName = "Builder"

        if (StructureGenerator.serverHasFallibleBuilder(shape, model, symbolProvider)) {
            Attribute.Derives(setOf(RuntimeType.Debug)).render(writer)
            // TODO(): `#[non_exhaustive] until we commit to making builders of builders public.
            Attribute.NonExhaustive.render(writer)
            writer.rustBlock("pub enum ValidationFailure") {
                validationFailures().forEach { renderValidationFailure(this, it) }
            }

            renderImplDisplayValidationFailure(writer)
            writer.rust("impl std::error::Error for ValidationFailure { }")
            renderImplFromValidationFailureForRequestRejection(writer)

            renderImplFromBuilderForValidated(writer)
        }

        writer.docs("A builder for #D.", structureSymbol)
        // Matching derives to the main structure + `Default` since we are a builder and everything is optional.
        // TODO Manually implement `Default` so that we can add custom docs.
        val baseDerives = structureSymbol.expectRustMetadata().derives
        // TODO Document breaking `PartialEq`.
        val derives = baseDerives.derives.intersect(setOf(RuntimeType.Debug, RuntimeType.Clone)) + RuntimeType.Default
        baseDerives.copy(derives = derives).render(writer)
        writer.rustBlock("pub struct $builderName") {
            for (member in members) {
                // All fields in the builder are optional.
                val memberSymbol = builderMemberSymbol(member)
                val memberName = symbolProvider.toMemberName(member)
                renderBuilderMember(this, memberName, memberSymbol)
            }
        }

        writer.rustBlock("impl $builderName") {
            for (member in members) {
                renderBuilderMemberFn(this, member)

                if (member.targetNeedsValidation(model, symbolProvider)) {
                    renderBuilderMemberSetterFn(this, member)
                }

                // Unlike in `BuilderGenerator.kt`, we don't add helper methods to add items to vectors and hash maps.
            }
            renderBuildFn(this)
        }
    }

    private fun renderBuildFn(implBlockWriter: RustWriter) {
        val fallibleBuilder = StructureGenerator.serverHasFallibleBuilder(shape, model, symbolProvider)
        val outputSymbol = symbolProvider.toSymbol(shape)
        val returnType = when (fallibleBuilder) {
            true -> "Result<${implBlockWriter.format(outputSymbol)}, ValidationFailure>"
            false -> implBlockWriter.format(outputSymbol)
        }
        // TODO Document when builder can fail.
        // TODO Document it returns the first error.
        implBlockWriter.docs("Consumes the builder and constructs a #D.", outputSymbol)
        implBlockWriter.rustBlock("pub fn build(self) -> $returnType") {
//            if (members.any { targetNeedsValidation(it) }) {
//                implBlockWriter.rust("use std::convert::TryInto;")
//            }
            conditionalBlock("Ok(", ")", conditional = fallibleBuilder) {
                // If a wrapper is specified, use the `::new` associated function to construct the wrapper.
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

    // TODO(EventStream): [DX] Consider updating builders to take EventInputStream as Into<EventInputStream>
    private fun renderBuilderMember(writer: RustWriter, memberName: String, memberSymbol: Symbol) {
        // Builder members are crate-public to enable using them directly in serializers/deserializers.
        // During XML deserialization, `builder.<field>.take` is used to append to lists and maps.
        writer.write("pub(crate) $memberName: #T,", memberSymbol)
    }

    private fun renderBuilderMemberFn(
        writer: RustWriter,
        member: MemberShape,
    ) {
        val symbol = symbolProvider.toSymbol(member)
        val memberName = symbolProvider.toMemberName(member)

        writer.documentShape(member, model)
        writer.rustBlock("pub fn $memberName(mut self, input: ${symbol.rustType().render()}) -> Self") {
            rust("self.$memberName = ")
            conditionalBlock("Some(", ")", conditional = !symbol.isOptional()) {
                if (member.targetNeedsValidation(model, symbolProvider)) {
                    val validatedType = "${symbol.wrapValidated().rustType().namespace}::Validated::Validated"
                    if (symbol.isOptional()) {
                        write("input.map(|v| $validatedType(v))")
                    } else {
                        write("$validatedType(input)")
                    }
                } else {
                    write("input")
                }
            }
            rust(";")
            rust("self")
        }
    }


    /*
     * Render a `set_foo` method. This method is able to take in builders of structure shape types.
     */
    private fun renderBuilderMemberSetterFn(
        writer: RustWriter,
        member: MemberShape,
    ) {
        check(model.expectShape(member.target, StructureShape::class.java) != null)

        val builderMemberSymbol = builderMemberSymbol(member)
        val inputType = builderMemberSymbol.rustType().stripOuter<RustType.Option>().implInto().let {
            if (member.isOptional) {
                "Option<$it>"
            } else {
                it
            }
        }
        val memberName = symbolProvider.toMemberName(member)

        writer.documentShape(member, model)
        // TODO: This method is only used by deserializers, so it will remain unused for shapes that are not (transitively)
        // part of an operation input. We therefore `[allow(dead_code)]` here.
        Attribute.AllowDeadCode.render(writer)
        // TODO(): `pub(crate)` until we commit to making builders of builders public.
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

    // TODO Docs
    enum class ValidationFailureKind {
        MISSING_MEMBER,
        BUILDER_FAILURE,
    }
    data class ValidationFailure(val forMember: MemberShape, val kind: ValidationFailureKind) {
        fun name() = when (kind) {
            ValidationFailureKind.MISSING_MEMBER -> "Missing${forMember.memberName.toPascalCase()}"
            ValidationFailureKind.BUILDER_FAILURE -> "${forMember.memberName.toPascalCase()}ValidationFailure"
        }

        fun hasInner() = kind == ValidationFailureKind.BUILDER_FAILURE
    }

    private fun renderValidationFailure(writer: RustWriter, validationFailure: ValidationFailure) {
        if (validationFailure.kind == ValidationFailureKind.BUILDER_FAILURE) {
            // TODO(): `#[doc(hidden)]` until we commit to making builders of builders public.
            Attribute.DocHidden.render(writer)
        }

        // TODO Add Rust docs.

        when (validationFailure.kind) {
            ValidationFailureKind.MISSING_MEMBER -> writer.rust("${validationFailure.name()},")
            ValidationFailureKind.BUILDER_FAILURE -> {
                val targetStructureShape = model.expectShape(validationFailure.forMember.target, StructureShape::class.java)
                writer.rust("${validationFailure.name()}(<#{T} as std::convert::TryFrom<#{T}>>::Error),",
                    symbolProvider.toSymbol(targetStructureShape),
                    targetStructureShape.builderSymbol(symbolProvider)
                )
            }
        }
    }

    // ONLY RETURNS BUILDER validation failure, intentional
    private fun builderValidationFailureForMember(member: MemberShape) =
        if (model.expectShape(member.target).isStructureShape) {
            Optional.of(ValidationFailure(member, ValidationFailureKind.BUILDER_FAILURE))
        } else {
            Optional.empty()
        }

    private fun validationFailureErrorMessage(validationFailure: ValidationFailure): String {
        val memberName = symbolProvider.toMemberName(validationFailure.forMember)
        // TODO $structureSymbol here is not quite right because it's printing the full namespace: crate:: in the context of the user will surely be different.
        return when (validationFailure.kind) {
            ValidationFailureKind.MISSING_MEMBER -> "`$memberName` was not specified but it is required when building `$structureSymbol`"
            // TODO Nest errors.
            ValidationFailureKind.BUILDER_FAILURE -> "validation failure occurred when building member `$memberName`, when building `$structureSymbol`"
        }
    }

    private fun validationFailures() = members.flatMap { member ->
        val ret = mutableListOf<ValidationFailure>()
        if (mustProvideValueForMember(member)) {
            ret.add(ValidationFailure(member, ValidationFailureKind.MISSING_MEMBER))
        }
        val builderValidationFailure = builderValidationFailureForMember(member)
        if (builderValidationFailure.isPresent) {
            ret.add(builderValidationFailure.get())
        }

        // TODO Constrained shapes.

        ret
    }

    private fun renderTryFromBuilderImpl(writer: RustWriter) {
        val shapeSymbol = symbolProvider.toSymbol(shape)
        val builderSymbol = shape.builderSymbol(symbolProvider)
        writer.rustTemplate(
            """
            impl std::convert::TryFrom<#{Builder}> for #{Shape} {
                type Error = $moduleName::ValidationFailure;
                
                fn try_from(value: #{Builder}) -> Result<Self, Self::Error> {
                    value.build()
                }
            }
            """,
            "Builder" to builderSymbol,
            "Shape" to shapeSymbol,
        )
    }

    private fun renderFromBuilderImpl(writer: RustWriter) {
        val shapeSymbol = symbolProvider.toSymbol(shape)
        val builderSymbol = shape.builderSymbol(symbolProvider)
        writer.rustTemplate(
            """
            impl From<#{Builder}> for #{Shape} {
                fn from(value: #{Builder}) -> Self {
                    value.build()
                }
            }
            """,
            "Builder" to builderSymbol,
            "Shape" to shapeSymbol,
        )
    }

    // TODO Docs
    private fun builderMemberSymbol(member: MemberShape): Symbol =
        symbolProvider.toSymbol(member)
            .mapRustType { it.stripOuter<RustType.Option>() }
            .let {
                if (member.targetNeedsValidation(model, symbolProvider)) it.wrapValidated()
                else it
            }.makeOptional()

    /**
     * TODO DOCS
     */
    private fun coreBuilder(writer: RustWriter) {
        // Builder type:
        //     Option<Validated<T>>
        //     Option<T>
        //
        // Struct type:
        //     Option<T>
        //     T
        writer.rustBlock("#T", structureSymbol) {
            for (member in members) {
                val memberName = symbolProvider.toMemberName(member)

                withBlock("$memberName: self.$memberName", ",") {
                    // Write the modifier(s).
                    if (member.targetNeedsValidation(model, symbolProvider)) {
                        // TODO Remove `TryInto` import when we switch to 2021 edition.
                        rustTemplate(
                            """
                            .map(|v| match v {
                                #{Validated}::Validated(x) => Ok(x),
                                #{Validated}::Unvalidated(x) => {
                                    use std::convert::TryInto;
                                    x.try_into()
                                }
                            })
                            .map(|v| v.map_err(|err| ValidationFailure::${builderValidationFailureForMember(member).get().name()}(err)))
                            .transpose()?
                            """,
                            "Validated" to RuntimeType.Validated()
                        )
                    }
                    if (mustProvideValueForMember(member)) {
                        rust(".ok_or(ValidationFailure::${ValidationFailure(member, ValidationFailureKind.MISSING_MEMBER).name()})?")
                    }
                }
            }
        }
    }

    // TODO We could move to extension function in `StructureGenerator.kt`.
    // TODO Is it.isOptional() necessary? Won't canUseDefault take into account `Option`s already?
    private fun mustProvideValueForMember(member: MemberShape) =
        !symbolProvider.toSymbol(member).isOptional()
}
